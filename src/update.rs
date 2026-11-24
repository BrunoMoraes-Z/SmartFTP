//! Self-update from GitHub Releases.
//!
//! The release workflow publishes, per release:
//!   `smartftp-windows-x86_64.exe`         the bare executable (used by the updater)
//!   `smartftp-windows-x86_64.exe.sha256`  its SHA-256 (hex)
//!   `SmartFTP-<version>-windows-x86_64.zip` for people downloading by hand
//! The updater only installs a binary whose SHA-256 matches the published checksum.

use std::io::{Read, Write};
use std::path::PathBuf;
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
    Ok(Some(Release {
        version,
        page_url: rel.html_url,
        asset_url: asset.map(|a| a.browser_download_url.clone()),
        asset_size: asset.map(|a| a.size).unwrap_or(0),
        checksum_url: checksum.map(|a| a.browser_download_url.clone()),
    }))
}

/// Download the new executable, verify it and replace the running one.
/// `progress` receives downloaded bytes. Restart the app afterwards with [`restart`].
pub fn install(release: &Release, progress: &AtomicU64) -> Result<()> {
    let asset_url =
        release.asset_url.as_ref().ok_or_else(|| anyhow!("this release has no {ASSET} asset; download it from {}", release.page_url))?;
    let checksum_url =
        release.checksum_url.as_ref().ok_or_else(|| anyhow!("this release has no checksum; refusing to install it automatically"))?;

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

    let tmp: PathBuf = std::env::temp_dir().join(format!("smartftp-update-{}", release.version));
    let mut resp = agent().get(asset_url).header("User-Agent", USER_AGENT).call()?;
    let mut reader = resp.body_mut().as_reader();
    let mut file = std::fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
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

    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        let _ = std::fs::remove_file(&tmp);
        bail!("downloaded file is corrupt (checksum mismatch)");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    self_replace::self_replace(&tmp).context("replacing the executable")?;
    let _ = std::fs::remove_file(&tmp);
    Ok(())
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
