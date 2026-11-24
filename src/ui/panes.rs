use std::collections::HashMap;

use gpui::{
    Context, ExternalPaths, FontWeight, MouseButton, MouseDownEvent, SharedString, Window, div, prelude::*, px, relative,
};

use super::components::*;
use super::theme::{Theme, theme};
use super::workspace::{ContextMenu, DraggedFiles, DrawerTab, Side, SortBy, Workspace};
use crate::core::model::{Entry, Origin, human_size, join_remote};
use crate::core::{Direction, JobState};

impl Workspace {
    pub fn render_main(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let session = self.session();
        let header = self.render_session_header(cx);
        let local = self.render_pane(Side::Local, cx).into_any_element();
        let remote = if session.is_some() { self.render_pane(Side::Remote, cx).into_any_element() } else { self.render_empty_remote(cx).into_any_element() };
        let drawer = self.render_drawer(cx);
        let t = theme(cx).clone();
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(div().flex().flex_col().flex_1().min_w_0().child(local))
                    .child(div().w(px(1.)).h_full().bg(t.border))
                    .child(div().flex().flex_col().flex_1().min_w_0().child(remote)),
            )
            .child(drawer)
    }

    fn render_session_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let base = div().flex().flex_none().items_center().gap(px(12.)).h(px(56.)).px(px(16.)).border_b_1().border_color(t.border);
        let Some(s) = self.session() else {
            return base
                .child(icon("laptop", 18., t.muted))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Not connected"))
                        .child(div().text_size(px(11.)).text_color(t.faint).child("Pick a site on the left, paste a URL into quick connect, or let an agent connect over MCP.")),
                )
                .child(div().flex_1())
                .child(button("hdr-new", "New site", Some("plus"), BtnKind::Primary, cx).on_click(cx.listener(|this, _, window, cx| this.open_site_editor(None, window, cx))));
        };
        let (ic, col) = self.identity(&s.site);
        let color = site_color(&col);
        let saved = self.core.config.lock().sites.iter().find(|x| x.id == s.site.id).cloned();
        let id = s.id.clone();
        let mut meta = format!("{} · {}", s.site.address(), s.site.protocol.label());
        if let Origin::Agent(name) = &s.origin {
            meta.push_str(&format!(" · opened by {name}"));
        }
        base.child(tile(&ic, color, 32.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(s.site.name.clone()))
                            .when(!s.site.group.is_empty(), |d| {
                                d.child(
                                    div()
                                        .px(px(6.))
                                        .rounded(px(4.))
                                        .bg(color.opacity(0.16))
                                        .text_size(px(10.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(color)
                                        .child(s.site.group.to_lowercase()),
                                )
                            }),
                    )
                    .child(div().text_size(px(11.)).text_color(t.faint).font_family(super::MONO_FONT).truncate().child(meta)),
            )
            .child(div().flex_1())
            .child(button("hdr-refresh", "Refresh", Some("rotate-cw"), BtnKind::Outline, cx).on_click(cx.listener(|this, _, _, cx| this.refresh_all(cx))))
            .when_some(saved, |d, site| {
                d.child(button("hdr-edit", "Edit site", Some("pencil"), BtnKind::Outline, cx).on_click(cx.listener(move |this, _, window, cx| this.open_site_editor(Some(site.clone()), window, cx))))
            })
            .child(button("hdr-disconnect", "Disconnect", Some("unplug"), BtnKind::Outline, cx).on_click(cx.listener(move |this, _, _, cx| this.disconnect(id.clone(), cx))))
    }

    fn render_empty_remote(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let sites = self.core.config.lock().sites.clone();
        div()
            .flex()
            .flex_col()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .p(px(24.))
            .child(icon("cloud", 32., t.faint))
            .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("No remote connected"))
            .child(div().text_size(px(12.)).text_color(t.muted).max_w(px(340.)).text_center().child("Connect to a site to see its files here. Then click → / ← on any row, double-click a file, or drag files between the two sides."))
            .child(
                div().flex().flex_col().gap(px(4.)).w(px(300.)).children(sites.into_iter().take(6).map(|site| {
                    let s = site.clone();
                    div()
                        .id(SharedString::from(format!("empty-{}", site.id)))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(10.))
                        .h(px(40.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(t.border)
                        .cursor_pointer()
                        .hover(|d| d.bg(t.hover))
                        .child(tile(&site.icon, site_color(&site.color), 24.))
                        .child(div().flex_1().child(site.name.clone()))
                        .child(div().text_size(px(10.)).text_color(t.faint).child(site.protocol.label()))
                        .on_click(cx.listener(move |this, _, _, cx| this.connect_site(s.clone(), cx)))
                })),
            )
            .child(button("empty-new", "New site", Some("plus"), BtnKind::Primary, cx).on_click(cx.listener(|this, _, window, cx| this.open_site_editor(None, window, cx))))
    }

    pub fn render_pane(&mut self, side: Side, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let session = self.session();
        let other: HashMap<String, (Option<i64>, bool)> = self
            .pane(side.other())
            .map(|p| p.entries.iter().map(|e| (e.name.clone(), (e.modified, e.is_dir))).collect())
            .unwrap_or_default();
        let compare = session.is_some();
        let Some(pane) = self.pane(side) else { return div().id("pane-none") };
        let rows: Vec<Entry> = pane.visible().into_iter().cloned().collect();
        let selected = pane.selected.clone();
        let path = pane.path.clone();
        let loading = pane.loading;
        let error = pane.error.clone();
        let sort = pane.sort;
        let desc = pane.sort_desc;
        let show_hidden = pane.show_hidden;
        let sel_entries = pane.selected_entries();
        let site_col = session.as_ref().map(|s| site_color(&self.identity(&s.site).1)).unwrap_or(t.muted);
        let site_name = session.as_ref().map(|s| s.site.name.clone()).unwrap_or_default();
        let remote_dir = self.pane(Side::Remote).map(|p| p.path.clone()).unwrap_or_default();
        let local_dir = self.local.path.clone();
        let focus = match side {
            Side::Local => self.local_focus.clone(),
            Side::Remote => self.remote_focus.clone(),
        };
        let tag = match side {
            Side::Local => "L",
            Side::Remote => "R",
        };
        let accent_soft = t.accent_soft;
        let drop_color = site_col;

        // ---------- path bar
        let crumbs = breadcrumbs(side, &path);
        let crumb_count = crumbs.len();
        let path_bar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(44.))
            .pl(px(12.))
            .pr(px(8.))
            .border_b_1()
            .border_color(t.border)
            .child(match side {
                Side::Local => div().flex().items_center().gap(px(8.)).child(icon("laptop", 15., t.muted)).child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(t.muted).child("This computer")),
                Side::Remote => div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().w(px(3.)).h(px(18.)).rounded(px(2.)).bg(site_col))
                    .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(site_col).child(site_name)),
            })
            .child(
                div()
                    .id(SharedString::from(format!("crumbs-{tag}")))
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(px(2.))
                    .h(px(28.))
                    .px(px(8.))
                    .rounded(px(5.))
                    .bg(t.surface)
                    .overflow_x_scroll()
                    .font_family(super::MONO_FONT)
                    .text_size(px(12.))
                    .children(crumbs.into_iter().enumerate().map(|(i, (label, target))| {
                        let last = i + 1 == crumb_count;
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .when(i > 0 && !(side == Side::Remote && i == 1), |d| d.child(div().text_color(t.faint).px(px(2.)).child("/")))
                            .child(
                                div()
                                    .id(SharedString::from(format!("crumb-{tag}-{i}")))
                                    .px(px(2.))
                                    .rounded(px(3.))
                                    .cursor_pointer()
                                    .text_color(if last { t.ink } else { t.muted })
                                    .hover(|d| d.bg(t.hover))
                                    .child(label)
                                    .on_click(cx.listener(move |this, _, _, cx| this.navigate(side, target.clone(), cx))),
                            )
                    })),
            )
            .child(icon_button(SharedString::from(format!("up-{tag}")), "arrow-up", cx).tooltip(tip("Parent folder (Backspace)")).on_click(cx.listener(move |this, _, _, cx| this.go_up(side, cx))))
            .child(
                icon_button(SharedString::from(format!("refresh-{tag}")), "rotate-cw", cx).tooltip(tip("Refresh (F5)")).on_click(cx.listener(move |this, _, _, cx| match side {
                    Side::Local => this.load_local(cx),
                    Side::Remote => {
                        if let Some(id) = this.active.clone() {
                            this.load_remote(&id, cx)
                        }
                    }
                })),
            )
            .child(
                icon_button(SharedString::from(format!("mkdir-{tag}")), "folder-plus", cx)
                    .tooltip(tip("New folder (F7)"))
                    .on_click(cx.listener(move |this, _, window, cx| this.open_prompt(super::dialogs::PromptKind::NewFolder, side, None, window, cx))),
            )
            .child(
                icon_button(SharedString::from(format!("hidden-{tag}")), if show_hidden { "eye" } else { "eye-off" }, cx).tooltip(tip("Show / hide dotfiles")).on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(p) = this.pane_mut(side) {
                        p.show_hidden = !p.show_hidden;
                    }
                    cx.notify();
                })),
            );

        // ---------- column header
        let col = |label: &'static str, by: SortBy, w: Option<f32>, right: bool, cx: &mut Context<Self>| {
            let active = sort == by;
            div()
                .id(SharedString::from(format!("col-{tag}-{label}")))
                .flex()
                .items_center()
                .gap(px(3.))
                .when_some(w, |d, w| d.w(px(w)).flex_none())
                .when(w.is_none(), |d| d.flex_1())
                .when(right, |d| d.justify_end())
                .cursor_pointer()
                .text_color(if active { t.muted } else { t.faint })
                .child(label)
                .when(active, |d| d.child(icon(if desc { "arrow-down" } else { "arrow-up" }, 11., t.muted)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(p) = this.pane_mut(side) {
                        if p.sort == by {
                            p.sort_desc = !p.sort_desc;
                        } else {
                            p.sort = by;
                            p.sort_desc = false;
                        }
                    }
                    cx.notify();
                }))
        };
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(26.))
            .px(px(12.))
            .text_size(px(11.))
            .when(side == Side::Remote, |d| d.pl(px(44.)))
            .child(col("Name", SortBy::Name, None, false, cx))
            .child(col("Size", SortBy::Size, Some(72.), true, cx))
            .child(col("Modified", SortBy::Modified, Some(84.), true, cx))
            .when(side == Side::Remote, |d| d.child(div().w(px(84.)).flex_none().text_color(t.faint).child("Perms")))
            .when(side == Side::Local, |d| d.child(div().w(px(24.)).flex_none()));

        // ---------- rows
        let mut list = div().flex().flex_col().px(px(4.)).pb(px(8.));
        if let Some(err) = &error {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .m(px(8.))
                    .p(px(10.))
                    .rounded(px(6.))
                    .bg(t.err.opacity(0.1))
                    .text_color(t.err)
                    .text_size(px(12.))
                    .child(icon("circle-alert", 14., t.err))
                    .child(div().flex_1().child(err.clone())),
            );
        }
        if rows.is_empty() && error.is_none() {
            list = list.child(
                div().flex().justify_center().py(px(24.)).text_size(px(12.)).text_color(t.faint).child(if loading { "Loading…" } else { "This folder is empty" }),
            );
        }
        for (ix, e) in rows.into_iter().enumerate() {
            let is_sel = selected.contains(&e.name);
            let badge = if compare && !e.is_dir {
                match other.get(&e.name) {
                    None => Some(("only here", t.faint)),
                    Some((m, false)) => match (e.modified, m) {
                        (Some(a), Some(b)) if a > b + time_tolerance(a, *b) => Some(("▲ newer", t.signal)),
                        (Some(a), Some(b)) if a + time_tolerance(a, *b) < *b => Some(("older", t.faint)),
                        _ => None,
                    },
                    _ => None,
                }
            } else {
                None
            };
            let name = e.name.clone();
            let name_click = e.name.clone();
            let name_menu = e.name.clone();
            let entry_menu = e.clone();
            let name_send = e.name.clone();
            let drag_paths: Vec<String> = if is_sel { sel_entries.iter().map(|x| x.path.clone()).collect() } else { vec![e.path.clone()] };
            let drag_label = if drag_paths.len() > 1 { format!("{} items", drag_paths.len()) } else { e.name.clone() };
            let drag = DraggedFiles { side, paths: drag_paths, label: drag_label };
            let folder_target = e.is_dir.then(|| e.path.clone());
            let ic = file_icon(&e.name, e.is_dir, e.is_symlink);
            let send_btn = |id: String, ic: &'static str, cx: &mut Context<Self>| {
                let n = name_send.clone();
                div()
                    .id(SharedString::from(id))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(24.))
                    .h(px(22.))
                    .rounded(px(4.))
                    .opacity(if is_sel { 1. } else { 0. })
                    .group_hover("file-row", |s| s.opacity(1.))
                    .hover(|s| s.bg(t.hover))
                    .when(session.is_some(), |d| d.child(icon(ic, 14., t.ink)))
                    .tooltip(tip(match side {
                        Side::Local => "Upload to the remote folder",
                        Side::Remote => "Download to the local folder",
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        let paths = this.targets(side, Some(&n)).into_iter().map(|e| e.path).collect();
                        this.transfer(side, paths, None, cx);
                    }))
            };
            let row = div()
                .id(SharedString::from(format!("row-{tag}-{ix}")))
                .group("file-row")
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .h(px(30.))
                .px(px(8.))
                .rounded(px(5.))
                .border_1()
                .border_color(if is_sel { t.ink.opacity(0.16) } else { t.bg.opacity(0.) })
                .when(is_sel, |d| d.bg(accent_soft))
                .when(!is_sel, |d| d.hover(|s| s.bg(t.surface2)))
                .cursor_pointer()
                .when(side == Side::Remote, |d| d.child(send_btn(format!("dl-{ix}"), "arrow-left", cx)))
                .child(icon(ic, 15., if e.is_dir { t.folder } else { t.muted }))
                .child(div().flex_1().min_w_0().truncate().font_weight(if e.is_dir { FontWeight::MEDIUM } else { FontWeight::NORMAL }).child(name))
                .when_some(badge, |d, (b, c)| d.child(div().flex_none().text_size(px(10.)).font_weight(FontWeight::SEMIBOLD).text_color(c).child(b)))
                .child(div().w(px(72.)).flex_none().text_right().text_size(px(12.)).text_color(t.muted).font_family(super::MONO_FONT).child(if e.is_dir { "—".to_string() } else { human_size(e.size) }))
                .child(div().w(px(84.)).flex_none().text_right().text_size(px(12.)).text_color(t.faint).font_family(super::MONO_FONT).child(format_time(e.modified)))
                .when(side == Side::Remote, |d| d.child(div().w(px(84.)).flex_none().text_size(px(11.)).text_color(t.faint).font_family(super::MONO_FONT).child(e.permissions.clone().unwrap_or_default())))
                .when(side == Side::Local, |d| d.child(send_btn(format!("ul-{ix}"), "arrow-right", cx)))
                .on_click(cx.listener(move |this, ev, window, cx| this.click_row(side, name_click.clone(), ev, window, cx)))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        this.last_side = side;
                        if let Some(p) = this.pane_mut(side)
                            && !p.selected.contains(&name_menu)
                        {
                            p.selected = [name_menu.clone()].into();
                            p.anchor = Some(name_menu.clone());
                        }
                        this.menu = Some(ContextMenu { pos: ev.position, side, entry: Some(entry_menu.clone()) });
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .on_drag(drag, |d: &DraggedFiles, _, _, cx| cx.new(|_| d.clone()));
            let row = match folder_target {
                Some(target) => row
                    .drag_over::<DraggedFiles>(move |s, _, _, _| s.bg(drop_color.opacity(0.14)).border_color(drop_color.opacity(0.7)))
                    .on_drop(cx.listener(move |this, d: &DraggedFiles, _, cx| {
                        cx.stop_propagation();
                        this.drop_into(side, d.clone(), target.clone(), cx);
                    })),
                None => row,
            };
            list = list.child(row);
        }

        // ---------- footer
        let sel_size: u64 = sel_entries.iter().map(|e| e.size).sum();
        let footer = div().flex().flex_none().items_center().gap(px(10.)).h(px(46.)).px(px(12.)).bg(t.surface).border_t_1().border_color(t.border);
        let footer = if !sel_entries.is_empty() && session.is_some() {
            let label = match side {
                Side::Local => format!("Upload to {remote_dir}"),
                Side::Remote => format!("Download to {}", shorten(&local_dir, 34)),
            };
            footer
                .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child(format!("{} selected", sel_entries.len())))
                .child(div().text_size(px(12.)).text_color(t.faint).font_family(super::MONO_FONT).child(human_size(sel_size)))
                .child(div().flex_1())
                .child(
                    button(SharedString::from(format!("clear-{tag}")), "Clear", None, BtnKind::Outline, cx).on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(p) = this.pane_mut(side) {
                            p.selected.clear();
                        }
                        cx.notify();
                    })),
                )
                .child(
                    button(SharedString::from(format!("send-{tag}")), label, Some(if side == Side::Local { "arrow-right" } else { "arrow-left" }), BtnKind::Primary, cx).on_click(cx.listener(move |this, _, _, cx| {
                        let paths = this.targets(side, None).into_iter().map(|e| e.path).collect();
                        this.transfer(side, paths, None, cx);
                    })),
                )
        } else {
            let count = self.pane(side).map(|p| p.entries.len()).unwrap_or(0);
            footer
                .child(div().text_size(px(12.)).text_color(t.faint).child(format!("{count} items")))
                .child(div().flex_1())
                .when(session.is_some(), |d| {
                    d.child(icon("info", 13., t.faint)).child(div().text_size(px(12.)).text_color(t.faint).truncate().child("Click → / ← on a row, double-click, or drag between panes"))
                })
        };

        div()
            .id(SharedString::from(format!("pane-{tag}")))
            .key_context("FilePane")
            .track_focus(&focus)
            .flex()
            .flex_col()
            .size_full()
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, _| this.last_side = side))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    this.last_side = side;
                    this.menu = Some(ContextMenu { pos: ev.position, side, entry: None });
                    cx.notify();
                }),
            )
            .drag_over::<DraggedFiles>(move |s, d, _, _| if d.side != side { s.bg(accent_soft) } else { s })
            .drag_over::<ExternalPaths>(move |s, _, _, _| if side == Side::Remote { s.bg(accent_soft) } else { s })
            .on_drop(cx.listener(move |this, d: &DraggedFiles, _, cx| {
                if d.side != side {
                    this.transfer(d.side, d.paths.clone(), None, cx);
                }
            }))
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                if side == Side::Remote {
                    let p = paths.paths().iter().map(|p| p.to_string_lossy().to_string()).collect();
                    this.transfer(Side::Local, p, None, cx);
                }
            }))
            .child(path_bar)
            .child(header)
            .child(div().id(SharedString::from(format!("rows-{tag}"))).flex_1().min_h_0().overflow_y_scroll().child(list))
            .child(footer)
    }

    /// Drop dragged entries onto a folder row: transfer from the other side, or move within the same side.
    fn drop_into(&mut self, side: Side, d: DraggedFiles, target: String, cx: &mut Context<Self>) {
        if d.side != side {
            self.transfer(d.side, d.paths, Some(target), cx);
            return;
        }
        if d.paths.iter().any(|p| *p == target) {
            return;
        }
        match side {
            Side::Local => {
                for p in &d.paths {
                    let src = std::path::Path::new(p);
                    if let Some(name) = src.file_name()
                        && let Err(e) = std::fs::rename(src, std::path::Path::new(&target).join(name))
                    {
                        self.toast(format!("{e}"), true);
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
                        for p in d.paths {
                            let name = p.trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string();
                            core.rename(&session, &p, &join_remote(&target, &name), &Origin::User).await?;
                        }
                        Ok(())
                    },
                    move |this, r, cx| {
                        if let Err(e) = r {
                            this.toast(format!("Move failed: {e:#}"), true);
                        }
                        this.load_remote(&sid, cx);
                    },
                );
            }
        }
    }

    pub fn render_drawer(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let jobs = self.core.jobs();
        let active: Vec<_> = jobs.iter().filter(|j| !j.state.lock().finished()).cloned().collect();
        let done: Vec<_> = jobs.iter().filter(|j| matches!(*j.state.lock(), JobState::Done | JobState::Cancelled)).cloned().collect();
        let failed: Vec<_> = jobs.iter().filter(|j| matches!(*j.state.lock(), JobState::Failed(_))).cloned().collect();
        let running = |d: Direction| -> f64 { jobs.iter().filter(|j| j.direction == d && matches!(*j.state.lock(), JobState::Running)).map(|j| j.speed()).sum() };
        let (up, down) = (running(Direction::Upload), running(Direction::Download));
        let open = self.drawer_open;
        let tab = self.drawer_tab;

        let tab_btn = |label: String, which: DrawerTab, cx: &mut Context<Self>| {
            let on = tab == which && open;
            div()
                .id(SharedString::from(format!("dtab-{label}")))
                .h(px(36.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(if on { t.ink } else { t.faint })
                .when(on, |d| d.font_weight(FontWeight::MEDIUM).border_b_2().border_color(t.ink))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.drawer_tab = which;
                    this.drawer_open = true;
                    cx.notify();
                }))
        };

        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(14.))
            .h(px(36.))
            .px(px(14.))
            .child(
                div()
                    .id("drawer-toggle")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .child(icon(if open { "chevron-down" } else { "chevron-right" }, 14., t.muted))
                    .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child("Transfers"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.drawer_open = !this.drawer_open;
                        cx.notify();
                    })),
            )
            .child(tab_btn(format!("Active {}", active.len()), DrawerTab::Active, cx))
            .child(tab_btn(format!("Done {}", done.len()), DrawerTab::Done, cx))
            .child(tab_btn(format!("Failed {}", failed.len()), DrawerTab::Failed, cx))
            .child(tab_btn("Activity".into(), DrawerTab::Log, cx))
            .child(div().flex_1())
            .child(div().text_size(px(12.)).font_family(super::MONO_FONT).text_color(if up > 0. { t.signal } else { t.faint }).child(format!("↑ {}/s", human_size(up as u64))))
            .child(div().text_size(px(12.)).font_family(super::MONO_FONT).text_color(if down > 0. { t.ink } else { t.faint }).child(format!("↓ {}/s", human_size(down as u64))))
            .child(button("clear-done", "Clear finished", None, BtnKind::Ghost, cx).on_click(cx.listener(|this, _, _, cx| {
                this.core.clear_finished_jobs();
                cx.notify();
            })));

        let body = if !open {
            None
        } else if tab == DrawerTab::Log {
            let log = self.core.log_entries();
            Some(
                div()
                    .id("drawer-log")
                    .h(px(170.))
                    .overflow_y_scroll()
                    .px(px(14.))
                    .pb(px(8.))
                    .when(log.is_empty(), |d| d.child(div().py(px(12.)).text_size(px(12.)).text_color(t.faint).child("Nothing yet")))
                    .children(log.into_iter().take(200).map(|l| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .h(px(24.))
                            .text_size(px(12.))
                            .child(div().w(px(64.)).flex_none().font_family(super::MONO_FONT).text_color(t.faint).child(l.time.format("%H:%M:%S").to_string()))
                            .child(
                                div()
                                    .w(px(96.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .when(l.origin.is_agent(), |d| d.child(icon("bot", 12., t.signal)))
                                    .text_color(if l.origin.is_agent() { t.signal } else { t.muted })
                                    .child(l.origin.label()),
                            )
                            .child(div().w(px(90.)).flex_none().font_family(super::MONO_FONT).child(l.action.clone()))
                            .child(div().flex_1().min_w_0().truncate().font_family(super::MONO_FONT).text_color(t.muted).child(l.target.clone()))
                            .child(status_pill(if l.ok { "ok" } else { "error" }, l.ok, &t))
                            .when(!l.ok, |d| d.child(div().max_w(px(320.)).truncate().text_color(t.err).child(l.detail.clone())))
                    })),
            )
        } else {
            let list = match tab {
                DrawerTab::Active => active,
                DrawerTab::Done => done,
                _ => failed,
            };
            Some(
                div()
                    .id("drawer-jobs")
                    .h(px(170.))
                    .overflow_y_scroll()
                    .px(px(14.))
                    .pb(px(8.))
                    .when(list.is_empty(), |d| {
                        d.child(div().py(px(12.)).text_size(px(12.)).text_color(t.faint).child(match tab {
                            DrawerTab::Active => "No active transfers. Select files and press → or ←, or drag them across.",
                            DrawerTab::Done => "Finished transfers show up here.",
                            _ => "No failures.",
                        }))
                    })
                    .children(list.into_iter().map(|j| {
                        let st = j.state.lock().clone();
                        let up = j.direction == Direction::Upload;
                        let route = if up { format!("{} → {}:{}", shorten(&j.local.to_string_lossy(), 40), j.session_name, j.remote) } else { format!("{}:{} → {}", j.session_name, j.remote, shorten(&j.local.to_string_lossy(), 40)) };
                        let frac = j.fraction();
                        let id = j.id;
                        let bar_color = match &st {
                            JobState::Failed(_) => t.err,
                            JobState::Done => t.ok,
                            _ if up => t.signal,
                            _ => t.muted,
                        };
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .py(px(6.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .text_size(px(12.))
                                    .child(icon(if up { "arrow-up" } else { "arrow-down" }, 13., if up { t.signal } else { t.muted }))
                                    .child(div().flex_none().font_weight(FontWeight::MEDIUM).child(j.name.clone()))
                                    .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.faint).font_family(super::MONO_FONT).child(route))
                                    .when(j.origin.is_agent(), |d| d.child(div().flex().items_center().gap(px(4.)).text_size(px(11.)).text_color(t.signal).child(icon("bot", 12., t.signal)).child(j.origin.label())))
                                    .child(div().text_size(px(11.)).text_color(t.muted).font_family(super::MONO_FONT).child(match &st {
                                        JobState::Running => format!("{}/s", human_size(j.speed() as u64)),
                                        JobState::Queued => "queued".into(),
                                        JobState::Done => human_size(j.total.max(j.done_bytes())),
                                        JobState::Cancelled => "cancelled".into(),
                                        JobState::Failed(e) => shorten(e, 60),
                                    }))
                                    .child(div().w(px(36.)).text_right().text_size(px(11.)).font_family(super::MONO_FONT).child(format!("{:.0}%", frac * 100.)))
                                    .child(match &st {
                                        JobState::Failed(_) | JobState::Cancelled => icon_button(SharedString::from(format!("retry-{id}")), "rotate-cw", cx).on_click(cx.listener(move |this, _, _, cx| {
                                            this.core.retry_job(id);
                                            this.drawer_tab = DrawerTab::Active;
                                            cx.notify();
                                        })),
                                        JobState::Done => icon_button(SharedString::from(format!("ok-{id}")), "check", cx),
                                        _ => icon_button(SharedString::from(format!("cancel-{id}")), "x", cx).on_click(cx.listener(move |this, _, _, cx| {
                                            this.core.cancel_job(id);
                                            cx.notify();
                                        })),
                                    }),
                            )
                            .child(div().w_full().h(px(2.)).bg(t.border).child(div().h_full().w(relative(frac)).bg(bar_color)))
                    })),
            )
        };

        div().flex().flex_col().flex_none().bg(t.surface).border_t_1().border_color(t.border).child(header).children(body)
    }
}

fn status_pill(text: &'static str, ok: bool, t: &Theme) -> impl IntoElement + use<> {
    let c = if ok { t.ok } else { t.err };
    div().px(px(6.)).rounded(px(4.)).bg(c.opacity(0.12)).text_color(c).text_size(px(10.)).font_weight(FontWeight::SEMIBOLD).child(text)
}

pub fn tip(text: &'static str) -> impl Fn(&mut Window, &mut gpui::App) -> gpui::AnyView + 'static {
    move |_, cx| cx.new(|_| super::workspace::Tip(text.into())).into()
}

/// FTP LIST output only has minute (recent files) or day (older files) precision.
fn time_tolerance(a: i64, b: i64) -> i64 {
    let coarse = |t: i64| if t % 86400 == 0 { 86400 } else if t % 60 == 0 { 60 } else { 2 };
    coarse(a).max(coarse(b))
}

pub fn shorten(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let tail: String = s.chars().skip(n - (max - 1)).collect();
    format!("…{tail}")
}

/// (label, absolute path) pairs for each breadcrumb segment.
fn breadcrumbs(side: Side, path: &str) -> Vec<(String, String)> {
    match side {
        Side::Remote => {
            let mut out = vec![("/".to_string(), "/".to_string())];
            let mut acc = String::new();
            for part in path.split('/').filter(|p| !p.is_empty()) {
                acc = format!("{acc}/{part}");
                out.push((part.to_string(), acc.clone()));
            }
            out
        }
        Side::Local => {
            let p = std::path::Path::new(path);
            let mut out: Vec<(String, String)> = p
                .ancestors()
                .map(|a| {
                    let label = a.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| a.to_string_lossy().trim_end_matches(['\\', '/']).to_string());
                    (label, a.to_string_lossy().to_string())
                })
                .filter(|(l, _)| !l.is_empty())
                .collect();
            out.reverse();
            if let Some(home) = dirs::home_dir() {
                let home = home.to_string_lossy().to_string();
                if let Some(i) = out.iter().position(|(_, p)| *p == home) {
                    out.drain(..i);
                    out[0].0 = "~".into();
                }
            }
            out
        }
    }
}
