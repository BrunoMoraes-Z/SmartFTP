use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use async_trait::async_trait;
use russh::client::{self, Handle};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, load_secret_key};
use russh::Disconnect;
use russh::keys::PublicKeyOrCertificate;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::config::app_dir;
use super::model::{Entry, Site, join_remote, mode_to_string};
use super::remote::{CHUNK, Progress, RemoteFs, cancelled_err, sort_entries};

/// Trust-on-first-use store for server host keys: "host:port" -> SHA256 fingerprint.
fn known_hosts_path() -> std::path::PathBuf {
    app_dir().join("known_hosts.json")
}

fn load_known_hosts() -> HashMap<String, String> {
    std::fs::read_to_string(known_hosts_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_known_host(key: &str, fp: &str) {
    let mut all = load_known_hosts();
    all.insert(key.to_string(), fp.to_string());
    let _ = std::fs::create_dir_all(app_dir());
    let _ = std::fs::write(known_hosts_path(), serde_json::to_vec_pretty(&all).unwrap_or_default());
}

struct Client {
    host_key: String,
}

impl client::Handler for Client {
    type Error = anyhow::Error;

    async fn check_server_key(&mut self, server_public_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let fp = match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.fingerprint(HashAlg::Sha256).to_string(),
            PublicKeyOrCertificate::Certificate(cert) => cert.public_key().fingerprint(HashAlg::Sha256).to_string(),
        };
        let known = load_known_hosts();
        match known.get(&self.host_key) {
            Some(expected) if expected != &fp => Err(anyhow!(
                "HOST KEY CHANGED for {}: expected {expected}, got {fp}. Remove the entry from {} if this is expected.",
                self.host_key,
                known_hosts_path().display()
            )),
            Some(_) => Ok(true),
            None => {
                tracing::info!("trusting new host key for {}: {fp}", self.host_key);
                save_known_host(&self.host_key, &fp);
                Ok(true)
            }
        }
    }
}

pub struct SftpFs {
    handle: Handle<Client>,
    sftp: SftpSession,
}

impl SftpFs {
    pub async fn connect(site: &Site) -> Result<Self> {
        let config = Arc::new(client::Config {
            inactivity_timeout: None,
            keepalive_interval: Some(Duration::from_secs(30)),
            keepalive_max: 4,
            ..Default::default()
        });
        let handler = Client { host_key: format!("{}:{}", site.host, site.port) };
        let mut handle = tokio::time::timeout(
            Duration::from_secs(20),
            client::connect(config, (site.host.as_str(), site.port), handler),
        )
        .await
        .map_err(|_| anyhow!("timed out connecting to {}:{}", site.host, site.port))??;

        let mut authenticated = false;
        if let Some(key_path) = site.key_path.as_deref().filter(|p| !p.is_empty()) {
            let key_path = expand_tilde(key_path);
            let key = load_secret_key(&key_path, site.password.as_deref())
                .with_context(|| format!("loading private key {key_path}"))?;
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            let res = handle
                .authenticate_publickey(site.user.clone(), PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await?;
            authenticated = res.success();
        }
        if !authenticated && let Some(pw) = site.password.as_deref() {
            authenticated = handle.authenticate_password(site.user.clone(), pw.to_string()).await?.success();
        }
        if !authenticated {
            bail!("authentication failed for {}", site.address());
        }

        let channel = handle.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let sftp = SftpSession::new(channel.into_stream()).await.context("starting SFTP subsystem")?;
        Ok(SftpFs { handle, sftp })
    }
}

pub fn expand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~")
        && let Some(home) = dirs::home_dir()
    {
        return format!("{}{}", home.display(), rest);
    }
    p.to_string()
}

fn entry_from(dir: &str, name: String, attrs: &FileAttributes) -> Entry {
    let is_dir = attrs.is_dir();
    let is_link = attrs.is_symlink();
    Entry {
        path: join_remote(dir, &name),
        name,
        is_dir,
        is_symlink: is_link,
        size: attrs.size.unwrap_or(0),
        modified: attrs.mtime.map(|t| t as i64),
        permissions: attrs.permissions.map(|p| mode_to_string(p, is_dir, is_link)),
    }
}

#[async_trait]
impl RemoteFs for SftpFs {
    async fn home(&self) -> Result<String> {
        Ok(self.sftp.canonicalize(".".to_string()).await?.to_string())
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        let mut out = Vec::new();
        for item in self.sftp.read_dir(path.to_string()).await? {
            let name = item.file_name().to_string();
            if name == "." || name == ".." {
                continue;
            }
            let mut entry = entry_from(path, name, &item.metadata());
            // resolve symlinks to directories so they can be opened
            if entry.is_symlink
                && let Ok(target) = self.sftp.metadata(entry.path.clone()).await
            {
                entry.is_dir = target.is_dir();
            }
            out.push(entry);
        }
        sort_entries(&mut out);
        Ok(out)
    }

    async fn is_dir(&self, path: &str) -> Result<bool> {
        Ok(self.sftp.metadata(path.to_string()).await?.is_dir())
    }

    async fn mkdir(&self, path: &str) -> Result<()> {
        Ok(self.sftp.create_dir(path.to_string()).await?)
    }

    async fn remove_file(&self, path: &str) -> Result<()> {
        Ok(self.sftp.remove_file(path.to_string()).await?)
    }

    async fn remove_dir(&self, path: &str) -> Result<()> {
        Ok(self.sftp.remove_dir(path.to_string()).await?)
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        Ok(self.sftp.rename(from.to_string(), to.to_string()).await?)
    }

    async fn chmod(&self, path: &str, mode: u32) -> Result<()> {
        let mut attrs = FileAttributes::empty();
        attrs.permissions = Some(mode);
        Ok(self.sftp.set_metadata(path.to_string(), attrs).await?)
    }

    async fn download(&self, remote: &str, local: &Path, progress: &Progress) -> Result<u64> {
        let mut src = self.sftp.open(remote.to_string()).await?;
        let mut dst = tokio::fs::File::create(local).await?;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            if progress.cancelled() {
                drop(dst);
                let _ = tokio::fs::remove_file(local).await;
                return Err(cancelled_err());
            }
            let n = src.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).await?;
            total += n as u64;
            progress.add(n as u64);
        }
        dst.flush().await?;
        let _ = src.shutdown().await;
        Ok(total)
    }

    async fn upload(&self, local: &Path, remote: &str, progress: &Progress) -> Result<u64> {
        let mut src = tokio::fs::File::open(local).await?;
        let mut dst = self
            .sftp
            .open_with_flags(remote.to_string(), OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE)
            .await?;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            if progress.cancelled() {
                let _ = dst.shutdown().await;
                return Err(cancelled_err());
            }
            let n = src.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).await?;
            total += n as u64;
            progress.add(n as u64);
        }
        dst.shutdown().await?;
        Ok(total)
    }

    async fn read(&self, remote: &str, max: usize) -> Result<Vec<u8>> {
        let mut f = self.sftp.open(remote.to_string()).await?;
        let mut out = Vec::new();
        let mut buf = vec![0u8; CHUNK];
        while out.len() < max {
            let n = f.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n.min(max - out.len())]);
        }
        let _ = f.shutdown().await;
        Ok(out)
    }

    async fn write(&self, remote: &str, data: &[u8]) -> Result<()> {
        let mut f = self
            .sftp
            .open_with_flags(remote.to_string(), OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE)
            .await?;
        f.write_all(data).await?;
        f.shutdown().await?;
        Ok(())
    }

    async fn set_mtime(&self, path: &str, mtime: i64) -> Result<()> {
        let mut attrs = FileAttributes::empty();
        attrs.atime = Some(mtime as u32);
        attrs.mtime = Some(mtime as u32);
        Ok(self.sftp.set_metadata(path.to_string(), attrs).await?)
    }

    async fn close(&self) {
        let _ = self.sftp.close().await;
        let _ = self.handle.disconnect(Disconnect::ByApplication, "bye", "en").await;
    }
}
