//! General, Connection and Transfers settings pages (modelled on FileZilla's options).

use std::rc::Rc;

use gpui::{Context, FontWeight, SharedString, div, prelude::*, px};

use super::components::*;
use super::theme::{Theme, theme};
use super::workspace::Workspace;
use crate::core::config::{Config, ExistsAction, FtpMode, app_dir};
use crate::i18n::{self, Lang, tr, trf};

type Setter = Rc<dyn Fn(&mut Config)>;
type ChoiceSetter<T> = Rc<dyn Fn(&mut Config, T)>;

/// Speed limit presets in KiB/s (0 = unlimited).
const SPEED_STEPS: &[u64] = &[0, 64, 128, 256, 512, 1024, 2048, 5120, 10240, 20480, 51200, 102400];

fn speed_label(kib: u64) -> String {
    match kib {
        0 => tr("Unlimited").into(),
        k if k >= 1024 => format!("{} MiB/s", k / 1024),
        k => format!("{k} KiB/s"),
    }
}

fn step_speed(cur: u64, up: bool) -> u64 {
    let i = SPEED_STEPS.iter().position(|s| *s >= cur).unwrap_or(SPEED_STEPS.len() - 1);
    if up { SPEED_STEPS[(i + 1).min(SPEED_STEPS.len() - 1)] } else { SPEED_STEPS[i.saturating_sub(1)] }
}

fn section(title: &str, t: &Theme) -> gpui::Div {
    div().flex().flex_col().p(px(4.)).rounded(px(10.)).bg(t.surface).border_1().border_color(t.border).child(
        div()
            .px(px(14.))
            .pt(px(12.))
            .pb(px(6.))
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(t.muted)
            .child(title.to_string().to_uppercase()),
    )
}

fn row(title: &str, desc: &str, control: impl IntoElement, t: &Theme) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(24.))
        .px(px(14.))
        .py(px(12.))
        .border_t_1()
        .border_color(t.border.opacity(0.6))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(div().font_weight(FontWeight::MEDIUM).child(title.to_string()))
                .when(!desc.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(t.faint).child(desc.to_string()))),
        )
        .child(div().flex_none().child(control))
}

impl Workspace {
    fn set(&self, cx: &mut Context<Self>, f: &Setter) {
        let f = f.clone();
        self.core.update_config(move |c| f(c));
        cx.notify();
    }

    fn stepper(&self, id: &str, value: String, dec: Setter, inc: Setter, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let btn = |key: String, ic: &'static str, f: Setter, cx: &mut Context<Self>| {
            div()
                .id(SharedString::from(key))
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded(px(5.))
                .cursor_pointer()
                .hover(|s| s.bg(t.hover))
                .child(icon(ic, 14., t.muted))
                .on_click(cx.listener(move |this, _, _, cx| this.set(cx, &f)))
        };
        div()
            .flex()
            .items_center()
            .h(px(32.))
            .px(px(2.))
            .rounded(px(7.))
            .bg(t.bg)
            .border_1()
            .border_color(t.border)
            .child(btn(format!("{id}-dec"), "minus", dec, cx))
            .child(div().min_w(px(92.)).text_center().font_family(super::MONO_FONT).text_size(px(12.)).child(value))
            .child(btn(format!("{id}-inc"), "plus", inc, cx))
    }

    fn segmented<T: Copy + PartialEq + 'static>(
        &self,
        id: &str,
        options: &[(&'static str, T)],
        current: T,
        set: ChoiceSetter<T>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<T> {
        let t = theme(cx).clone();
        div().flex().h(px(32.)).p(px(2.)).gap(px(2.)).rounded(px(7.)).bg(t.bg).border_1().border_color(t.border).children(
            options.iter().map(|(label, value)| {
                let on = *value == current;
                let set = set.clone();
                let value = *value;
                div()
                    .id(SharedString::from(format!("{id}-{label}")))
                    .flex()
                    .items_center()
                    .px(px(10.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .when(on, |d| d.bg(t.hover).font_weight(FontWeight::SEMIBOLD))
                    .text_color(if on { t.ink } else { t.muted })
                    .child(*label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let set = set.clone();
                        this.core.update_config(move |c| set(c, value));
                        cx.notify();
                    }))
            }),
        )
    }

    fn switch(&self, id: &str, on: bool, f: Setter, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        toggle(SharedString::from(id.to_string()), on, cx).on_click(cx.listener(move |this, _, _, cx| this.set(cx, &f)))
    }

    fn page_header(title: &str, subtitle: &str, t: &Theme) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
            .child(div().text_color(t.muted).child(subtitle.to_string()))
    }

    /// Language selector: "auto" plus every supported language (shown by its native name).
    fn language_picker(&self, current: &str, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let mut options: Vec<(&'static str, &'static str)> = vec![("auto", tr("Automatic (system)"))];
        options.extend(Lang::ALL.iter().map(|l| (l.code(), l.name())));
        let current = if options.iter().any(|(code, _)| *code == current) { current.to_string() } else { "auto".to_string() };
        div().flex().h(px(32.)).p(px(2.)).gap(px(2.)).rounded(px(7.)).bg(t.bg).border_1().border_color(t.border).children(
            options.into_iter().map(|(code, label)| {
                let on = code == current;
                div()
                    .id(SharedString::from(format!("lang-{code}")))
                    .flex()
                    .items_center()
                    .px(px(10.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .when(on, |d| d.bg(t.hover).font_weight(FontWeight::SEMIBOLD))
                    .text_color(if on { t.ink } else { t.muted })
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.core.update_config(move |c| c.interface.language = code.to_string());
                        i18n::set_lang(i18n::resolve(code));
                        cx.refresh_windows();
                        cx.notify();
                    }))
            }),
        )
    }

    pub fn render_general(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let ui = self.core.config.lock().interface.clone();
        let dir = app_dir();
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(Self::page_header(tr("General"), tr("Language and how the file panes behave."), &t))
            .child(section(tr("Language"), &t).child(row(
                tr("Language"),
                &trf("Automatic follows the system language (currently {lang}).", &[("lang", i18n::detect().name())]),
                self.language_picker(&ui.language, cx),
                &t,
            )))
            .child(
                section(tr("File panes"), &t)
                    .child(row(
                        tr("Show hidden files"),
                        tr("Show dotfiles (e.g. .env, .htaccess) when a pane opens. Toggle per pane with the eye button."),
                        self.switch("show-hidden", ui.show_hidden, Rc::new(|c| c.interface.show_hidden = !c.interface.show_hidden), cx),
                        &t,
                    ))
                    .child(row(
                        tr("Double-clicking a file"),
                        tr("Transfer it to the other side, or open it in your editor like View / Edit (remote files are uploaded back when you save)."),
                        self.segmented(
                            "dbl",
                            &[(tr("Transfer"), true), (tr("Open for editing"), false)],
                            ui.double_click_transfers,
                            Rc::new(|c, v| c.interface.double_click_transfers = v),
                            cx,
                        ),
                        &t,
                    ))
                    .child(row(
                        tr("Confirm before deleting"),
                        tr("Ask before deleting local or remote files and folders."),
                        self.switch(
                            "confirm-del",
                            ui.confirm_delete,
                            Rc::new(|c| c.interface.confirm_delete = !c.interface.confirm_delete),
                            cx,
                        ),
                        &t,
                    ))
                    .child(row(
                        tr("Open the transfer panel"),
                        tr("Expand the transfer panel when a transfer starts."),
                        self.switch(
                            "open-drawer",
                            ui.open_drawer_on_transfer,
                            Rc::new(|c| c.interface.open_drawer_on_transfer = !c.interface.open_drawer_on_transfer),
                            cx,
                        ),
                        &t,
                    )),
            )
            .child(self.render_editor_section(cx))
            .child(
                section(tr("Updates"), &t)
                    .child(row(
                        &trf("Version {version}", &[("version", env!("CARGO_PKG_VERSION"))]),
                        &{
                            let status = self.update_status();
                            if status.is_empty() {
                                tr("Updates are downloaded from GitHub Releases and verified with SHA-256 before installing.").to_string()
                            } else {
                                status
                            }
                        },
                        div().flex().gap(px(8.)).children(self.render_update_pill(cx)).child(
                            button("check-updates", tr("Check now"), Some("refresh-cw"), BtnKind::Outline, cx)
                                .on_click(cx.listener(|this, _, _, cx| this.check_for_updates(true, cx))),
                        ),
                        &t,
                    ))
                    .child(row(
                        tr("Check for updates on startup"),
                        tr("Look for a newer release on GitHub when SmartFTP starts. Nothing is installed without asking."),
                        self.switch(
                            "check-updates-startup",
                            ui.check_updates,
                            Rc::new(|c| c.interface.check_updates = !c.interface.check_updates),
                            cx,
                        ),
                        &t,
                    )),
            )
            .child(section(tr("Data"), &t).child(row(
                tr("Settings folder"),
                &dir.display().to_string(),
                button("open-cfg", tr("Open folder"), Some("folder-open"), BtnKind::Outline, cx).on_click(cx.listener(
                    move |_, _, _, cx| {
                        let d = app_dir();
                        let _ = std::fs::create_dir_all(&d);
                        cx.open_with_system(&d);
                    },
                )),
                &t,
            )))
    }

    fn set_editor(&mut self, command: String, cx: &mut Context<Self>) {
        let input = self.settings.editor_input.clone();
        input.update(cx, |i, cx| i.set_text(command.clone(), cx));
        self.core.update_config(move |c| c.interface.editor = command);
        cx.notify();
    }

    fn render_editor_section(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let current = self.core.config.lock().interface.editor.clone();
        let presets = super::edit::detect_editors();
        let using_default = current.trim().is_empty();
        section(tr("File editor"), &t).child(
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .px(px(14.))
                .py(px(12.))
                .border_t_1()
                .border_color(t.border.opacity(0.6))
                .child(div().font_weight(FontWeight::MEDIUM).child(tr("Open files with")))
                .child(div().text_size(px(12.)).text_color(t.faint).child(tr(
                    "Used by View / Edit (F4). Remote files are downloaded to a temporary folder; when you save, SmartFTP asks whether to upload the file and replace the one on the server.",
                )))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .child(
                            button("editor-default", tr("System default"), if using_default { Some("check") } else { None }, if using_default { BtnKind::Primary } else { BtnKind::Outline }, cx)
                                .on_click(cx.listener(|this, _, _, cx| this.set_editor(String::new(), cx))),
                        )
                        .children(presets.into_iter().map(|(label, command)| {
                            let on = current.trim() == command;
                            let cmd = command.clone();
                            button(SharedString::from(format!("editor-{label}")), label, if on { Some("check") } else { None }, if on { BtnKind::Primary } else { BtnKind::Outline }, cx)
                                .on_click(cx.listener(move |this, _, _, cx| this.set_editor(cmd.clone(), cx)))
                        }))
                        .child(button("editor-browse", tr("Choose program…"), Some("folder-open"), BtnKind::Ghost, cx).on_click(cx.listener(|_this, _, window, cx| {
                            let rx = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(tr("Choose editor").into()) });
                            cx.spawn_in(window, async move |this, cx| {
                                if let Ok(Ok(Some(paths))) = rx.await
                                    && let Some(p) = paths.first()
                                {
                                    let command = format!("\"{}\" \"{{file}}\"", p.display());
                                    this.update(cx, |this, cx| this.set_editor(command, cx)).ok();
                                }
                            })
                            .detach();
                        }))),
                )
                .child(self.settings.editor_input.clone())
                .child(div().text_size(px(11.)).text_color(t.faint).child(tr("Command line. {file} is replaced by the file path; leave empty to use the system default app."))),
        )
    }

    pub fn render_connection(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let c = self.core.config.lock().connection.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(Self::page_header(tr("Connection"), tr("Timeouts, retries and protocol options. Changes apply to new connections."), &t))
            .child(
                section(tr("General"), &t)
                    .child(row(
                        tr("Timeout"),
                        tr("Give up connecting or logging in after this long."),
                        self.stepper(
                            "timeout",
                            format!("{} s", c.timeout_secs),
                            Rc::new(|c| c.connection.timeout_secs = c.connection.timeout_secs.saturating_sub(5).max(5)),
                            Rc::new(|c| c.connection.timeout_secs = (c.connection.timeout_secs + 5).min(600)),
                            cx,
                        ),
                        &t,
                    ))
                    .child(row(
                        tr("Maximum number of retries"),
                        tr("Extra attempts when a connection or login fails."),
                        self.stepper(
                            "retries",
                            format!("{}", c.retries),
                            Rc::new(|c| c.connection.retries = c.connection.retries.saturating_sub(1)),
                            Rc::new(|c| c.connection.retries = (c.connection.retries + 1).min(10)),
                            cx,
                        ),
                        &t,
                    ))
                    .child(row(
                        tr("Delay between failed attempts"),
                        tr("Wait before retrying a failed connection, login or transfer."),
                        self.stepper(
                            "retry-delay",
                            format!("{} s", c.retry_delay_secs),
                            Rc::new(|c| c.connection.retry_delay_secs = c.connection.retry_delay_secs.saturating_sub(1)),
                            Rc::new(|c| c.connection.retry_delay_secs = (c.connection.retry_delay_secs + 1).min(120)),
                            cx,
                        ),
                        &t,
                    ))
                    .child(row(
                        tr("Reconnect automatically"),
                        tr("When the server drops the connection, reconnect and retry the operation once."),
                        self.switch(
                            "auto-reconnect",
                            c.auto_reconnect,
                            Rc::new(|c| c.connection.auto_reconnect = !c.connection.auto_reconnect),
                            cx,
                        ),
                        &t,
                    )),
            )
            .child(section("SFTP", &t).child(row(
                tr("Keep-alive interval"),
                tr("Send a keep-alive when the connection is idle, so routers and servers don't drop it. 0 turns it off."),
                self.stepper(
                    "keepalive",
                    if c.keepalive_secs == 0 { tr("Off").into() } else { format!("{} s", c.keepalive_secs) },
                    Rc::new(|c| c.connection.keepalive_secs = c.connection.keepalive_secs.saturating_sub(15)),
                    Rc::new(|c| c.connection.keepalive_secs = (c.connection.keepalive_secs + 15).min(600)),
                    cx,
                ),
                &t,
            )))
            .child(
                section("FTP / FTPS", &t)
                    .child(row(
                        tr("Transfer mode"),
                        tr("Passive works behind most firewalls. Active needs the server to reach this computer."),
                        self.segmented(
                            "ftp-mode",
                            &[
                                (tr("Passive"), FtpMode::Passive),
                                (tr("Extended passive"), FtpMode::ExtendedPassive),
                                (tr("Active"), FtpMode::Active),
                            ],
                            c.ftp_mode,
                            Rc::new(|c, v| c.connection.ftp_mode = v),
                            cx,
                        ),
                        &t,
                    ))
                    .child(row(
                        tr("Fix passive-mode addresses"),
                        tr("Use the server's address when it replies to PASV with a private IP (common behind NAT)."),
                        self.switch(
                            "nat",
                            c.ftp_nat_workaround,
                            Rc::new(|c| c.connection.ftp_nat_workaround = !c.connection.ftp_nat_workaround),
                            cx,
                        ),
                        &t,
                    )),
            )
    }

    pub fn render_transfers(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let xf = self.core.config.lock().transfers.clone();
        let exists_opts: &[(&str, &str, ExistsAction)] = &[
            (tr("Overwrite"), tr("Always replace the existing file."), ExistsAction::Overwrite),
            (tr("Overwrite if newer"), tr("Replace only when the source file is newer."), ExistsAction::OverwriteIfNewer),
            (tr("Overwrite if different"), tr("Replace when size or date differ (like a sync)."), ExistsAction::OverwriteIfDifferent),
            (tr("Rename"), tr("Keep both: the new copy becomes \"name (1).ext\"."), ExistsAction::Rename),
            (tr("Skip"), tr("Leave the existing file and skip this one."), ExistsAction::Skip),
        ];
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(Self::page_header(tr("Transfers"), tr("Queue concurrency, speed limits and what happens when a file already exists."), &t))
            .child(
                section(tr("Queue"), &t)
                    .child(row(
                        tr("Maximum simultaneous transfers"),
                        tr("How many files are transferred at the same time. Applies immediately."),
                        self.stepper("max-conc", format!("{}", xf.max_concurrent), Rc::new(|c| c.transfers.max_concurrent = c.transfers.max_concurrent.saturating_sub(1).max(1)), Rc::new(|c| c.transfers.max_concurrent = (c.transfers.max_concurrent + 1).min(10)), cx),
                        &t,
                    ))
                    .child(row(
                        tr("Retries for failed transfers"),
                        tr("Retry a file this many times (after the delay set in Connection) before marking it failed."),
                        self.stepper("tr-retries", format!("{}", xf.retries), Rc::new(|c| c.transfers.retries = c.transfers.retries.saturating_sub(1)), Rc::new(|c| c.transfers.retries = (c.transfers.retries + 1).min(10)), cx),
                        &t,
                    ))
                    .child(row(
                        tr("Preserve timestamps"),
                        tr("Give the copy the same modification time as the original (needed for the compare badges and \"overwrite if newer\")."),
                        self.switch("preserve", xf.preserve_timestamps, Rc::new(|c| c.transfers.preserve_timestamps = !c.transfers.preserve_timestamps), cx),
                        &t,
                    )),
            )
            .child(
                section(tr("Speed limits"), &t)
                    .child(row(
                        tr("Download limit"),
                        tr("Shared by all downloads. Applies immediately, also to running transfers."),
                        self.stepper("dl-limit", speed_label(xf.download_limit_kib), Rc::new(|c| c.transfers.download_limit_kib = step_speed(c.transfers.download_limit_kib, false)), Rc::new(|c| c.transfers.download_limit_kib = step_speed(c.transfers.download_limit_kib, true)), cx),
                        &t,
                    ))
                    .child(row(
                        tr("Upload limit"),
                        tr("Shared by all uploads."),
                        self.stepper("ul-limit", speed_label(xf.upload_limit_kib), Rc::new(|c| c.transfers.upload_limit_kib = step_speed(c.transfers.upload_limit_kib, false)), Rc::new(|c| c.transfers.upload_limit_kib = step_speed(c.transfers.upload_limit_kib, true)), cx),
                        &t,
                    )),
            )
            .child(
                section(tr("When the file already exists"), &t).children(exists_opts.iter().map(|(label, desc, value)| {
                    let on = xf.on_exists == *value;
                    let value = *value;
                    div()
                        .id(SharedString::from(format!("exists-{label}")))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(14.))
                        .py(px(10.))
                        .border_t_1()
                        .border_color(t.border.opacity(0.6))
                        .cursor_pointer()
                        .hover(|s| s.bg(t.hover.opacity(0.5)))
                        .child(div().size(px(16.)).rounded(px(8.)).border_color(if on { t.signal } else { t.faint }).when(on, |d| d.border_4()).when(!on, |d| d.border_1()))
                        .child(div().flex().flex_col().gap(px(2.)).child(div().font_weight(FontWeight::MEDIUM).child(*label)).child(div().text_size(px(12.)).text_color(t.faint).child(*desc)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.core.update_config(move |c| c.transfers.on_exists = value);
                            cx.notify();
                        }))
                })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_ladder() {
        assert_eq!(step_speed(0, true), 64);
        assert_eq!(step_speed(64, false), 0);
        assert_eq!(step_speed(0, false), 0);
        assert_eq!(step_speed(102400, true), 102400);
        assert_eq!(speed_label(2048), "2 MiB/s");
    }
}
