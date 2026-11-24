use std::path::PathBuf;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use super::model::Site;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Bearer token required by the HTTP endpoint.
    #[serde(default = "new_token")]
    pub token: String,
    /// Tool names the user switched off.
    #[serde(default)]
    pub disabled_tools: Vec<String>,
}

impl Default for McpConfig {
    fn default() -> Self {
        McpConfig { enabled: true, port: default_port(), token: new_token(), disabled_tools: vec![] }
    }
}

fn yes() -> bool {
    true
}
fn default_port() -> u16 {
    7331
}
pub fn new_token() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let s: String = (0..32).map(|_| format!("{:x}", rng.random_range(0..16u8))).collect();
    format!("sftp-{s}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub theme_mode: ThemeMode,
    #[serde(default = "default_dark")]
    pub dark_theme: String,
    #[serde(default = "default_light")]
    pub light_theme: String,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub sites: Vec<Site>,
    #[serde(default)]
    pub last_local_dir: Option<String>,
}

fn default_dark() -> String {
    "graphite-dark".into()
}
fn default_light() -> String {
    "graphite-light".into()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme_mode: ThemeMode::System,
            dark_theme: default_dark(),
            light_theme: default_light(),
            sidebar_collapsed: false,
            mcp: McpConfig::default(),
            sites: vec![],
            last_local_dir: None,
        }
    }
}

pub fn app_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SMARTFTP_HOME") {
        return PathBuf::from(dir);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("SmartFTP")
}

pub fn config_path() -> PathBuf {
    app_dir().join("config.json")
}

pub fn themes_dir() -> PathBuf {
    app_dir().join("themes")
}

impl Config {
    pub fn load() -> Config {
        let path = config_path();
        let cfg = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::error!("config {} is invalid ({e}); starting with defaults", path.display());
                let backup = path.with_extension("json.bak");
                let _ = std::fs::copy(&path, backup);
                Config::default()
            }),
            Err(_) => Config::default(),
        };
        // persist generated defaults such as the MCP token
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn find_site(&self, key: &str) -> Option<&Site> {
        self.sites
            .iter()
            .find(|s| s.id == key)
            .or_else(|| self.sites.iter().find(|s| s.name.eq_ignore_ascii_case(key)))
    }

    pub fn upsert_site(&mut self, site: Site) {
        match self.sites.iter_mut().find(|s| s.id == site.id) {
            Some(s) => *s = site,
            None => self.sites.push(site),
        }
    }
}
