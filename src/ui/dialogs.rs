use std::sync::Arc;

use gpui::{
    AnyElement, Context, Entity, EventEmitter, Focusable, FontWeight, MouseButton, PathPromptOptions, Pixels, Point, SharedString,
    Subscription, Window, anchored, deferred, div, hsla, prelude::*, px,
};

use super::components::*;
use super::input::{InputEvent, TextInput};
use super::theme::{parse_color, theme};
use super::workspace::{Modal, Side, Workspace};
use crate::core::Core;
use crate::core::model::{Entry, Protocol, Site};
use crate::i18n::{tr, trf};

pub const SITE_ICONS: &[&str] = &[
    "server",
    "server-cog",
    "database",
    "hard-drive",
    "cloud",
    "globe",
    "image",
    "film",
    "archive",
    "flask-conical",
    "rocket",
    "shield",
    "briefcase",
    "house",
    "box",
    "terminal",
    "folder-git-2",
    "package",
    "cpu",
    "zap",
];

pub const SITE_COLORS: &[&str] =
    &["#E5484D", "#F0A23B", "#E2C044", "#5BB974", "#2BB3A3", "#3E9BF0", "#8E7CF0", "#D86FB4", "#A18072", "#8E8C99"];

#[derive(Clone, Copy, PartialEq)]
pub enum PromptKind {
    NewFolder,
    NewFile,
    Rename,
    Chmod,
    QuickConnect,
    RenameFolder,
}

pub struct PromptState {
    pub kind: PromptKind,
    pub side: Side,
    pub target: Option<Entry>,
    /// Extra id for prompts that are not about a file (e.g. the folder being renamed).
    pub extra: String,
    pub input: Entity<TextInput>,
    pub _sub: Subscription,
}

pub struct IconPicker {
    pub site: Site,
    pub anchor: Point<Pixels>,
}

impl IconPicker {
    pub fn new(site: Site, anchor: Point<Pixels>) -> Self {
        IconPicker { site, anchor }
    }
}

// ------------------------------------------------------------------ site editor

#[allow(clippy::large_enum_variant)]
pub enum SiteEditorEvent {
    Close,
    Connect(Site),
}

struct Fields {
    name: Entity<TextInput>,
    group: Entity<TextInput>,
    host: Entity<TextInput>,
    port: Entity<TextInput>,
    user: Entity<TextInput>,
    password: Entity<TextInput>,
    key_path: Entity<TextInput>,
    remote_dir: Entity<TextInput>,
    local_dir: Entity<TextInput>,
}

pub struct SiteEditor {
    core: Arc<Core>,
    draft: Site,
    is_new: bool,
    fields: Fields,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<SiteEditorEvent> for SiteEditor {}

impl SiteEditor {
    pub fn new(core: Arc<Core>, site: Option<Site>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let is_new = site.is_none();
        let draft = site.unwrap_or_else(Site::new_empty);
        let fields = Self::make_fields(&draft, cx);
        let mut ed = SiteEditor { core, draft, is_new, fields, error: None, _subs: vec![] };
        ed.subscribe_fields(window, cx);
        window.focus(&ed.fields.host.read(cx).focus_handle(cx));
        ed
    }

    fn make_fields(site: &Site, cx: &mut Context<Self>) -> Fields {
        let mk = |cx: &mut Context<Self>, ph: &str, v: &str| cx.new(|cx| TextInput::new(cx, ph.to_string()).with_text(v.to_string()));
        let mono =
            |cx: &mut Context<Self>, ph: &str, v: &str| cx.new(|cx| TextInput::new(cx, ph.to_string()).mono().with_text(v.to_string()));
        Fields {
            name: mk(cx, tr("My server"), &site.name),
            group: mk(cx, tr("e.g. Production"), &site.group),
            host: mk(cx, "sftp.example.com", &site.host),
            port: mk(cx, &site.protocol.default_port().to_string(), &site.port.to_string()),
            user: mk(cx, tr("username"), &site.user),
            password: cx.new(|cx| {
                TextInput::new(
                    cx,
                    if site.password.is_some() { tr("Saved — leave empty to keep") } else { tr("Password or key passphrase") },
                )
                .masked()
            }),
            key_path: mono(cx, tr("~/.ssh/id_ed25519 (optional)"), site.key_path.as_deref().unwrap_or("")),
            remote_dir: mono(cx, tr("/ (server default)"), &site.remote_dir),
            local_dir: mono(cx, tr("local start folder (optional)"), &site.local_dir),
        }
    }

    fn subscribe_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let all = [
            &self.fields.name,
            &self.fields.group,
            &self.fields.host,
            &self.fields.port,
            &self.fields.user,
            &self.fields.password,
            &self.fields.key_path,
            &self.fields.remote_dir,
            &self.fields.local_dir,
        ]
        .map(|e| e.clone());
        self._subs = all
            .iter()
            .map(|e| {
                cx.subscribe_in(e, window, |this, _, ev: &InputEvent, _, cx| match ev {
                    InputEvent::Submit => this.save(true, cx),
                    InputEvent::Cancel => cx.emit(SiteEditorEvent::Close),
                    InputEvent::Changed => {}
                })
            })
            .collect();
    }

    fn select_site(&mut self, site: Site, is_new: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = site;
        self.is_new = is_new;
        self.error = None;
        self.fields = Self::make_fields(&self.draft, cx);
        self.subscribe_fields(window, cx);
        cx.notify();
    }

    fn collect(&mut self, cx: &mut Context<Self>) -> Result<Site, String> {
        let f = &self.fields;
        let text = |e: &Entity<TextInput>| e.read(cx).text().trim().to_string();
        let mut s = self.draft.clone();
        s.name = text(&f.name);
        s.group = text(&f.group);
        s.host = text(&f.host);
        if let Some(parsed) = Site::from_url(&s.host).filter(|_| s.host.contains("://")) {
            // allow pasting a full URL in the host field
            s.protocol = parsed.protocol;
            s.host = parsed.host;
            s.port = parsed.port;
            if !parsed.user.is_empty() {
                s.user = parsed.user;
            }
        }
        let port = text(&f.port);
        s.port =
            if port.is_empty() { s.protocol.default_port() } else { port.parse().map_err(|_| tr("Port must be a number").to_string())? };
        let user = text(&f.user);
        if !user.is_empty() {
            s.user = user;
        }
        let pw = f.password.read(cx).text();
        if !pw.is_empty() {
            s.password = Some(pw);
        }
        let key = text(&f.key_path);
        s.key_path = (!key.is_empty()).then_some(key);
        s.remote_dir = text(&f.remote_dir);
        s.local_dir = text(&f.local_dir);
        if s.host.is_empty() {
            return Err(tr("Host is required").into());
        }
        if s.name.is_empty() {
            s.name = s.host.clone();
        }
        Ok(s)
    }

    fn save(&mut self, connect: bool, cx: &mut Context<Self>) {
        match self.collect(cx) {
            Ok(site) => {
                let s = site.clone();
                self.core.update_config(|c| c.upsert_site(s));
                self.draft = site.clone();
                self.is_new = false;
                if connect {
                    cx.emit(SiteEditorEvent::Connect(site));
                } else {
                    cx.emit(SiteEditorEvent::Close);
                }
            }
            Err(e) => {
                self.error = Some(e);
                cx.notify();
            }
        }
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        let id = self.draft.id.clone();
        self.core.update_config(|c| c.remove_site(&id));
        cx.emit(SiteEditorEvent::Close);
    }

    fn browse_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("Choose private key").into()),
        });
        let input = self.fields.key_path.clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(p) = paths.first()
            {
                let p = p.to_string_lossy().to_string();
                input.update(cx, |i, cx| i.set_text(p, cx)).ok();
            }
        })
        .detach();
    }
}

impl Render for SiteEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let sites = self.core.config.lock().sites.clone();
        let draft = self.draft.clone();
        let color = site_color(&draft.color);

        let list = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .w(px(240.))
            .flex_none()
            .h_full()
            .p(px(12.))
            .bg(t.surface2)
            .border_r_1()
            .border_color(t.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pb(px(10.))
                    .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(tr("Sites")))
                    .child(
                        button("ed-new", tr("New"), Some("plus"), BtnKind::Outline, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.select_site(Site::new_empty(), true, window, cx))),
                    ),
            )
            .child(div().id("ed-list").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().gap(px(2.)).children(
                sites.into_iter().map(|s| {
                    let active = s.id == draft.id;
                    let s2 = s.clone();
                    div()
                        .id(SharedString::from(format!("ed-site-{}", s.id)))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .h(px(36.))
                        .px(px(8.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .when(active, |d| d.bg(t.hover))
                        .hover(|d| d.bg(t.hover))
                        .child(tile(&s.icon, site_color(&s.color), 22.))
                        .child(div().flex_1().min_w_0().truncate().child(s.name.clone()))
                        .child(div().text_size(px(10.)).text_color(t.faint).font_weight(FontWeight::SEMIBOLD).child(s.protocol.label()))
                        .on_click(cx.listener(move |this, _, window, cx| this.select_site(s2.clone(), false, window, cx)))
                }),
            ));

        let proto_seg = div().flex().h(px(34.)).p(px(3.)).gap(px(3.)).rounded(px(8.)).bg(t.bg).border_1().border_color(t.border).children(
            [Protocol::Ftp, Protocol::Ftps, Protocol::Sftp].into_iter().map(|p| {
                let on = draft.protocol == p;
                let label = match p {
                    Protocol::Ftp => "FTP",
                    Protocol::Ftps => tr("FTPS (explicit TLS)"),
                    Protocol::Sftp => "SFTP",
                };
                div()
                    .id(label)
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .when(on, |d| d.bg(t.hover).font_weight(FontWeight::SEMIBOLD))
                    .text_color(if on { t.ink } else { t.muted })
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let old_default = this.draft.protocol.default_port();
                        this.draft.protocol = p;
                        let port_text = this.fields.port.read(cx).text();
                        if port_text.is_empty() || port_text == old_default.to_string() {
                            this.fields.port.update(cx, |i, cx| i.set_text(p.default_port().to_string(), cx));
                        }
                        cx.notify();
                    }))
            }),
        );

        let identity = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().text_size(px(12.)).text_color(t.muted).font_weight(FontWeight::MEDIUM).child(tr("Icon & color")))
            .child(div().flex().items_center().gap(px(12.)).child(tile(&draft.icon, color, 40.)).child(
                div().flex().flex_wrap().gap(px(3.)).flex_1().children(SITE_ICONS.iter().map(|ic| {
                    let on = draft.icon == *ic;
                    div()
                        .id(SharedString::from(format!("ed-ic-{ic}")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(28.))
                        .rounded(px(5.))
                        .cursor_pointer()
                        .when(on, |d| d.bg(t.hover).border_1().border_color(t.muted))
                        .hover(|d| d.bg(t.hover))
                        .child(icon(ic, 15., if on { t.ink } else { t.muted }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.draft.icon = ic.to_string();
                            cx.notify();
                        }))
                })),
            ))
            .child(div().flex().gap(px(6.)).children(SITE_COLORS.iter().map(|c| {
                let on = draft.color.eq_ignore_ascii_case(c);
                div()
                    .id(SharedString::from(format!("ed-col-{c}")))
                    .size(px(22.))
                    .p(px(3.))
                    .rounded(px(11.))
                    .cursor_pointer()
                    .border_2()
                    .border_color(if on { t.ink } else { hsla(0., 0., 0., 0.) })
                    .child(div().size_full().rounded(px(8.)).bg(site_color(c)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.draft.color = c.to_string();
                        cx.notify();
                    }))
            })));

        let allow = draft.allow_mcp;
        let form = div()
            .id("ed-form")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .gap(px(14.))
            .p(px(22.))
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(if self.is_new {
                        tr("New site").to_string()
                    } else {
                        draft.name.clone()
                    }))
                    .child(icon_button("ed-close", "x", cx).on_click(cx.listener(|_, _, _, cx| cx.emit(SiteEditorEvent::Close)))),
            )
            .child(proto_seg)
            .child(div().flex().gap(px(12.)).child(field(tr("Name"), self.fields.name.clone(), &t)).child(div().w(px(200.)).child(field(
                tr("Group"),
                self.fields.group.clone(),
                &t,
            ))))
            .child(div().flex().gap(px(12.)).child(field(tr("Host"), self.fields.host.clone(), &t)).child(div().w(px(100.)).child(field(
                tr("Port"),
                self.fields.port.clone(),
                &t,
            ))))
            .child(div().flex().gap(px(12.)).child(field(tr("Username"), self.fields.user.clone(), &t)).child(field(
                tr("Password"),
                self.fields.password.clone(),
                &t,
            )))
            .when(draft.protocol == Protocol::Sftp, |d| {
                d.child(
                    div().flex().items_end().gap(px(8.)).child(field(tr("Private key"), self.fields.key_path.clone(), &t)).child(
                        button("ed-browse", tr("Browse"), Some("folder"), BtnKind::Outline, cx)
                            .h(px(32.))
                            .on_click(cx.listener(|this, _, window, cx| this.browse_key(window, cx))),
                    ),
                )
            })
            .child(div().flex().gap(px(12.)).child(field(tr("Remote start folder"), self.fields.remote_dir.clone(), &t)).child(field(
                tr("Local start folder"),
                self.fields.local_dir.clone(),
                &t,
            )))
            .child(identity)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .bg(t.surface2)
                    .border_1()
                    .border_color(t.border)
                    .child(icon("bot", 18., t.ink))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap(px(2.))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(tr("Allow AI agents (MCP)")))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(t.faint)
                                    .child(tr("Agents can open this site and run file operations. Passwords are never exposed to them.")),
                            ),
                    )
                    .child(toggle("ed-mcp", allow, cx).on_click(cx.listener(|this, _, _, cx| {
                        this.draft.allow_mcp = !this.draft.allow_mcp;
                        cx.notify();
                    }))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .bg(t.surface2)
                    .border_1()
                    .border_color(t.border)
                    .child(icon("file-pen", 18., t.ink))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap(px(2.))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(tr("Upload edited files automatically")))
                            .child(div().text_size(px(12.)).text_color(t.faint).child(tr(
                                "With View / Edit, every save is uploaded right away and replaces the file on the server, without asking.",
                            ))),
                    )
                    .child(toggle("ed-auto-upload", draft.edit_auto_upload, cx).on_click(cx.listener(|this, _, _, cx| {
                        this.draft.edit_auto_upload = !this.draft.edit_auto_upload;
                        cx.notify();
                    }))),
            )
            .when_some(self.error.clone(), |d, e| d.child(div().text_color(t.err).text_size(px(12.)).child(e)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .pt(px(6.))
                    .when(!self.is_new, |d| {
                        d.child(
                            button("ed-delete", tr("Delete"), Some("trash-2"), BtnKind::Danger, cx)
                                .on_click(cx.listener(|this, _, _, cx| this.delete(cx))),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        button("ed-save", tr("Save"), None, BtnKind::Outline, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.save(false, cx))),
                    )
                    .child(
                        button("ed-connect", tr("Save & connect"), Some("plug-zap"), BtnKind::Primary, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.save(true, cx))),
                    ),
            );

        div()
            .id("site-editor")
            .occlude()
            .flex()
            .w(px(900.))
            .h(px(640.))
            .max_h_full()
            .rounded(px(12.))
            .bg(t.surface)
            .border_1()
            .border_color(t.border)
            .shadow_2xl()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(list)
            .child(form)
    }
}

// ------------------------------------------------------------------ modals on the workspace

impl Workspace {
    pub fn render_modal(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = theme(cx).clone();
        let modal = self.modal.as_ref()?;
        let scrim = |dim: bool, cx: &mut Context<Self>| {
            div()
                .id("scrim")
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .when(dim, |d| d.bg(hsla(0., 0., 0., 0.55)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.modal = None;
                        cx.notify();
                    }),
                )
        };
        Some(match modal {
            Modal::Site(editor) => deferred(scrim(true, cx).child(editor.clone())).with_priority(1).into_any_element(),
            Modal::Prompt(p) => {
                let (title, action) = match p.kind {
                    PromptKind::NewFolder => (tr("New folder"), tr("Create")),
                    PromptKind::NewFile => (tr("New empty file"), tr("Create")),
                    PromptKind::Rename => (tr("Rename"), tr("Rename")),
                    PromptKind::Chmod => (tr("Permissions"), tr("Apply")),
                    PromptKind::QuickConnect => (tr("Quick connect"), tr("Connect")),
                    PromptKind::RenameFolder => (tr("Rename folder"), tr("Rename")),
                };
                let site = self.session().map(|s| s.site.name.clone()).unwrap_or_default();
                let dir = self.pane(p.side).map(|x| x.path.clone()).unwrap_or_default();
                let subtitle = match (&p.kind, &p.target, p.side) {
                    (PromptKind::QuickConnect, _, _) => tr("sftp://, ftp:// or ftps:// — the connection is not saved").to_string(),
                    (PromptKind::RenameFolder, _, _) => tr("Folder in the connection rail").to_string(),
                    (_, Some(e), Side::Local) => trf("{name} on this computer", &[("name", &e.name)]),
                    (_, Some(e), Side::Remote) => trf("{name} on {site}", &[("name", &e.name), ("site", &site)]),
                    (_, None, Side::Local) => trf("in {path} on this computer", &[("path", &dir)]),
                    (_, None, Side::Remote) => trf("in {path} on {site}", &[("path", &dir), ("site", &site)]),
                };
                deferred(
                    scrim(true, cx).child(
                        div()
                            .id("prompt")
                            .occlude()
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .w(px(420.))
                            .p(px(18.))
                            .rounded(px(10.))
                            .bg(t.surface)
                            .border_1()
                            .border_color(t.border)
                            .shadow_2xl()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(title))
                                    .child(div().text_size(px(12.)).text_color(t.faint).truncate().child(subtitle)),
                            )
                            .child(p.input.clone())
                            .when(p.kind == PromptKind::Chmod, |d| {
                                d.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(t.faint)
                                        .child(tr("755 = rwxr-xr-x (folders, scripts) · 644 = rw-r--r-- (files) · 600 = owner only")),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap(px(8.))
                                    .child(button("pr-cancel", tr("Cancel"), None, BtnKind::Outline, cx).on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.modal = None;
                                            cx.notify();
                                        },
                                    )))
                                    .child(
                                        button("pr-ok", action, None, BtnKind::Primary, cx)
                                            .on_click(cx.listener(|this, _, window, cx| this.submit_prompt(window, cx))),
                                    ),
                            ),
                    ),
                )
                .with_priority(1)
                .into_any_element()
            }
            Modal::IconPicker(p) => {
                let site = p.site.clone();
                let color = site_color(&site.color);
                let anchor = p.anchor;
                deferred(
                    div().absolute().inset_0().child(scrim(false, cx)).child(
                        anchored().position(anchor).snap_to_window_with_margin(px(8.)).child(
                            div()
                                .id("icon-picker")
                                .occlude()
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .w(px(292.))
                                .p(px(14.))
                                .rounded(px(8.))
                                .bg(t.surface2)
                                .border_1()
                                .border_color(t.border)
                                .shadow_2xl()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(
                                    div().flex().items_center().gap(px(10.)).child(tile(&site.icon, color, 40.)).child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .child(div().font_weight(FontWeight::SEMIBOLD).child(site.name.clone()))
                                            .child(div().text_size(px(11.)).text_color(t.faint).child(tr("Customize icon and color"))),
                                    ),
                                )
                                .child(section_label(tr("ICON"), &t))
                                .child(div().flex().flex_wrap().gap(px(4.)).children(SITE_ICONS.iter().map(|ic| {
                                    let on = site.icon == *ic;
                                    div()
                                        .id(SharedString::from(format!("pk-ic-{ic}")))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .w(px(48.))
                                        .h(px(32.))
                                        .rounded(px(5.))
                                        .cursor_pointer()
                                        .when(on, |d| d.bg(t.hover).border_1().border_color(t.muted))
                                        .hover(|d| d.bg(t.hover))
                                        .child(icon(ic, 16., if on { t.ink } else { t.muted }))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(Modal::IconPicker(p)) = &mut this.modal {
                                                p.site.icon = ic.to_string();
                                            }
                                            cx.notify();
                                        }))
                                })))
                                .child(section_label(tr("COLOR"), &t))
                                .child(div().flex().justify_between().children(SITE_COLORS.iter().map(|c| {
                                    let on = site.color.eq_ignore_ascii_case(c);
                                    div()
                                        .id(SharedString::from(format!("pk-col-{c}")))
                                        .size(px(22.))
                                        .p(px(3.))
                                        .rounded(px(11.))
                                        .cursor_pointer()
                                        .border_2()
                                        .border_color(if on { t.ink } else { hsla(0., 0., 0., 0.) })
                                        .child(div().size_full().rounded(px(8.)).bg(parse_color(c).unwrap()))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(Modal::IconPicker(p)) = &mut this.modal {
                                                p.site.color = c.to_string();
                                            }
                                            cx.notify();
                                        }))
                                })))
                                .child(div().h(px(1.)).bg(t.border))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(div().flex_1().text_size(px(11.)).text_color(t.faint).child(tr("Saved with the site")))
                                        .child(button("pk-cancel", tr("Cancel"), None, BtnKind::Ghost, cx).on_click(cx.listener(
                                            |this, _, _, cx| {
                                                this.modal = None;
                                                cx.notify();
                                            },
                                        )))
                                        .child(button("pk-done", tr("Done"), None, BtnKind::Primary, cx).on_click(cx.listener(
                                            |this, _, _, cx| {
                                                if let Some(Modal::IconPicker(p)) = this.modal.take() {
                                                    let site = p.site;
                                                    this.core.update_config(|c| c.upsert_site(site));
                                                }
                                                cx.notify();
                                            },
                                        ))),
                                ),
                        ),
                    ),
                )
                .with_priority(1)
                .into_any_element()
            }
        })
    }
}
