//! In-app update flow: check GitHub Releases, show an "Update" pill, download, verify,
//! replace the executable and restart.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{Context, FontWeight, SharedString, Window, div, prelude::*, px};

use super::components::icon;
use super::theme::theme;
use super::workspace::Workspace;
use crate::i18n::{tr, trf};
use crate::update::{self, Release};

#[derive(Clone)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading { release: Release, done: Arc<AtomicU64> },
    Failed(String),
}

impl Workspace {
    /// Ask GitHub for a newer release. `manual` reports "up to date" / errors as toasts.
    pub fn check_for_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if matches!(self.update, UpdateState::Checking | UpdateState::Downloading { .. }) {
            return;
        }
        self.update = UpdateState::Checking;
        self.run(cx, async { tokio::task::spawn_blocking(update::check).await.map_err(|e| anyhow::anyhow!(e))? }, move |this, r, _| {
            this.update = match r {
                Ok(Some(rel)) => {
                    this.toast(trf("SmartFTP {version} is available.", &[("version", &rel.version.to_string())]), false);
                    UpdateState::Available(rel)
                }
                Ok(None) => {
                    if manual {
                        this.toast(tr("SmartFTP is up to date."), false);
                    }
                    UpdateState::UpToDate
                }
                Err(e) => {
                    if manual {
                        this.toast(format!("{}: {e:#}", tr("Could not check for updates")), true);
                    }
                    UpdateState::Failed(format!("{e:#}"))
                }
            };
        });
        cx.notify();
    }

    pub fn confirm_install(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let UpdateState::Available(rel) = self.update.clone() else { return };
        let running = self.core.jobs().iter().any(|j| !j.state.lock().finished());
        let detail =
            if running { tr("Transfers in progress will be interrupted.") } else { tr("SmartFTP restarts when the update is installed.") };
        let answer = self.ask(
            super::ask::AskLevel::Info,
            trf("Install SmartFTP {version}?", &[("version", &rel.version.to_string())]),
            Some(detail),
            &[tr("Install and restart"), tr("Later")],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() == Some(0) {
                this.update(cx, |this, cx| this.install_update(rel, cx)).ok();
            }
        })
        .detach();
    }

    fn install_update(&mut self, release: Release, cx: &mut Context<Self>) {
        let done = Arc::new(AtomicU64::new(0));
        self.update = UpdateState::Downloading { release: release.clone(), done: done.clone() };
        let rel = release.clone();
        self.run(
            cx,
            async move { tokio::task::spawn_blocking(move || update::install(&rel, &done)).await.map_err(|e| anyhow::anyhow!(e))? },
            move |this, r, cx| match r {
                Ok(update::Installed::InstallerLaunched) => cx.quit(),
                Ok(update::Installed::Replaced) => {
                    if let Err(e) = update::restart() {
                        this.toast(format!("{}: {e:#}", tr("Updated. Restart SmartFTP to use the new version")), false);
                        this.update = UpdateState::UpToDate;
                        return;
                    }
                    cx.quit();
                }
                Err(e) => {
                    this.toast(format!("{}: {e:#}", tr("Update failed")), true);
                    this.update = UpdateState::Available(release);
                }
            },
        );
        cx.notify();
    }

    /// Pill shown in the title bar while an update is available or downloading.
    pub fn render_update_pill(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let t = theme(cx).clone();
        let (label, progress): (String, Option<f32>) = match &self.update {
            UpdateState::Available(rel) => (trf("Update to {version}", &[("version", &rel.version.to_string())]), None),
            UpdateState::Downloading { release, done } => {
                let total = release.download_size().max(1) as f32;
                let frac = (done.load(Ordering::Relaxed) as f32 / total).min(1.0);
                (trf("Updating… {pct}%", &[("pct", &format!("{:.0}", frac * 100.))]), Some(frac))
            }
            _ => return None,
        };
        let clickable = progress.is_none();
        Some(
            div()
                .id("update-pill")
                .occlude()
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(22.))
                .px(px(10.))
                .mr(px(8.))
                .rounded(px(11.))
                .bg(t.signal.opacity(0.16))
                .border_1()
                .border_color(t.signal.opacity(0.5))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.signal)
                .child(icon("download", 12., t.signal))
                .child(SharedString::from(label))
                .when(clickable, |d| d.cursor_pointer().hover(|s| s.bg(t.signal.opacity(0.26))))
                .when(clickable, |d| d.on_click(cx.listener(|this, _, window, cx| this.confirm_install(window, cx))))
                .into_any_element(),
        )
    }

    /// Short status for the Settings → General "Updates" row.
    pub fn update_status(&self) -> String {
        match &self.update {
            UpdateState::Idle => String::new(),
            UpdateState::Checking => tr("Checking…").to_string(),
            UpdateState::UpToDate => tr("Up to date").to_string(),
            UpdateState::Available(r) => trf("Version {version} is available", &[("version", &r.version.to_string())]),
            UpdateState::Downloading { .. } => tr("Downloading the update…").to_string(),
            UpdateState::Failed(e) => format!("{}: {e}", tr("Could not check for updates")),
        }
    }
}
