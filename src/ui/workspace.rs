use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use gpui::{
    App, BorrowAppContext, ClickEvent, Context, Entity, FocusHandle, Focusable, FontWeight, KeyBinding, Pixels, Point, SharedString,
    Subscription, Window, actions, anchored, deferred, div, prelude::*, px,
};

use super::components::*;
use super::dialogs::{IconPicker, PromptKind, PromptState, SiteEditor, SiteEditorEvent};
use super::input::{InputEvent, TextInput};
use super::settings::SettingsState;
use super::theme::{Themes, appearance_of, theme};
use crate::core::model::{Entry, Origin, Site, join_remote, parent_remote};
use crate::core::remote::{list_local, sort_entries};
use crate::core::{Core, Direction, JobState, Session};
use crate::i18n::{tr, trf};

actions!(workspace, [DeleteSelected, OpenSelected, GoUp, Refresh, RenameSelected, SelectAllRows, Dismiss, NewFolder, EditPath, EditFile]);

pub fn bind_keys(cx: &mut App) {
    let ctx = Some("FilePane");
    cx.bind_keys([
        KeyBinding::new("delete", DeleteSelected, ctx),
        KeyBinding::new("enter", OpenSelected, ctx),
        KeyBinding::new("backspace", GoUp, ctx),
        KeyBinding::new("f5", Refresh, None),
        KeyBinding::new("f2", RenameSelected, ctx),
        KeyBinding::new("f7", NewFolder, ctx),
        KeyBinding::new("f4", EditFile, ctx),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-a", SelectAllRows, ctx),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-a", SelectAllRows, ctx),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-l", EditPath, ctx),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-l", EditPath, ctx),
        KeyBinding::new("escape", Dismiss, Some("Workspace")),
    ]);
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Side {
    Local,
    Remote,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Local => Side::Remote,
            Side::Remote => Side::Local,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Name,
    Size,
    Modified,
}

pub struct Pane {
    pub path: String,
    pub entries: Vec<Entry>,
    pub selected: HashSet<String>,
    pub anchor: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub sort: SortBy,
    pub sort_desc: bool,
    pub show_hidden: bool,
}

impl Pane {
    pub fn new(path: String) -> Self {
        Pane {
            path,
            entries: vec![],
            selected: HashSet::new(),
            anchor: None,
            loading: false,
            error: None,
            sort: SortBy::Name,
            sort_desc: false,
            show_hidden: true,
        }
    }

    pub fn visible(&self) -> Vec<&Entry> {
        let mut v: Vec<&Entry> = self.entries.iter().filter(|e| self.show_hidden || !e.name.starts_with('.')).collect();
        v.sort_by(|a, b| {
            let ord = match self.sort {
                SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Modified => a.modified.cmp(&b.modified),
            };
            let ord = if self.sort_desc { ord.reverse() } else { ord };
            b.is_dir.cmp(&a.is_dir).then(ord)
        });
        v
    }

    pub fn selected_entries(&self) -> Vec<Entry> {
        self.entries.iter().filter(|e| self.selected.contains(&e.name)).cloned().collect()
    }
}

/// Payload carried while dragging rows between the panes.
#[derive(Clone)]
pub struct DraggedFiles {
    pub side: Side,
    pub paths: Vec<String>,
    pub label: String,
}

impl Render for DraggedFiles {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(6.))
            .bg(t.surface2)
            .border_1()
            .border_color(t.border)
            .shadow_lg()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(t.ink)
            .child(icon(if self.paths.len() > 1 { "copy" } else { "file" }, 14., t.ink))
            .child(self.label.clone())
            .child(div().text_color(t.muted).font_weight(FontWeight::NORMAL).child(match self.side {
                Side::Local => tr("→ remote"),
                Side::Remote => tr("→ local"),
            }))
    }
}

pub enum Modal {
    Site(Entity<SiteEditor>),
    Prompt(PromptState),
    IconPicker(IconPicker),
}

pub struct ContextMenu {
    pub pos: Point<Pixels>,
    pub side: Side,
    pub entry: Option<Entry>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DrawerTab {
    Active,
    Queue,
    Editing,
    Done,
    Failed,
    Log,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Main,
    Settings,
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

pub struct Workspace {
    pub core: Arc<Core>,
    pub focus: FocusHandle,
    pub local_focus: FocusHandle,
    pub remote_focus: FocusHandle,
    pub active: Option<String>,
    pub local: Pane,
    pub remotes: HashMap<String, Pane>,
    pub drawer_open: bool,
    pub drawer_tab: DrawerTab,
    pub connecting: HashSet<String>,
    pub toasts: Vec<Toast>,
    pub modal: Option<Modal>,
    pub menu: Option<ContextMenu>,
    pub view: View,
    pub settings: SettingsState,
    pub rail_menu: Option<super::rail::RailMenu>,
    pub path_edit: Option<PathEdit>,
    pub last_side: Side,
    pub update: super::updates::UpdateState,
    pub edits: Vec<super::edit::EditWatch>,
    pub asks: std::collections::VecDeque<super::ask::Ask>,
    pub ask_focus: FocusHandle,
    /// SmartFTP runs as administrator: Explorer cannot drop files on it (Windows UIPI).
    pub elevated_notice: bool,
    last_version: u64,
    seen_jobs: HashSet<u64>,
    last_log: Option<chrono::DateTime<chrono::Local>>,
    known_sessions: HashSet<String>,
    _subs: Vec<Subscription>,
}

impl Workspace {
    pub fn new(core: Arc<Core>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let start_local = core
            .config
            .lock()
            .last_local_dir
            .clone()
            .filter(|p| std::path::Path::new(p).is_dir())
            .or_else(|| dirs::home_dir().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| "/".into());
        let mut subs = vec![];
        subs.push(cx.observe_window_appearance(window, |this, window, cx| {
            let a = appearance_of(window.appearance());
            let core_cfg = cx.global::<Themes>().os_appearance;
            if a != core_cfg {
                cx.update_global::<Themes, _>(|th, _| th.os_appearance = a);
                this.apply_theme(cx);
            }
        }));
        let settings = SettingsState::new(&core, window, cx);
        let mut ws = Workspace {
            core,
            focus: cx.focus_handle(),
            local_focus: cx.focus_handle(),
            remote_focus: cx.focus_handle(),
            active: None,
            local: Pane::new(start_local),
            remotes: HashMap::new(),
            drawer_open: true,
            drawer_tab: DrawerTab::Active,
            connecting: HashSet::new(),
            toasts: vec![],
            modal: None,
            menu: None,
            view: View::Main,
            settings,
            rail_menu: None,
            path_edit: None,
            last_side: Side::Local,
            update: super::updates::UpdateState::Idle,
            edits: vec![],
            asks: Default::default(),
            ask_focus: cx.focus_handle(),
            elevated_notice: crate::platform::is_elevated(),
            last_version: 0,
            seen_jobs: HashSet::new(),
            last_log: None,
            known_sessions: HashSet::new(),
            _subs: subs,
        };
        ws.local.show_hidden = ws.core.config.lock().interface.show_hidden;
        ws.load_local(cx);
        if ws.core.config.lock().interface.check_updates {
            ws.check_for_updates(false, cx);
        }
        // Poll the shared core for changes made by transfers or MCP agents.
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(200)).await;
                let alive = this.update_in(cx, |this, window, cx| {
                    this.poll(cx);
                    this.poll_edits(window, cx);
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        ws
    }

    pub fn apply_theme(&self, cx: &mut App) {
        let cfg = self.core.config.lock().clone();
        cx.update_global::<Themes, _>(|th, _| th.select(&cfg));
        cx.refresh_windows();
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        let v = self.core.version();
        let jobs = self.core.jobs();
        let running = jobs.iter().any(|j| matches!(*j.state.lock(), JobState::Running));
        self.toasts.retain(|t| t.at.elapsed() < Duration::from_secs(6));
        if v == self.last_version && !running && self.toasts.is_empty() {
            return;
        }
        self.last_version = v;

        // sessions opened / closed elsewhere (e.g. by an agent)
        let sessions = self.core.sessions();
        let ids: HashSet<String> = sessions.iter().map(|s| s.id.clone()).collect();
        for s in &sessions {
            if !self.known_sessions.contains(&s.id) {
                self.known_sessions.insert(s.id.clone());
                if s.origin.is_agent() {
                    self.toast(trf("{agent} opened a session to {site}", &[("agent", &s.origin.label()), ("site", &s.site.name)]), false);
                }
                if self.active.is_none() {
                    self.activate(&s.id, cx);
                }
            }
        }
        self.known_sessions.retain(|id| ids.contains(id));
        self.remotes.retain(|id, _| ids.contains(id));
        if let Some(a) = &self.active
            && !ids.contains(a)
        {
            self.active = sessions.first().map(|s| s.id.clone());
            if let Some(id) = self.active.clone() {
                self.activate(&id, cx);
            }
        }

        // refresh panes affected by finished transfers
        let mut refresh_local = false;
        let mut refresh_remote: HashSet<String> = HashSet::new();
        for j in &jobs {
            let st = j.state.lock().clone();
            if st.finished() && self.seen_jobs.insert(j.id) {
                match j.direction {
                    Direction::Upload => {
                        if let Some(p) = self.remotes.get(&j.session_id)
                            && j.remote.starts_with(p.path.trim_end_matches('/'))
                        {
                            refresh_remote.insert(j.session_id.clone());
                        }
                    }
                    Direction::Download => {
                        if j.local.starts_with(&self.local.path) {
                            refresh_local = true;
                        }
                    }
                }
                if let JobState::Failed(e) = &st {
                    self.toast(trf("{name} failed: {error}", &[("name", &j.name), ("error", e)]), true);
                }
            }
        }
        // agents changing files in a folder we are showing
        let log = self.core.log_entries();
        let newest = log.first().map(|l| l.time);
        if let Some(since) = self.last_log {
            for l in log.iter().take_while(|l| l.time > since) {
                if !l.ok || !l.origin.is_agent() || !matches!(l.action.as_str(), "mkdir" | "delete" | "rename" | "write_file" | "chmod") {
                    continue;
                }
                for s in &sessions {
                    if let Some(p) = self.remotes.get(&s.id)
                        && let Some(rest) = l.target.strip_prefix(&format!("{}:", s.site.name))
                        && rest.starts_with(p.path.trim_end_matches('/'))
                    {
                        refresh_remote.insert(s.id.clone());
                    }
                }
            }
        }
        if newest.is_some() {
            self.last_log = newest;
        }
        if refresh_local {
            self.load_local(cx);
        }
        for id in refresh_remote {
            self.load_remote(&id, cx);
        }
        cx.notify();
    }

    // ------------------------------------------------------------ helpers

    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toasts.push(Toast { text: text.into(), error, at: Instant::now() });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    /// Run a future on the tokio runtime and deliver its result back on the UI thread.
    pub fn run<T, F>(&self, cx: &mut Context<Self>, fut: F, then: impl FnOnce(&mut Self, Result<T>, &mut Context<Self>) + 'static)
    where
        T: Send + 'static,
        F: Future<Output = Result<T>> + Send + 'static,
    {
        let handle = self.core.rt.spawn(fut);
        cx.spawn(async move |this, cx| {
            let r = handle.await.unwrap_or_else(|e| Err(anyhow!("task failed: {e}")));
            this.update(cx, |this, cx| {
                then(this, r, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn session(&self) -> Option<Arc<Session>> {
        self.active.as_ref().and_then(|id| self.core.session(id).ok())
    }

    pub fn pane(&self, side: Side) -> Option<&Pane> {
        match side {
            Side::Local => Some(&self.local),
            Side::Remote => self.active.as_ref().and_then(|id| self.remotes.get(id)),
        }
    }

    pub fn pane_mut(&mut self, side: Side) -> Option<&mut Pane> {
        match side {
            Side::Local => Some(&mut self.local),
            Side::Remote => match &self.active {
                Some(id) => self.remotes.get_mut(id),
                None => None,
            },
        }
    }

    /// Icon and color for a session: the saved site's customization wins.
    pub fn identity(&self, site: &Site) -> (String, String) {
        let cfg = self.core.config.lock();
        match cfg.sites.iter().find(|s| s.id == site.id) {
            Some(s) => (s.icon.clone(), s.color.clone()),
            None => (site.icon.clone(), site.color.clone()),
        }
    }

    // ------------------------------------------------------------ loading

    pub fn load_local(&mut self, cx: &mut Context<Self>) {
        let path = self.local.path.clone();
        match list_local(std::path::Path::new(&path)) {
            Ok(entries) => {
                self.local.entries = entries;
                self.local.error = None;
                let names: HashSet<_> = self.local.entries.iter().map(|e| e.name.clone()).collect();
                self.local.selected.retain(|n| names.contains(n));
            }
            Err(e) => self.local.error = Some(e.to_string()),
        }
        cx.notify();
    }

    pub fn load_remote(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let Ok(session) = self.core.session(session_id) else { return };
        let Some(pane) = self.remotes.get_mut(session_id) else { return };
        pane.loading = true;
        let path = pane.path.clone();
        let core = self.core.clone();
        let sid = session_id.to_string();
        self.run(cx, async move { core.list(&session, &path).await.map(|e| (path, e)) }, move |this, r, _| {
            let Some(pane) = this.remotes.get_mut(&sid) else { return };
            pane.loading = false;
            match r {
                Ok((path, mut entries)) if path == pane.path => {
                    sort_entries(&mut entries);
                    let names: HashSet<_> = entries.iter().map(|e| e.name.clone()).collect();
                    pane.selected.retain(|n| names.contains(n));
                    pane.entries = entries;
                    pane.error = None;
                }
                Ok(_) => {}
                Err(e) => pane.error = Some(format!("{e:#}")),
            }
        });
    }

    pub fn navigate(&mut self, side: Side, path: String, cx: &mut Context<Self>) {
        match side {
            Side::Local => {
                if !std::path::Path::new(&path).is_dir() {
                    self.toast(trf("{path} is not a folder", &[("path", &path)]), true);
                    return;
                }
                self.local.path = path.clone();
                self.local.selected.clear();
                self.local.anchor = None;
                self.load_local(cx);
                self.core.update_config(|c| c.last_local_dir = Some(path));
            }
            Side::Remote => {
                let Some(id) = self.active.clone() else { return };
                if let Some(p) = self.remotes.get_mut(&id) {
                    p.path = path.clone();
                    p.selected.clear();
                    p.anchor = None;
                    p.entries.clear();
                }
                if let Ok(s) = self.core.session(&id) {
                    *s.cwd.lock() = path;
                }
                self.load_remote(&id, cx);
            }
        }
    }

    pub fn go_up(&mut self, side: Side, cx: &mut Context<Self>) {
        let Some(p) = self.pane(side) else { return };
        let parent = match side {
            Side::Local => std::path::Path::new(&p.path).parent().map(|p| p.to_string_lossy().to_string()),
            Side::Remote => (p.path != "/").then(|| parent_remote(&p.path)),
        };
        if let Some(parent) = parent {
            self.navigate(side, parent, cx);
        }
    }

    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        self.load_local(cx);
        if let Some(id) = self.active.clone() {
            self.load_remote(&id, cx);
        }
    }

    // ------------------------------------------------------------ sessions

    pub fn activate(&mut self, id: &str, cx: &mut Context<Self>) {
        let Ok(session) = self.core.session(id) else { return };
        self.active = Some(id.to_string());
        self.view = View::Main;
        if !self.remotes.contains_key(id) {
            let cwd = session.cwd.lock().clone();
            let mut pane = Pane::new(cwd);
            pane.show_hidden = self.core.config.lock().interface.show_hidden;
            self.remotes.insert(id.to_string(), pane);
            self.load_remote(id, cx);
            if !session.site.local_dir.is_empty() && std::path::Path::new(&session.site.local_dir).is_dir() {
                self.navigate(Side::Local, session.site.local_dir.clone(), cx);
            }
        }
        cx.notify();
    }

    pub fn connect_site(&mut self, site: Site, cx: &mut Context<Self>) {
        if let Some(open) = self.core.sessions().into_iter().find(|s| s.site.id == site.id) {
            self.activate(&open.id.clone(), cx);
            return;
        }
        if !self.connecting.insert(site.id.clone()) {
            return;
        }
        let core = self.core.clone();
        let site_id = site.id.clone();
        let name = site.name.clone();
        self.run(cx, async move { core.connect(site, Origin::User).await }, move |this, r, cx| {
            this.connecting.remove(&site_id);
            match r {
                Ok(s) => {
                    this.known_sessions.insert(s.id.clone());
                    this.activate(&s.id.clone(), cx);
                }
                Err(e) => this.toast(trf("Could not connect to {site}: {error}", &[("site", &name), ("error", &format!("{e:#}"))]), true),
            }
        });
        cx.notify();
    }

    pub fn quick_connect(&mut self, url: &str, cx: &mut Context<Self>) {
        match Site::from_url(url) {
            Some(site) => self.connect_site(site, cx),
            None => self.toast(tr("Use a URL like sftp://user:password@host:22/path"), true),
        }
    }

    pub fn disconnect(&mut self, id: String, cx: &mut Context<Self>) {
        let core = self.core.clone();
        self.run(cx, async move { core.disconnect(&id, &Origin::User).await }, |this, r, cx| {
            if let Err(e) = r {
                this.toast(format!("{e:#}"), true);
            }
            this.poll(cx);
        });
    }

    // ------------------------------------------------------------ selection

    pub fn click_row(&mut self, side: Side, name: String, ev: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.last_side = side;
        self.menu = None;
        window.focus(match side {
            Side::Local => &self.local_focus,
            Side::Remote => &self.remote_focus,
        });
        let m = ev.modifiers();
        if ev.click_count() >= 2 {
            self.open_entry(side, &name, cx);
            return;
        }
        let Some(pane) = self.pane_mut(side) else { return };
        if m.shift
            && let Some(anchor) = pane.anchor.clone()
        {
            let names: Vec<String> = pane.visible().iter().map(|e| e.name.clone()).collect();
            let a = names.iter().position(|n| *n == anchor).unwrap_or(0);
            let b = names.iter().position(|n| *n == name).unwrap_or(0);
            let (lo, hi) = (a.min(b), a.max(b));
            pane.selected = names[lo..=hi].iter().cloned().collect();
        } else if m.secondary() {
            if !pane.selected.remove(&name) {
                pane.selected.insert(name.clone());
            }
            pane.anchor = Some(name);
        } else {
            pane.selected = HashSet::from([name.clone()]);
            pane.anchor = Some(name);
        }
        cx.notify();
    }

    pub fn open_entry(&mut self, side: Side, name: &str, cx: &mut Context<Self>) {
        let Some(pane) = self.pane(side) else { return };
        let Some(e) = pane.entries.iter().find(|e| e.name == name).cloned() else { return };
        if e.is_dir {
            self.navigate(side, e.path, cx);
        } else if self.core.config.lock().interface.double_click_transfers {
            self.transfer(side, vec![e.path], None, cx);
        } else {
            // "Open for editing": same as View / Edit (remote files are watched and uploaded on save)
            self.view_edit(side, e, cx);
        }
    }

    /// Paths to act on: the selection if `name` is part of it, otherwise just `name`.
    pub fn targets(&self, side: Side, name: Option<&str>) -> Vec<Entry> {
        let Some(p) = self.pane(side) else { return vec![] };
        match name {
            Some(n) if !p.selected.contains(n) => p.entries.iter().filter(|e| e.name == n).cloned().collect(),
            _ => p.selected_entries(),
        }
    }

    // ------------------------------------------------------------ transfers

    /// Send paths from `from` side to the other side. `into` overrides the target directory.
    pub fn transfer(&mut self, from: Side, paths: Vec<String>, into: Option<String>, cx: &mut Context<Self>) {
        self.transfer_with(from, paths, into, false, cx);
    }

    /// Add to the transfer queue without starting (FileZilla's "Add to queue").
    pub fn enqueue(&mut self, from: Side, paths: Vec<String>, cx: &mut Context<Self>) {
        self.transfer_with(from, paths, None, true, cx);
    }

    fn transfer_with(&mut self, from: Side, paths: Vec<String>, into: Option<String>, queue_only: bool, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let Some(session) = self.session() else {
            self.toast(tr("Connect to a site first"), true);
            return;
        };
        let core = self.core.clone();
        let n = paths.len();
        if queue_only {
            self.drawer_open = true;
            self.drawer_tab = DrawerTab::Queue;
            self.toast(trf("Added {n} item(s) to the queue", &[("n", &n.to_string())]), false);
        } else if self.core.config.lock().interface.open_drawer_on_transfer {
            self.drawer_open = true;
            self.drawer_tab = DrawerTab::Active;
        }
        match from {
            Side::Local => {
                let dir = into.unwrap_or_else(|| self.pane(Side::Remote).map(|p| p.path.clone()).unwrap_or_default());
                let locals = paths.into_iter().map(PathBuf::from).collect();
                self.run(cx, async move { core.upload(&session, locals, &dir, Origin::User, queue_only).await }, move |this, r, _| {
                    if let Err(e) = r {
                        this.toast(trf("Upload failed: {error}", &[("error", &format!("{e:#}"))]), true);
                    } else if let Some(p) = this.pane_mut(Side::Local) {
                        p.selected.clear();
                    }
                    let _ = n;
                });
            }
            Side::Remote => {
                let dir = PathBuf::from(into.unwrap_or_else(|| self.local.path.clone()));
                self.run(cx, async move { core.download(&session, paths, &dir, Origin::User, queue_only).await }, move |this, r, _| {
                    if let Err(e) = r {
                        this.toast(trf("Download failed: {error}", &[("error", &format!("{e:#}"))]), true);
                    } else if let Some(p) = this.pane_mut(Side::Remote) {
                        p.selected.clear();
                    }
                });
            }
        }
        cx.notify();
    }

    // ------------------------------------------------------------ file ops

    pub fn delete_entries(&mut self, side: Side, entries: Vec<Entry>, _window: &mut Window, cx: &mut Context<Self>) {
        if entries.is_empty() {
            return;
        }
        if !self.core.config.lock().interface.confirm_delete {
            self.do_delete(side, entries, cx);
            return;
        }
        let site = self.session().map(|s| s.site.name.clone()).unwrap_or_default();
        let n = entries.len().to_string();
        let one = entries[0].name.clone();
        let message = match (entries.len() == 1, side) {
            (true, Side::Local) => trf("Delete \"{name}\" from this computer?", &[("name", &one)]),
            (true, Side::Remote) => trf("Delete \"{name}\" from {site}?", &[("name", &one), ("site", &site)]),
            (false, Side::Local) => trf("Delete {n} items from this computer?", &[("n", &n)]),
            (false, Side::Remote) => trf("Delete {n} items from {site}?", &[("n", &n), ("site", &site)]),
        };
        let answer = self.ask(
            super::ask::AskLevel::Warning,
            &message,
            Some(tr("Folders are deleted with everything inside. This cannot be undone.")),
            &[tr("Delete"), tr("Cancel")],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            this.update(cx, |this, cx| this.do_delete(side, entries, cx)).ok();
        })
        .detach();
    }

    fn do_delete(&mut self, side: Side, entries: Vec<Entry>, cx: &mut Context<Self>) {
        match side {
            Side::Local => {
                for e in entries {
                    let r = if e.is_dir { std::fs::remove_dir_all(&e.path) } else { std::fs::remove_file(&e.path) };
                    if let Err(err) = r {
                        self.toast(format!("{}: {err}", e.name), true);
                    }
                }
                self.load_local(cx);
            }
            Side::Remote => {
                let Some(session) = self.session() else { return };
                let core = self.core.clone();
                let sid = session.id.clone();
                self.run(
                    cx,
                    async move {
                        let mut errors = vec![];
                        for e in entries {
                            if let Err(err) = core.delete(&session, &e.path, true, &Origin::User).await {
                                errors.push(format!("{}: {err:#}", e.name));
                            }
                        }
                        Ok(errors)
                    },
                    move |this, r, cx| {
                        for e in r.unwrap_or_default() {
                            this.toast(e, true);
                        }
                        this.load_remote(&sid, cx);
                    },
                );
            }
        }
    }

    pub fn open_prompt(&mut self, kind: PromptKind, side: Side, target: Option<Entry>, window: &mut Window, cx: &mut Context<Self>) {
        let initial = match (&kind, &target) {
            (PromptKind::Rename, Some(e)) => e.name.clone(),
            (PromptKind::Chmod, Some(e)) => e.permissions.as_deref().map(perm_to_octal).unwrap_or_else(|| "644".into()),
            _ => String::new(),
        };
        self.open_prompt_with(kind, side, target, initial, String::new(), window, cx);
    }

    pub fn open_folder_rename(&mut self, folder_id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open_prompt_with(PromptKind::RenameFolder, Side::Remote, None, name, folder_id, window, cx);
    }

    #[allow(clippy::too_many_arguments)]
    fn open_prompt_with(
        &mut self,
        kind: PromptKind,
        side: Side,
        target: Option<Entry>,
        initial: String,
        extra: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = match kind {
            PromptKind::NewFolder => tr("Folder name"),
            PromptKind::NewFile => tr("File name"),
            PromptKind::Rename => tr("New name"),
            PromptKind::Chmod => tr("Octal mode, e.g. 755"),
            PromptKind::QuickConnect => "sftp://user:password@host:22/path",
            PromptKind::RenameFolder => tr("Folder name"),
        };
        let input = cx.new(|cx| {
            let i = TextInput::new(cx, placeholder).with_text(initial.clone());
            if kind == PromptKind::QuickConnect { i.mono() } else { i }
        });
        if matches!(kind, PromptKind::Rename) {
            input.update(cx, |i, cx| i.select_stem(cx));
        } else {
            input.update(cx, |i, cx| i.select_all_text(cx));
        }
        let sub = cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Submit => this.submit_prompt(window, cx),
            InputEvent::Cancel => {
                this.modal = None;
                cx.notify();
            }
            InputEvent::Changed => {}
        });
        window.focus(&input.focus_handle(cx));
        self.modal = Some(Modal::Prompt(PromptState { kind, side, target, extra, input, _sub: sub }));
        self.menu = None;
        cx.notify();
    }

    pub fn submit_prompt(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(Modal::Prompt(p)) = self.modal.take() else { return };
        let value = p.input.read(cx).text().trim().to_string();
        if value.is_empty() {
            return;
        }
        match p.kind {
            PromptKind::QuickConnect => {
                self.quick_connect(&value, cx);
                return;
            }
            PromptKind::RenameFolder => {
                let id = p.extra.clone();
                self.core.update_config(|c| {
                    if let Some((name, _, _)) = c.folder_mut(&id) {
                        *name = value;
                    }
                    c.sync_rail();
                });
                cx.notify();
                return;
            }
            _ => {}
        }
        let side = p.side;
        let dir = self.pane(side).map(|p| p.path.clone()).unwrap_or_default();
        match side {
            Side::Local => {
                let base = std::path::Path::new(&dir);
                let r = match (&p.kind, &p.target) {
                    (PromptKind::NewFolder, _) => std::fs::create_dir(base.join(&value)),
                    (PromptKind::NewFile, _) => std::fs::File::create_new(base.join(&value)).map(|_| ()),
                    (PromptKind::Rename, Some(e)) => std::fs::rename(&e.path, base.join(&value)),
                    _ => Ok(()),
                };
                if let Err(e) = r {
                    self.toast(e.to_string(), true);
                }
                self.load_local(cx);
            }
            Side::Remote => {
                let Some(session) = self.session() else { return };
                let core = self.core.clone();
                let sid = session.id.clone();
                let kind = p.kind;
                let target = p.target.clone();
                self.run(
                    cx,
                    async move {
                        let o = Origin::User;
                        match (kind, target) {
                            (PromptKind::NewFolder, _) => core.mkdir(&session, &join_remote(&dir, &value), &o).await.map(|_| ()),
                            (PromptKind::NewFile, _) => core.write_text(&session, &join_remote(&dir, &value), b"", &o).await.map(|_| ()),
                            (PromptKind::Rename, Some(e)) => core.rename(&session, &e.path, &join_remote(&dir, &value), &o).await,
                            (PromptKind::Chmod, Some(e)) => {
                                let mode = u32::from_str_radix(&value, 8).map_err(|_| anyhow!("{}", tr("mode must be octal, e.g. 755")))?;
                                core.chmod(&session, &e.path, mode, &o).await
                            }
                            _ => Ok(()),
                        }
                    },
                    move |this, r, cx| {
                        if let Err(e) = r {
                            this.toast(format!("{e:#}"), true);
                        }
                        this.load_remote(&sid, cx);
                    },
                );
            }
        }
        cx.notify();
    }

    // ------------------------------------------------------------ dialogs

    pub fn open_site_editor(&mut self, site: Option<Site>, window: &mut Window, cx: &mut Context<Self>) {
        let core = self.core.clone();
        let editor = cx.new(|cx| SiteEditor::new(core, site, window, cx));
        let sub = cx.subscribe_in(&editor, window, |this, _, ev: &SiteEditorEvent, _, cx| {
            match ev {
                SiteEditorEvent::Close => {}
                SiteEditorEvent::Connect(site) => this.connect_site(site.clone(), cx),
            }
            this.modal = None;
            cx.notify();
        });
        self._subs.push(sub);
        self.modal = Some(Modal::Site(editor));
        self.menu = None;
        cx.notify();
    }

    pub fn open_icon_picker(&mut self, site_id: String, anchor: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(site) = self.core.config.lock().sites.iter().find(|s| s.id == site_id).cloned() {
            self.modal = Some(Modal::IconPicker(IconPicker::new(site, anchor)));
        } else if let Some(s) = self.core.sessions().into_iter().find(|s| s.site.id == site_id) {
            // quick-connect session without a saved site: save it so the identity sticks
            let site = s.site.clone();
            self.modal = Some(Modal::IconPicker(IconPicker::new(site, anchor)));
        }
        cx.notify();
    }

    // ------------------------------------------------------------ actions

    fn act_delete(&mut self, _: &DeleteSelected, window: &mut Window, cx: &mut Context<Self>) {
        let side = self.last_side;
        let entries = self.targets(side, None);
        self.delete_entries(side, entries, window, cx);
    }

    fn act_open(&mut self, _: &OpenSelected, _: &mut Window, cx: &mut Context<Self>) {
        let side = self.last_side;
        let sel = self.targets(side, None);
        match sel.as_slice() {
            [one] if one.is_dir => self.navigate(side, one.path.clone(), cx),
            [] => {}
            many => {
                let paths = many.iter().map(|e| e.path.clone()).collect();
                self.transfer(side, paths, None, cx)
            }
        }
    }

    fn act_up(&mut self, _: &GoUp, _: &mut Window, cx: &mut Context<Self>) {
        self.go_up(self.last_side, cx);
    }

    fn act_refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        self.refresh_all(cx);
    }

    fn act_rename(&mut self, _: &RenameSelected, window: &mut Window, cx: &mut Context<Self>) {
        let side = self.last_side;
        if let [one] = self.targets(side, None).as_slice() {
            self.open_prompt(PromptKind::Rename, side, Some(one.clone()), window, cx);
        }
    }

    fn act_new_folder(&mut self, _: &NewFolder, window: &mut Window, cx: &mut Context<Self>) {
        self.open_prompt(PromptKind::NewFolder, self.last_side, None, window, cx);
    }

    fn act_select_all(&mut self, _: &SelectAllRows, _: &mut Window, cx: &mut Context<Self>) {
        let side = self.last_side;
        if let Some(p) = self.pane_mut(side) {
            p.selected = p.entries.iter().map(|e| e.name.clone()).collect();
        }
        cx.notify();
    }

    fn act_dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        if self.menu.take().is_none()
            && self.rail_menu.take().is_none()
            && self.path_edit.take().is_none()
            && self.modal.take().is_none()
            && self.view == View::Settings
        {
            self.view = View::Main;
        }
        cx.notify();
    }

    fn act_edit_file(&mut self, _: &EditFile, _: &mut Window, cx: &mut Context<Self>) {
        let side = self.last_side;
        if let [one] = self.targets(side, None).as_slice()
            && !one.is_dir
        {
            self.view_edit(side, one.clone(), cx);
        }
    }

    fn act_edit_path(&mut self, _: &EditPath, window: &mut Window, cx: &mut Context<Self>) {
        self.start_path_edit(self.last_side, window, cx);
    }

    // ------------------------------------------------------------ path editing

    /// Turn a pane's breadcrumb bar into a text field with the current path.
    pub fn start_path_edit(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.pane(side).map(|p| p.path.clone()) else { return };
        let input = cx.new(|cx| {
            TextInput::new(cx, if side == Side::Local { tr("Folder on this computer") } else { "/remote/path" }).mono().with_text(path)
        });
        input.update(cx, |i, cx| i.select_all_text(cx));
        let sub = cx.subscribe_in(&input, window, move |this, input, ev: &InputEvent, _, cx| match ev {
            InputEvent::Submit => {
                let value = input.read(cx).text();
                this.path_edit = None;
                this.go_to_path(side, value.trim(), cx);
            }
            InputEvent::Cancel => {
                this.path_edit = None;
                cx.notify();
            }
            InputEvent::Changed => {}
        });
        window.focus(&input.focus_handle(cx));
        self.path_edit = Some(PathEdit { side, input, _sub: sub });
        cx.notify();
    }

    /// Navigate to a typed path. Local paths accept `~`; remote paths may be relative.
    pub fn go_to_path(&mut self, side: Side, value: &str, cx: &mut Context<Self>) {
        if value.is_empty() {
            return;
        }
        match side {
            Side::Local => {
                let path = crate::core::sftp::expand_tilde(value);
                let p = std::path::Path::new(&path);
                if p.is_dir() {
                    self.navigate(Side::Local, path, cx);
                } else if p.is_file() {
                    // a file path: open its folder and select the file
                    if let (Some(dir), Some(name)) = (p.parent(), p.file_name()) {
                        let name = name.to_string_lossy().to_string();
                        self.navigate(Side::Local, dir.to_string_lossy().to_string(), cx);
                        self.local.selected = HashSet::from([name.clone()]);
                        self.local.anchor = Some(name);
                    }
                } else {
                    self.toast(trf("Folder not found: {path}", &[("path", &path)]), true);
                }
            }
            Side::Remote => {
                let Some(session) = self.session() else { return };
                let current = self.pane(Side::Remote).map(|p| p.path.clone()).unwrap_or_default();
                let target = if value.starts_with('/') {
                    value.to_string()
                } else if value == "~" || value.starts_with("~/") {
                    session.resolve(value)
                } else {
                    join_remote(&current, value)
                };
                let target = normalize_remote(&target);
                let core = self.core.clone();
                let t2 = target.clone();
                self.run(cx, async move { core.is_dir(&session, &t2).await }, move |this, r, cx| match r {
                    Ok(true) => this.navigate(Side::Remote, target, cx),
                    Ok(false) => this.toast(trf("{path} is not a folder", &[("path", &target)]), true),
                    Err(e) => this.toast(format!("{target}: {e:#}"), true),
                });
            }
        }
        cx.notify();
    }

    pub fn browse_local_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr("Open folder").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(p) = paths.first()
            {
                let p = p.to_string_lossy().to_string();
                this.update(cx, |this, cx| this.navigate(Side::Local, p, cx)).ok();
            }
        })
        .detach();
    }

    fn render_menu(&mut self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let menu = self.menu.as_ref()?;
        let t = theme(cx).clone();
        let side = menu.side;
        let entry = menu.entry.clone();
        let connected = self.session().is_some();
        let mut items: Vec<(&'static str, &'static str, u8)> = vec![];
        if let Some(e) = &entry {
            if e.is_dir {
                items.push((tr("Open"), "folder", 0));
            }
            if connected {
                items.push(match side {
                    Side::Local => (tr("Upload"), "arrow-right", 1),
                    Side::Remote => (tr("Download"), "arrow-left", 1),
                });
                items.push((tr("Add to queue"), "list-plus", 10));
            }
            if !e.is_dir {
                items.push((tr("View / Edit"), "file-pen", 11));
            }
            items.push((tr("Rename"), "pencil", 2));
            if side == Side::Remote {
                items.push((tr("Permissions…"), "key-round", 3));
            }
            items.push((tr("Copy path"), "copy", 4));
            if side == Side::Local {
                items.push((tr("Show in file manager"), "folder", 8));
            }
            items.push((tr("Delete"), "trash-2", 5));
        }
        items.push((tr("New folder"), "folder-plus", 6));
        items.push((tr("New file"), "file-plus", 9));
        items.push((tr("Refresh"), "rotate-cw", 7));

        let pos = menu.pos;
        Some(
            deferred(
                anchored().position(pos).snap_to_window().child(
                    div()
                        .id("context-menu")
                        .occlude()
                        .flex()
                        .flex_col()
                        .min_w(px(190.))
                        .p(px(4.))
                        .rounded(px(7.))
                        .bg(t.surface2)
                        .border_1()
                        .border_color(t.border)
                        .shadow_lg()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu = None;
                            cx.notify();
                        }))
                        .children(items.into_iter().map(|(label, ic, code)| {
                            let entry = entry.clone();
                            let danger = code == 5;
                            div()
                                .id(label)
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .h(px(28.))
                                .px(px(8.))
                                .rounded(px(5.))
                                .cursor_pointer()
                                .text_size(px(13.))
                                .text_color(if danger { t.err } else { t.ink })
                                .hover(|s| s.bg(t.hover))
                                .child(icon(ic, 14., if danger { t.err } else { t.muted }))
                                .child(label)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.menu = None;
                                    this.menu_action(code, side, entry.clone(), window, cx);
                                }))
                        })),
                ),
            )
            .with_priority(2),
        )
    }

    fn menu_action(&mut self, code: u8, side: Side, entry: Option<Entry>, window: &mut Window, cx: &mut Context<Self>) {
        let name = entry.as_ref().map(|e| e.name.clone());
        match code {
            0 => {
                if let Some(e) = entry {
                    self.navigate(side, e.path, cx)
                }
            }
            1 => {
                let paths = self.targets(side, name.as_deref()).into_iter().map(|e| e.path).collect();
                self.transfer(side, paths, None, cx);
            }
            2 => self.open_prompt(PromptKind::Rename, side, entry, window, cx),
            3 => self.open_prompt(PromptKind::Chmod, side, entry, window, cx),
            4 => {
                if let Some(e) = entry {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(e.path));
                    self.toast(tr("Path copied"), false);
                }
            }
            5 => {
                let targets = self.targets(side, name.as_deref());
                self.delete_entries(side, targets, window, cx);
            }
            6 => self.open_prompt(PromptKind::NewFolder, side, None, window, cx),
            9 => self.open_prompt(PromptKind::NewFile, side, None, window, cx),
            7 => self.refresh_all(cx),
            10 => {
                let paths = self.targets(side, name.as_deref()).into_iter().map(|e| e.path).collect();
                self.enqueue(side, paths, cx);
            }
            11 => {
                if let Some(e) = entry {
                    self.view_edit(side, e, cx);
                }
            }
            8 => {
                if let Some(e) = entry {
                    cx.reveal_path(std::path::Path::new(&e.path));
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn render_toasts(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        div().absolute().bottom(px(16.)).right(px(16.)).flex().flex_col().gap(px(8.)).children(self.toasts.iter().enumerate().map(
            |(i, toast)| {
                div()
                    .id(("toast", i))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .max_w(px(420.))
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(px(7.))
                    .bg(t.surface2)
                    .border_1()
                    .border_color(if toast.error { t.err.opacity(0.5) } else { t.border })
                    .shadow_lg()
                    .text_size(px(12.))
                    .text_color(t.ink)
                    .child(icon(if toast.error { "circle-alert" } else { "info" }, 14., if toast.error { t.err } else { t.muted }))
                    .child(div().flex_1().child(toast.text.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if i < this.toasts.len() {
                            this.toasts.remove(i);
                        }
                        cx.notify();
                    }))
            },
        ))
    }
}

impl Workspace {
    fn render_elevated_notice(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = theme(cx).clone();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .px(px(14.))
            .py(px(7.))
            .bg(t.warn.opacity(0.12))
            .border_b_1()
            .border_color(t.warn.opacity(0.35))
            .text_size(px(12.))
            .child(icon("circle-alert", 14., t.warn))
            .child(div().flex_1().child(tr(
                "SmartFTP is running as administrator, so Windows blocks dragging files in from Explorer. Close it and open it again from the Start menu to fix this.",
            )))
            .child(button("dismiss-elevated", tr("Dismiss"), None, BtnKind::Ghost, cx).on_click(cx.listener(|this, _, _, cx| {
                this.elevated_notice = false;
                cx.notify();
            })))
            .into_any_element()
    }
}

pub struct PathEdit {
    pub side: Side,
    pub input: Entity<TextInput>,
    _sub: Subscription,
}

/// Resolve `.` and `..` segments of an absolute remote path.
pub fn normalize_remote(path: &str) -> String {
    let mut parts: Vec<&str> = vec![];
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("/{}", parts.join("/"))
}

pub fn perm_to_octal(p: &str) -> String {
    let chars: Vec<char> = p.chars().collect();
    if chars.len() < 10 {
        return "644".into();
    }
    let mut out = String::new();
    for g in 0..3 {
        let mut v = 0;
        if chars[1 + g * 3] == 'r' {
            v += 4;
        }
        if chars[2 + g * 3] == 'w' {
            v += 2;
        }
        if matches!(chars[3 + g * 3], 'x' | 's' | 't') {
            v += 1;
        }
        out.push(char::from(b'0' + v));
    }
    out
}

/// Plain tooltip view.
pub struct Tip(pub SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(5.))
            .bg(t.ink)
            .text_color(t.bg)
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(self.0.clone())
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let rail = self.render_rail(cx);
        let content = match self.view {
            View::Main => self.render_main(window, cx).into_any_element(),
            View::Settings => self.render_settings(window, cx).into_any_element(),
        };
        let menu = self.render_menu(cx);
        let rail_menu = self.render_rail_menu(cx);
        let modal = self.render_modal(window, cx);
        let toasts = self.render_toasts(cx);
        let ask = self.render_ask(window, cx);
        let notice = self.elevated_notice.then(|| self.render_elevated_notice(cx));
        let titlebar = self.render_titlebar(window, cx);
        // The title bar must not sit inside an element that handles mouse-down (e.g. focus
        // tracking): gpui offers caption clicks to the app first, and a handled click would
        // never reach Windows' drag / minimize / maximize / close handling.
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .text_color(t.ink)
            .font_family(super::UI_FONT)
            .text_size(px(13.))
            .child(titlebar)
            .children(notice)
            .child(
                div()
                    .id("workspace")
                    .key_context("Workspace")
                    .track_focus(&self.focus)
                    .on_action(cx.listener(Self::act_delete))
                    .on_action(cx.listener(Self::act_open))
                    .on_action(cx.listener(Self::act_up))
                    .on_action(cx.listener(Self::act_refresh))
                    .on_action(cx.listener(Self::act_rename))
                    .on_action(cx.listener(Self::act_new_folder))
                    .on_action(cx.listener(Self::act_select_all))
                    .on_action(cx.listener(Self::act_dismiss))
                    .on_action(cx.listener(Self::act_edit_path))
                    .on_action(cx.listener(Self::act_edit_file))
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(rail)
                    .child(div().flex_1().min_w_0().h_full().child(content)),
            )
            .children(menu)
            .children(rail_menu)
            .children(modal)
            .children(ask)
            .child(toasts)
    }
}
