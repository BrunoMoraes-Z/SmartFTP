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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExistsAction {
    /// Always replace the target.
    #[default]
    Overwrite,
    /// Replace only when the source is newer than the target.
    OverwriteIfNewer,
    /// Replace only when size or modification time differ.
    OverwriteIfDifferent,
    /// Keep the target, skip the file.
    Skip,
    /// Keep both: write to "name (1).ext".
    Rename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FtpMode {
    #[default]
    Passive,
    ExtendedPassive,
    Active,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnectionSettings {
    /// Seconds before connecting / logging in gives up.
    pub timeout_secs: u64,
    /// Extra attempts when connecting fails (FileZilla: "Maximum number of retries").
    pub retries: u32,
    /// Seconds between failed connection / login attempts.
    pub retry_delay_secs: u64,
    /// Reconnect transparently when an operation finds the connection dropped.
    pub auto_reconnect: bool,
    /// SFTP keep-alive interval in seconds (0 = off).
    pub keepalive_secs: u64,
    pub ftp_mode: FtpMode,
    /// Use the server's control-connection address when PASV returns a private IP.
    pub ftp_nat_workaround: bool,
}

impl Default for ConnectionSettings {
    fn default() -> Self {
        ConnectionSettings {
            timeout_secs: 20,
            retries: 2,
            retry_delay_secs: 5,
            auto_reconnect: true,
            keepalive_secs: 30,
            ftp_mode: FtpMode::Passive,
            ftp_nat_workaround: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TransferSettings {
    pub max_concurrent: u32,
    /// KiB/s, 0 = unlimited.
    pub download_limit_kib: u64,
    /// KiB/s, 0 = unlimited.
    pub upload_limit_kib: u64,
    pub on_exists: ExistsAction,
    pub preserve_timestamps: bool,
    /// Automatic retries for a failed transfer.
    pub retries: u32,
}

impl Default for TransferSettings {
    fn default() -> Self {
        TransferSettings {
            max_concurrent: 3,
            download_limit_kib: 0,
            upload_limit_kib: 0,
            on_exists: ExistsAction::Overwrite,
            preserve_timestamps: true,
            retries: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InterfaceSettings {
    pub show_hidden: bool,
    pub confirm_delete: bool,
    /// Double-clicking a file transfers it to the other side.
    pub double_click_transfers: bool,
    /// Open the transfer drawer when a transfer starts.
    pub open_drawer_on_transfer: bool,
    /// UI language: "auto" (follow the OS), "en", "pt-BR" or "es".
    pub language: String,
    /// Look for a newer GitHub release at startup.
    pub check_updates: bool,
    /// Command used by "View / Edit". `{file}` is replaced by the path; empty = system default app.
    pub editor: String,
}

impl Default for InterfaceSettings {
    fn default() -> Self {
        InterfaceSettings {
            show_hidden: true,
            confirm_delete: true,
            double_click_transfers: true,
            open_drawer_on_transfer: true,
            language: "auto".into(),
            check_updates: true,
            editor: String::new(),
        }
    }
}

/// One entry of the connection rail: a site, or a folder of sites.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RailNode {
    Site {
        id: String,
    },
    Folder {
        id: String,
        name: String,
        color: String,
        sites: Vec<String>,
        #[serde(default)]
        expanded: bool,
    },
}

impl RailNode {
    pub fn id(&self) -> &str {
        match self {
            RailNode::Site { id } | RailNode::Folder { id, .. } => id,
        }
    }
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
    #[serde(default)]
    pub connection: ConnectionSettings,
    #[serde(default)]
    pub transfers: TransferSettings,
    #[serde(default)]
    pub interface: InterfaceSettings,
    /// Order of sites and folders in the rail.
    #[serde(default)]
    pub rail: Vec<RailNode>,
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
            connection: ConnectionSettings::default(),
            transfers: TransferSettings::default(),
            interface: InterfaceSettings::default(),
            rail: vec![],
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
        let mut cfg = cfg;
        cfg.sync_rail();
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
        self.sites.iter().find(|s| s.id == key).or_else(|| self.sites.iter().find(|s| s.name.eq_ignore_ascii_case(key)))
    }

    pub fn upsert_site(&mut self, site: Site) {
        let regroup = match self.sites.iter_mut().find(|s| s.id == site.id) {
            Some(s) => {
                let changed = !s.group.eq_ignore_ascii_case(&site.group);
                *s = site.clone();
                changed
            }
            None => {
                self.sites.push(site.clone());
                false
            }
        };
        if regroup {
            // the group was edited in the site form: move the site to that folder
            self.take_node(&site.id);
        }
        self.sync_rail();
    }

    pub fn remove_site(&mut self, id: &str) {
        self.sites.retain(|s| s.id != id);
        self.sync_rail();
    }

    /// Keep the rail consistent with the site list: drop unknown ids, add new sites
    /// (into the folder named by their `group`, creating it if needed), remove empty
    /// folders and mirror folder names into `Site::group`.
    pub fn sync_rail(&mut self) {
        use std::collections::{HashMap, HashSet};
        let ids: HashSet<String> = self.sites.iter().map(|s| s.id.clone()).collect();
        for node in &mut self.rail {
            if let RailNode::Folder { sites, .. } = node {
                sites.retain(|id| ids.contains(id));
            }
        }
        self.rail.retain(|n| match n {
            RailNode::Site { id } => ids.contains(id),
            RailNode::Folder { sites, .. } => !sites.is_empty(),
        });
        let placed: HashSet<String> = self
            .rail
            .iter()
            .flat_map(|n| match n {
                RailNode::Site { id } => vec![id.clone()],
                RailNode::Folder { sites, .. } => sites.clone(),
            })
            .collect();
        for site in self.sites.clone() {
            if placed.contains(&site.id) {
                continue;
            }
            let group = site.group.trim().to_string();
            if group.is_empty() {
                self.rail.push(RailNode::Site { id: site.id.clone() });
                continue;
            }
            let existing = self.rail.iter_mut().find(|n| matches!(n, RailNode::Folder { name, .. } if name.eq_ignore_ascii_case(&group)));
            match existing {
                Some(RailNode::Folder { sites, .. }) => sites.push(site.id.clone()),
                _ => self.rail.push(RailNode::Folder {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: group,
                    color: "#3E9BF0".into(),
                    sites: vec![site.id.clone()],
                    expanded: false,
                }),
            }
        }
        // the folder a site lives in is its group
        let mut group_of: HashMap<String, String> = HashMap::new();
        for n in &self.rail {
            if let RailNode::Folder { name, sites, .. } = n {
                for id in sites {
                    group_of.insert(id.clone(), name.clone());
                }
            }
        }
        for s in &mut self.sites {
            s.group = group_of.get(&s.id).cloned().unwrap_or_default();
        }
    }

    /// Take a node (site or folder) out of the rail, wherever it is.
    fn take_node(&mut self, id: &str) -> Option<RailNode> {
        if let Some(i) = self.rail.iter().position(|n| n.id() == id) {
            return Some(self.rail.remove(i));
        }
        for n in &mut self.rail {
            if let RailNode::Folder { sites, .. } = n
                && let Some(i) = sites.iter().position(|s| s == id)
            {
                sites.remove(i);
                return Some(RailNode::Site { id: id.to_string() });
            }
        }
        None
    }

    /// Move a node before position `index` at the top level, or inside `folder` when given.
    pub fn move_node(&mut self, id: &str, folder: Option<&str>, index: usize) {
        if Some(id) == folder {
            return;
        }
        // remember the target by id: removing the node may shift indices
        let anchor = match folder {
            Some(f) => self.rail.iter().find_map(|n| match n {
                RailNode::Folder { id: fid, sites, .. } if fid == f => sites.get(index).cloned(),
                _ => None,
            }),
            None => self.rail.get(index).map(|n| n.id().to_string()),
        };
        if anchor.as_deref() == Some(id) {
            return;
        }
        let Some(node) = self.take_node(id) else { return };
        match (folder, node) {
            (Some(f), RailNode::Site { id }) => {
                if let Some(RailNode::Folder { sites, .. }) = self.rail.iter_mut().find(|n| n.id() == f) {
                    let at = anchor.and_then(|a| sites.iter().position(|s| *s == a)).unwrap_or(sites.len());
                    sites.insert(at, id);
                }
            }
            (Some(_), folder_node) => {
                // folders cannot be nested: drop it at the end of the rail
                self.rail.push(folder_node);
            }
            (None, node) => {
                let at = anchor.and_then(|a| self.rail.iter().position(|n| n.id() == a)).unwrap_or(self.rail.len());
                self.rail.insert(at, node);
            }
        }
        self.sync_rail();
    }

    /// Drop site `dragged` onto top-level site `target`: make a new folder with both.
    pub fn merge_into_folder(&mut self, dragged: &str, target: &str) -> Option<String> {
        if dragged == target || !self.rail.iter().any(|n| matches!(n, RailNode::Site { id } if id == target)) {
            return None;
        }
        self.take_node(dragged)?;
        let pos = self.rail.iter().position(|n| n.id() == target)?;
        let fid = uuid::Uuid::new_v4().to_string();
        let mut n = 1;
        let mut name = "New folder".to_string();
        while self.rail.iter().any(|x| matches!(x, RailNode::Folder { name: nm, .. } if *nm == name)) {
            n += 1;
            name = format!("New folder {n}");
        }
        self.rail[pos] = RailNode::Folder {
            id: fid.clone(),
            name,
            color: "#3E9BF0".into(),
            sites: vec![target.to_string(), dragged.to_string()],
            expanded: true,
        };
        self.sync_rail();
        Some(fid)
    }

    pub fn folder_mut(&mut self, folder_id: &str) -> Option<(&mut String, &mut String, &mut bool)> {
        self.rail.iter_mut().find_map(|n| match n {
            RailNode::Folder { id, name, color, expanded, .. } if id == folder_id => Some((name, color, expanded)),
            _ => None,
        })
    }

    /// Dissolve a folder, putting its sites back where it was.
    pub fn ungroup(&mut self, folder_id: &str) {
        if let Some(pos) = self.rail.iter().position(|n| n.id() == folder_id)
            && let RailNode::Folder { sites, .. } = self.rail.remove(pos)
        {
            for (i, id) in sites.into_iter().enumerate() {
                self.rail.insert(pos + i, RailNode::Site { id });
            }
        }
        self.sync_rail();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(name: &str, group: &str) -> Site {
        let mut s = Site::new_empty();
        s.name = name.into();
        s.group = group.into();
        s
    }

    #[test]
    fn rail_layout() {
        let mut c = Config { sites: vec![site("a", ""), site("b", "Prod"), site("c", "Prod"), site("d", "")], ..Default::default() };
        c.sync_rail();
        assert_eq!(c.rail.len(), 3); // a, Prod{b,c}, d
        let ids: Vec<String> = c.sites.iter().map(|s| s.id.clone()).collect();
        // drop d onto a: new folder
        let f = c.merge_into_folder(&ids[3], &ids[0]).unwrap();
        assert_eq!(c.rail.len(), 2);
        assert_eq!(c.sites[3].group, "New folder");
        // move b out of Prod to the top
        c.move_node(&ids[1], None, 0);
        assert_eq!(c.rail[0], RailNode::Site { id: ids[1].clone() });
        assert_eq!(c.sites[1].group, "");
        // move c into the new folder at the start; Prod becomes empty and disappears
        c.move_node(&ids[2], Some(&f), 0);
        assert!(!c.rail.iter().any(|n| matches!(n, RailNode::Folder { name, .. } if name == "Prod")));
        match c.rail.iter().find(|n| n.id() == f).unwrap() {
            RailNode::Folder { sites, .. } => assert_eq!(sites[0], ids[2]),
            _ => unreachable!(),
        }
        // editing the group in the site form moves it
        let mut b = c.sites[1].clone();
        b.group = "Staging".into();
        c.upsert_site(b);
        assert!(
            c.rail.iter().any(|n| matches!(n, RailNode::Folder { name, sites, .. } if name == "Staging" && sites == &vec![ids[1].clone()]))
        );
        c.ungroup(&f);
        assert!(c.sites.iter().filter(|s| s.group.is_empty()).count() == 3);
    }
}
