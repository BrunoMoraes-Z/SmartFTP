use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;

use super::config::ConnectionSettings;
use super::model::{Entry, Protocol, Site};

/// Token bucket shared by all transfers in one direction. 0 bytes/s = unlimited.
#[derive(Default, Debug)]
pub struct RateLimiter {
    bytes_per_sec: AtomicU64,
    state: tokio::sync::Mutex<Option<(Instant, f64)>>,
}

impl RateLimiter {
    pub fn set_limit(&self, bytes_per_sec: u64) {
        self.bytes_per_sec.store(bytes_per_sec, Ordering::Relaxed);
    }

    pub async fn take(&self, n: u64) {
        let rate = self.bytes_per_sec.load(Ordering::Relaxed);
        if rate == 0 {
            return;
        }
        let rate = rate as f64;
        // holding the lock while sleeping queues the other transfers behind us
        let mut st = self.state.lock().await;
        let now = Instant::now();
        let (last, tokens) = st.unwrap_or((now, rate));
        let mut tokens = (tokens + now.duration_since(last).as_secs_f64() * rate).min(rate);
        tokens -= n as f64;
        if tokens < 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(-tokens / rate)).await;
            tokens = 0.0;
        }
        *st = Some((Instant::now(), tokens));
    }
}

/// Shared progress / cancellation state for a single transfer.
#[derive(Default, Debug)]
pub struct Progress {
    pub done: AtomicU64,
    pub cancel: AtomicBool,
    pub limiter: Option<Arc<RateLimiter>>,
}

impl Progress {
    pub fn with_limiter(limiter: Arc<RateLimiter>) -> Self {
        Progress { limiter: Some(limiter), ..Default::default() }
    }
    /// Count `n` transferred bytes, waiting if a speed limit applies.
    pub async fn advance(&self, n: u64) {
        if let Some(l) = &self.limiter {
            l.take(n).await;
        }
        self.done.fetch_add(n, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub const CHUNK: usize = 64 * 1024;

/// Operations every protocol backend supports.
#[async_trait]
pub trait RemoteFs: Send + Sync {
    /// Directory the server put us in after login.
    async fn home(&self) -> Result<String>;
    async fn list(&self, path: &str) -> Result<Vec<Entry>>;
    async fn is_dir(&self, path: &str) -> Result<bool>;
    async fn mkdir(&self, path: &str) -> Result<()>;
    async fn remove_file(&self, path: &str) -> Result<()>;
    async fn remove_dir(&self, path: &str) -> Result<()>;
    async fn rename(&self, from: &str, to: &str) -> Result<()>;
    async fn chmod(&self, path: &str, mode: u32) -> Result<()>;
    async fn download(&self, remote: &str, local: &Path, progress: &Progress) -> Result<u64>;
    async fn upload(&self, local: &Path, remote: &str, progress: &Progress) -> Result<u64>;
    async fn read(&self, remote: &str, max: usize) -> Result<Vec<u8>>;
    async fn write(&self, remote: &str, data: &[u8]) -> Result<()>;
    async fn close(&self);
    /// Set the modification time (unix seconds). Best effort.
    async fn set_mtime(&self, _path: &str, _mtime: i64) -> Result<()> {
        Ok(())
    }
}

pub async fn connect(site: &Site, settings: &ConnectionSettings) -> Result<Box<dyn RemoteFs>> {
    match site.protocol {
        Protocol::Sftp => Ok(Box::new(super::sftp::SftpFs::connect(site, settings).await?)),
        Protocol::Ftp => Ok(Box::new(super::ftp::connect_plain(site, settings).await?)),
        Protocol::Ftps => Ok(Box::new(super::ftp::connect_tls(site, settings).await?)),
    }
}

pub fn cancelled_err() -> anyhow::Error {
    anyhow::anyhow!("transfer cancelled")
}

pub fn list_local(path: &Path) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for item in std::fs::read_dir(path)? {
        let Ok(item) = item else { continue };
        let Ok(meta) = item.metadata() else { continue };
        let name = item.file_name().to_string_lossy().to_string();
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        out.push(Entry {
            path: item.path().to_string_lossy().to_string(),
            name,
            is_dir: meta.is_dir(),
            is_symlink: item.file_type().map(|t| t.is_symlink()).unwrap_or(false),
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified,
            permissions: None,
        });
    }
    sort_entries(&mut out);
    Ok(out)
}

pub fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}
