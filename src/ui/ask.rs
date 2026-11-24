//! In-app confirmation dialogs, styled with the theme (replaces the native Windows message box).
//! `ask` returns a receiver like `window.prompt`: it yields the index of the chosen button,
//! or an error when the dialog is dismissed (Esc / click outside).

use futures::channel::oneshot;
use gpui::{AnyElement, Context, FontWeight, KeyDownEvent, MouseButton, SharedString, Window, deferred, div, hsla, prelude::*, px};

use super::components::*;
use super::theme::theme;
use super::workspace::Workspace;

#[derive(Clone, Copy, PartialEq)]
pub enum AskLevel {
    Info,
    Warning,
}

pub struct Ask {
    pub level: AskLevel,
    pub title: String,
    pub detail: Option<String>,
    pub buttons: Vec<String>,
    tx: Option<oneshot::Sender<usize>>,
}

impl Workspace {
    /// Show a themed dialog. The first button is the main action (Enter); Esc dismisses.
    pub fn ask<D: Into<String>>(
        &mut self,
        level: AskLevel,
        title: impl Into<String>,
        detail: Option<D>,
        buttons: &[&str],
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<usize> {
        let (tx, rx) = oneshot::channel();
        self.asks.push_back(Ask {
            level,
            title: title.into(),
            detail: detail.map(Into::into),
            buttons: buttons.iter().map(|b| b.to_string()).collect(),
            tx: Some(tx),
        });
        cx.notify();
        rx
    }

    fn answer(&mut self, choice: Option<usize>, cx: &mut Context<Self>) {
        if let Some(mut a) = self.asks.pop_front()
            && let (Some(tx), Some(i)) = (a.tx.take(), choice)
        {
            let _ = tx.send(i);
        }
        cx.notify();
    }

    pub fn render_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let a = self.asks.front()?;
        let t = theme(cx).clone();
        if !self.ask_focus.is_focused(window) {
            window.focus(&self.ask_focus);
        }
        let warning = a.level == AskLevel::Warning;
        let accent = if warning { t.err } else { t.signal };
        let (title, detail, buttons) = (a.title.clone(), a.detail.clone(), a.buttons.clone());
        let more = self.asks.len() - 1;
        Some(
            deferred(
                div()
                    .id("ask-scrim")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .bg(hsla(0., 0., 0., 0.5))
                    .track_focus(&self.ask_focus)
                    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| match ev.keystroke.key.as_str() {
                        "enter" => this.answer(Some(0), cx),
                        "escape" => this.answer(None, cx),
                        _ => {}
                    }))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.answer(None, cx)))
                    .child(
                        div()
                            .id("ask-dialog")
                            .occlude()
                            .flex()
                            .flex_col()
                            .w(px(440.))
                            .max_w_full()
                            .rounded(px(12.))
                            .bg(t.surface)
                            .border_1()
                            .border_color(t.border)
                            .shadow_2xl()
                            .overflow_hidden()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(div().h(px(3.)).w_full().bg(accent))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(14.))
                                    .p(px(20.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_none()
                                            .items_center()
                                            .justify_center()
                                            .size(px(36.))
                                            .rounded(px(9.))
                                            .bg(accent.opacity(0.14))
                                            .child(icon(if warning { "circle-alert" } else { "info" }, 18., accent)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .min_w_0()
                                            .gap(px(6.))
                                            .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(title))
                                            .when_some(detail, |d, text| {
                                                d.child(div().text_size(px(13.)).text_color(t.muted).line_height(px(19.)).child(text))
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .px(px(20.))
                                    .py(px(14.))
                                    .bg(t.surface2)
                                    .border_t_1()
                                    .border_color(t.border)
                                    .when(more > 0, |d| d.child(div().text_size(px(11.)).text_color(t.faint).child(format!("+{more}"))))
                                    .child(div().flex_1())
                                    // main action last on the right, like the platform convention
                                    .children(buttons.iter().enumerate().rev().map(|(i, label)| {
                                        let kind = match (i, warning) {
                                            (0, true) => BtnKind::Danger,
                                            (0, false) => BtnKind::Primary,
                                            _ => BtnKind::Outline,
                                        };
                                        let b = button(SharedString::from(format!("ask-btn-{i}")), label.clone(), None, kind, cx)
                                            .on_click(cx.listener(move |this, _, _, cx| this.answer(Some(i), cx)));
                                        if i == 0 && warning {
                                            let err = t.err;
                                            b.bg(err).text_color(gpui::white()).border_color(err).hover(move |s| s.bg(err.opacity(0.85)))
                                        } else {
                                            b
                                        }
                                    })),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}
