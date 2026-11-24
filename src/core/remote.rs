use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::Result;
use async_trait::async_trait;

use super::model::{Entry, Protocol, Site};

/// Shared progress / cancellation state for a single transfer.
#[derive(Default, Debug)]
pub struct Progress {
    pub done: AtomicU64,
    pub cancel: AtomicBool,
}

impl Progress {
    pub fn add(&self, n: u64) {
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

pub async fn connect(site: &Site) -> Result<Box<dyn RemoteFs>> {
    match site.protocol {
        Protocol::Sftp => Ok(Box::new(super::sftp::SftpFs::connect(site).await?)),
        Protocol::Ftp => Ok(Box::new(super::ftp::connect_plain(site).await?)),
        Protocol::Ftps => Ok(Box::new(super::ftp::connect_tls(site).await?)),
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
