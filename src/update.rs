//! Self-update from GitHub Releases.
//!
//! The release workflow publishes, per release:
//!   `smartftp-windows-x86_64.exe`         the bare executable (used by the updater)
//!   `smartftp-windows-x86_64.exe.sha256`  its SHA-256 (hex)
//!   `SmartFTP-<version>-windows-x86_64.zip` for people downloading by hand
//! The updater only installs a binary whose SHA-256 matches the published checksum.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context as _, Result, anyhow, bail};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const REPO: &str = "BrunoMoraes-Z/SmartFTP";
const USER_AGENT: &str = concat!("SmartFTP-updater/", env!("CARGO_PKG_VERSION"));

#[cfg(target_os = "windows")]
pub const ASSET: &str = "smartftp-windows-x86_64.exe";
#[cfg(target_os = "macos")]
pub const ASSET: &str = "smartftp-macos-universal";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const ASSET: &str = "smartftp-linux-x86_64";

#[derive(Debug, Clone)]
pub struct Release {
    pub version: Version,
    pub page_url: String,
    pub asset_url: Option<String>,
    pub asset_size: u64,
    pub checksum_url: Option<String>,
    /// The Windows installer, used when the app is installed where we cannot write (Program Files).
    pub setup_url: Option<String>,
    pub setup_size: u64,
    pub setup_checksum_url: Option<String>,
}

impl Release {
    /// Bytes the update will download, for the progress display.
    pub fn download_size(&self) -> u64 {
        if exe_dir_writable() { self.asset_size } else { self.setup_size }
    }
}

/// What `install` did.
pub enum Installed {
    /// The running executable was replaced; restart it with [`restart`].
    Replaced,
    /// The installer was started (it asks for admin rights and restarts SmartFTP); quit now.
    InstallerLaunched,
}

/// Can we replace files next to the running executable? False for an all-users install in
/// Program Files, which needs the elevated installer instead.
pub fn exe_dir_writable() -> bool {
    let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) else { return false };
    let probe = dir.join(format!(".smartftp-write-test-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is semver")
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(30))).build().into()
}

fn parse_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.trim_start_matches(['v', 'V'])).ok()
}

/// Ask GitHub for the latest published release. `Ok(None)` when we are up to date.
pub fn check() -> Result<Option<Release>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut resp = match agent().get(&url).header("User-Agent", USER_AGENT).header("Accept", "application/vnd.github+json").call() {
        Ok(r) => r,
        // no release published yet: nothing to update to
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => bail!("checking for updates failed: {e}"),
    };
    let rel: GhRelease = resp.body_mut().read_json().context("reading release information")?;
    if rel.draft || rel.prerelease {
        return Ok(None);
    }
    let Some(version) = parse_tag(&rel.tag_name) else { bail!("release tag '{}' is not a version", rel.tag_name) };
    if version <= current_version() {
        return Ok(None);
    }
    let asset = rel.assets.iter().find(|a| a.name == ASSET);
    let checksum = rel.assets.iter().find(|a| a.name == format!("{ASSET}.sha256"));
    let setup_name = format!("SmartFTP-{version}-setup.exe");
    let setup = rel.assets.iter().find(|a| a.name == setup_name);
    let setup_checksum = rel.assets.iter().find(|a| a.name == format!("{setup_name}.sha256"));
    Ok(Some(Release {
        version,
        page_url: rel.html_url,
        asset_url: asset.map(|a| a.browser_download_url.clone()),
        asset_size: asset.map(|a| a.size).unwrap_or(0),
        checksum_url: checksum.map(|a| a.browser_download_url.clone()),
        setup_url: setup.map(|a| a.browser_download_url.clone()),
        setup_size: setup.map(|a| a.size).unwrap_or(0),
        setup_checksum_url: setup_checksum.map(|a| a.browser_download_url.clone()),
    }))
}

/// Download a file and check it against the SHA-256 published next to it.
fn download_verified(url: &str, checksum_url: &str, dest: &Path, progress: &AtomicU64) -> Result<()> {
    let expected = agent()
        .get(checksum_url)
        .header("User-Agent", USER_AGENT)
        .call()?
        .body_mut()
        .read_to_string()?
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    if expected.len() != 64 {
        bail!("invalid checksum file");
    }
    let mut resp = agent().get(url).header("User-Agent", USER_AGENT).call()?;
    let mut reader = resp.body_mut().as_reader();
    let mut file = std::fs::File::create(dest).with_context(|| format!("creating {}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        progress.fetch_add(n as u64, Ordering::Relaxed);
    }
    file.sync_all()?;
    drop(file);
    if format!("{:x}", hasher.finalize()) != expected {
        let _ = std::fs::remove_file(dest);
        bail!("downloaded file is corrupt (checksum mismatch)");
    }
    Ok(())
}

/// Install `release`. Portable and per-user installs swap the executable in place; an all-users
/// install (Program Files) runs the verified installer, which asks Windows for admin rights.
pub fn install(release: &Release, progress: &AtomicU64) -> Result<Installed> {
    if !exe_dir_writable() {
        return run_installer(release, progress);
    }
    let asset_url =
        release.asset_url.as_ref().ok_or_else(|| anyhow!("this release has no {ASSET} asset; download it from {}", release.page_url))?;
    let checksum_url =
        release.checksum_url.as_ref().ok_or_else(|| anyhow!("this release has no checksum; refusing to install it automatically"))?;
    let tmp: PathBuf = std::env::temp_dir().join(format!("smartftp-update-{}", release.version));
    download_verified(asset_url, checksum_url, &tmp, progress)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    self_replace::self_replace(&tmp).context("replacing the executable")?;
    let _ = std::fs::remove_file(&tmp);
    Ok(Installed::Replaced)
}

fn run_installer(release: &Release, progress: &AtomicU64) -> Result<Installed> {
    if !cfg!(windows) {
        bail!("SmartFTP cannot write to its own folder; update it with your package manager or download it from {}", release.page_url);
    }
    let (Some(url), Some(checksum)) = (release.setup_url.as_ref(), release.setup_checksum_url.as_ref()) else {
        bail!("SmartFTP is installed for all users and this release has no verified installer; download it from {}", release.page_url);
    };
    let setup = std::env::temp_dir().join(format!("SmartFTP-{}-setup.exe", release.version));
    download_verified(url, checksum, &setup, progress)?;
    // /ALLUSERS keeps the existing Program Files install (Windows shows the admin prompt);
    // /UPDATE=1 makes the installer start SmartFTP again when it is done.
    std::process::Command::new(&setup)
        .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/ALLUSERS", "/UPDATE=1"])
        .spawn()
        .context("starting the installer")?;
    Ok(Installed::InstallerLaunched)
}

/// Start the (new) executable and let the caller quit the current process.
pub fn restart() -> Result<()> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe).spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        assert_eq!(parse_tag("v1.2.3"), Some(Version::new(1, 2, 3)));
        assert_eq!(parse_tag("0.4.0"), Some(Version::new(0, 4, 0)));
        assert!(parse_tag("nightly").is_none());
        assert!(parse_tag("v9.9.9").unwrap() > current_version());
    }
}
