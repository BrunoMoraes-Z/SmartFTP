//! Custom, theme-aware title bar replacing the native one.
//! On Windows the caption buttons are tagged with `WindowControlArea`, so the OS keeps
//! handling drag, double-click to maximize, snap layouts and the min/max/close actions.

use std::sync::{Arc, OnceLock};

use gpui::{Context, FontWeight, Image, ImageFormat, Window, WindowControlArea, div, img, prelude::*, px, rgb};

use super::components::site_color;
use super::theme::theme;
use super::workspace::Workspace;

pub const TITLEBAR_HEIGHT: f32 = 34.;

pub fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!("../../assets/app-icon.png").to_vec())))
        .clone()
}

impl Workspace {
    pub fn render_titlebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx).clone();
        let session = self.session();
        let title = match &session {
            Some(s) => format!("{} — SmartFTP", s.site.name),
            None => "SmartFTP".to_string(),
        };
        window.set_window_title(&title);
        let active = window.is_window_active();
        let maximized = window.is_maximized();
        let fg = if active { t.ink } else { t.faint };

        let bar = div()
            .id("titlebar")
            .flex()
            .flex_none()
            .items_center()
            .w_full()
            .h(px(TITLEBAR_HEIGHT))
            .bg(t.surface)
            .border_b_1()
            .border_color(t.border)
            .window_control_area(WindowControlArea::Drag)
            .when(cfg!(target_os = "macos"), |d| d.pl(px(78.)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(12.))
                    .text_size(px(12.))
                    .when(!cfg!(target_os = "macos"), |d| d.child(img(app_icon()).size(px(16.))))
                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(fg).child("SmartFTP"))
                    .when_some(session, |d, s| {
                        let (_, col) = self.identity(&s.site);
                        d.child(div().text_color(t.faint).child("/"))
                            .child(div().size(px(7.)).rounded(px(4.)).bg(site_color(&col)))
                            .child(div().text_color(if active { t.muted } else { t.faint }).child(s.site.name.clone()))
                    }),
            )
            .child(div().flex_1().h_full());

        #[cfg(target_os = "macos")]
        {
            let _ = maximized;
            bar
        }
        #[cfg(not(target_os = "macos"))]
        {
            bar.child(caption_button("min", WindowControlArea::Min, Glyph::Min, &t, fg, cx))
                .child(caption_button("max", WindowControlArea::Max, if maximized { Glyph::Restore } else { Glyph::Max }, &t, fg, cx))
                .child(caption_button("close", WindowControlArea::Close, Glyph::Close, &t, fg, cx))
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[derive(Clone, Copy)]
enum Glyph {
    Min,
    Max,
    Restore,
    Close,
}

#[cfg(not(target_os = "macos"))]
fn caption_button(id: &'static str, area: WindowControlArea, glyph: Glyph, t: &super::theme::Theme, fg: gpui::Hsla, cx: &mut Context<Workspace>) -> impl IntoElement + use<> {
    let is_close = matches!(glyph, Glyph::Close);
    let hover_bg = if is_close { rgb(0xC42B1C).into() } else { t.hover };
    let _ = cx;
    let btn = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(46.))
        .h_full()
        .text_color(fg)
        // occlude: otherwise the title bar's Drag area (painted first) also matches here
        // and wins the hit test, turning button clicks into caption drags.
        .occlude()
        .window_control_area(area)
        .hover(move |s| if is_close { s.bg(hover_bg).text_color(gpui::white()) } else { s.bg(hover_bg) });

    // Windows draws its own caption glyphs with the Segoe Fluent Icons font.
    #[cfg(target_os = "windows")]
    {
        let ch = match glyph {
            Glyph::Min => "\u{E921}",
            Glyph::Max => "\u{E922}",
            Glyph::Restore => "\u{E923}",
            Glyph::Close => "\u{E8BB}",
        };
        btn.font_family("Segoe Fluent Icons").text_size(px(10.)).child(ch)
    }
    // Linux: no native handling of the areas, so wire the actions up ourselves.
    #[cfg(not(target_os = "windows"))]
    {
        let icon_name = match glyph {
            Glyph::Min => "minus",
            Glyph::Max => "square",
            Glyph::Restore => "copy",
            Glyph::Close => "x",
        };
        btn.child(super::components::icon(icon_name, 14., fg)).on_click(move |_, window, _| match glyph {
            Glyph::Min => window.minimize_window(),
            Glyph::Max | Glyph::Restore => window.zoom_window(),
            Glyph::Close => window.remove_window(),
        })
    }
}
