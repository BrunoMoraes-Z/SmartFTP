pub mod config;
pub mod ftp;
pub mod model;
pub mod remote;
pub mod sftp;

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use chrono::{DateTime, Local};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;

use config::{Config, ExistsAction};
use model::{Entry, Origin, Site, join_remote};
use remote::{Progress, RateLimiter, RemoteFs};

pub struct Session {
    pub id: String,
    pub site: Site,
    fs: RwLock<Arc<dyn RemoteFs>>,
    reconnecting: tokio::sync::Mutex<()>,
    pub home: String,
    pub cwd: Mutex<String>,
    pub origin: Origin,
    pub connected_at: DateTime<Local>,
}

impl Session {
    /// The current connection. It may be replaced by an automatic reconnect.
    pub fn fs(&self) -> Arc<dyn RemoteFs> {
        self.fs.read().clone()
    }

    pub fn resolve(&self, path: &str) -> String {
        if path.is_empty() || path == "." {
            self.cwd.lock().clone()
        } else if path.starts_with('/') {
            path.to_string()
        } else if path == "~" {
            self.home.clone()
        } else if let Some(rest) = path.strip_prefix("~/") {
            join_remote(&self.home, rest)
        } else {
            join_remote(&self.cwd.lock(), path)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Upload,
    Download,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", content = "error", rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Skipped,
    Failed(String),
    Cancelled,
}

impl JobState {
    pub fn finished(&self) -> bool {
        !matches!(self, JobState::Queued | JobState::Running)
    }
}

pub struct Job {
    pub id: u64,
    pub batch: u64,
    pub direction: Direction,
    pub session_id: String,
    pub session_name: String,
    pub local: PathBuf,
    pub remote: String,
    pub name: String,
    pub total: u64,
    /// modification time of the source, applied to the target after the transfer
    pub mtime: Option<i64>,
    pub progress: Arc<Progress>,
    pub state: Mutex<JobState>,
    pub origin: Origin,
    pub attempts: AtomicU32,
    pub started: Mutex<Option<Instant>>,
    pub finished: Mutex<Option<Instant>>,
}

impl Job {
    pub fn done_bytes(&self) -> u64 {
        self.progress.done.load(Ordering::Relaxed)
    }
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            return if self.state.lock().finished() { 1.0 } else { 0.0 };
        }
        (self.done_bytes() as f64 / self.total as f64).min(1.0) as f32
    }
    /// bytes per second since start
    pub fn speed(&self) -> f64 {
        let Some(start) = *self.started.lock() else { return 0.0 };
        let end = self.finished.lock().unwrap_or_else(Instant::now);
        let secs = end.duration_since(start).as_secs_f64().max(0.001);
        self.done_bytes() as f64 / secs
    }
    pub fn summary(&self) -> JobSummary {
        JobSummary {
            id: self.id,
            direction: self.direction,
            session: self.session_id.clone(),
            local: self.local.to_string_lossy().to_string(),
            remote: self.remote.clone(),
            total_bytes: self.total,
            done_bytes: self.done_bytes(),
            state: self.state.lock().clone(),
            origin: self.origin.label(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct JobSummary {
    pub id: u64,
    pub direction: Direction,
    pub session: String,
    pub local: String,
    pub remote: String,
    pub total_bytes: u64,
    pub done_bytes: u64,
    #[serde(flatten)]
    pub state: JobState,
    pub origin: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub time: DateTime<Local>,
    pub origin: Origin,
    pub action: String,
    pub target: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub site_id: String,
    pub name: String,
    pub protocol: &'static str,
    pub address: String,
    pub cwd: String,
    pub opened_by: String,
    pub connected_at: String,
}

/// Errors that mean the connection itself is gone (worth a reconnect), as opposed to
/// e.g. "no such file" or "permission denied".
pub fn is_connection_error(e: &anyhow::Error) -> bool {
    let msg = format!("{e:#}").to_lowercase();
    [
        "connection reset",
        "connection aborted",
        "connection closed",
        "connection refused",
        "not connected",
        "broken pipe",
        "channel closed",
        "session closed",
        "disconnected",
        "unexpected eof",
        "early eof",
        "timed out",
        "connection error",
        "os error 10053",
        "os error 10054",
    ]
    .iter()
    .any(|k| msg.contains(k))
}

pub struct Core {
    pub config: Mutex<Config>,
    sessions: Mutex<Vec<Arc<Session>>>,
    jobs: Mutex<Vec<Arc<Job>>>,
    log: Mutex<VecDeque<LogEntry>>,
    version: AtomicU64,
    next_id: AtomicU64,
    running: AtomicU32,
    up_limit: Arc<RateLimiter>,
    down_limit: Arc<RateLimiter>,
    pub rt: tokio::runtime::Handle,
}

impl Core {
    pub fn new(rt: tokio::runtime::Handle) -> Arc<Core> {
        let core = Arc::new(Core {
            config: Mutex::new(Config::load()),
            sessions: Mutex::new(vec![]),
            jobs: Mutex::new(vec![]),
            log: Mutex::new(VecDeque::new()),
            version: AtomicU64::new(1),
            next_id: AtomicU64::new(1),
            running: AtomicU32::new(0),
            up_limit: Arc::new(RateLimiter::default()),
            down_limit: Arc::new(RateLimiter::default()),
            rt,
        });
        core.apply_limits();
        core
    }

    /// Monotonic counter bumped on every observable change; the UI polls it.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }
    pub fn touch(&self) {
        self.version.fetch_add(1, Ordering::Relaxed);
    }
    fn next(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn apply_limits(&self) {
        let t = self.config.lock().transfers.clone();
        self.up_limit.set_limit(t.upload_limit_kib * 1024);
        self.down_limit.set_limit(t.download_limit_kib * 1024);
    }

    pub fn update_config(&self, f: impl FnOnce(&mut Config)) {
        let mut cfg = self.config.lock();
        f(&mut cfg);
        if let Err(e) = cfg.save() {
            tracing::error!("saving config: {e:#}");
        }
        drop(cfg);
        self.apply_limits();
        self.touch();
    }

    pub fn record(&self, origin: &Origin, action: &str, target: &str, result: &Result<impl Sized>) {
        let (ok, detail) = match result {
            Ok(_) => (true, String::new()),
            Err(e) => (false, format!("{e:#}")),
        };
        let mut log = self.log.lock();
        log.push_front(LogEntry { time: Local::now(), origin: origin.clone(), action: action.into(), target: target.into(), ok, detail });
        log.truncate(500);
        drop(log);
        self.touch();
    }

    pub fn log_entries(&self) -> Vec<LogEntry> {
        self.log.lock().iter().cloned().collect()
    }

    // ---------------------------------------------------------------- sessions

    pub fn sessions(&self) -> Vec<Arc<Session>> {
        self.sessions.lock().clone()
    }

    pub fn session_infos(&self) -> Vec<SessionInfo> {
        self.sessions()
            .iter()
            .map(|s| SessionInfo {
                id: s.id.clone(),
                site_id: s.site.id.clone(),
                name: s.site.name.clone(),
                protocol: s.site.protocol.label(),
                address: s.site.address(),
                cwd: s.cwd.lock().clone(),
                opened_by: s.origin.label(),
                connected_at: s.connected_at.to_rfc3339(),
            })
            .collect()
    }

    /// Find a session by id, or by site name when that is unambiguous.
    pub fn session(&self, key: &str) -> Result<Arc<Session>> {
        let sessions = self.sessions.lock();
        if let Some(s) = sessions.iter().find(|s| s.id == key) {
            return Ok(s.clone());
        }
        let by_name: Vec<_> = sessions.iter().filter(|s| s.site.name.eq_ignore_ascii_case(key)).collect();
        match by_name.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(anyhow!("no open session '{key}'. Use list_sessions or connect first.")),
            _ => Err(anyhow!("'{key}' matches several sessions; use the session id")),
        }
    }

    /// Connect with the configured timeout, retrying failed attempts after the configured delay.
    async fn open_fs(&self, site: &Site) -> Result<Arc<dyn RemoteFs>> {
        let settings = self.config.lock().connection.clone();
        let timeout = Duration::from_secs(settings.timeout_secs.max(1));
        let mut last = anyhow!("could not connect");
        for attempt in 0..=settings.retries {
            if attempt > 0 {
                tracing::info!("retrying connection to {} ({attempt}/{})", site.address(), settings.retries);
                tokio::time::sleep(Duration::from_secs(settings.retry_delay_secs)).await;
            }
            match tokio::time::timeout(timeout, remote::connect(site, &settings)).await {
                Ok(Ok(fs)) => return Ok(Arc::from(fs)),
                Ok(Err(e)) => last = e,
                Err(_) => last = anyhow!("timed out after {}s connecting to {}", timeout.as_secs(), site.address()),
            }
            // a changed host key will not fix itself
            if format!("{last:#}").contains("HOST KEY CHANGED") {
                break;
            }
        }
        Err(last)
    }

    pub async fn connect(self: &Arc<Self>, site: Site, origin: Origin) -> Result<Arc<Session>> {
        let target = site.address();
        let result = self.connect_inner(site, origin.clone()).await;
        self.record(&origin, "connect", &target, &result);
        result
    }

    async fn connect_inner(self: &Arc<Self>, site: Site, origin: Origin) -> Result<Arc<Session>> {
        if site.host.trim().is_empty() {
            bail!("host is empty");
        }
        let fs = self.open_fs(&site).await?;
        let home = fs.home().await.unwrap_or_else(|_| "/".into());
        let start = if site.remote_dir.trim().is_empty() { home.clone() } else { site.remote_dir.clone() };
        let session = Arc::new(Session {
            id: format!("s{}", self.next()),
            site,
            fs: RwLock::new(fs),
            reconnecting: tokio::sync::Mutex::new(()),
            home,
            cwd: Mutex::new(start),
            origin,
            connected_at: Local::now(),
        });
        self.sessions.lock().push(session.clone());
        self.touch();
        Ok(session)
    }

    /// Replace a dropped connection. Concurrent callers wait for the same reconnect.
    async fn reconnect(&self, session: &Session, broken: &Arc<dyn RemoteFs>) -> Result<()> {
        let _guard = session.reconnecting.lock().await;
        if !Arc::ptr_eq(&session.fs(), broken) {
            return Ok(()); // someone else already reconnected
        }
        let fs = self.open_fs(&session.site).await?;
        *session.fs.write() = fs;
        broken.close().await;
        self.record(&Origin::User, "reconnect", &session.site.name, &Ok::<(), anyhow::Error>(()));
        Ok(())
    }

    /// Run an operation, reconnecting once and retrying when the connection was lost.
    pub async fn call<T, F, Fut>(&self, session: &Session, f: F) -> Result<T>
    where
        F: Fn(Arc<dyn RemoteFs>) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let fs = session.fs();
        match f(fs.clone()).await {
            Err(e) if self.config.lock().connection.auto_reconnect && is_connection_error(&e) => {
                tracing::warn!("{}: connection lost ({e:#}); reconnecting", session.site.name);
                self.reconnect(session, &fs).await.map_err(|re| anyhow!("connection lost ({e:#}) and reconnecting failed: {re:#}"))?;
                f(session.fs()).await
            }
            r => r,
        }
    }

    pub async fn disconnect(&self, id: &str, origin: &Origin) -> Result<()> {
        let session = self.session(id)?;
        self.sessions.lock().retain(|s| s.id != session.id);
        for job in self.jobs.lock().iter().filter(|j| j.session_id == session.id) {
            job.progress.cancel.store(true, Ordering::Relaxed);
        }
        session.fs().close().await;
        self.record(origin, "disconnect", &session.site.name, &Ok::<(), anyhow::Error>(()));
        Ok(())
    }

    // ---------------------------------------------------------------- file ops

    pub async fn list(&self, session: &Session, path: &str) -> Result<Vec<Entry>> {
        let path = session.resolve(path);
        self.call(session, |fs| {
            let p = path.clone();
            async move { fs.list(&p).await }
        })
        .await
    }

    pub async fn is_dir(&self, session: &Session, path: &str) -> Result<bool> {
        let path = session.resolve(path);
        self.call(session, |fs| {
            let p = path.clone();
            async move { fs.is_dir(&p).await }
        })
        .await
    }

    pub async fn change_dir(&self, session: &Session, path: &str) -> Result<String> {
        let path = session.resolve(path);
        if !self.is_dir(session, &path).await? {
            bail!("{path} is not a directory");
        }
        *session.cwd.lock() = path.clone();
        self.touch();
        Ok(path)
    }

    pub async fn mkdir(&self, session: &Session, path: &str, origin: &Origin) -> Result<String> {
        let path = session.resolve(path);
        let r = self
            .call(session, |fs| {
                let p = path.clone();
                async move { fs.mkdir(&p).await }
            })
            .await;
        self.record(origin, "mkdir", &format!("{}:{path}", session.site.name), &r);
        r.map(|_| path)
    }

    pub async fn rename(&self, session: &Session, from: &str, to: &str, origin: &Origin) -> Result<()> {
        let (from, to) = (session.resolve(from), session.resolve(to));
        let r = self
            .call(session, |fs| {
                let (a, b) = (from.clone(), to.clone());
                async move { fs.rename(&a, &b).await }
            })
            .await;
        self.record(origin, "rename", &format!("{}:{from} -> {to}", session.site.name), &r);
        r
    }

    pub async fn chmod(&self, session: &Session, path: &str, mode: u32, origin: &Origin) -> Result<()> {
        let path = session.resolve(path);
        let r = self
            .call(session, |fs| {
                let p = path.clone();
                async move { fs.chmod(&p, mode).await }
            })
            .await;
        self.record(origin, "chmod", &format!("{}:{path} {mode:o}", session.site.name), &r);
        r
    }

    pub async fn delete(&self, session: &Session, path: &str, recursive: bool, origin: &Origin) -> Result<u64> {
        let path = session.resolve(path);
        if path == "/" || path.is_empty() {
            bail!("refusing to delete the root directory");
        }
        let r = async {
            if self.is_dir(session, &path).await? {
                if !recursive {
                    if !self.list(session, &path).await?.is_empty() {
                        bail!("{path} is not empty; pass recursive=true to delete it with its contents");
                    }
                    self.call(session, |fs| {
                        let p = path.clone();
                        async move { fs.remove_dir(&p).await }
                    })
                    .await?;
                    return Ok(1);
                }
                self.call(session, |fs| {
                    let p = path.clone();
                    async move { delete_tree(fs.as_ref(), &p).await }
                })
                .await
            } else {
                self.call(session, |fs| {
                    let p = path.clone();
                    async move { fs.remove_file(&p).await }
                })
                .await?;
                Ok(1)
            }
        }
        .await;
        self.record(origin, "delete", &format!("{}:{path}", session.site.name), &r);
        r
    }

    pub async fn read_text(&self, session: &Session, path: &str, max: usize, origin: &Origin) -> Result<Vec<u8>> {
        let path = session.resolve(path);
        let r = self
            .call(session, |fs| {
                let p = path.clone();
                async move { fs.read(&p, max).await }
            })
            .await;
        self.record(origin, "read_file", &format!("{}:{path}", session.site.name), &r);
        r
    }

    pub async fn write_text(&self, session: &Session, path: &str, data: &[u8], origin: &Origin) -> Result<String> {
        let path = session.resolve(path);
        let r = self
            .call(session, |fs| {
                let p = path.clone();
                let d = data.to_vec();
                async move { fs.write(&p, &d).await }
            })
            .await;
        self.record(origin, "write_file", &format!("{}:{path}", session.site.name), &r);
        r.map(|_| path)
    }

    // ---------------------------------------------------------------- transfers

    pub fn jobs(&self) -> Vec<Arc<Job>> {
        self.jobs.lock().clone()
    }

    pub fn cancel_job(&self, id: u64) {
        if let Some(j) = self.jobs.lock().iter().find(|j| j.id == id) {
            j.progress.cancel.store(true, Ordering::Relaxed);
            let mut st = j.state.lock();
            if *st == JobState::Queued {
                *st = JobState::Cancelled;
            }
        }
        self.touch();
    }

    pub fn clear_finished_jobs(&self) {
        self.jobs.lock().retain(|j| !j.state.lock().finished());
        self.touch();
    }

    pub fn retry_job(self: &Arc<Self>, id: u64) {
        let job = self.jobs.lock().iter().find(|j| j.id == id).cloned();
        if let Some(job) = job
            && let Ok(session) = self.session(&job.session_id)
        {
            let batch = self.next();
            self.push_job(&session, job.direction, job.local.clone(), job.remote.clone(), job.total, job.mtime, batch, job.origin.clone());
        }
    }

    /// Upload local files or folders (recursively) into `remote_dir`. Returns the batch id.
    pub async fn upload(self: &Arc<Self>, session: &Arc<Session>, locals: Vec<PathBuf>, remote_dir: &str, origin: Origin) -> Result<u64> {
        let remote_dir = session.resolve(remote_dir);
        let batch = self.next();
        let action = self.config.lock().transfers.on_exists;
        let mut listings: HashMap<String, Vec<Entry>> = HashMap::new();
        for local in locals {
            let name =
                local.file_name().map(|n| n.to_string_lossy().to_string()).ok_or_else(|| anyhow!("invalid path {}", local.display()))?;
            let meta = std::fs::metadata(&local).map_err(|e| anyhow!("{}: {e}", local.display()))?;
            if meta.is_dir() {
                let target = join_remote(&remote_dir, &name);
                let _ = self.mkdir_quiet(session, &target).await;
                let mut stack = vec![(local.clone(), target)];
                while let Some((dir, rdir)) = stack.pop() {
                    for item in std::fs::read_dir(&dir)?.flatten() {
                        let p = item.path();
                        let item_name = item.file_name().to_string_lossy().to_string();
                        let Ok(m) = item.metadata() else { continue };
                        if m.is_dir() {
                            let rn = join_remote(&rdir, &item_name);
                            let _ = self.mkdir_quiet(session, &rn).await;
                            stack.push((p, rn));
                        } else {
                            let existing = self.remote_listing(session, &rdir, &mut listings, action).await;
                            if let Some(rn) = resolve_target(action, m.len(), mtime_of(&m), &item_name, &existing) {
                                self.push_job(
                                    session,
                                    Direction::Upload,
                                    p,
                                    join_remote(&rdir, &rn),
                                    m.len(),
                                    mtime_of(&m),
                                    batch,
                                    origin.clone(),
                                );
                            } else {
                                self.push_skipped(
                                    session,
                                    Direction::Upload,
                                    p,
                                    join_remote(&rdir, &item_name),
                                    m.len(),
                                    batch,
                                    origin.clone(),
                                );
                            }
                        }
                    }
                }
            } else {
                let existing = self.remote_listing(session, &remote_dir, &mut listings, action).await;
                match resolve_target(action, meta.len(), mtime_of(&meta), &name, &existing) {
                    Some(rn) => self.push_job(
                        session,
                        Direction::Upload,
                        local,
                        join_remote(&remote_dir, &rn),
                        meta.len(),
                        mtime_of(&meta),
                        batch,
                        origin.clone(),
                    ),
                    None => self.push_skipped(
                        session,
                        Direction::Upload,
                        local,
                        join_remote(&remote_dir, &name),
                        meta.len(),
                        batch,
                        origin.clone(),
                    ),
                }
            }
        }
        self.record(&origin, "upload", &format!("{}:{remote_dir}", session.site.name), &Ok::<(), anyhow::Error>(()));
        Ok(batch)
    }

    async fn mkdir_quiet(&self, session: &Session, path: &str) -> Result<()> {
        let p = path.to_string();
        self.call(session, |fs| {
            let p = p.clone();
            async move { fs.mkdir(&p).await }
        })
        .await
    }

    /// Existing entries of a remote directory (cached per batch), name -> (size, mtime).
    async fn remote_listing(
        &self,
        session: &Session,
        dir: &str,
        cache: &mut HashMap<String, Vec<Entry>>,
        action: ExistsAction,
    ) -> HashMap<String, (u64, Option<i64>)> {
        if action == ExistsAction::Overwrite {
            return HashMap::new();
        }
        if !cache.contains_key(dir) {
            let entries = self.list(session, dir).await.unwrap_or_default();
            cache.insert(dir.to_string(), entries);
        }
        cache[dir].iter().filter(|e| !e.is_dir).map(|e| (e.name.clone(), (e.size, e.modified))).collect()
    }

    /// Download remote files or folders (recursively) into `local_dir`. Returns the batch id.
    pub async fn download(self: &Arc<Self>, session: &Arc<Session>, remotes: Vec<String>, local_dir: &Path, origin: Origin) -> Result<u64> {
        let batch = self.next();
        let action = self.config.lock().transfers.on_exists;
        std::fs::create_dir_all(local_dir)?;
        for remote in remotes {
            let remote = session.resolve(&remote);
            let name = remote.trim_end_matches('/').rsplit('/').next().unwrap_or("download").to_string();
            if self.is_dir(session, &remote).await? {
                let target = local_dir.join(&name);
                std::fs::create_dir_all(&target)?;
                let mut stack = vec![(remote.clone(), target)];
                while let Some((rdir, ldir)) = stack.pop() {
                    let existing = local_listing(&ldir);
                    for e in self.list(session, &rdir).await? {
                        if e.is_dir {
                            let lp = ldir.join(&e.name);
                            std::fs::create_dir_all(&lp)?;
                            stack.push((e.path, lp));
                        } else {
                            match resolve_target(action, e.size, e.modified, &e.name, &existing) {
                                Some(ln) => self.push_job(
                                    session,
                                    Direction::Download,
                                    ldir.join(ln),
                                    e.path,
                                    e.size,
                                    e.modified,
                                    batch,
                                    origin.clone(),
                                ),
                                None => self.push_skipped(
                                    session,
                                    Direction::Download,
                                    ldir.join(&e.name),
                                    e.path,
                                    e.size,
                                    batch,
                                    origin.clone(),
                                ),
                            }
                        }
                    }
                }
            } else {
                let found =
                    self.list(session, &model::parent_remote(&remote)).await.ok().and_then(|l| l.into_iter().find(|e| e.path == remote));
                let (size, mtime) = found.map(|e| (e.size, e.modified)).unwrap_or((0, None));
                let existing = local_listing(local_dir);
                match resolve_target(action, size, mtime, &name, &existing) {
                    Some(ln) => self.push_job(session, Direction::Download, local_dir.join(ln), remote, size, mtime, batch, origin.clone()),
                    None => self.push_skipped(session, Direction::Download, local_dir.join(&name), remote, size, batch, origin.clone()),
                }
            }
        }
        self.record(&origin, "download", &format!("{} -> {}", session.site.name, local_dir.display()), &Ok::<(), anyhow::Error>(()));
        Ok(batch)
    }

    #[allow(clippy::too_many_arguments)]
    fn new_job(
        &self,
        session: &Session,
        direction: Direction,
        local: PathBuf,
        remote: String,
        total: u64,
        mtime: Option<i64>,
        batch: u64,
        origin: Origin,
        state: JobState,
    ) -> Arc<Job> {
        let name = match direction {
            Direction::Upload => local.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            Direction::Download => remote.rsplit('/').next().unwrap_or_default().to_string(),
        };
        let limiter = match direction {
            Direction::Upload => self.up_limit.clone(),
            Direction::Download => self.down_limit.clone(),
        };
        let job = Arc::new(Job {
            id: self.next(),
            batch,
            direction,
            session_id: session.id.clone(),
            session_name: session.site.name.clone(),
            local,
            remote,
            name,
            total,
            mtime,
            progress: Arc::new(Progress::with_limiter(limiter)),
            state: Mutex::new(state),
            origin,
            attempts: AtomicU32::new(0),
            started: Mutex::new(None),
            finished: Mutex::new(None),
        });
        self.jobs.lock().push(job.clone());
        self.touch();
        job
    }

    #[allow(clippy::too_many_arguments)]
    fn push_skipped(
        &self,
        session: &Session,
        direction: Direction,
        local: PathBuf,
        remote: String,
        total: u64,
        batch: u64,
        origin: Origin,
    ) {
        let job = self.new_job(session, direction, local, remote, total, None, batch, origin, JobState::Skipped);
        *job.finished.lock() = Some(Instant::now());
    }

    #[allow(clippy::too_many_arguments)]
    fn push_job(
        self: &Arc<Self>,
        session: &Arc<Session>,
        direction: Direction,
        local: PathBuf,
        remote: String,
        total: u64,
        mtime: Option<i64>,
        batch: u64,
        origin: Origin,
    ) {
        let job = self.new_job(session, direction, local, remote, total, mtime, batch, origin, JobState::Queued);
        let core = self.clone();
        let session = session.clone();
        self.rt.spawn(async move {
            let _slot = core.acquire_slot(&job).await;
            if job.progress.cancelled() {
                *job.state.lock() = JobState::Cancelled;
                core.touch();
                return;
            }
            *job.state.lock() = JobState::Running;
            *job.started.lock() = Some(Instant::now());
            core.touch();
            let (retries, delay, preserve) = {
                let c = core.config.lock();
                (c.transfers.retries, c.connection.retry_delay_secs, c.transfers.preserve_timestamps)
            };
            let mut result;
            loop {
                job.attempts.fetch_add(1, Ordering::Relaxed);
                job.progress.done.store(0, Ordering::Relaxed);
                result = core.run_job(&session, &job).await;
                let attempts = job.attempts.load(Ordering::Relaxed);
                if result.is_ok() || job.progress.cancelled() || attempts > retries {
                    break;
                }
                tracing::warn!("{} failed (attempt {attempts}): {:#}; retrying", job.name, result.as_ref().err().unwrap());
                tokio::time::sleep(Duration::from_secs(delay.max(1))).await;
            }
            if result.is_ok()
                && preserve
                && let Some(mtime) = job.mtime
            {
                match job.direction {
                    Direction::Upload => {
                        let _ = session.fs().set_mtime(&job.remote, mtime).await;
                    }
                    Direction::Download => set_local_mtime(&job.local, mtime),
                }
            }
            *job.finished.lock() = Some(Instant::now());
            *job.state.lock() = match result {
                Ok(_) => JobState::Done,
                Err(_) if job.progress.cancelled() => JobState::Cancelled,
                Err(e) => JobState::Failed(format!("{e:#}")),
            };
            core.touch();
        });
    }

    async fn run_job(&self, session: &Session, job: &Arc<Job>) -> Result<u64> {
        self.call(session, |fs| {
            let job = job.clone();
            async move {
                match job.direction {
                    Direction::Upload => fs.upload(&job.local, &job.remote, &job.progress).await,
                    Direction::Download => fs.download(&job.remote, &job.local, &job.progress).await,
                }
            }
        })
        .await
    }

    /// Wait until fewer than `max_concurrent` transfers run (the limit can change live).
    async fn acquire_slot(self: &Arc<Self>, job: &Job) -> SlotGuard {
        loop {
            let max = self.config.lock().transfers.max_concurrent.max(1);
            if self.running.try_update(Ordering::SeqCst, Ordering::SeqCst, |r| (r < max).then_some(r + 1)).is_ok() {
                return SlotGuard(self.clone());
            }
            if job.progress.cancelled() {
                // let the caller observe the cancel without holding a slot
                self.running.fetch_add(1, Ordering::SeqCst);
                return SlotGuard(self.clone());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub fn batch_jobs(&self, batch: u64) -> Vec<Arc<Job>> {
        self.jobs.lock().iter().filter(|j| j.batch == batch).cloned().collect()
    }

    pub async fn wait_batch(&self, batch: u64, timeout: Duration) -> Vec<JobSummary> {
        let deadline = Instant::now() + timeout;
        loop {
            let jobs = self.batch_jobs(batch);
            if jobs.iter().all(|j| j.state.lock().finished()) || Instant::now() > deadline {
                return jobs.iter().map(|j| j.summary()).collect();
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }
}

struct SlotGuard(Arc<Core>);

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Decide what to do with a file whose target may already exist.
/// Returns the target file name to write, or `None` to skip.
pub fn resolve_target(
    action: ExistsAction,
    size: u64,
    mtime: Option<i64>,
    name: &str,
    existing: &HashMap<String, (u64, Option<i64>)>,
) -> Option<String> {
    let Some((dsize, dmtime)) = existing.get(name) else { return Some(name.to_string()) };
    match action {
        ExistsAction::Overwrite => Some(name.to_string()),
        ExistsAction::Skip => None,
        // compare with a little slack: FTP listings may only have minute precision
        ExistsAction::OverwriteIfNewer => match (mtime, dmtime) {
            (Some(a), Some(b)) if a > b + 60 => Some(name.to_string()),
            (_, None) => Some(name.to_string()),
            _ => None,
        },
        ExistsAction::OverwriteIfDifferent => {
            let time_differs = matches!((mtime, dmtime), (Some(a), Some(b)) if (a - b).abs() > 60);
            (size != *dsize || time_differs).then(|| name.to_string())
        }
        ExistsAction::Rename => {
            let (stem, ext) = match name.rfind('.') {
                Some(i) if i > 0 => (&name[..i], &name[i..]),
                _ => (name, ""),
            };
            (1..).map(|n| format!("{stem} ({n}){ext}")).find(|candidate| !existing.contains_key(candidate))
        }
    }
}

fn local_listing(dir: &Path) -> HashMap<String, (u64, Option<i64>)> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let m = e.metadata().ok()?;
                    (!m.is_dir()).then(|| (e.file_name().to_string_lossy().to_string(), (m.len(), mtime_of(&m))))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn mtime_of(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

fn set_local_mtime(path: &Path, mtime: i64) {
    if let Ok(f) = std::fs::File::options().write(true).open(path) {
        let _ = f.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(mtime.max(0) as u64));
    }
}

async fn delete_tree(fs: &dyn RemoteFs, path: &str) -> Result<u64> {
    let mut count = 0;
    for e in fs.list(path).await? {
        if e.is_dir && !e.is_symlink {
            count += Box::pin(delete_tree(fs, &e.path)).await?;
        } else {
            fs.remove_file(&e.path).await?;
            count += 1;
        }
    }
    fs.remove_dir(path).await?;
    Ok(count + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exists_policy() {
        let mut ex = HashMap::new();
        ex.insert("a.txt".to_string(), (10u64, Some(1_000_000i64)));
        ex.insert("a (1).txt".to_string(), (10u64, Some(1_000_000i64)));
        assert_eq!(resolve_target(ExistsAction::Overwrite, 1, None, "a.txt", &ex).as_deref(), Some("a.txt"));
        assert_eq!(resolve_target(ExistsAction::Skip, 1, None, "a.txt", &ex), None);
        assert_eq!(resolve_target(ExistsAction::Skip, 1, None, "new.txt", &ex).as_deref(), Some("new.txt"));
        assert_eq!(resolve_target(ExistsAction::OverwriteIfNewer, 1, Some(1_000_030), "a.txt", &ex), None);
        assert_eq!(resolve_target(ExistsAction::OverwriteIfNewer, 1, Some(1_000_500), "a.txt", &ex).as_deref(), Some("a.txt"));
        assert_eq!(resolve_target(ExistsAction::OverwriteIfDifferent, 10, Some(1_000_000), "a.txt", &ex), None);
        assert_eq!(resolve_target(ExistsAction::OverwriteIfDifferent, 11, Some(1_000_000), "a.txt", &ex).as_deref(), Some("a.txt"));
        assert_eq!(resolve_target(ExistsAction::Rename, 1, None, "a.txt", &ex).as_deref(), Some("a (2).txt"));
    }

    #[test]
    fn connection_errors() {
        assert!(is_connection_error(&anyhow!("Connection reset by peer (os error 104)")));
        assert!(is_connection_error(&anyhow!("channel closed")));
        assert!(!is_connection_error(&anyhow!("No such file")));
        assert!(!is_connection_error(&anyhow!("Permission denied")));
    }
}
