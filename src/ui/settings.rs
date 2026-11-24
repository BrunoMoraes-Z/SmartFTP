use std::sync::Arc;

use gpui::{ClipboardItem, Context, FontWeight, SharedString, Window, div, prelude::*, px};

use super::components::*;
use super::theme::{Appearance, Theme, Themes, theme};
use super::workspace::Workspace;
use crate::core::Core;
use crate::core::config::{ThemeMode, new_token, themes_dir};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Connection,
    Transfers,
    Appearance,
    Mcp,
}

pub struct SettingsState {
    pub tab: SettingsTab,
    pub show_token: bool,
}

impl SettingsState {
    pub fn new(_core: &Arc<Core>, _window: &mut Window, _cx: &mut Context<Workspace>) -> Self {
        SettingsState { tab: SettingsTab::General, show_token: false }
    }
}

fn card(t: &Theme) -> gpui::Div {
    div().flex().flex_col().gap(px(14.)).p(px(18.)).rounded(px(10.)).bg(t.surface).border_1().border_color(t.border)
}

impl Workspace {
    pub fn reload_themes(&mut self, cx: &mut Context<Self>) {
        let cfg = self.core.config.lock().clone();
        let os = cx.global::<Themes>().os_appearance;
        let themes = Themes::load(&cfg, os);
        for e in &themes.errors {
            self.toast(format!("Theme error: {e}"), true);
        }
        cx.set_global(themes);
        cx.refresh_windows();
    }

    fn pick_theme(&mut self, th: &Theme, cx: &mut Context<Self>) {
        let os = cx.global::<Themes>().os_appearance;
        let (id, app) = (th.id.clone(), th.appearance);
        self.core.update_config(|c| {
            match app {
                Appearance::Dark => c.dark_theme = id,
                Appearance::Light => c.light_theme = id,
            }
            let showing = match c.theme_mode {
                ThemeMode::System => os,
                ThemeMode::Light => Appearance::Light,
                ThemeMode::Dark => Appearance::Dark,
            };
            if showing != app {
                c.theme_mode = if app == Appearance::Dark { ThemeMode::Dark } else { ThemeMode::Light };
            }
        });
        self.apply_theme(cx);
    }

    fn duplicate_theme(&mut self, cx: &mut Context<Self>) {
        let cur = theme(cx).clone();
        let dir = themes_dir();
        let _ = std::fs::create_dir_all(&dir);
        let mut n = 1;
        let mut path = dir.join(format!("{}-custom.toml", cur.id));
        while path.exists() {
            n += 1;
            path = dir.join(format!("{}-custom-{n}.toml", cur.id));
        }
        let src = cur.source.replacen(&format!("name = \"{}\"", cur.name), &format!("name = \"{} (custom)\"", cur.name), 1);
        match std::fs::write(&path, src) {
            Ok(_) => {
                self.toast(format!("Created {} — edit it and press Reload", path.display()), false);
                self.reload_themes(cx);
                cx.open_with_system(&path);
            }
            Err(e) => self.toast(format!("Could not write theme: {e}"), true),
        }
    }

    pub fn render_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let tab = self.settings.tab;
        let nav_item = |label: &'static str, ic: &'static str, which: SettingsTab, cx: &mut Context<Self>| {
            let on = tab == which;
            div()
                .id(label)
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(32.))
                .px(px(10.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(on, |d| d.bg(t.hover))
                .hover(|d| d.bg(t.hover))
                .child(icon(ic, 15., if on { t.ink } else { t.muted }))
                .child(div().text_color(if on { t.ink } else { t.muted }).child(label))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings.tab = which;
                    cx.notify();
                }))
        };
        let content = match tab {
            SettingsTab::General => self.render_general(cx).into_any_element(),
            SettingsTab::Connection => self.render_connection(cx).into_any_element(),
            SettingsTab::Transfers => self.render_transfers(cx).into_any_element(),
            SettingsTab::Appearance => self.render_appearance(cx).into_any_element(),
            SettingsTab::Mcp => self.render_mcp(cx).into_any_element(),
        };
        div()
            .flex()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .w(px(210.))
                    .h_full()
                    .gap(px(2.))
                    .px(px(10.))
                    .py(px(16.))
                    .border_r_1()
                    .border_color(t.border)
                    .child(
                        div()
                            .id("settings-back")
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .px(px(6.))
                            .pb(px(12.))
                            .cursor_pointer()
                            .child(icon("arrow-left", 14., t.muted))
                            .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("Settings"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.view = super::workspace::View::Main;
                                cx.notify();
                            })),
                    )
                    .child(nav_item("General", "sliders-horizontal", SettingsTab::General, cx))
                    .child(nav_item("Connection", "plug", SettingsTab::Connection, cx))
                    .child(nav_item("Transfers", "arrow-up-down", SettingsTab::Transfers, cx))
                    .child(nav_item("Appearance", "palette", SettingsTab::Appearance, cx))
                    .child(nav_item("MCP & Agents", "bot", SettingsTab::Mcp, cx)),
            )
            .child(div().id("settings-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().child(div().flex().flex_col().gap(px(20.)).px(px(36.)).py(px(28.)).max_w(px(1100.)).child(content)))
    }

    fn render_appearance(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let themes = cx.global::<Themes>().all.clone();
        let errors = cx.global::<Themes>().errors.clone();
        let cfg = self.core.config.lock().clone();
        let active_id = t.id.clone();

        let mode_seg = div().flex().h(px(30.)).p(px(2.)).gap(px(2.)).rounded(px(6.)).border_1().border_color(t.border).children(
            [(ThemeMode::System, "System", "monitor"), (ThemeMode::Light, "Light", "sun"), (ThemeMode::Dark, "Dark", "moon")].into_iter().map(|(m, label, ic)| {
                let on = cfg.theme_mode == m;
                div()
                    .id(label)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(t.hover))
                    .child(icon(ic, 13., if on { t.ink } else { t.muted }))
                    .child(div().text_size(px(12.)).text_color(if on { t.ink } else { t.muted }).child(label))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.core.update_config(|c| c.theme_mode = m);
                        this.apply_theme(cx);
                    }))
            }),
        );

        let cards = themes.into_iter().map(|th| {
            let on = th.id == active_id;
            let selected_for_slot = match th.appearance {
                Appearance::Dark => cfg.dark_theme == th.id,
                Appearance::Light => cfg.light_theme == th.id,
            };
            let th2 = th.clone();
            div()
                .id(SharedString::from(format!("theme-{}", th.id)))
                .flex()
                .flex_col()
                .gap(px(10.))
                .w(px(250.))
                .cursor_pointer()
                .child(theme_preview(&th).border_color(if on { t.signal } else { t.border }).when(on, |d| d.border_2()))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().size(px(14.)).rounded(px(7.)).border_color(if selected_for_slot { t.signal } else { t.faint }).when(selected_for_slot, |d| d.border_4()).when(!selected_for_slot, |d| d.border_1()))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .child(div().font_weight(FontWeight::MEDIUM).child(th.name.clone()))
                                .child(div().text_size(px(11.)).text_color(t.faint).child(if th.builtin { "Built-in" } else { "Custom" })),
                        )
                        .child(div().text_size(px(10.)).font_weight(FontWeight::SEMIBOLD).text_color(t.muted).child(match th.appearance {
                            Appearance::Dark => "Dark",
                            Appearance::Light => "Light",
                        })),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.pick_theme(&th2, cx)))
        });

        let sample = BUILTIN_SAMPLE;
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(div().flex().flex_col().gap(px(4.)).child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child("Appearance")).child(div().text_color(t.muted).child("Pick a theme, or drop your own .toml file into the themes folder.")))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap(px(2.))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Mode"))
                            .child(div().text_size(px(12.)).text_color(t.faint).child("System follows the OS and switches between your selected light and dark theme.")),
                    )
                    .child(mode_seg),
            )
            .child(div().h(px(1.)).bg(t.border))
            .child(div().flex().flex_wrap().gap(px(20.)).children(cards))
            .when(!errors.is_empty(), |d| d.child(div().flex().flex_col().gap(px(4.)).text_size(px(12.)).text_color(t.err).children(errors.into_iter().map(|e| div().child(e)))))
            .child(
                card(&t).flex_row().gap(px(20.)).child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .w(px(320.))
                        .flex_none()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child("Make your own"))
                        .child(div().text_size(px(12.)).text_color(t.muted).child("A theme is a small TOML file with 16 colors. Duplicate the current one, edit the hex values, then press Reload."))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(8.))
                                .pt(px(6.))
                                .child(button("th-dup", "Duplicate current", Some("copy"), BtnKind::Primary, cx).on_click(cx.listener(|this, _, _, cx| this.duplicate_theme(cx))))
                                .child(button("th-open", "Open folder", Some("folder"), BtnKind::Outline, cx).on_click(cx.listener(|_, _, _, cx| {
                                    let dir = themes_dir();
                                    let _ = std::fs::create_dir_all(&dir);
                                    cx.open_with_system(&dir);
                                })))
                                .child(button("th-reload", "Reload", Some("rotate-cw"), BtnKind::Outline, cx).on_click(cx.listener(|this, _, _, cx| this.reload_themes(cx)))),
                        )
                        .child(div().text_size(px(11.)).text_color(t.faint).font_family(super::MONO_FONT).child(themes_dir().display().to_string())),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .p(px(12.))
                        .rounded(px(6.))
                        .bg(t.bg)
                        .border_1()
                        .border_color(t.border)
                        .font_family(super::MONO_FONT)
                        .text_size(px(12.))
                        .children(sample.lines().map(|l| {
                            let color = if l.starts_with('#') {
                                t.faint
                            } else if l.starts_with('[') {
                                t.signal
                            } else {
                                t.ink
                            };
                            div().text_color(color).child(if l.is_empty() { " ".to_string() } else { l.to_string() })
                        })),
                ),
            )
    }

    fn render_mcp(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let cfg = self.core.config.lock().mcp.clone();
        let url = format!("http://127.0.0.1:{}/mcp", cfg.port);
        let error = crate::mcp::status_error();
        let running = cfg.enabled && error.is_none();
        let token_shown = if self.settings.show_token { cfg.token.clone() } else { format!("{}••••••••••••{}", &cfg.token[..5], &cfg.token[cfg.token.len() - 4..]) };
        let claude_cmd = format!("claude mcp add --transport http smartftp {url} --header \"Authorization: Bearer {}\"", cfg.token);
        let json_cfg = format!("{{\n  \"mcpServers\": {{\n    \"smartftp\": {{\n      \"type\": \"http\",\n      \"url\": \"{url}\",\n      \"headers\": {{ \"Authorization\": \"Bearer {}\" }}\n    }}\n  }}\n}}", cfg.token);
        let log: Vec<_> = self.core.log_entries().into_iter().filter(|l| l.origin.is_agent()).take(12).collect();
        let disabled = cfg.disabled_tools.clone();

        let copy_row = |label: &'static str, value: String, id: &'static str, cx: &mut Context<Self>| {
            let v = value.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(div().text_size(px(12.)).text_color(t.muted).font_weight(FontWeight::MEDIUM).child(label))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .p(px(10.))
                        .rounded(px(6.))
                        .bg(t.bg)
                        .border_1()
                        .border_color(t.border)
                        .child(div().flex_1().min_w_0().font_family(super::MONO_FONT).text_size(px(12.)).children(value.lines().map(|l| div().child(l.to_string()))))
                        .child(icon_button(id, "copy", cx).on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(v.clone()));
                            this.toast("Copied to clipboard", false);
                            cx.notify();
                        }))),
                )
        };

        div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap(px(4.))
                            .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child("MCP server"))
                            .child(div().text_color(t.muted).child("Let AI agents drive SmartFTP through the Model Context Protocol. Agents see the same sessions you do.")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .h(px(30.))
                            .px(px(12.))
                            .rounded(px(15.))
                            .bg((if running { t.ok } else { t.faint }).opacity(0.12))
                            .child(div().size(px(8.)).rounded(px(4.)).bg(if running { t.ok } else { t.faint }))
                            .child(div().font_weight(FontWeight::SEMIBOLD).text_color(if running { t.ok } else { t.muted }).child(if running { "Running" } else if cfg.enabled { "Error" } else { "Off" })),
                    )
                    .child(div().w(px(12.)))
                    .child(toggle("mcp-enabled", cfg.enabled, cx).on_click(cx.listener(|this, _, _, cx| {
                        this.core.update_config(|c| c.mcp.enabled = !c.mcp.enabled);
                        crate::mcp::ensure_http(this.core.clone());
                        cx.notify();
                    }))),
            )
            .when_some(error, |d, e| d.child(div().text_color(t.err).text_size(px(12.)).child(e)))
            .child(
                card(&t)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Connect an agent"))
                    .child(copy_row("Endpoint (Streamable HTTP)", url.clone(), "cp-url", cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(div().text_size(px(12.)).text_color(t.muted).font_weight(FontWeight::MEDIUM).child("Bearer token"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .h(px(36.))
                                    .px(px(10.))
                                    .rounded(px(6.))
                                    .bg(t.bg)
                                    .border_1()
                                    .border_color(t.border)
                                    .child(div().flex_1().font_family(super::MONO_FONT).text_size(px(12.)).child(token_shown))
                                    .child(icon_button("tok-eye", if self.settings.show_token { "eye-off" } else { "eye" }, cx).on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.show_token = !this.settings.show_token;
                                        cx.notify();
                                    })))
                                    .child(icon_button("tok-copy", "copy", cx).on_click(cx.listener(|this, _, _, cx| {
                                        let tok = this.core.config.lock().mcp.token.clone();
                                        cx.write_to_clipboard(ClipboardItem::new_string(tok));
                                        this.toast("Token copied", false);
                                        cx.notify();
                                    })))
                                    .child(button("tok-new", "Regenerate", Some("refresh-cw"), BtnKind::Ghost, cx).on_click(cx.listener(|this, _, _, cx| {
                                        this.core.update_config(|c| c.mcp.token = new_token());
                                        this.toast("New token generated — update your agents", false);
                                        cx.notify();
                                    }))),
                            ),
                    )
                    .child(copy_row("Claude Code", claude_cmd, "cp-claude", cx))
                    .child(copy_row("Other MCP clients (JSON)", json_cfg, "cp-json", cx))
                    .child(div().text_size(px(12.)).text_color(t.faint).child("Headless alternative: run `smartftp --mcp-stdio` as a stdio server (uses the same saved sites, but its own sessions).")),
            )
            .child(
                card(&t).child(div().font_weight(FontWeight::SEMIBOLD).child("Exposed tools")).child(div().flex().flex_wrap().gap(px(8.)).children(crate::mcp::TOOLS.iter().map(|(name, desc)| {
                    let on = !disabled.iter().any(|d| d == name);
                    let n = name.to_string();
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .w(px(330.))
                        .h(px(44.))
                        .px(px(12.))
                        .rounded(px(7.))
                        .bg(t.surface2)
                        .child(div().flex().flex_col().flex_1().min_w_0().child(div().font_family(super::MONO_FONT).text_size(px(12.)).child(name.to_string())).child(div().text_size(px(11.)).text_color(t.faint).truncate().child(desc.to_string())))
                        .child(toggle(SharedString::from(format!("tool-{name}")), on, cx).on_click(cx.listener(move |this, _, _, cx| {
                            let n = n.clone();
                            this.core.update_config(|c| {
                                if let Some(i) = c.mcp.disabled_tools.iter().position(|d| *d == n) {
                                    c.mcp.disabled_tools.remove(i);
                                } else {
                                    c.mcp.disabled_tools.push(n);
                                }
                            });
                            cx.notify();
                        })))
                }))),
            )
            .child(
                card(&t)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Recent agent activity"))
                    .when(log.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(t.faint).child("No agent activity yet.")))
                    .children(log.into_iter().map(|l| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .text_size(px(12.))
                            .child(div().w(px(64.)).font_family(super::MONO_FONT).text_color(t.faint).child(l.time.format("%H:%M:%S").to_string()))
                            .child(div().w(px(90.)).text_color(t.signal).font_weight(FontWeight::SEMIBOLD).child(l.origin.label()))
                            .child(div().w(px(110.)).font_family(super::MONO_FONT).child(l.action.clone()))
                            .child(div().flex_1().min_w_0().truncate().font_family(super::MONO_FONT).text_color(t.muted).child(l.target.clone()))
                            .child(div().px(px(6.)).rounded(px(4.)).bg((if l.ok { t.ok } else { t.err }).opacity(0.12)).text_color(if l.ok { t.ok } else { t.err }).text_size(px(10.)).font_weight(FontWeight::SEMIBOLD).child(if l.ok { "ok" } else { "error" }))
                    })),
            )
    }
}

const BUILTIN_SAMPLE: &str = "# themes/my-theme.toml\nname = \"My Theme\"\nappearance = \"dark\"\n\n[colors]\nbg = \"#282A36\"      surface = \"#21222C\"\nink = \"#F8F8F2\"     muted = \"#C3C5D9\"\naccent = \"#BD93F9\"  signal = \"#FFB86C\"\n# …16 keys in total, see the built-in files";

/// Miniature of the app drawn with a theme's colors.
fn theme_preview(th: &Theme) -> gpui::Div {
    let row = |w: f32, folder: bool, sel: bool| {
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(10.))
            .px(px(2.))
            .rounded(px(2.))
            .when(sel, |d| d.bg(th.accent_soft))
            .child(div().size(px(7.)).rounded(px(1.)).bg(if folder { th.folder } else { th.muted }))
            .child(div().w(px(w)).h(px(4.)).rounded(px(2.)).bg(th.ink))
            .child(div().flex_1())
            .child(div().w(px(14.)).h(px(4.)).rounded(px(2.)).bg(th.faint))
    };
    let pane = |offset: f32| {
        div().flex().flex_col().flex_1().gap(px(5.)).p(px(8.)).children((0..5).map(move |r| row(30. + ((r as f32 * 13. + offset) % 28.), r < 2, offset == 0. && r == 2)))
    };
    div()
        .flex()
        .h(px(140.))
        .w_full()
        .rounded(px(8.))
        .overflow_hidden()
        .bg(th.bg)
        .border_1()
        .child(
            div().flex().flex_col().items_center().gap(px(6.)).w(px(26.)).py(px(8.)).bg(th.surface).border_r_1().border_color(th.border).children(
                ["#E5484D", "#3E9BF0", "#5BB974"].into_iter().map(|c| {
                    let col = site_color(c);
                    div().size(px(14.)).rounded(px(3.)).bg(col.opacity(0.25)).border_1().border_color(col)
                }),
            ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .child(div().flex().items_center().h(px(22.)).px(px(8.)).border_b_1().border_color(th.border).child(div().w(px(40.)).h(px(5.)).rounded(px(2.)).bg(th.ink)).child(div().flex_1()).child(div().w(px(26.)).h(px(10.)).rounded(px(2.)).bg(th.accent)))
                .child(div().flex().flex_1().child(pane(0.)).child(div().w(px(1.)).bg(th.border)).child(pane(7.)))
                .child(div().flex().items_center().h(px(20.)).px(px(8.)).bg(th.surface).border_t_1().border_color(th.border).child(div().w_full().h(px(2.)).bg(th.border).child(div().w(px(80.)).h_full().bg(th.signal)))),
        )
}
