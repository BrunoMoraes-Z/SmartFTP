use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Ftp,
    Ftps,
    Sftp,
}

impl Protocol {
    pub fn default_port(self) -> u16 {
        match self {
            Protocol::Ftp | Protocol::Ftps => 21,
            Protocol::Sftp => 22,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Protocol::Ftp => "FTP",
            Protocol::Ftps => "FTPS",
            Protocol::Sftp => "SFTP",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "ftp" => Some(Protocol::Ftp),
            "ftps" | "ftpes" => Some(Protocol::Ftps),
            "sftp" | "ssh" => Some(Protocol::Sftp),
            _ => None,
        }
    }
}

/// A saved connection profile ("site").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Site {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub group: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Stored locally in the config file. Never returned through MCP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Private key file for SFTP. When set, key auth is tried first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(default)]
    pub remote_dir: String,
    #[serde(default)]
    pub local_dir: String,
    #[serde(default = "default_icon")]
    pub icon: String,
    #[serde(default = "default_color")]
    pub color: String,
    /// Whether AI agents may open this site through MCP.
    #[serde(default = "yes")]
    pub allow_mcp: bool,
}

fn default_icon() -> String {
    "server".into()
}
fn default_color() -> String {
    "#3E9BF0".into()
}
fn yes() -> bool {
    true
}

impl Site {
    pub fn new_empty() -> Self {
        Site {
            id: uuid::Uuid::new_v4().to_string(),
            name: "New site".into(),
            group: String::new(),
            protocol: Protocol::Sftp,
            host: String::new(),
            port: 22,
            user: String::new(),
            password: None,
            key_path: None,
            remote_dir: String::new(),
            local_dir: String::new(),
            icon: default_icon(),
            color: default_color(),
            allow_mcp: true,
        }
    }

    pub fn address(&self) -> String {
        if self.user.is_empty() {
            format!("{}:{}", self.host, self.port)
        } else {
            format!("{}@{}:{}", self.user, self.host, self.port)
        }
    }

    /// Parse `sftp://user:pass@host:port/path` style quick-connect strings.
    pub fn from_url(url: &str) -> Option<Site> {
        let url = url.trim();
        let (proto, rest) = match url.split_once("://") {
            Some((p, r)) => (Protocol::parse(p)?, r),
            None => (Protocol::Sftp, url),
        };
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        let (userinfo, hostport) = match authority.rsplit_once('@') {
            Some((u, h)) => (Some(u), h),
            None => (None, authority),
        };
        let (user, password) = match userinfo {
            Some(u) => match u.split_once(':') {
                Some((u, p)) => (u.to_string(), Some(p.to_string())),
                None => (u.to_string(), None),
            },
            None => (String::new(), None),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().ok()?),
            None => (hostport.to_string(), proto.default_port()),
        };
        if host.is_empty() {
            return None;
        }
        let mut site = Site::new_empty();
        site.name = host.clone();
        site.protocol = proto;
        site.host = host;
        site.port = port;
        site.user = if user.is_empty() && proto != Protocol::Sftp { "anonymous".into() } else { user };
        site.password = password;
        site.remote_dir = path.to_string();
        Some(site)
    }
}

/// One entry of a directory listing (local or remote).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    #[serde(default)]
    pub is_symlink: bool,
    pub size: u64,
    /// Unix timestamp (seconds).
    #[serde(default)]
    pub modified: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "name", rename_all = "lowercase")]
pub enum Origin {
    User,
    Agent(String),
}

impl Origin {
    pub fn label(&self) -> String {
        match self {
            Origin::User => "you".into(),
            Origin::Agent(n) => n.clone(),
        }
    }
    pub fn is_agent(&self) -> bool {
        matches!(self, Origin::Agent(_))
    }
}

pub fn mode_to_string(mode: u32, is_dir: bool, is_link: bool) -> String {
    let mut s = String::with_capacity(10);
    s.push(if is_link { 'l' } else if is_dir { 'd' } else { '-' });
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}

pub fn join_remote(dir: &str, name: &str) -> String {
    if name.starts_with('/') {
        return name.to_string();
    }
    if dir.is_empty() || dir == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", dir.trim_end_matches('/'), name)
    }
}

pub fn parent_remote(path: &str) -> String {
    let p = path.trim_end_matches('/');
    match p.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => p[..i].to_string(),
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{bytes} B") } else if v < 10.0 { format!("{v:.1} {}", UNITS[u]) } else { format!("{v:.0} {}", UNITS[u]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_urls() {
        let s = Site::from_url("sftp://deploy:pw@example.com:2222/var/www").unwrap();
        assert_eq!(s.protocol, Protocol::Sftp);
        assert_eq!(s.user, "deploy");
        assert_eq!(s.password.as_deref(), Some("pw"));
        assert_eq!(s.host, "example.com");
        assert_eq!(s.port, 2222);
        assert_eq!(s.remote_dir, "/var/www");
        let s = Site::from_url("ftp://ftp.example.org").unwrap();
        assert_eq!(s.port, 21);
        assert_eq!(s.user, "anonymous");
        assert!(Site::from_url("sftp://").is_none());
    }

    #[test]
    fn remote_paths() {
        assert_eq!(join_remote("/", "a"), "/a");
        assert_eq!(join_remote("/var/www/", "a"), "/var/www/a");
        assert_eq!(parent_remote("/var/www"), "/var");
        assert_eq!(parent_remote("/var"), "/");
        assert_eq!(mode_to_string(0o755, true, false), "drwxr-xr-x");
        assert_eq!(human_size(1536), "1.5 KB");
    }
}
