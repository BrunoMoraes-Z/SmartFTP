//! "View / Edit": download a remote file to a temp folder, open it in the configured editor,
//! watch it, and offer to upload it back (replacing the remote file) whenever it is saved.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context as _, Result, anyhow};
use gpui::{Context, PromptLevel, Window};

use super::workspace::{Side, Workspace};
use crate::core::model::{Entry, Origin};
use crate::i18n::{tr, trf};

/// A remote file being edited locally.
pub struct EditWatch {
    pub id: u64,
    pub session_id: String,
    pub site_name: String,
    pub remote: String,
    pub local: PathBuf,
    /// Modification time and size of the copy we last downloaded or uploaded.
    pub stamp: (Option<SystemTime>, u64),
    /// A prompt for this file is open; don't ask twice.
    pub asking: bool,
    pub uploads: u32,
}

impl EditWatch {
    pub fn name(&self) -> String {
        self.local.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    }
}

fn stamp_of(path: &Path) -> (Option<SystemTime>, u64) {
    match std::fs::metadata(path) {
        Ok(m) => (m.modified().ok(), m.len()),
        Err(_) => (None, 0),
    }
}

/// Editors we can offer as one-click presets, if installed: (label, command).
pub fn detect_editors() -> Vec<(&'static str, String)> {
    let mut out = vec![];
    #[cfg(target_os = "windows")]
    {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
        let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());
        let candidates: [(&'static str, Vec<String>); 5] = [
            ("VS Code", vec![format!("{local}\\Programs\\Microsoft VS Code\\Code.exe"), format!("{pf}\\Microsoft VS Code\\Code.exe")]),
            ("Cursor", vec![format!("{local}\\Programs\\cursor\\Cursor.exe")]),
            ("Notepad++", vec![format!("{pf}\\Notepad++\\notepad++.exe"), format!("{pf86}\\Notepad++\\notepad++.exe")]),
            ("Sublime Text", vec![format!("{pf}\\Sublime Text\\sublime_text.exe"), format!("{pf}\\Sublime Text 3\\sublime_text.exe")]),
            ("Notepad", vec!["C:\\Windows\\System32\\notepad.exe".into()]),
        ];
        for (label, paths) in candidates {
            if let Some(p) = paths.into_iter().find(|p| Path::new(p).exists()) {
                out.push((label, format!("\"{p}\" \"{{file}}\"")));
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        for (label, bin) in [("VS Code", "code"), ("Sublime Text", "subl"), ("gedit", "gedit")] {
            if std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).exists())) {
                out.push((label, format!("{bin} \"{{file}}\"")));
            }
        }
    }
    out
}

/// Split a command line into program + arguments, honoring double quotes.
fn split_command(cmd: &str) -> Vec<String> {
    let mut parts = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in cmd.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    parts.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        parts.push(cur);
    }
    parts
}

/// Open `file` with the configured editor command. `{file}` is replaced by the path; without it
/// the path is appended. An empty command uses the system's default app for the file type.
pub fn open_in_editor(command: &str, file: &Path, cx: &mut gpui::App) -> Result<()> {
    let command = command.trim();
    if command.is_empty() {
        cx.open_with_system(file);
        return Ok(());
    }
    let path = file.to_string_lossy().to_string();
    let mut parts = split_command(command);
    if parts.is_empty() {
        return Err(anyhow!("the editor command is empty"));
    }
    if parts.iter().any(|p| p.contains("{file}")) {
        for p in &mut parts {
            *p = p.replace("{file}", &path);
        }
    } else {
        parts.push(path);
    }
    let program = parts.remove(0);
    let lower = program.to_lowercase();
    // .cmd/.bat launchers (e.g. `code`) need cmd.exe on Windows
    let mut cmd = if cfg!(windows) && (lower.ends_with(".cmd") || lower.ends_with(".bat")) {
        let mut c = std::process::Command::new("cmd");
        c.arg("/C").arg(&program);
        c
    } else {
        std::process::Command::new(&program)
    };
    cmd.args(&parts);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        if lower.ends_with(".cmd") || lower.ends_with(".bat") {
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
    }
    cmd.spawn().with_context(|| format!("could not start the editor ({program})"))?;
    Ok(())
}

impl Workspace {
    /// "View / Edit" on a file. Local files open directly; remote files are downloaded first
    /// and then watched for changes.
    pub fn view_edit(&mut self, side: Side, entry: Entry, cx: &mut Context<Self>) {
        if entry.is_dir {
            return;
        }
        let editor = self.core.config.lock().interface.editor.clone();
        match side {
            Side::Local => {
                if let Err(e) = open_in_editor(&editor, Path::new(&entry.path), cx) {
                    self.toast(format!("{e:#}"), true);
                }
            }
            Side::Remote => {
                let Some(session) = self.session() else { return };
                // already editing this file: just reopen it
                if let Some(w) = self.edits.iter().find(|w| w.session_id == session.id && w.remote == entry.path) {
                    let local = w.local.clone();
                    if let Err(e) = open_in_editor(&editor, &local, cx) {
                        self.toast(format!("{e:#}"), true);
                    }
                    return;
                }
                let core = self.core.clone();
                let remote = entry.path.clone();
                let (sid, site_name) = (session.id.clone(), session.site.name.clone());
                self.toast(trf("Opening {name}…", &[("name", &entry.name)]), false);
                self.run(
                    cx,
                    async move { core.fetch_for_edit(&session, &remote, &Origin::User).await.map(|p| (p, remote)) },
                    move |this, r, cx| match r {
                        Ok((local, remote)) => {
                            let editor = this.core.config.lock().interface.editor.clone();
                            if let Err(e) = open_in_editor(&editor, &local, cx) {
                                this.toast(format!("{e:#}"), true);
                            }
                            let id = this.next_edit_id();
                            this.edits.push(EditWatch {
                                id,
                                session_id: sid,
                                site_name,
                                stamp: stamp_of(&local),
                                remote,
                                local,
                                asking: false,
                                uploads: 0,
                            });
                        }
                        Err(e) => this.toast(trf("Could not open the file: {error}", &[("error", &format!("{e:#}"))]), true),
                    },
                );
            }
        }
    }

    fn next_edit_id(&self) -> u64 {
        self.edits.iter().map(|w| w.id).max().unwrap_or(0) + 1
    }

    /// Called by the workspace poll loop: ask to upload edited files that were saved.
    pub fn poll_edits(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // forget files whose session was closed
        let open: Vec<String> = self.core.sessions().iter().map(|s| s.id.clone()).collect();
        self.edits.retain(|w| open.contains(&w.session_id));
        let changed: Vec<u64> = self
            .edits
            .iter()
            .filter(|w| !w.asking)
            .filter(|w| {
                let now = stamp_of(&w.local);
                now.0.is_some() && now != w.stamp
            })
            .map(|w| w.id)
            .collect();
        for id in changed {
            self.ask_upload(id, window, cx);
        }
    }

    fn ask_upload(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(w) = self.edits.iter_mut().find(|w| w.id == id) else { return };
        w.asking = true;
        w.stamp = stamp_of(&w.local);
        let (name, site, remote) = (w.name(), w.site_name.clone(), w.remote.clone());
        window.activate_window();
        let answer = window.prompt(
            PromptLevel::Info,
            &trf("\"{name}\" was changed.", &[("name", &name)]),
            Some(&trf("Upload it to {site} and replace {path}?", &[("site", &site), ("path", &remote)])),
            &[tr("Upload and replace"), tr("Not now"), tr("Stop editing")],
            cx,
        );
        cx.spawn(async move |this, cx| {
            let choice = answer.await.ok();
            this.update(cx, |this, cx| {
                match choice {
                    Some(0) => this.upload_edit(id, cx),
                    Some(2) => this.stop_edit(id, cx),
                    _ => {}
                }
                if let Some(w) = this.edits.iter_mut().find(|w| w.id == id) {
                    w.asking = false;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn upload_edit(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(w) = self.edits.iter_mut().find(|w| w.id == id) else { return };
        let Ok(session) = self.core.session(&w.session_id) else {
            self.toast(tr("The session for this file was closed."), true);
            return;
        };
        w.stamp = stamp_of(&w.local);
        w.uploads += 1;
        let (local, remote, name) = (w.local.clone(), w.remote.clone(), w.name());
        match self.core.upload_replace(&session, local, remote, Origin::User) {
            Ok(_) => self.toast(trf("Uploading {name}…", &[("name", &name)]), false),
            Err(e) => self.toast(format!("{e:#}"), true),
        }
        cx.notify();
    }

    pub fn stop_edit(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(i) = self.edits.iter().position(|w| w.id == id) {
            let w = self.edits.remove(i);
            let _ = std::fs::remove_file(&w.local);
            if let Some(dir) = w.local.parent() {
                let _ = std::fs::remove_dir(dir);
            }
        }
        cx.notify();
    }

    pub fn reopen_edit(&mut self, id: u64, cx: &mut Context<Self>) {
        let editor = self.core.config.lock().interface.editor.clone();
        if let Some(w) = self.edits.iter().find(|w| w.id == id)
            && let Err(e) = open_in_editor(&editor, &w.local, cx)
        {
            self.toast(format!("{e:#}"), true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_command;

    #[test]
    fn splits_commands() {
        assert_eq!(split_command(r#""C:\Program Files\Code.exe" "{file}""#), vec![r"C:\Program Files\Code.exe", "{file}"]);
        assert_eq!(split_command("code --wait {file}"), vec!["code", "--wait", "{file}"]);
        assert_eq!(split_command("  notepad  "), vec!["notepad"]);
        assert!(split_command("").is_empty());
    }
}
