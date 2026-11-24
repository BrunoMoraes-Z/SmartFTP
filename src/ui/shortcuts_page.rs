//! Settings → Shortcuts: pick a keyboard shortcut and a mouse button for each command.

use gpui::{Context, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, SharedString, Window, div, prelude::*, px};

use super::components::*;
use super::shortcuts::{self, Cmd, display_key, display_mouse, key_for, mouse_for, mouse_id};
use super::theme::theme;
use super::workspace::Workspace;
use crate::i18n::{tr, trf};

const MODIFIER_KEYS: &[&str] = &["shift", "control", "ctrl", "alt", "platform", "cmd", "fn", "function", "capslock"];

impl Workspace {
    fn start_recording(&mut self, cmd: Cmd, mouse: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.recording = Some((cmd, mouse));
        if !mouse {
            // no shortcut may fire while the user presses the new one
            cx.clear_key_bindings();
        }
        window.focus(&self.settings.rec_focus);
        cx.notify();
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        self.settings.recording = None;
        let cfg = self.core.config.lock().clone();
        shortcuts::apply(cx, &cfg);
        cx.notify();
    }

    /// Assign a key (or mouse button) to `cmd`, taking it away from any other command.
    fn assign(&mut self, cmd: Cmd, mouse: bool, value: String, cx: &mut Context<Self>) {
        let cfg = self.core.config.lock().clone();
        let mut moved_from = vec![];
        if !value.is_empty() {
            for other in Cmd::ALL.into_iter().filter(|c| *c != cmd) {
                let current = if mouse { mouse_for(&cfg, other) } else { key_for(&cfg, other) };
                if current.as_deref() == Some(value.as_str()) {
                    moved_from.push(other);
                }
            }
        }
        let shown = if mouse { display_mouse(&value).to_string() } else { display_key(&value) };
        self.core.update_config(|c| {
            let map = if mouse { &mut c.shortcuts.mouse } else { &mut c.shortcuts.keys };
            for other in &moved_from {
                map.insert(other.id().to_string(), String::new());
            }
            map.insert(cmd.id().to_string(), value);
        });
        for other in moved_from {
            self.toast(trf("{key} was removed from \"{action}\"", &[("key", &shown), ("action", other.label())]), false);
        }
        self.stop_recording(cx);
    }

    fn on_record_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some((cmd, false)) = self.settings.recording else { return };
        let ks = &ev.keystroke;
        if MODIFIER_KEYS.contains(&ks.key.as_str()) {
            return; // wait for the actual key
        }
        let m = &ks.modifiers;
        if ks.key == "escape" && !(m.control || m.alt || m.shift || m.platform) {
            self.stop_recording(cx);
            return;
        }
        self.assign(cmd, false, ks.unparse(), cx);
    }

    fn on_record_mouse(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some((cmd, mouse)) = self.settings.recording else { return };
        match mouse_id(ev.button) {
            Some(id) if mouse => {
                cx.stop_propagation();
                self.assign(cmd, true, id.to_string(), cx);
            }
            // a left click elsewhere ends recording
            _ if ev.button == MouseButton::Left => self.stop_recording(cx),
            _ => {}
        }
    }

    pub fn render_shortcuts(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let cfg = self.core.config.lock().clone();
        let recording = self.settings.recording;

        let cell = |cmd: Cmd, mouse: bool, cx: &mut Context<Self>| {
            let value = if mouse { mouse_for(&cfg, cmd) } else { key_for(&cfg, cmd) };
            let active = recording == Some((cmd, mouse));
            let text: String = if active {
                if mouse {
                    tr("Press a mouse button… (side buttons or middle)").into()
                } else {
                    tr("Press a key… (Esc to cancel)").into()
                }
            } else {
                match &value {
                    Some(v) if mouse => display_mouse(v).into(),
                    Some(v) => display_key(v),
                    None => "—".into(),
                }
            };
            let key = format!("{}-{}", cmd.id(), if mouse { "mouse" } else { "key" });
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .w(px(250.))
                .flex_none()
                .child(
                    div()
                        .id(SharedString::from(format!("rec-{key}")))
                        .flex()
                        .flex_1()
                        .items_center()
                        .h(px(30.))
                        .px(px(10.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .border_1()
                        .border_color(if active { t.signal } else { t.border })
                        .bg(if active { t.signal.opacity(0.1) } else { t.bg })
                        .text_size(px(12.))
                        .when(!mouse && !active, |d| d.font_family(super::MONO_FONT))
                        .text_color(if active {
                            t.signal
                        } else if value.is_some() {
                            t.ink
                        } else {
                            t.faint
                        })
                        .hover(|s| s.border_color(t.muted))
                        .child(div().truncate().child(text))
                        .on_click(cx.listener(move |this, _, window, cx| this.start_recording(cmd, mouse, window, cx))),
                )
                .child(
                    icon_button(SharedString::from(format!("clear-{key}")), "x", cx)
                        .when(value.is_none(), |d| d.opacity(0.))
                        .tooltip(super::panes::tip(tr("Remove")))
                        .on_click(cx.listener(move |this, _, _, cx| this.assign(cmd, mouse, String::new(), cx))),
                )
        };

        let rows: Vec<_> = Cmd::ALL
            .into_iter()
            .map(|cmd| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .px(px(14.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(t.border.opacity(0.6))
                    .child(div().flex_1().min_w_0().font_weight(FontWeight::MEDIUM).child(cmd.label()))
                    .child(cell(cmd, false, cx))
                    .child(cell(cmd, true, cx))
            })
            .collect();

        div()
            .id("shortcuts-page")
            .track_focus(&self.settings.rec_focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_record_key(ev, cx)))
            .on_any_mouse_down(cx.listener(|this, ev: &MouseDownEvent, _, cx| this.on_record_mouse(ev, cx)))
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap(px(4.))
                            .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child(tr("Shortcuts")))
                            .child(div().text_color(t.muted).child(tr(
                                "Click a cell, then press the key or mouse button to use. Mouse buttons act on the pane under the pointer.",
                            ))),
                    )
                    .child(button("reset-shortcuts", tr("Reset to defaults"), Some("rotate-cw"), BtnKind::Outline, cx).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.core.update_config(|c| c.shortcuts = Default::default());
                            this.stop_recording(cx);
                        }),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(4.))
                    .rounded(px(10.))
                    .bg(t.surface)
                    .border_1()
                    .border_color(t.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .px(px(14.))
                            .pt(px(10.))
                            .pb(px(8.))
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.faint)
                            .child(div().flex_1().child(tr("ACTION")))
                            .child(div().w(px(250.)).flex_none().child(tr("KEYBOARD")))
                            .child(div().w(px(250.)).flex_none().child(tr("MOUSE"))),
                    )
                    .children(rows),
            )
    }
}
