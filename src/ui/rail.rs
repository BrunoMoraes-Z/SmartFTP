//! Discord-style connection rail: saved sites and folders of sites, reorderable by drag
//! and drop. Dropping a site on another site creates a folder; dropping on a folder adds it;
//! the thin gaps between items reorder.

use std::sync::Arc;

use gpui::{
    AnyElement, Context, FontWeight, Hsla, MouseButton, MouseDownEvent, Pixels, Point, PromptLevel, SharedString, Window, anchored,
    deferred, div, hsla, prelude::*, px,
};

use super::components::*;
use super::dialogs::{PromptKind, SITE_COLORS};
use super::panes::tip;
use super::theme::{Theme, theme};
use super::workspace::{Tip, View, Workspace};
use crate::core::config::RailNode;
use crate::core::model::Site;
use crate::core::{JobState, Session};
use crate::i18n::{tr, trf};

type MenuAction = Box<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

pub const RAIL_WIDTH: f32 = 68.;
const TILE: f32 = 42.;

/// Payload while dragging a rail item.
#[derive(Clone)]
pub struct DraggedRail {
    pub id: String,
    pub is_folder: bool,
    pub icon: String,
    pub color: Hsla,
}

impl Render for DraggedRail {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().opacity(0.85).child(tile(if self.is_folder { "folder" } else { &self.icon }, self.color, TILE))
    }
}

#[derive(Clone)]
pub enum RailTarget {
    Site(String),
    /// A session whose site is not saved (quick connect / agent URL).
    Session(String),
    Folder(String),
}

pub struct RailMenu {
    pub pos: Point<Pixels>,
    pub target: RailTarget,
}

/// Status of a site's sessions for the dot on its tile.
#[derive(Clone, Copy, PartialEq)]
enum Status {
    Offline,
    Online,
    Busy,
}

impl Workspace {
    fn site_status(&self, site_id: &str, sessions: &[Arc<Session>]) -> (Status, bool, bool) {
        let jobs = self.core.jobs();
        let mine: Vec<_> = sessions.iter().filter(|s| s.site.id == site_id).collect();
        if mine.is_empty() {
            return (Status::Offline, false, false);
        }
        let busy = jobs
            .iter()
            .any(|j| mine.iter().any(|s| s.id == j.session_id) && matches!(*j.state.lock(), JobState::Running | JobState::Queued));
        let active = mine.iter().any(|s| self.active.as_deref() == Some(s.id.as_str()));
        let agent = mine.iter().any(|s| s.origin.is_agent());
        (if busy { Status::Busy } else { Status::Online }, active, agent)
    }

    pub fn open_site(&mut self, site_id: &str, cx: &mut Context<Self>) {
        if let Some(s) = self.core.sessions().into_iter().find(|s| s.site.id == site_id) {
            self.activate(&s.id.clone(), cx);
            return;
        }
        let site = self.core.config.lock().sites.iter().find(|s| s.id == site_id).cloned();
        if let Some(site) = site {
            self.connect_site(site, cx);
        }
    }

    fn rail_drop(&mut self, d: &DraggedRail, folder: Option<String>, index: usize, cx: &mut Context<Self>) {
        let id = d.id.clone();
        self.core.update_config(|c| c.move_node(&id, folder.as_deref(), index));
        cx.notify();
    }

    /// Thin drop zone between items: highlights while dragging, reorders on drop.
    fn gap(&self, key: String, folder: Option<String>, index: usize, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let line = t.ink;
        div()
            .id(SharedString::from(key))
            .w(px(RAIL_WIDTH - 12.))
            .h(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .child(div().w(px(32.)).h(px(3.)).rounded(px(2.)).invisible_bar())
            .drag_over::<DraggedRail>(move |s, _, _, _| s.child_bar(line))
            .on_drop(cx.listener(move |this, d: &DraggedRail, _, cx| {
                cx.stop_propagation();
                this.rail_drop(d, folder.clone(), index, cx);
            }))
    }

    #[allow(clippy::too_many_arguments)]
    fn site_tile(
        &self,
        site: &Site,
        size: f32,
        sessions: &[Arc<Session>],
        in_folder: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (status, active, agent) = self.site_status(&site.id, sessions);
        let color = site_color(&site.color);
        let id = site.id.clone();
        let id_menu = site.id.clone();
        let id_drop = site.id.clone();
        let name: SharedString = site.name.clone().into();
        let connecting = self.connecting.contains(&site.id);
        let drag = DraggedRail { id: site.id.clone(), is_folder: false, icon: site.icon.clone(), color };
        let accept = t.ink;
        div()
            .id(SharedString::from(format!("rail-site-{}", site.id)))
            .relative()
            .w(px(RAIL_WIDTH - 12.))
            .h(px(size + 2.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            // Discord-style pill on the left edge for the active / connected site
            .when(!in_folder, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(-6.))
                        .top(px(if active { 7. } else { (size + 2.) / 2. - 4. }))
                        .w(px(4.))
                        .h(px(if active { size - 12. } else { 8. }))
                        .rounded_r(px(3.))
                        .bg(t.ink)
                        .when(status == Status::Offline, |d| d.invisible_bar()),
                )
            })
            .child(
                div()
                    .rounded(px((size / 5.).round() + 2.))
                    .border_2()
                    .border_color(if active { t.ink.opacity(0.7) } else { hsla(0., 0., 0., 0.) })
                    .child(tile(&site.icon, color, size - 4.).when(connecting, |d| d.opacity(0.5))),
            )
            .when(status != Status::Offline, |d| {
                d.child(
                    div()
                        .absolute()
                        .right(px((RAIL_WIDTH - 12. - size) / 2. - 2.))
                        .bottom(px(-1.))
                        .size(px(11.))
                        .rounded(px(6.))
                        .border_2()
                        .border_color(t.surface)
                        .bg(if status == Status::Busy { t.warn } else { t.ok }),
                )
            })
            .when(agent, |d| {
                d.child(div().absolute().left(px((RAIL_WIDTH - 12. - size) / 2. - 3.)).top(px(-2.)).child(icon("bot", 12., t.signal)))
            })
            .tooltip(move |_, cx| cx.new(|_| Tip(name.clone())).into())
            .on_click(cx.listener(move |this, _, _, cx| this.open_site(&id, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    this.rail_menu = Some(RailMenu { pos: ev.position, target: RailTarget::Site(id_menu.clone()) });
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_drag(drag, |d: &DraggedRail, _, _, cx| cx.new(|_| d.clone()))
            // dropping another site here creates a folder (top level only)
            .when(!in_folder, |d| {
                d.drag_over::<DraggedRail>(move |s, dr, _, _| if !dr.is_folder { s.bg(accept.opacity(0.08)).rounded(px(12.)) } else { s })
                    .on_drop(cx.listener(move |this, dr: &DraggedRail, _, cx| {
                        cx.stop_propagation();
                        if dr.is_folder || dr.id == id_drop {
                            return;
                        }
                        let (a, b) = (dr.id.clone(), id_drop.clone());
                        this.core.update_config(|c| {
                            c.merge_into_folder(&a, &b);
                        });
                        cx.notify();
                    }))
            })
            .into_any_element()
    }

    pub fn render_rail(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let sessions = self.core.sessions();
        let (rail, sites, mcp_on) = {
            let c = self.core.config.lock();
            (c.rail.clone(), c.sites.clone(), c.mcp.enabled)
        };
        let site = |id: &str| sites.iter().find(|s| s.id == id).cloned();

        // sessions whose site is not saved
        let loose: Vec<_> = sessions.iter().filter(|s| !sites.iter().any(|x| x.id == s.site.id)).cloned().collect();

        let mut list = div().flex().flex_col().items_center().w_full().pt(px(2.));
        for (i, node) in rail.iter().enumerate() {
            list = list.child(self.gap(format!("gap-{i}"), None, i, &t, cx));
            match node {
                RailNode::Site { id } => {
                    if let Some(s) = site(id) {
                        list = list.child(self.site_tile(&s, TILE, &sessions, false, &t, cx));
                    }
                }
                RailNode::Folder { id, name, color, sites: members, expanded } => {
                    let fcolor = site_color(color);
                    let member_sites: Vec<Site> = members.iter().filter_map(|m| site(m)).collect();
                    let any_online = member_sites.iter().any(|s| self.site_status(&s.id, &sessions).0 != Status::Offline);
                    let any_active = member_sites.iter().any(|s| self.site_status(&s.id, &sessions).1);
                    let fid = id.clone();
                    let fid_menu = id.clone();
                    let fid_drop = id.clone();
                    let count = members.len();
                    let label: SharedString = if count == 1 {
                        trf("{name} · 1 site", &[("name", name)])
                    } else {
                        trf("{name} · {n} sites", &[("name", name), ("n", &count.to_string())])
                    }
                    .into();
                    let accept = t.ink;
                    let header = div()
                        .id(SharedString::from(format!("rail-folder-{id}")))
                        .relative()
                        .w(px(RAIL_WIDTH - 12.))
                        .h(px(TILE + 2.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .when(!*expanded && any_online, |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .left(px(-6.))
                                    .top(px(if any_active { 7. } else { (TILE + 2.) / 2. - 4. }))
                                    .w(px(4.))
                                    .h(px(if any_active { TILE - 12. } else { 8. }))
                                    .rounded_r(px(3.))
                                    .bg(t.ink),
                            )
                        })
                        .child(if *expanded {
                            // open folder: a folder glyph in the folder's color
                            div().flex().items_center().justify_center().size(px(TILE - 4.)).rounded(px(10.)).child(icon(
                                "folder-open",
                                20.,
                                fcolor,
                            ))
                        } else {
                            folder_tile(&member_sites, fcolor, TILE - 4., &t)
                        })
                        .tooltip(move |_, cx| cx.new(|_| Tip(label.clone())).into())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let f = fid.clone();
                            this.core.update_config(|c| {
                                if let Some((_, _, exp)) = c.folder_mut(&f) {
                                    *exp = !*exp;
                                }
                            });
                            cx.notify();
                        }))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                this.rail_menu = Some(RailMenu { pos: ev.position, target: RailTarget::Folder(fid_menu.clone()) });
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        )
                        .on_drag(
                            DraggedRail { id: id.clone(), is_folder: true, icon: "folder".into(), color: fcolor },
                            |d: &DraggedRail, _, _, cx| cx.new(|_| d.clone()),
                        )
                        .drag_over::<DraggedRail>(
                            move |s, dr, _, _| if !dr.is_folder { s.bg(accept.opacity(0.08)).rounded(px(12.)) } else { s },
                        )
                        .on_drop(cx.listener(move |this, dr: &DraggedRail, _, cx| {
                            cx.stop_propagation();
                            if !dr.is_folder {
                                this.rail_drop(dr, Some(fid_drop.clone()), usize::MAX, cx);
                            }
                        }));
                    if *expanded {
                        let mut inner = div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .w(px(RAIL_WIDTH - 12.))
                            .py(px(2.))
                            .rounded(px(14.))
                            .bg(fcolor.opacity(0.12))
                            .child(header);
                        for (j, s) in member_sites.iter().enumerate() {
                            inner = inner.child(self.gap(format!("gap-{id}-{j}"), Some(id.clone()), j, &t, cx));
                            inner = inner.child(self.site_tile(s, TILE - 6., &sessions, true, &t, cx));
                        }
                        inner = inner.child(self.gap(format!("gap-{id}-end"), Some(id.clone()), member_sites.len(), &t, cx));
                        list = list.child(inner);
                    } else {
                        list = list.child(header);
                    }
                }
            }
        }
        list = list.child(self.gap("gap-end".into(), None, rail.len(), &t, cx));
        list = list
            .child(
                rail_button("rail-new", "plus", &t)
                    .tooltip(tip(tr("New site")))
                    .on_click(cx.listener(|this, _, window, cx| this.open_site_editor(None, window, cx))),
            )
            .child(div().h(px(6.)))
            .child(rail_button("rail-quick", "zap", &t).tooltip(tip(tr("Quick connect (URL)"))).on_click(cx.listener(
                |this, _, window, cx| this.open_prompt(PromptKind::QuickConnect, super::workspace::Side::Remote, None, window, cx),
            )))
            .child(div().h(px(10.)));

        div()
            .flex()
            .flex_col()
            .items_center()
            .flex_none()
            .w(px(RAIL_WIDTH))
            .h_full()
            .pt(px(10.))
            .pb(px(10.))
            .bg(t.surface)
            .border_r_1()
            .border_color(t.border)
            // unsaved sessions (quick connect, agent URLs) sit above the saved list
            .when(!loose.is_empty(), |d| {
                d.children(loose.iter().map(|s| {
                    let color = site_color(&s.site.color);
                    let active = self.active.as_deref() == Some(s.id.as_str());
                    let id = s.id.clone();
                    let id_menu = s.id.clone();
                    let name: SharedString = trf("{name} (not saved)", &[("name", &s.site.name)]).into();
                    div()
                        .id(SharedString::from(format!("rail-loose-{}", s.id)))
                        .relative()
                        .w(px(RAIL_WIDTH - 12.))
                        .h(px(TILE + 6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .child(
                            div()
                                .absolute()
                                .left(px(-6.))
                                .top(px(9.))
                                .w(px(4.))
                                .h(px(if active { TILE - 12. } else { 8. }))
                                .rounded_r(px(3.))
                                .bg(t.ink),
                        )
                        .child(
                            div()
                                .rounded(px(10.))
                                .border_2()
                                .border_color(if active { t.ink.opacity(0.7) } else { hsla(0., 0., 0., 0.) })
                                .child(tile(&s.site.icon, color, TILE - 4.)),
                        )
                        .when(s.origin.is_agent(), |d| d.child(div().absolute().left(px(4.)).top(px(0.)).child(icon("bot", 12., t.signal))))
                        .tooltip(move |_, cx| cx.new(|_| Tip(name.clone())).into())
                        .on_click(cx.listener(move |this, _, _, cx| this.activate(&id, cx)))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                this.rail_menu = Some(RailMenu { pos: ev.position, target: RailTarget::Session(id_menu.clone()) });
                                cx.notify();
                            }),
                        )
                }))
                .child(div().w(px(28.)).h(px(2.)).my(px(6.)).rounded(px(1.)).bg(t.border))
            })
            .child(div().id("rail-scroll").flex_1().min_h_0().w_full().overflow_y_scroll().child(list))
            .child(div().w(px(28.)).h(px(2.)).mb(px(8.)).rounded(px(1.)).bg(t.border))
            .child(
                div()
                    .id("rail-mcp")
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(36.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(t.hover))
                    .tooltip(tip(tr("MCP & Agents")))
                    .child(icon("bot", 18., t.muted))
                    .child(div().absolute().top(px(6.)).right(px(6.)).size(px(7.)).rounded(px(4.)).bg(if mcp_on { t.ok } else { t.faint }))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.tab = super::settings::SettingsTab::Mcp;
                        this.view = View::Settings;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("rail-settings")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(36.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(t.hover))
                    .tooltip(tip(tr("Settings")))
                    .child(icon("settings", 18., t.muted))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.view = if this.view == View::Settings { View::Main } else { View::Settings };
                        cx.notify();
                    })),
            )
    }

    pub fn render_rail_menu(&mut self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let menu = self.rail_menu.as_ref()?;
        let t = theme(cx).clone();
        let pos = menu.pos;
        let target = menu.target.clone();
        let sessions = self.core.sessions();
        let cfg = self.core.config.lock().clone();

        let item = |label: String, ic: &'static str, danger: bool, cx: &mut Context<Self>, f: MenuAction| {
            div()
                .id(SharedString::from(format!("rm-{label}")))
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
                    this.rail_menu = None;
                    f(this, window, cx);
                    cx.notify();
                }))
                .into_any_element()
        };

        let mut items: Vec<AnyElement> = vec![];
        let title: String;
        match &target {
            RailTarget::Site(id) => {
                let site = cfg.sites.iter().find(|s| s.id == *id).cloned()?;
                title = site.name.clone();
                let open = sessions.iter().find(|s| s.site.id == *id).map(|s| s.id.clone());
                let in_folder = cfg.rail.iter().any(|n| matches!(n, RailNode::Folder { sites, .. } if sites.contains(id)));
                let (i1, i2, i3, i4, i5) = (id.clone(), site.clone(), id.clone(), id.clone(), site.clone());
                items.push(item(
                    if open.is_some() { tr("Open").into() } else { tr("Connect").into() },
                    "plug-zap",
                    false,
                    cx,
                    Box::new(move |this, _, cx| this.open_site(&i1, cx)),
                ));
                items.push(item(
                    tr("Edit site…").into(),
                    "pencil",
                    false,
                    cx,
                    Box::new(move |this, window, cx| this.open_site_editor(Some(i2.clone()), window, cx)),
                ));
                items.push(item(
                    tr("Icon & color…").into(),
                    "palette",
                    false,
                    cx,
                    Box::new(move |this, _, cx| this.open_icon_picker(i3.clone(), pos, cx)),
                ));
                if in_folder {
                    items.push(item(
                        tr("Remove from folder").into(),
                        "folder-input",
                        false,
                        cx,
                        Box::new(move |this, _, cx| {
                            let id = i4.clone();
                            this.core.update_config(|c| {
                                let pos = c.rail.len();
                                c.move_node(&id, None, pos);
                            });
                            cx.notify();
                        }),
                    ));
                }
                if let Some(sid) = open {
                    items.push(item(
                        tr("Disconnect").into(),
                        "unplug",
                        false,
                        cx,
                        Box::new(move |this, _, cx| this.disconnect(sid.clone(), cx)),
                    ));
                }
                items.push(item(
                    tr("Delete site").into(),
                    "trash-2",
                    true,
                    cx,
                    Box::new(move |this, window, cx| this.confirm_delete_site(i5.clone(), window, cx)),
                ));
            }
            RailTarget::Session(sid) => {
                let s = sessions.iter().find(|s| s.id == *sid).cloned()?;
                title = s.site.name.clone();
                let (site, sid2) = (s.site.clone(), sid.clone());
                items.push(item(
                    tr("Save as site…").into(),
                    "folder-plus",
                    false,
                    cx,
                    Box::new(move |this, window, cx| this.open_site_editor(Some(site.clone()), window, cx)),
                ));
                items.push(item(
                    tr("Disconnect").into(),
                    "unplug",
                    false,
                    cx,
                    Box::new(move |this, _, cx| this.disconnect(sid2.clone(), cx)),
                ));
            }
            RailTarget::Folder(fid) => {
                let Some(RailNode::Folder { name, color, expanded, .. }) = cfg.rail.iter().find(|n| n.id() == fid).cloned() else {
                    return None;
                };
                title = name.clone();
                let (f1, f2, f3, n1) = (fid.clone(), fid.clone(), fid.clone(), name.clone());
                items.push(item(
                    if expanded { tr("Collapse").into() } else { tr("Expand").into() },
                    "folder-open",
                    false,
                    cx,
                    Box::new(move |this, _, cx| {
                        let f = f1.clone();
                        this.core.update_config(|c| {
                            if let Some((_, _, e)) = c.folder_mut(&f) {
                                *e = !*e;
                            }
                        });
                        cx.notify();
                    }),
                ));
                items.push(item(
                    tr("Rename folder…").into(),
                    "pencil",
                    false,
                    cx,
                    Box::new(move |this, window, cx| this.open_folder_rename(f2.clone(), n1.clone(), window, cx)),
                ));
                items.push(item(
                    tr("Ungroup").into(),
                    "ungroup",
                    false,
                    cx,
                    Box::new(move |this, _, cx| {
                        let f = f3.clone();
                        this.core.update_config(|c| c.ungroup(&f));
                        cx.notify();
                    }),
                ));
                // color swatches
                let fid_c = fid.clone();
                items.push(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(4.))
                        .px(px(8.))
                        .py(px(6.))
                        .children(SITE_COLORS.iter().map(|c| {
                            let on = color.eq_ignore_ascii_case(c);
                            let f = fid_c.clone();
                            div()
                                .id(SharedString::from(format!("fc-{c}")))
                                .size(px(18.))
                                .p(px(2.))
                                .rounded(px(9.))
                                .cursor_pointer()
                                .border_2()
                                .border_color(if on { t.ink } else { hsla(0., 0., 0., 0.) })
                                .child(div().size_full().rounded(px(7.)).bg(site_color(c)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let f = f.clone();
                                    this.core.update_config(|cfg| {
                                        if let Some((_, col, _)) = cfg.folder_mut(&f) {
                                            *col = c.to_string();
                                        }
                                    });
                                    cx.notify();
                                }))
                        }))
                        .into_any_element(),
                );
            }
        }

        Some(
            deferred(
                anchored().position(pos).snap_to_window().child(
                    div()
                        .id("rail-menu")
                        .occlude()
                        .flex()
                        .flex_col()
                        .min_w(px(200.))
                        .p(px(4.))
                        .rounded(px(7.))
                        .bg(t.surface2)
                        .border_1()
                        .border_color(t.border)
                        .shadow_lg()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.rail_menu = None;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .px(px(8.))
                                .py(px(4.))
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(t.faint)
                                .truncate()
                                .child(title),
                        )
                        .children(items),
                ),
            )
            .with_priority(2),
        )
    }

    fn confirm_delete_site(&mut self, site: Site, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Warning,
            &trf("Delete the site \"{name}\"?", &[("name", &site.name)]),
            Some(tr("Only the saved connection is removed; no files are touched.")),
            &[tr("Delete"), tr("Cancel")],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() == Some(0) {
                this.update(cx, |this, cx| {
                    this.core.update_config(|c| c.remove_site(&site.id));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
}

/// Closed folder: a rounded square with up to four mini tiles of its sites (like Discord).
fn folder_tile(sites: &[Site], color: Hsla, size: f32, t: &Theme) -> gpui::Div {
    let mini = (size - 14.) / 2.;
    let cell = |s: Option<&Site>| match s {
        Some(s) => {
            let c = site_color(&s.color);
            div().flex().items_center().justify_center().size(px(mini)).rounded(px(4.)).bg(c.opacity(0.3)).child(icon(
                &s.icon,
                (mini * 0.6).round(),
                c,
            ))
        }
        None => div().size(px(mini)).rounded(px(4.)).bg(t.ink.opacity(0.06)),
    };
    div()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(3.))
        .p(px(5.))
        .size(px(size))
        .rounded(px(10.))
        .bg(color.opacity(0.22))
        .border_1()
        .border_color(color.opacity(0.45))
        .child(div().flex().gap(px(3.)).child(cell(sites.first())).child(cell(sites.get(1))))
        .child(div().flex().gap(px(3.)).child(cell(sites.get(2))).child(cell(sites.get(3))))
}

fn rail_button(id: &'static str, ic: &'static str, t: &Theme) -> gpui::Stateful<gpui::Div> {
    let ok = t.ok;
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(TILE - 4.))
        .rounded(px(10.))
        .cursor_pointer()
        .bg(t.surface2)
        .border_1()
        .border_color(t.border)
        .hover(move |s| s.bg(ok.opacity(0.18)).border_color(ok.opacity(0.5)))
        .child(icon(ic, 18., t.muted))
}

/// Small helpers so drop-zone bars can be hidden until something is dragged over them.
trait BarExt: Sized {
    fn invisible_bar(self) -> Self;
}

impl BarExt for gpui::Div {
    fn invisible_bar(self) -> Self {
        self.opacity(0.)
    }
}

trait ChildBar {
    fn child_bar(self, color: Hsla) -> Self;
}

impl ChildBar for gpui::StyleRefinement {
    fn child_bar(self, color: Hsla) -> Self {
        // the gap itself lights up as an insertion line
        self.bg(color.opacity(0.85)).rounded(px(2.))
    }
}
