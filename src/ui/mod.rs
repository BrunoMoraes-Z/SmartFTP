pub mod assets;
pub mod components;
pub mod dialogs;
pub mod input;
pub mod panes;
pub mod rail;
pub mod settings;
pub mod settings_pages;
pub mod theme;
pub mod titlebar;
pub mod updates;
pub mod workspace;

use std::sync::Arc;

use gpui::{App, AppContext, Application, BorrowAppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};

use crate::core::Core;

#[cfg(target_os = "windows")]
pub const UI_FONT: &str = "Segoe UI";
#[cfg(target_os = "windows")]
pub const MONO_FONT: &str = "Consolas";
#[cfg(target_os = "macos")]
pub const UI_FONT: &str = ".SystemUIFont";
#[cfg(target_os = "macos")]
pub const MONO_FONT: &str = "Menlo";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const UI_FONT: &str = "Noto Sans";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const MONO_FONT: &str = "DejaVu Sans Mono";

pub fn run(core: Arc<Core>) {
    Application::new().with_assets(assets::Assets).run(move |cx: &mut App| {
        input::bind_keys(cx);
        workspace::bind_keys(cx);
        let cfg = core.config.lock().clone();
        crate::i18n::set_lang(crate::i18n::resolve(&cfg.interface.language));
        cx.set_global(theme::Themes::load(&cfg, theme::Appearance::Dark));
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions { title: Some("SmartFTP".into()), appears_transparent: true, ..Default::default() }),
                    window_min_size: Some(size(px(960.), px(600.))),
                    ..Default::default()
                },
                |window, cx| {
                    let os = theme::appearance_of(window.appearance());
                    cx.update_global::<theme::Themes, _>(|th, _| {
                        th.os_appearance = os;
                        th.select(&cfg);
                    });
                    cx.new(|cx| workspace::Workspace::new(core.clone(), window, cx))
                },
            )
            .expect("failed to open window");
        window
            .update(cx, |view, window, cx| {
                window.focus(&gpui::Focusable::focus_handle(view, cx));
                cx.activate(true);
            })
            .ok();
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
