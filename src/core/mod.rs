pub mod config;
pub mod ftp;
pub mod model;
pub mod remote;
pub mod sftp;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use chrono::{DateTime, Local};
use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::Semaphore;

use config::Config;
use model::{Entry, Origin, Site, join_remote};
use remote::{Progress, RemoteFs};

pub struct Session {
    pub id: String,
    pub site: Site,
    pub fs: Arc<dyn RemoteFs>,
    pub home: String,
    pub cwd: Mutex<String>,
    pub origin: Origin,
    pub connected_at: DateTime<Local>,
}

impl Session {
    pub fn resolve(&self, path: &str) -> String {
        if path.is_empty() || path == "." {
            self.cwd.lock().clone()
        } else if path.starts_with('/') {
            path.to_string()
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

pub struct Core {
    pub config: Mutex<Config>,
    sessions: Mutex<Vec<Arc<Session>>>,
    jobs: Mutex<Vec<Arc<Job>>>,
    log: Mutex<VecDeque<LogEntry>>,
    version: AtomicU64,
    next_id: AtomicU64,
    slots: Arc<Semaphore>,
    pub rt: tokio::runtime::Handle,
}

impl Core {
    pub fn new(rt: tokio::runtime::Handle) -> Arc<Core> {
        Arc::new(Core {
            config: Mutex::new(Config::load()),
            sessions: Mutex::new(vec![]),
            jobs: Mutex::new(vec![]),
            log: Mutex::new(VecDeque::new()),
            version: AtomicU64::new(1),
            next_id: AtomicU64::new(1),
            slots: Arc::new(Semaphore::new(3)),
            rt,
        })
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

    pub fn update_config(&self, f: impl FnOnce(&mut Config)) {
        let mut cfg = self.config.lock();
        f(&mut cfg);
        if let Err(e) = cfg.save() {
            tracing::error!("saving config: {e:#}");
        }
        drop(cfg);
        self.touch();
    }

    pub fn record(&self, origin: &Origin, action: &str, target: &str, result: &Result<impl Sized>) {
        let (ok, detail) = match result {
            Ok(_) => (true, String::new()),
            Err(e) => (false, format!("{e:#}")),
        };
        let mut log = self.log.lock();
        log.push_front(LogEntry {
            time: Local::now(),
            origin: origin.clone(),
            action: action.into(),
            target: target.into(),
            ok,
            detail,
        });
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
        let fs: Arc<dyn RemoteFs> = Arc::from(remote::connect(&site).await?);
        let home = fs.home().await.unwrap_or_else(|_| "/".into());
        let start = if site.remote_dir.trim().is_empty() { home.clone() } else { site.remote_dir.clone() };
        let session = Arc::new(Session {
            id: format!("s{}", self.next()),
            site,
            fs,
            home,
            cwd: Mutex::new(start),
            origin,
            connected_at: Local::now(),
        });
        self.sessions.lock().push(session.clone());
        self.touch();
        Ok(session)
    }

    pub async fn disconnect(&self, id: &str, origin: &Origin) -> Result<()> {
        let session = self.session(id)?;
        self.sessions.lock().retain(|s| s.id != session.id);
        for job in self.jobs.lock().iter().filter(|j| j.session_id == session.id) {
            job.progress.cancel.store(true, Ordering::Relaxed);
        }
        session.fs.close().await;
        self.record(origin, "disconnect", &session.site.name, &Ok::<(), anyhow::Error>(()));
        Ok(())
    }

    // ---------------------------------------------------------------- file ops

    pub async fn list(&self, session: &Session, path: &str) -> Result<Vec<Entry>> {
        let path = session.resolve(path);
        session.fs.list(&path).await
    }

    pub async fn change_dir(&self, session: &Session, path: &str) -> Result<String> {
        let path = session.resolve(path);
        if !session.fs.is_dir(&path).await? {
            bail!("{path} is not a directory");
        }
        *session.cwd.lock() = path.clone();
        self.touch();
        Ok(path)
    }

    pub async fn mkdir(&self, session: &Session, path: &str, origin: &Origin) -> Result<String> {
        let path = session.resolve(path);
        let r = session.fs.mkdir(&path).await;
        self.record(origin, "mkdir", &format!("{}:{path}", session.site.name), &r);
        r.map(|_| path)
    }

    pub async fn rename(&self, session: &Session, from: &str, to: &str, origin: &Origin) -> Result<()> {
        let (from, to) = (session.resolve(from), session.resolve(to));
        let r = session.fs.rename(&from, &to).await;
        self.record(origin, "rename", &format!("{}:{from} -> {to}", session.site.name), &r);
        r
    }

    pub async fn chmod(&self, session: &Session, path: &str, mode: u32, origin: &Origin) -> Result<()> {
        let path = session.resolve(path);
        let r = session.fs.chmod(&path, mode).await;
        self.record(origin, "chmod", &format!("{}:{path} {mode:o}", session.site.name), &r);
        r
    }

    pub async fn delete(&self, session: &Session, path: &str, recursive: bool, origin: &Origin) -> Result<u64> {
        let path = session.resolve(path);
        if path == "/" || path.is_empty() {
            bail!("refusing to delete the root directory");
        }
        let r = async {
            if session.fs.is_dir(&path).await? {
                if !recursive {
                    let children = session.fs.list(&path).await?;
                    if !children.is_empty() {
                        bail!("{path} is not empty; pass recursive=true to delete it with its contents");
                    }
                    session.fs.remove_dir(&path).await?;
                    return Ok(1);
                }
                Box::pin(delete_tree(session.fs.as_ref(), &path)).await
            } else {
                session.fs.remove_file(&path).await?;
                Ok(1)
            }
        }
        .await;
        self.record(origin, "delete", &format!("{}:{path}", session.site.name), &r);
        r
    }

    pub async fn read_text(&self, session: &Session, path: &str, max: usize, origin: &Origin) -> Result<Vec<u8>> {
        let path = session.resolve(path);
        let r = session.fs.read(&path, max).await;
        self.record(origin, "read_file", &format!("{}:{path}", session.site.name), &r);
        r
    }

    pub async fn write_text(&self, session: &Session, path: &str, data: &[u8], origin: &Origin) -> Result<String> {
        let path = session.resolve(path);
        let r = session.fs.write(&path, data).await;
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
        for local in locals {
            let name = local.file_name().map(|n| n.to_string_lossy().to_string()).ok_or_else(|| anyhow!("invalid path {}", local.display()))?;
            let meta = std::fs::metadata(&local).map_err(|e| anyhow!("{}: {e}", local.display()))?;
            let target = join_remote(&remote_dir, &name);
            if meta.is_dir() {
                let _ = session.fs.mkdir(&target).await;
                let mut stack = vec![(local.clone(), target)];
                while let Some((dir, rdir)) = stack.pop() {
                    for item in std::fs::read_dir(&dir)?.flatten() {
                        let p = item.path();
                        let rn = join_remote(&rdir, &item.file_name().to_string_lossy());
                        let Ok(m) = item.metadata() else { continue };
                        if m.is_dir() {
                            let _ = session.fs.mkdir(&rn).await;
                            stack.push((p, rn));
                        } else {
                            self.push_job(session, Direction::Upload, p, rn, m.len(), mtime_of(&m), batch, origin.clone());
                        }
                    }
                }
            } else {
                self.push_job(session, Direction::Upload, local, target, meta.len(), mtime_of(&meta), batch, origin.clone());
            }
        }
        self.record(&origin, "upload", &format!("{}:{remote_dir}", session.site.name), &Ok::<(), anyhow::Error>(()));
        Ok(batch)
    }

    /// Download remote files or folders (recursively) into `local_dir`. Returns the batch id.
    pub async fn download(self: &Arc<Self>, session: &Arc<Session>, remotes: Vec<String>, local_dir: &Path, origin: Origin) -> Result<u64> {
        let batch = self.next();
        std::fs::create_dir_all(local_dir)?;
        for remote in remotes {
            let remote = session.resolve(&remote);
            let name = remote.trim_end_matches('/').rsplit('/').next().unwrap_or("download").to_string();
            let target = local_dir.join(&name);
            if session.fs.is_dir(&remote).await? {
                std::fs::create_dir_all(&target)?;
                let mut stack = vec![(remote.clone(), target)];
                while let Some((rdir, ldir)) = stack.pop() {
                    for e in session.fs.list(&rdir).await? {
                        let lp = ldir.join(&e.name);
                        if e.is_dir {
                            std::fs::create_dir_all(&lp)?;
                            stack.push((e.path, lp));
                        } else {
                            self.push_job(session, Direction::Download, lp, e.path, e.size, e.modified, batch, origin.clone());
                        }
                    }
                }
            } else {
                let found = session
                    .fs
                    .list(&model::parent_remote(&remote))
                    .await
                    .ok()
                    .and_then(|l| l.into_iter().find(|e| e.path == remote));
                let (size, mtime) = found.map(|e| (e.size, e.modified)).unwrap_or((0, None));
                self.push_job(session, Direction::Download, target, remote, size, mtime, batch, origin.clone());
            }
        }
        self.record(&origin, "download", &format!("{} -> {}", session.site.name, local_dir.display()), &Ok::<(), anyhow::Error>(()));
        Ok(batch)
    }

    #[allow(clippy::too_many_arguments)]
    fn push_job(self: &Arc<Self>, session: &Arc<Session>, direction: Direction, local: PathBuf, remote: String, total: u64, mtime: Option<i64>, batch: u64, origin: Origin) {
        let name = match direction {
            Direction::Upload => local.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            Direction::Download => remote.rsplit('/').next().unwrap_or_default().to_string(),
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
            progress: Arc::new(Progress::default()),
            state: Mutex::new(JobState::Queued),
            origin,
            started: Mutex::new(None),
            finished: Mutex::new(None),
        });
        self.jobs.lock().push(job.clone());
        self.touch();
        let core = self.clone();
        let session = session.clone();
        self.rt.spawn(async move {
            let _permit = core.slots.clone().acquire_owned().await;
            if job.progress.cancelled() {
                *job.state.lock() = JobState::Cancelled;
                core.touch();
                return;
            }
            *job.state.lock() = JobState::Running;
            *job.started.lock() = Some(Instant::now());
            core.touch();
            let result = match job.direction {
                Direction::Upload => session.fs.upload(&job.local, &job.remote, &job.progress).await,
                Direction::Download => session.fs.download(&job.remote, &job.local, &job.progress).await,
            };
            if result.is_ok()
                && let Some(mtime) = job.mtime
            {
                match job.direction {
                    Direction::Upload => {
                        let _ = session.fs.set_mtime(&job.remote, mtime).await;
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

    pub fn batch_jobs(&self, batch: u64) -> Vec<Arc<Job>> {
        self.jobs.lock().iter().filter(|j| j.batch == batch).cloned().collect()
    }

    pub async fn wait_batch(&self, batch: u64, timeout: std::time::Duration) -> Vec<JobSummary> {
        let deadline = Instant::now() + timeout;
        loop {
            let jobs = self.batch_jobs(batch);
            if jobs.iter().all(|j| j.state.lock().finished()) || Instant::now() > deadline {
                return jobs.iter().map(|j| j.summary()).collect();
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
    }
}

fn mtime_of(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

fn set_local_mtime(path: &Path, mtime: i64) {
    if let Ok(f) = std::fs::File::options().write(true).open(path) {
        let _ = f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(mtime.max(0) as u64));
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
