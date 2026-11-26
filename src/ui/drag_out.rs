//! Explorer and the local pane. Rows dragged out of the window into Explorer or onto the
//! desktop (Windows; see `crate::dragdrop`): local rows are moved there, like Ctrl+X then
//! Ctrl+V, and remote rows are downloaded into the folder they were dropped on. Files dragged
//! from Explorer onto the local pane are moved into its folder the same way.

use std::path::{Path, PathBuf};

use gpui::{Context, DragMoveEvent, Window, px};

use super::workspace::{DraggedFiles, Workspace};
use crate::i18n::trf;

impl Workspace {
    /// Files from Explorer dropped on the local pane (or one of its folders): move them there,
    /// or copy them when `copy` (Ctrl held).
    pub fn drop_local(&mut self, paths: Vec<String>, into: Option<String>, copy: bool, cx: &mut Context<Self>) {
        let dest = PathBuf::from(into.unwrap_or_else(|| self.local.path.clone()));
        let paths: Vec<PathBuf> = paths
            .into_iter()
            .map(PathBuf::from)
            // already there, or a folder dropped into itself
            .filter(|p| p.parent() != Some(dest.as_path()) && !dest.starts_with(p))
            .collect();
        if paths.is_empty() {
            return;
        }
        let job = self.core.rt.spawn_blocking(move || crate::dragdrop::move_into(&paths, Path::new(&dest), copy));
        self.run(cx, async move { job.await? }, move |this, r, cx| {
            if let Err(e) = r {
                let msg = if copy { "Copy failed: {error}" } else { "Move failed: {error}" };
                this.toast(trf(msg, &[("error", &format!("{e:#}"))]), true);
            }
            this.load_local(cx);
        });
    }

    /// Called on every mouse move of a row drag: once the cursor leaves the window, hand the
    /// drag over to Windows.
    pub fn drag_move(&mut self, ev: &DragMoveEvent<DraggedFiles>, window: &mut Window, cx: &mut Context<Self>) {
        let pos = ev.event.position;
        let size = window.viewport_size();
        let inside = pos.x >= px(0.) && pos.y >= px(0.) && pos.x < size.width && pos.y < size.height;
        if inside || self.dragging_out {
            return;
        }
        let Some(hwnd) = crate::dragdrop::window_handle(window) else { return };
        let d = ev.drag(cx).clone();
        self.dragging_out = true;
        cx.stop_active_drag(window);
        self.start_drag_out(hwnd, d, cx);
    }

    #[cfg(not(windows))]
    fn start_drag_out(&mut self, _hwnd: isize, _d: DraggedFiles, _cx: &mut Context<Self>) {
        self.dragging_out = false;
    }

    /// Runs the OS drag outside of any GPUI update: its modal loop dispatches window messages
    /// that GPUI handles re-entrantly.
    #[cfg(windows)]
    fn start_drag_out(&mut self, hwnd: isize, d: DraggedFiles, cx: &mut Context<Self>) {
        use std::time::Duration;

        use futures::future::{Either, select};

        use super::workspace::Side;
        use crate::i18n::tr;

        cx.spawn(async move |this, cx| match d.side {
            Side::Local => {
                let paths: Vec<PathBuf> = d.paths.iter().map(PathBuf::from).collect();
                let result = crate::dragdrop::drag_out_local(hwnd, &paths);
                this.update(cx, |this, cx| {
                    this.dragging_out = false;
                    if let Err(e) = result {
                        this.toast(trf("Could not drag the files out: {error}", &[("error", &format!("{e:#}"))]), true);
                    }
                    this.load_local(cx);
                    cx.notify();
                })
                .ok();
                // Explorer moves the files in the background: look again once it is done
                for wait in [800, 2500] {
                    cx.background_executor().timer(Duration::from_millis(wait)).await;
                    this.update(cx, |this, cx| this.load_local(cx)).ok();
                }
            }
            Side::Remote => {
                let result = crate::dragdrop::drag_out_remote(hwnd);
                this.update(cx, |this, cx| {
                    this.dragging_out = false;
                    cx.notify();
                })
                .ok();
                let mut pending = match result {
                    Ok(Some(p)) => p,
                    Ok(None) => return,
                    Err(e) => {
                        this.update(cx, |this, cx| {
                            this.toast(trf("Could not drag the files out: {error}", &[("error", &format!("{e:#}"))]), true);
                            cx.notify();
                        })
                        .ok();
                        return;
                    }
                };
                let timeout = Box::pin(cx.background_executor().timer(Duration::from_secs(15)));
                let landed = match select(&mut pending.dir, timeout).await {
                    Either::Left((Ok(dir), _)) => Some(dir),
                    _ => None,
                };
                this.update(cx, |this, cx| {
                    match landed {
                        Some(dir) => {
                            let dir = dir.to_string_lossy().into_owned();
                            this.toast(trf("Downloading to {dir}", &[("dir", &dir)]), false);
                            this.transfer(Side::Remote, d.paths, Some(dir), cx);
                        }
                        None => this.toast(tr("Drop the files on a folder to download them there"), true),
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
}
