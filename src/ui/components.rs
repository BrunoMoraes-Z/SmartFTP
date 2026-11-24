use gpui::{App, Div, ElementId, Hsla, SharedString, Stateful, Svg, div, prelude::*, px, svg};

use super::theme::{Theme, parse_color, theme};

pub fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg().path(SharedString::from(format!("icons/{name}.svg"))).size(px(size)).flex_none().text_color(color)
}

pub fn site_color(hex: &str) -> Hsla {
    parse_color(hex).unwrap_or(gpui::rgb(0x3E9BF0).into())
}

/// Colored rounded square with an icon: the identity of a site / session.
pub fn tile(icon_name: &str, color: Hsla, size: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px((size / 5.0).round()))
        .bg(color.opacity(0.16))
        .border_1()
        .border_color(color.opacity(0.38))
        .child(icon(icon_name, (size * 0.5).round(), color))
}

#[derive(Clone, Copy, PartialEq)]
pub enum BtnKind {
    Primary,
    Outline,
    Ghost,
    Danger,
}

pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, icon_name: Option<&str>, kind: BtnKind, cx: &App) -> Stateful<Div> {
    let t = theme(cx);
    let (bg, fg, border, hover) = match kind {
        BtnKind::Primary => (t.accent, t.accent_ink, t.accent, t.accent.opacity(0.85)),
        BtnKind::Outline => (t.bg.opacity(0.0), t.ink, t.border, t.hover),
        BtnKind::Ghost => (t.bg.opacity(0.0), t.muted, t.bg.opacity(0.0), t.hover),
        BtnKind::Danger => (t.bg.opacity(0.0), t.err, t.bg.opacity(0.0), t.err.opacity(0.12)),
    };
    let label: SharedString = label.into();
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(28.))
        .px(px(10.))
        .rounded(px(6.))
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(fg)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .when_some(icon_name, |d, i| d.child(icon(i, 14., fg)))
        .when(!label.is_empty(), |d| d.child(label))
}

pub fn icon_button(id: impl Into<ElementId>, icon_name: &str, cx: &App) -> Stateful<Div> {
    let t = theme(cx);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(26.))
        .rounded(px(5.))
        .cursor_pointer()
        .hover(|s| s.bg(t.hover))
        .child(icon(icon_name, 15., t.muted))
}

pub fn section_label(text: impl Into<SharedString>, t: &Theme) -> Div {
    div().text_size(px(11.)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(t.faint).child(text.into())
}

pub fn field(label: impl Into<SharedString>, input: impl IntoElement, t: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .flex_1()
        .child(div().text_size(px(12.)).text_color(t.muted).font_weight(gpui::FontWeight::MEDIUM).child(label.into()))
        .child(input)
}

pub fn toggle(id: impl Into<ElementId>, on: bool, cx: &App) -> Stateful<Div> {
    let t = theme(cx);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .w(px(34.))
        .h(px(20.))
        .p(px(2.))
        .rounded(px(10.))
        .cursor_pointer()
        .bg(if on { t.accent } else { t.border })
        .when(on, |d| d.justify_end())
        .child(div().size(px(16.)).rounded(px(8.)).bg(if on { t.accent_ink } else { t.muted }))
}

pub fn format_time(ts: Option<i64>) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let Some(ts) = ts else { return String::new() };
    let Some(dt) = Local.timestamp_opt(ts, 0).single() else { return String::new() };
    let now = Local::now();
    // pt-BR / es put the day first: "05 mar" instead of "Mar 05"
    let month = crate::i18n::month_abbr(dt.month0());
    if dt.date_naive() == now.date_naive() {
        dt.format("%H:%M").to_string()
    } else if dt.year() == now.year() {
        match month {
            Some(m) => format!("{} {m}", dt.format("%d")),
            None => dt.format("%b %d").to_string(),
        }
    } else {
        match month {
            Some(m) => format!("{} {m} {}", dt.format("%d"), dt.year()),
            None => dt.format("%b %d %Y").to_string(),
        }
    }
}

pub fn file_icon(name: &str, is_dir: bool, is_link: bool) -> &'static str {
    if is_dir {
        return if is_link { "folder-symlink" } else { "folder" };
    }
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "bmp" => "image",
        "mp4" | "mov" | "mkv" | "avi" | "webm" => "film",
        "zip" | "gz" | "tgz" | "tar" | "7z" | "rar" | "bz2" | "xz" => "archive",
        "md" | "txt" | "log" | "csv" | "pdf" | "doc" | "docx" => "file-text",
        "rs" | "js" | "ts" | "tsx" | "jsx" | "py" | "go" | "java" | "c" | "h" | "cpp" | "css" | "scss" | "html" | "htm" | "json"
        | "xml" | "yml" | "yaml" | "toml" | "php" | "rb" | "sh" | "sql" | "vue" => "file-code",
        "env" | "pem" | "key" | "crt" => "key-round",
        _ => "file",
    }
}
