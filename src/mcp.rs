//! MCP server exposing every file operation of the app to AI agents.
//!
//! Served over Streamable HTTP by the desktop app (shares live sessions with the UI)
//! and over stdio with `smartftp --mcp-stdio` (headless, own sessions).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{Peer, RoleServer, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::core::model::{Origin, Site};
use crate::core::{Core, remote};

pub const TOOLS: &[(&str, &str)] = &[
    ("list_sites", "Saved connection profiles"),
    ("list_sessions", "Currently open sessions"),
    ("connect", "Open a session to a saved site or URL"),
    ("disconnect", "Close an open session"),
    ("list_dir", "List a remote directory"),
    ("change_dir", "Change the session's working directory"),
    ("make_dir", "Create a remote directory"),
    ("delete", "Delete remote files or folders"),
    ("rename", "Rename / move a remote entry"),
    ("chmod", "Change remote permissions"),
    ("read_file", "Read a remote text file"),
    ("write_file", "Create or overwrite a remote file"),
    ("upload", "Upload local files/folders"),
    ("download", "Download remote files/folders"),
    ("list_transfers", "Transfer queue status"),
    ("cancel_transfer", "Cancel a queued or running transfer"),
    ("list_local_dir", "List a directory on this computer"),
    ("process_queue", "Start transfers waiting in the queue"),
];

#[derive(Clone)]
pub struct SmartFtpMcp {
    core: Arc<Core>,
}

impl SmartFtpMcp {
    pub fn new(core: Arc<Core>) -> Self {
        SmartFtpMcp { core }
    }

    fn guard(&self, tool: &str) -> Result<(), String> {
        if !self.core.config.lock().mcp.enabled {
            return Err("the SmartFTP MCP server is turned off (Settings → MCP & Agents)".into());
        }
        if self.core.config.lock().mcp.disabled_tools.iter().any(|t| t == tool) {
            return Err(format!("the tool '{tool}' is disabled in SmartFTP settings (Settings → MCP & Agents)"));
        }
        Ok(())
    }

    fn origin(peer: &Peer<RoleServer>) -> Origin {
        let name = peer.peer_info().map(|i| i.client_info.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "agent".into());
        Origin::Agent(name)
    }

    fn session(&self, key: &str) -> Result<Arc<crate::core::Session>, String> {
        self.core.session(key).map_err(|e| e.to_string())
    }
}

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

fn to_json(v: serde_json::Value) -> Result<String, String> {
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConnectArgs {
    /// Name or id of a saved site (see list_sites).
    #[serde(default)]
    pub site: Option<String>,
    /// Alternatively a URL such as sftp://user@host:22/path or ftp://host
    #[serde(default)]
    pub url: Option<String>,
    /// Password for `url` connections (saved sites use their stored credentials).
    #[serde(default)]
    pub password: Option<String>,
    /// Save a `url` connection as a new site.
    #[serde(default)]
    pub save_as: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionArg {
    /// Session id (e.g. "s3") or the site name of an open session.
    pub session: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PathArgs {
    /// Session id (e.g. "s3") or the site name of an open session.
    pub session: String,
    /// Absolute path, or relative to the session's working directory. Empty = working directory.
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteArgs {
    pub session: String,
    pub path: String,
    /// Required to delete non-empty directories.
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RenameArgs {
    pub session: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ChmodArgs {
    pub session: String,
    pub path: String,
    /// Octal mode, e.g. "644" or "755".
    pub mode: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadArgs {
    pub session: String,
    pub path: String,
    /// Maximum bytes to return (default 200000).
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteArgs {
    pub session: String,
    pub path: String,
    /// UTF-8 text content.
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UploadArgs {
    pub session: String,
    /// Local files or folders (absolute paths). Folders are uploaded recursively.
    pub local_paths: Vec<String>,
    /// Remote target directory. Empty = session working directory.
    #[serde(default)]
    pub remote_dir: String,
    /// Wait for the transfers to finish (default true, max 10 minutes).
    #[serde(default)]
    pub wait: Option<bool>,
    /// Only add the transfers to the queue (state "pending") without starting them.
    /// The user, or `process_queue`, starts them later.
    #[serde(default)]
    pub queue_only: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DownloadArgs {
    pub session: String,
    /// Remote files or folders. Folders are downloaded recursively.
    pub remote_paths: Vec<String>,
    /// Local target directory (absolute). Created if missing.
    pub local_dir: String,
    /// Wait for the transfers to finish (default true, max 10 minutes).
    #[serde(default)]
    pub wait: Option<bool>,
    /// Only add the transfers to the queue (state "pending") without starting them.
    /// The user, or `process_queue`, starts them later.
    #[serde(default)]
    pub queue_only: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProcessQueueArgs {
    /// Transfer ids to start (see list_transfers). Empty = every pending transfer.
    #[serde(default)]
    pub ids: Vec<u64>,
    /// Wait for the started transfers to finish (default false, max 10 minutes).
    #[serde(default)]
    pub wait: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct JobArg {
    pub id: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LocalArgs {
    /// Absolute local path. Empty = the user's home directory.
    #[serde(default)]
    pub path: String,
}

#[tool_router]
impl SmartFtpMcp {
    #[tool(description = "List saved connection profiles (sites). Credentials are never returned.")]
    async fn list_sites(&self) -> Result<String, String> {
        self.guard("list_sites")?;
        let cfg = self.core.config.lock();
        let sites: Vec<_> = cfg
            .sites
            .iter()
            .map(|s| {
                json!({
                    "id": s.id, "name": s.name, "group": s.group, "protocol": s.protocol.label(),
                    "host": s.host, "port": s.port, "user": s.user, "remote_dir": s.remote_dir,
                    "agent_access": s.allow_mcp,
                })
            })
            .collect();
        to_json(json!({ "sites": sites }))
    }

    #[tool(description = "List currently open sessions (both opened by the user in the UI and by agents).")]
    async fn list_sessions(&self) -> Result<String, String> {
        self.guard("list_sessions")?;
        to_json(json!({ "sessions": self.core.session_infos() }))
    }

    #[tool(
        description = "Open a new session. Pass `site` (name or id of a saved site) or `url` (sftp://user@host:port/path, ftp://..., ftps://...). Returns the session id used by the other tools. The session also appears in the SmartFTP window."
    )]
    async fn connect(&self, peer: Peer<RoleServer>, Parameters(args): Parameters<ConnectArgs>) -> Result<String, String> {
        self.guard("connect")?;
        let site = if let Some(key) = args.site.as_deref().filter(|s| !s.is_empty()) {
            let site = self.core.config.lock().find_site(key).cloned().ok_or_else(|| format!("no saved site '{key}'; call list_sites"))?;
            if !site.allow_mcp {
                return Err(format!("site '{}' does not allow agent access (enable it in the site settings)", site.name));
            }
            site
        } else if let Some(url) = args.url.as_deref() {
            let mut site = Site::from_url(url).ok_or("invalid url; expected e.g. sftp://user@host:22/path")?;
            if args.password.is_some() {
                site.password = args.password.clone();
            }
            if let Some(name) = args.save_as.filter(|n| !n.is_empty()) {
                site.name = name;
                self.core.update_config(|c| c.sites.push(site.clone()));
            }
            site
        } else {
            return Err("pass either `site` or `url`".into());
        };
        let session = self.core.connect(site, Self::origin(&peer)).await.map_err(err)?;
        to_json(json!({
            "session": session.id, "site": session.site.name, "protocol": session.site.protocol.label(),
            "cwd": session.cwd.lock().clone(), "home": session.home,
        }))
    }

    #[tool(description = "Close an open session.")]
    async fn disconnect(&self, peer: Peer<RoleServer>, Parameters(args): Parameters<SessionArg>) -> Result<String, String> {
        self.guard("disconnect")?;
        self.core.disconnect(&args.session, &Self::origin(&peer)).await.map_err(err)?;
        Ok("disconnected".into())
    }

    #[tool(
        description = "List a remote directory. Returns entries with name, path, is_dir, size (bytes), modified (unix seconds) and permissions."
    )]
    async fn list_dir(&self, peer: Peer<RoleServer>, Parameters(args): Parameters<PathArgs>) -> Result<String, String> {
        self.guard("list_dir")?;
        let s = self.session(&args.session)?;
        let path = s.resolve(&args.path);
        let r = self.core.list(&s, &path).await;
        self.core.record(&Self::origin(&peer), "list_dir", &format!("{}:{path}", s.site.name), &r);
        to_json(json!({ "path": path, "entries": r.map_err(err)? }))
    }

    #[tool(description = "Change the working directory of a session (relative paths in other tools resolve against it).")]
    async fn change_dir(&self, Parameters(args): Parameters<PathArgs>) -> Result<String, String> {
        self.guard("change_dir")?;
        let s = self.session(&args.session)?;
        let cwd = self.core.change_dir(&s, &args.path).await.map_err(err)?;
        to_json(json!({ "cwd": cwd }))
    }

    #[tool(description = "Create a remote directory.")]
    async fn make_dir(&self, peer: Peer<RoleServer>, Parameters(args): Parameters<PathArgs>) -> Result<String, String> {
        self.guard("make_dir")?;
        let s = self.session(&args.session)?;
        let p = self.core.mkdir(&s, &args.path, &Self::origin(&peer)).await.map_err(err)?;
        to_json(json!({ "created": p }))
    }

    #[tool(description = "Delete a remote file or directory. Non-empty directories need recursive=true.")]
    async fn delete(&self, peer: Peer<RoleServer>, Parameters(args): Parameters<DeleteArgs>) -> Result<String, String> {
        self.guard("delete")?;
        let s = self.session(&args.session)?;
        let n = self.core.delete(&s, &args.path, args.recursive, &Self::origin(&peer)).await.map_err(err)?;
        to_json(json!({ "deleted_entries": n }))
    }

    #[tool(description = "Rename or move a remote file or directory.")]
    async fn rename(&self, peer: Peer<RoleServer>, Parameters(args): Parameters<RenameArgs>) -> Result<String, String> {
        self.guard("rename")?;
        let s = self.session(&args.session)?;
        self.core.rename(&s, &args.from, &args.to, &Self::origin(&peer)).await.map_err(err)?;
        Ok("renamed".into())
    }

    #[tool(description = "Change permissions of a remote entry (octal mode, e.g. \"755\").")]
    async fn chmod(&self, peer: Peer<RoleServer>, Parameters(args): Parameters<ChmodArgs>) -> Result<String, String> {
        self.guard("chmod")?;
        let mode = u32::from_str_radix(args.mode.trim_start_matches("0o"), 8).map_err(|_| "mode must be octal, e.g. 644")?;
        let s = self.session(&args.session)?;
        self.core.chmod(&s, &args.path, mode, &Self::origin(&peer)).await.map_err(err)?;
        Ok("permissions changed".into())
    }

    #[tool(description = "Read a remote file as text (UTF-8, lossy). Binary files are reported as such.")]
    async fn read_file(&self, peer: Peer<RoleServer>, Parameters(args): Parameters<ReadArgs>) -> Result<String, String> {
        self.guard("read_file")?;
        let s = self.session(&args.session)?;
        let max = args.max_bytes.unwrap_or(200_000).min(5_000_000);
        let data = self.core.read_text(&s, &args.path, max, &Self::origin(&peer)).await.map_err(err)?;
        if data.iter().take(8000).any(|b| *b == 0) {
            return Ok(format!("<binary file, {} bytes read; use download instead>", data.len()));
        }
        Ok(String::from_utf8_lossy(&data).into_owned())
    }

    #[tool(description = "Create or overwrite a remote file with the given text content.")]
    async fn write_file(&self, peer: Peer<RoleServer>, Parameters(args): Parameters<WriteArgs>) -> Result<String, String> {
        self.guard("write_file")?;
        let s = self.session(&args.session)?;
        let p = self.core.write_text(&s, &args.path, args.content.as_bytes(), &Self::origin(&peer)).await.map_err(err)?;
        to_json(json!({ "written": p, "bytes": args.content.len() }))
    }

    #[tool(
        description = "Upload local files or folders to a remote directory. Transfers show up in the SmartFTP queue. By default waits for completion and returns per-file results. With queue_only they are only queued (pending) until process_queue."
    )]
    async fn upload(&self, peer: Peer<RoleServer>, Parameters(args): Parameters<UploadArgs>) -> Result<String, String> {
        self.guard("upload")?;
        let s = self.session(&args.session)?;
        let locals = args.local_paths.iter().map(|p| PathBuf::from(crate::core::sftp::expand_tilde(p))).collect();
        let batch = self.core.upload(&s, locals, &args.remote_dir, Self::origin(&peer), args.queue_only).await.map_err(err)?;
        self.batch_result(batch, !args.queue_only && args.wait.unwrap_or(true)).await
    }

    #[tool(
        description = "Download remote files or folders to a local directory. By default waits for completion and returns per-file results. With queue_only they are only queued (pending) until process_queue."
    )]
    async fn download(&self, peer: Peer<RoleServer>, Parameters(args): Parameters<DownloadArgs>) -> Result<String, String> {
        self.guard("download")?;
        let s = self.session(&args.session)?;
        let dir = PathBuf::from(crate::core::sftp::expand_tilde(&args.local_dir));
        let batch = self.core.download(&s, args.remote_paths, &dir, Self::origin(&peer), args.queue_only).await.map_err(err)?;
        self.batch_result(batch, !args.queue_only && args.wait.unwrap_or(true)).await
    }

    #[tool(description = "List transfers in the queue with progress and state.")]
    async fn list_transfers(&self) -> Result<String, String> {
        self.guard("list_transfers")?;
        let jobs: Vec<_> = self.core.jobs().iter().map(|j| j.summary()).collect();
        to_json(json!({ "transfers": jobs }))
    }

    #[tool(description = "Cancel a queued or running transfer by id.")]
    async fn cancel_transfer(&self, Parameters(args): Parameters<JobArg>) -> Result<String, String> {
        self.guard("cancel_transfer")?;
        self.core.cancel_job(args.id);
        Ok("cancel requested".into())
    }

    #[tool(
        description = "Start transfers waiting in the queue (state \"pending\", added by the user or with queue_only). Pass ids to start only some of them."
    )]
    async fn process_queue(&self, Parameters(args): Parameters<ProcessQueueArgs>) -> Result<String, String> {
        self.guard("process_queue")?;
        let ids = if args.ids.is_empty() { self.core.pending_jobs().iter().map(|j| j.id).collect() } else { args.ids.clone() };
        let started = self.core.process_queue(Some(&ids));
        if !args.wait || started == 0 {
            return to_json(json!({ "started": started }));
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(600);
        loop {
            let jobs: Vec<_> = self.core.jobs().into_iter().filter(|j| ids.contains(&j.id)).collect();
            if jobs.iter().all(|j| j.state.lock().finished()) || std::time::Instant::now() > deadline {
                let summaries: Vec<_> = jobs.iter().map(|j| j.summary()).collect();
                let failed = summaries.iter().filter(|j| matches!(j.state, crate::core::JobState::Failed(_))).count();
                return to_json(json!({ "started": started, "failed": failed, "transfers": summaries }));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    #[tool(description = "List a directory on the local computer (to find files to upload).")]
    async fn list_local_dir(&self, Parameters(args): Parameters<LocalArgs>) -> Result<String, String> {
        self.guard("list_local_dir")?;
        let path = if args.path.is_empty() {
            dirs::home_dir().unwrap_or_default()
        } else {
            PathBuf::from(crate::core::sftp::expand_tilde(&args.path))
        };
        let entries = remote::list_local(&path).map_err(err)?;
        to_json(json!({ "path": path, "entries": entries }))
    }
}

impl SmartFtpMcp {
    async fn batch_result(&self, batch: u64, wait: bool) -> Result<String, String> {
        let jobs = if wait {
            self.core.wait_batch(batch, Duration::from_secs(600)).await
        } else {
            self.core.batch_jobs(batch).iter().map(|j| j.summary()).collect()
        };
        let failed = jobs.iter().filter(|j| matches!(j.state, crate::core::JobState::Failed(_))).count();
        to_json(json!({ "batch": batch, "files": jobs.len(), "failed": failed, "transfers": jobs }))
    }
}

#[tool_handler(
    name = "smartftp",
    instructions = "SmartFTP: FTP/FTPS/SFTP client. Typical flow: list_sites or list_sessions -> connect (returns a session id) -> list_dir / upload / download / read_file / write_file / make_dir / rename / delete using that session id. Sessions opened here are visible to the user in the SmartFTP window, and sessions the user opened are available to you."
)]
impl ServerHandler for SmartFtpMcp {}

/// Serve MCP over Streamable HTTP on 127.0.0.1:<port>/mcp, protected by a bearer token.
pub async fn serve_http(core: Arc<Core>) -> anyhow::Result<()> {
    use axum::extract::Request;
    use axum::http::StatusCode;
    use axum::middleware::{self, Next};
    use axum::response::Response;
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

    let port = core.config.lock().mcp.port;
    let svc_core = core.clone();
    let service = StreamableHttpService::new(
        move || Ok(SmartFtpMcp::new(svc_core.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let auth_core = core.clone();
    let auth = move |req: Request, next: Next| {
        let core = auth_core.clone();
        async move {
            let (token, enabled) = {
                let c = core.config.lock();
                (c.mcp.token.clone(), c.mcp.enabled)
            };
            if !enabled {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
            let ok = req
                .headers()
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .is_some_and(|t| t == token);
            if ok { Ok::<Response, StatusCode>(next.run(req).await) } else { Err(StatusCode::UNAUTHORIZED) }
        }
    };
    let app = axum::Router::new().nest_service("/mcp", service).layer(middleware::from_fn(auth));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!("MCP listening on http://127.0.0.1:{port}/mcp");
    axum::serve(listener, app).await?;
    Ok(())
}

static HTTP_RUNNING: AtomicBool = AtomicBool::new(false);
static HTTP_ERROR: StdMutex<Option<String>> = StdMutex::new(None);

/// Last error of the HTTP server (e.g. port already in use).
pub fn status_error() -> Option<String> {
    HTTP_ERROR.lock().ok().and_then(|e| e.clone())
}

/// Start the HTTP server once, if enabled. Disabling later makes it refuse requests.
pub fn ensure_http(core: Arc<Core>) {
    if !core.config.lock().mcp.enabled || HTTP_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let rt = core.rt.clone();
    rt.spawn(async move {
        if let Err(e) = serve_http(core).await {
            tracing::error!("MCP server: {e:#}");
            if let Ok(mut g) = HTTP_ERROR.lock() {
                *g = Some(format!("Could not start the MCP server: {e:#}"));
            }
            HTTP_RUNNING.store(false, Ordering::SeqCst);
        }
    });
}

pub async fn serve_stdio(core: Arc<Core>) -> anyhow::Result<()> {
    use rmcp::ServiceExt;
    let server = SmartFtpMcp::new(core).serve(rmcp::transport::stdio()).await?;
    server.waiting().await?;
    Ok(())
}
