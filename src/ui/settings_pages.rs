//! General, Connection and Transfers settings pages (modelled on FileZilla's options).

use std::rc::Rc;

use gpui::{Context, FontWeight, SharedString, div, prelude::*, px};

use super::components::*;
use super::theme::{Theme, theme};
use super::workspace::Workspace;
use crate::core::config::{Config, ExistsAction, FtpMode, app_dir};

type Setter = Rc<dyn Fn(&mut Config)>;

/// Speed limit presets in KiB/s (0 = unlimited).
const SPEED_STEPS: &[u64] = &[0, 64, 128, 256, 512, 1024, 2048, 5120, 10240, 20480, 51200, 102400];

fn speed_label(kib: u64) -> String {
    match kib {
        0 => "Unlimited".into(),
        k if k >= 1024 => format!("{} MiB/s", k / 1024),
        k => format!("{k} KiB/s"),
    }
}

fn step_speed(cur: u64, up: bool) -> u64 {
    let i = SPEED_STEPS.iter().position(|s| *s >= cur).unwrap_or(SPEED_STEPS.len() - 1);
    if up { SPEED_STEPS[(i + 1).min(SPEED_STEPS.len() - 1)] } else { SPEED_STEPS[i.saturating_sub(1)] }
}

fn section(title: &str, t: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .p(px(4.))
        .rounded(px(10.))
        .bg(t.surface)
        .border_1()
        .border_color(t.border)
        .child(div().px(px(14.)).pt(px(12.)).pb(px(6.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(t.muted).child(title.to_string().to_uppercase()))
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

    fn segmented<T: Copy + PartialEq + 'static>(&self, id: &str, options: &[(&'static str, T)], current: T, set: Rc<dyn Fn(&mut Config, T)>, cx: &mut Context<Self>) -> impl IntoElement + use<T> {
        let t = theme(cx).clone();
        div().flex().h(px(32.)).p(px(2.)).gap(px(2.)).rounded(px(7.)).bg(t.bg).border_1().border_color(t.border).children(options.iter().map(|(label, value)| {
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
        }))
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

    pub fn render_general(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let ui = self.core.config.lock().interface.clone();
        let dir = app_dir();
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(Self::page_header("General", "How the file panes behave.", &t))
            .child(
                section("File panes", &t)
                    .child(row("Show hidden files", "Show dotfiles (e.g. .env, .htaccess) when a pane opens. Toggle per pane with the eye button.", self.switch("show-hidden", ui.show_hidden, Rc::new(|c| c.interface.show_hidden = !c.interface.show_hidden), cx), &t))
                    .child(row(
                        "Double-clicking a file",
                        "Transfer it to the other side, or open local files with their default app.",
                        self.segmented("dbl", &[("Transfer", true), ("Open", false)], ui.double_click_transfers, Rc::new(|c, v| c.interface.double_click_transfers = v), cx),
                        &t,
                    ))
                    .child(row("Confirm before deleting", "Ask before deleting local or remote files and folders.", self.switch("confirm-del", ui.confirm_delete, Rc::new(|c| c.interface.confirm_delete = !c.interface.confirm_delete), cx), &t))
                    .child(row("Open the transfer panel", "Expand the transfer panel when a transfer starts.", self.switch("open-drawer", ui.open_drawer_on_transfer, Rc::new(|c| c.interface.open_drawer_on_transfer = !c.interface.open_drawer_on_transfer), cx), &t)),
            )
            .child(
                section("Data", &t).child(row(
                    "Settings folder",
                    &dir.display().to_string(),
                    button("open-cfg", "Open folder", Some("folder-open"), BtnKind::Outline, cx).on_click(cx.listener(move |_, _, _, cx| {
                        let d = app_dir();
                        let _ = std::fs::create_dir_all(&d);
                        cx.open_with_system(&d);
                    })),
                    &t,
                )),
            )
    }

    pub fn render_connection(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let c = self.core.config.lock().connection.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(Self::page_header("Connection", "Timeouts, retries and protocol options. Changes apply to new connections.", &t))
            .child(
                section("General", &t)
                    .child(row(
                        "Timeout",
                        "Give up connecting or logging in after this long.",
                        self.stepper("timeout", format!("{} s", c.timeout_secs), Rc::new(|c| c.connection.timeout_secs = c.connection.timeout_secs.saturating_sub(5).max(5)), Rc::new(|c| c.connection.timeout_secs = (c.connection.timeout_secs + 5).min(600)), cx),
                        &t,
                    ))
                    .child(row(
                        "Maximum number of retries",
                        "Extra attempts when a connection or login fails.",
                        self.stepper("retries", format!("{}", c.retries), Rc::new(|c| c.connection.retries = c.connection.retries.saturating_sub(1)), Rc::new(|c| c.connection.retries = (c.connection.retries + 1).min(10)), cx),
                        &t,
                    ))
                    .child(row(
                        "Delay between failed attempts",
                        "Wait before retrying a failed connection, login or transfer.",
                        self.stepper("retry-delay", format!("{} s", c.retry_delay_secs), Rc::new(|c| c.connection.retry_delay_secs = c.connection.retry_delay_secs.saturating_sub(1)), Rc::new(|c| c.connection.retry_delay_secs = (c.connection.retry_delay_secs + 1).min(120)), cx),
                        &t,
                    ))
                    .child(row(
                        "Reconnect automatically",
                        "When the server drops the connection, reconnect and retry the operation once.",
                        self.switch("auto-reconnect", c.auto_reconnect, Rc::new(|c| c.connection.auto_reconnect = !c.connection.auto_reconnect), cx),
                        &t,
                    )),
            )
            .child(
                section("SFTP", &t).child(row(
                    "Keep-alive interval",
                    "Send a keep-alive when the connection is idle, so routers and servers don't drop it. 0 turns it off.",
                    self.stepper(
                        "keepalive",
                        if c.keepalive_secs == 0 { "Off".into() } else { format!("{} s", c.keepalive_secs) },
                        Rc::new(|c| c.connection.keepalive_secs = c.connection.keepalive_secs.saturating_sub(15)),
                        Rc::new(|c| c.connection.keepalive_secs = (c.connection.keepalive_secs + 15).min(600)),
                        cx,
                    ),
                    &t,
                )),
            )
            .child(
                section("FTP / FTPS", &t)
                    .child(row(
                        "Transfer mode",
                        "Passive works behind most firewalls. Active needs the server to reach this computer.",
                        self.segmented("ftp-mode", &[("Passive", FtpMode::Passive), ("Extended passive", FtpMode::ExtendedPassive), ("Active", FtpMode::Active)], c.ftp_mode, Rc::new(|c, v| c.connection.ftp_mode = v), cx),
                        &t,
                    ))
                    .child(row(
                        "Fix passive-mode addresses",
                        "Use the server's address when it replies to PASV with a private IP (common behind NAT).",
                        self.switch("nat", c.ftp_nat_workaround, Rc::new(|c| c.connection.ftp_nat_workaround = !c.connection.ftp_nat_workaround), cx),
                        &t,
                    )),
            )
    }

    pub fn render_transfers(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let tr = self.core.config.lock().transfers.clone();
        let exists_opts: &[(&str, &str, ExistsAction)] = &[
            ("Overwrite", "Always replace the existing file.", ExistsAction::Overwrite),
            ("Overwrite if newer", "Replace only when the source file is newer.", ExistsAction::OverwriteIfNewer),
            ("Overwrite if different", "Replace when size or date differ (like a sync).", ExistsAction::OverwriteIfDifferent),
            ("Rename", "Keep both: the new copy becomes \"name (1).ext\".", ExistsAction::Rename),
            ("Skip", "Leave the existing file and skip this one.", ExistsAction::Skip),
        ];
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(Self::page_header("Transfers", "Queue concurrency, speed limits and what happens when a file already exists.", &t))
            .child(
                section("Queue", &t)
                    .child(row(
                        "Maximum simultaneous transfers",
                        "How many files are transferred at the same time. Applies immediately.",
                        self.stepper("max-conc", format!("{}", tr.max_concurrent), Rc::new(|c| c.transfers.max_concurrent = c.transfers.max_concurrent.saturating_sub(1).max(1)), Rc::new(|c| c.transfers.max_concurrent = (c.transfers.max_concurrent + 1).min(10)), cx),
                        &t,
                    ))
                    .child(row(
                        "Retries for failed transfers",
                        "Retry a file this many times (after the delay set in Connection) before marking it failed.",
                        self.stepper("tr-retries", format!("{}", tr.retries), Rc::new(|c| c.transfers.retries = c.transfers.retries.saturating_sub(1)), Rc::new(|c| c.transfers.retries = (c.transfers.retries + 1).min(10)), cx),
                        &t,
                    ))
                    .child(row(
                        "Preserve timestamps",
                        "Give the copy the same modification time as the original (needed for the compare badges and \"overwrite if newer\").",
                        self.switch("preserve", tr.preserve_timestamps, Rc::new(|c| c.transfers.preserve_timestamps = !c.transfers.preserve_timestamps), cx),
                        &t,
                    )),
            )
            .child(
                section("Speed limits", &t)
                    .child(row(
                        "Download limit",
                        "Shared by all downloads. Applies immediately, also to running transfers.",
                        self.stepper("dl-limit", speed_label(tr.download_limit_kib), Rc::new(|c| c.transfers.download_limit_kib = step_speed(c.transfers.download_limit_kib, false)), Rc::new(|c| c.transfers.download_limit_kib = step_speed(c.transfers.download_limit_kib, true)), cx),
                        &t,
                    ))
                    .child(row(
                        "Upload limit",
                        "Shared by all uploads.",
                        self.stepper("ul-limit", speed_label(tr.upload_limit_kib), Rc::new(|c| c.transfers.upload_limit_kib = step_speed(c.transfers.upload_limit_kib, false)), Rc::new(|c| c.transfers.upload_limit_kib = step_speed(c.transfers.upload_limit_kib, true)), cx),
                        &t,
                    )),
            )
            .child(
                section("When the file already exists", &t).children(exists_opts.iter().map(|(label, desc, value)| {
                    let on = tr.on_exists == *value;
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
