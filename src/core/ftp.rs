use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use suppaftp::Mode;
use suppaftp::list::File as FtpFile;
use suppaftp::tokio::{AsyncRustlsConnector, AsyncRustlsFtpStream, ImplAsyncFtpStream, TokioTlsStream};
use suppaftp::tokio_rustls::rustls;
use suppaftp::types::FileType;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use super::config::{ConnectionSettings, FtpMode};
use super::model::{Entry, Site, join_remote};
use super::remote::{CHUNK, Progress, RemoteFs, cancelled_err, sort_entries};

pub struct FtpFs<T: TokioTlsStream + Send> {
    stream: Mutex<ImplAsyncFtpStream<T>>,
}

async fn login<T: TokioTlsStream + Send>(ftp: ImplAsyncFtpStream<T>, site: &Site, settings: &ConnectionSettings) -> Result<FtpFs<T>> {
    let mut ftp = match settings.ftp_mode {
        FtpMode::Active => ftp.active_mode(Duration::from_secs(settings.timeout_secs.max(1))),
        FtpMode::Passive => {
            let mut f = ftp;
            f.set_mode(Mode::Passive);
            f
        }
        FtpMode::ExtendedPassive => {
            let mut f = ftp;
            f.set_mode(Mode::ExtendedPassive);
            f
        }
    };
    ftp.set_passive_nat_workaround(settings.ftp_nat_workaround);
    let user = if site.user.is_empty() { "anonymous" } else { site.user.as_str() };
    let pass = site.password.clone().unwrap_or_default();
    ftp.login(user, &pass).await.map_err(|e| anyhow!("login failed: {e}"))?;
    ftp.transfer_type(FileType::Binary).await?;
    Ok(FtpFs { stream: Mutex::new(ftp) })
}

pub async fn connect_plain(site: &Site, settings: &ConnectionSettings) -> Result<FtpFs<suppaftp::tokio::AsyncNoTlsStream>> {
    let ftp = tokio::time::timeout(
        Duration::from_secs(settings.timeout_secs.max(1)),
        suppaftp::tokio::AsyncFtpStream::connect((site.host.as_str(), site.port)),
    )
    .await
    .map_err(|_| anyhow!("timed out connecting to {}:{}", site.host, site.port))??;
    login(ftp, site, settings).await
}

/// Explicit FTPS (AUTH TLS) using rustls with the Mozilla root store.
pub async fn connect_tls(site: &Site, settings: &ConnectionSettings) -> Result<FtpFs<suppaftp::tokio::AsyncRustlsStream>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = AsyncRustlsConnector::from(suppaftp::tokio_rustls::TlsConnector::from(Arc::new(config)));
    let ftp = tokio::time::timeout(
        Duration::from_secs(settings.timeout_secs.max(1)),
        AsyncRustlsFtpStream::connect((site.host.as_str(), site.port)),
    )
    .await
    .map_err(|_| anyhow!("timed out connecting to {}:{}", site.host, site.port))??;
    let ftp = ftp.into_secure(connector, &site.host).await.map_err(|e| anyhow!("TLS negotiation failed: {e}"))?;
    login(ftp, site, settings).await
}

fn to_entry(dir: &str, f: &FtpFile, raw: &str) -> Entry {
    let perms =
        raw.split_whitespace().next().filter(|p| p.len() == 10 && p.chars().skip(1).all(|c| "rwxsStT-".contains(c))).map(str::to_string);
    Entry {
        name: f.name().to_string(),
        path: join_remote(dir, f.name()),
        is_dir: f.is_directory(),
        is_symlink: f.is_symlink(),
        size: f.size() as u64,
        modified: f.modified().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64),
        permissions: perms,
    }
}

#[async_trait]
impl<T> RemoteFs for FtpFs<T>
where
    T: TokioTlsStream + Send + Sync + 'static,
{
    async fn home(&self) -> Result<String> {
        Ok(self.stream.lock().await.pwd().await?)
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let mut ftp = self.stream.lock().await;
        let mut out = Vec::new();
        match ftp.mlsd(Some(path)).await {
            Ok(lines) => {
                for line in lines {
                    if let Ok(f) = suppaftp::list::ListParser::parse_mlsd(&line) {
                        out.push(to_entry(path, &f, ""));
                    }
                }
            }
            Err(_) => {
                for line in ftp.list(Some(path)).await? {
                    if let Ok(f) = FtpFile::try_from(line.as_str()) {
                        out.push(to_entry(path, &f, &line));
                    }
                }
            }
        }
        out.retain(|e| e.name != "." && e.name != "..");
        sort_entries(&mut out);
        Ok(out)
    }

    async fn is_dir(&self, path: &str) -> Result<bool> {
        let mut ftp = self.stream.lock().await;
        let cwd = ftp.pwd().await?;
        let is_dir = ftp.cwd(path).await.is_ok();
        if is_dir {
            ftp.cwd(&cwd).await?;
        }
        Ok(is_dir)
    }

    async fn mkdir(&self, path: &str) -> Result<()> {
        Ok(self.stream.lock().await.mkdir(path).await?)
    }

    async fn remove_file(&self, path: &str) -> Result<()> {
        Ok(self.stream.lock().await.rm(path).await?)
    }

    async fn remove_dir(&self, path: &str) -> Result<()> {
        Ok(self.stream.lock().await.rmdir(path).await?)
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        Ok(self.stream.lock().await.rename(from, to).await?)
    }

    async fn chmod(&self, path: &str, mode: u32) -> Result<()> {
        self.stream.lock().await.site(format!("CHMOD {mode:o} {path}")).await?;
        Ok(())
    }

    async fn download(&self, remote: &str, local: &Path, progress: &Progress) -> Result<u64> {
        let mut ftp = self.stream.lock().await;
        let mut src = ftp.retr_as_stream(remote).await?;
        let mut dst = tokio::fs::File::create(local).await?;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            if progress.cancelled() {
                drop(dst);
                let _ = src.finish().await;
                let _ = tokio::fs::remove_file(local).await;
                return Err(cancelled_err());
            }
            let n = src.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).await?;
            total += n as u64;
            progress.advance(n as u64).await;
        }
        dst.flush().await?;
        src.finish().await?;
        Ok(total)
    }

    async fn upload(&self, local: &Path, remote: &str, progress: &Progress) -> Result<u64> {
        let mut ftp = self.stream.lock().await;
        let mut src = tokio::fs::File::open(local).await?;
        let mut dst = ftp.put_with_stream(remote).await?;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            if progress.cancelled() {
                let _ = dst.finish().await;
                return Err(cancelled_err());
            }
            let n = src.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).await?;
            total += n as u64;
            progress.advance(n as u64).await;
        }
        dst.finish().await?;
        Ok(total)
    }

    async fn read(&self, remote: &str, max: usize) -> Result<Vec<u8>> {
        let mut ftp = self.stream.lock().await;
        let mut src = ftp.retr_as_stream(remote).await?;
        let mut out = Vec::new();
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = src.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            if out.len() < max {
                out.extend_from_slice(&buf[..n.min(max - out.len())]);
            }
        }
        src.finish().await?;
        Ok(out)
    }

    async fn write(&self, remote: &str, data: &[u8]) -> Result<()> {
        let mut ftp = self.stream.lock().await;
        let mut dst = ftp.put_with_stream(remote).await?;
        dst.write_all(data).await?;
        dst.finish().await?;
        Ok(())
    }

    async fn set_mtime(&self, path: &str, mtime: i64) -> Result<()> {
        let ts = chrono::DateTime::from_timestamp(mtime, 0).ok_or_else(|| anyhow!("bad time"))?.format("%Y%m%d%H%M%S");
        let mut ftp = self.stream.lock().await;
        if ftp.custom_command(format!("MFMT {ts} {path}"), &[suppaftp::Status::File]).await.is_ok() {
            return Ok(());
        }
        // vsftpd and others accept the older "MDTM <time> <path>" form
        ftp.custom_command(format!("MDTM {ts} {path}"), &[suppaftp::Status::File]).await?;
        Ok(())
    }

    async fn close(&self) {
        let _ = self.stream.lock().await.quit().await;
    }
}
