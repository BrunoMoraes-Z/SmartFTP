//! Files dropped from other apps, done right on Windows.
//!
//! GPUI reads the dropped paths when a drag *enters* the window. Archive managers (WinRAR,
//! 7-Zip) only extract the files when the drag is *dropped*, so at enter time their paths are
//! placeholders ("Folder\file" or an empty temp folder). Explorer's zip folders, mail
//! attachments and the like have no path at all: they offer "virtual files" (a
//! FileGroupDescriptor plus FileContents streams).
//!
//! We wrap GPUI's drop target: on drop we read the paths again (by then archive managers have
//! extracted), or save virtual files to a temporary folder, and hand those paths to the UI
//! through [`dropped_paths`]. Virtual files also get placeholder paths at enter time, so GPUI
//! highlights the drop targets as usual.

use std::path::{Path, PathBuf};

#[cfg(windows)]
thread_local! {
    /// Real paths of the drop being delivered, set just around GPUI's own Drop handler.
    static DROPPED: std::cell::RefCell<Option<Vec<PathBuf>>> = const { std::cell::RefCell::new(None) };
}

/// The paths to use for an `ExternalPaths` drop (call from the `on_drop` handler).
pub fn dropped_paths(gpui_paths: &[PathBuf]) -> Vec<String> {
    #[cfg(windows)]
    if let Some(real) = DROPPED.with(|d| d.borrow().clone())
        && !real.is_empty()
    {
        return real.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    }
    gpui_paths.iter().map(|p| p.to_string_lossy().into_owned()).collect()
}

/// Where virtual files are saved before upload.
pub fn drop_dir() -> PathBuf {
    std::env::temp_dir().join("SmartFTP-drops")
}

/// Remove dropped-file folders older than a day (uploads of older drops are long finished).
pub fn clean_old_drops() {
    let Ok(entries) = std::fs::read_dir(drop_dir()) else { return };
    for e in entries.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() > 86_400);
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Move (or copy) dropped files into the folder `dest`. On Windows the shell does it, so the
/// user gets its progress, conflict prompts and undo. Blocking: call off the UI thread.
/// Returns false when the user cancelled part of it.
#[cfg(windows)]
pub fn move_into(paths: &[PathBuf], dest: &Path, copy: bool) -> anyhow::Result<bool> {
    use windows::Win32::UI::Shell::{FO_COPY, FO_MOVE, FOF_ALLOWUNDO, SHFILEOPSTRUCTW, SHFileOperationW};
    use windows::core::PCWSTR;

    // double-NUL terminated lists of NUL terminated paths
    let list = |paths: &[&Path]| -> Vec<u16> {
        let mut out = vec![];
        for p in paths {
            out.extend(p.as_os_str().to_string_lossy().encode_utf16());
            out.push(0);
        }
        out.push(0);
        out
    };
    let from = list(&paths.iter().map(PathBuf::as_path).collect::<Vec<_>>());
    let to = list(&[dest]);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: if copy { FO_COPY } else { FO_MOVE },
        pFrom: PCWSTR(from.as_ptr()),
        pTo: PCWSTR(to.as_ptr()),
        fFlags: FOF_ALLOWUNDO.0 as u16,
        ..Default::default()
    };
    // SAFETY: both lists outlive the call and are double-NUL terminated; the shell's copy
    // engine wants COM on this thread.
    let code = unsafe {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let code = SHFileOperationW(&mut op);
        if com {
            CoUninitialize();
        }
        code
    };
    // 1223 = ERROR_CANCELLED, 0x4C7 the user aborted
    if code != 0 && code != 1223 && code != 0x4C7 {
        anyhow::bail!("the file operation failed (code {code:#x})");
    }
    Ok(code == 0 && !op.fAnyOperationsAborted.as_bool())
}

#[cfg(not(windows))]
pub fn move_into(paths: &[PathBuf], dest: &Path, copy: bool) -> anyhow::Result<bool> {
    for p in paths {
        let Some(name) = p.file_name() else { continue };
        let target = dest.join(name);
        if copy {
            if p.is_dir() {
                anyhow::bail!("copying folders is not supported here");
            }
            std::fs::copy(p, &target)?;
        } else {
            std::fs::rename(p, &target)?;
        }
    }
    Ok(true)
}

/// Keep a name from a drag source inside `base`: no drive, root or "..".
fn safe_join(base: &Path, name: &str) -> Option<PathBuf> {
    let mut out = base.to_path_buf();
    let mut any = false;
    for part in name.split(['\\', '/']) {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return None;
        }
        let clean: String =
            part.chars().map(|c| if matches!(c, '<' | '>' | '"' | '|' | '?' | '*') || c.is_control() { '_' } else { c }).collect();
        out.push(clean);
        any = true;
    }
    any.then_some(out)
}

#[cfg(windows)]
pub use win::install;

#[cfg(not(windows))]
pub fn install(_window: &gpui::Window) {}

#[cfg(windows)]
pub use out::{drag_out_local, drag_out_remote, window_handle};

/// Dragging rows out of the window into Explorer is Windows only.
#[cfg(not(windows))]
pub fn window_handle(_window: &gpui::Window) -> Option<isize> {
    None
}

#[cfg(windows)]
mod win {
    use std::cell::RefCell;
    use std::mem::ManuallyDrop;
    use std::path::PathBuf;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{DV_E_FORMATETC, HGLOBAL, HWND, POINTL, S_OK};
    use windows::Win32::System::Com::{
        DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM, STGMEDIUM_0,
        TYMED_HGLOBAL, TYMED_ISTREAM,
    };
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::System::Ole::{
        CF_HDROP, DROPEFFECT, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
    };
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use windows::Win32::UI::Shell::{DROPFILES, DragQueryFileW, FILEDESCRIPTORW, FILEGROUPDESCRIPTORW, HDROP};
    use windows::Win32::UI::WindowsAndMessaging::GetPropW;
    use windows::core::{BOOL, HRESULT, Interface, Ref, Result, implement, w};

    use super::{DROPPED, drop_dir, safe_join};

    const FD_ATTRIBUTES: u32 = 0x4;
    const FD_FILESIZE: u32 = 0x40;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

    /// Put our wrapper in front of GPUI's drop target for this window.
    pub fn install(window: &gpui::Window) {
        let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
        let hwnd = HWND(h.hwnd.get() as *mut _);
        super::clean_old_drops();
        // SAFETY: OLE keeps the registered IDropTarget in this window property; we take our own
        // reference before revoking it, then register the wrapper that forwards to it.
        unsafe {
            let raw = GetPropW(hwnd, w!("OleDropTargetInterface"));
            if raw.0.is_null() {
                return;
            }
            let Some(inner) = IDropTarget::from_raw_borrowed(&raw.0).cloned() else { return };
            if RevokeDragDrop(hwnd).is_err() {
                return;
            }
            let wrapper: IDropTarget =
                DropFix { inner: inner.clone(), virtual_data: RefCell::new(None), refused: Default::default() }.into();
            if let Err(e) = RegisterDragDrop(hwnd, &wrapper) {
                tracing::warn!("drop fix not installed: {e}");
                let _ = RegisterDragDrop(hwnd, &inner);
            }
        }
    }

    pub(super) fn fmt(cf: u16, tymed: i32, lindex: i32) -> FORMATETC {
        FORMATETC { cfFormat: cf, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex, tymed: tymed as u32 }
    }

    fn cf_group() -> u16 {
        unsafe { RegisterClipboardFormatW(w!("FileGroupDescriptorW")) as u16 }
    }

    fn cf_contents() -> u16 {
        unsafe { RegisterClipboardFormatW(w!("FileContents")) as u16 }
    }

    pub(super) fn has(obj: &IDataObject, cf: u16) -> bool {
        unsafe { obj.QueryGetData(&fmt(cf, TYMED_HGLOBAL.0, -1)) == S_OK }
    }

    /// Paths in the CF_HDROP of `obj`, read now.
    fn read_hdrop(obj: &IDataObject) -> Vec<PathBuf> {
        let mut out = vec![];
        unsafe {
            let Ok(mut medium) = obj.GetData(&fmt(CF_HDROP.0, TYMED_HGLOBAL.0, -1)) else { return out };
            let hdrop = HDROP(medium.u.hGlobal.0);
            let count = DragQueryFileW(hdrop, u32::MAX, None);
            for i in 0..count {
                let len = DragQueryFileW(hdrop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                DragQueryFileW(hdrop, i, Some(&mut buf));
                out.push(PathBuf::from(String::from_utf16_lossy(&buf[..len])));
            }
            ReleaseStgMedium(&mut medium);
        }
        out
    }

    /// The virtual files described by a FileGroupDescriptorW: (relative name, is folder, size).
    fn virtual_files(obj: &IDataObject) -> Option<Vec<(String, bool, Option<u64>)>> {
        unsafe {
            let mut medium = obj.GetData(&fmt(cf_group(), TYMED_HGLOBAL.0, -1)).ok()?;
            let ptr = GlobalLock(medium.u.hGlobal) as *const FILEGROUPDESCRIPTORW;
            let mut out = vec![];
            if !ptr.is_null() {
                let count = (*ptr).cItems as usize;
                let first = std::ptr::addr_of!((*ptr).fgd) as *const FILEDESCRIPTORW;
                for i in 0..count {
                    // the struct is packed: copy it out before touching its fields
                    let d: FILEDESCRIPTORW = std::ptr::read_unaligned(first.add(i));
                    let file_name = d.cFileName;
                    let (flags, attrs, high, low) = (d.dwFlags, d.dwFileAttributes, d.nFileSizeHigh, d.nFileSizeLow);
                    let len = file_name.iter().position(|c| *c == 0).unwrap_or(file_name.len());
                    let name = String::from_utf16_lossy(&file_name[..len]);
                    let dir = flags & FD_ATTRIBUTES != 0 && attrs & FILE_ATTRIBUTE_DIRECTORY != 0;
                    let size = (flags & FD_FILESIZE != 0).then_some(((high as u64) << 32) | low as u64);
                    out.push((name, dir, size));
                }
                let _ = GlobalUnlock(medium.u.hGlobal);
            }
            ReleaseStgMedium(&mut medium);
            (!out.is_empty()).then_some(out)
        }
    }

    /// Top-level entries of a virtual drop, placed in `base`.
    fn top_level(base: &std::path::Path, files: &[(String, bool, Option<u64>)]) -> Vec<PathBuf> {
        let mut tops: Vec<PathBuf> = vec![];
        for (name, _, _) in files {
            let first = name.split(['\\', '/']).find(|p| !p.trim().is_empty()).unwrap_or_default();
            if let Some(p) = safe_join(base, first)
                && !tops.contains(&p)
            {
                tops.push(p);
            }
        }
        tops
    }

    /// Save the virtual files of `obj` into a new temporary folder; returns the top-level paths.
    fn save_virtual(obj: &IDataObject, files: &[(String, bool, Option<u64>)]) -> Vec<PathBuf> {
        let base = drop_dir().join(uuid::Uuid::new_v4().simple().to_string());
        for (index, (name, dir, size)) in files.iter().enumerate() {
            let Some(path) = safe_join(&base, name) else { continue };
            if *dir {
                let _ = std::fs::create_dir_all(&path);
                continue;
            }
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = save_one(obj, index as i32, *size, &path) {
                tracing::warn!("saving dropped file {name}: {e:#}");
            }
        }
        top_level(&base, files).into_iter().filter(|p| p.exists()).collect()
    }

    fn save_one(obj: &IDataObject, index: i32, size: Option<u64>, path: &std::path::Path) -> anyhow::Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(path)?;
        unsafe {
            let mut medium = obj.GetData(&fmt(cf_contents(), TYMED_ISTREAM.0 | TYMED_HGLOBAL.0, index))?;
            let result = (|| -> anyhow::Result<()> {
                if medium.tymed == TYMED_ISTREAM.0 as u32 {
                    let Some(stream) = medium.u.pstm.as_ref() else { anyhow::bail!("no stream") };
                    let mut buf = vec![0u8; 256 * 1024];
                    loop {
                        let mut read = 0u32;
                        let hr = stream.Read(buf.as_mut_ptr().cast(), buf.len() as u32, Some(&mut read));
                        if hr.is_err() {
                            anyhow::bail!("reading the dropped file: {hr:?}");
                        }
                        if read == 0 {
                            break;
                        }
                        file.write_all(&buf[..read as usize])?;
                    }
                } else if medium.tymed == TYMED_HGLOBAL.0 as u32 {
                    let h = medium.u.hGlobal;
                    let len = size.map(|s| s as usize).unwrap_or(GlobalSize(h)).min(GlobalSize(h));
                    let p = GlobalLock(h) as *const u8;
                    if !p.is_null() {
                        file.write_all(std::slice::from_raw_parts(p, len))?;
                        let _ = GlobalUnlock(h);
                    }
                } else {
                    anyhow::bail!("unsupported storage {}", medium.tymed);
                }
                Ok(())
            })();
            ReleaseStgMedium(&mut medium);
            result
        }
    }

    /// A DROPFILES block (CF_HDROP) for `paths`.
    fn make_hdrop(paths: &[PathBuf]) -> Result<HGLOBAL> {
        let mut wide: Vec<u16> = vec![];
        for p in paths {
            wide.extend(p.to_string_lossy().encode_utf16());
            wide.push(0);
        }
        wide.push(0);
        let header = std::mem::size_of::<DROPFILES>();
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, header + wide.len() * 2)?;
            let p = GlobalLock(h) as *mut u8;
            let df = p as *mut DROPFILES;
            (*df).pFiles = header as u32;
            (*df).fWide = BOOL(1);
            std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, p.add(header), wide.len() * 2);
            let _ = GlobalUnlock(h);
            Ok(h)
        }
    }

    #[implement(IDropTarget)]
    struct DropFix {
        inner: IDropTarget,
        /// For a virtual-file drag: the wrapper handed to GPUI (it reports placeholder paths).
        virtual_data: RefCell<Option<IDataObject>>,
        /// Our own remote-files drag came back over the window: it only carries a marker file.
        refused: std::cell::Cell<bool>,
    }

    impl DropFix {
        /// The data object to give GPUI: as is when it has real paths, wrapped when it only has
        /// virtual files.
        fn for_gpui(&self, obj: &IDataObject) -> IDataObject {
            if has(obj, CF_HDROP.0) {
                return obj.clone();
            }
            match virtual_files(obj) {
                Some(files) => {
                    let names = top_level(&drop_dir().join("pending"), &files);
                    let wrapped: IDataObject = VirtualAsPaths { inner: obj.clone(), names }.into();
                    *self.virtual_data.borrow_mut() = Some(wrapped.clone());
                    wrapped
                }
                None => obj.clone(),
            }
        }
    }

    impl IDropTarget_Impl for DropFix_Impl {
        fn DragEnter(&self, pdataobj: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
            let obj = pdataobj.ok()?;
            let refused = has(obj, super::out::cf_remote_drag());
            self.refused.set(refused);
            if refused {
                unsafe { *effect = DROPEFFECT_NONE };
                return Ok(());
            }
            let pass = self.for_gpui(obj);
            unsafe { self.inner.DragEnter(&pass, keys, *pt, effect) }
        }

        fn DragOver(&self, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
            if self.refused.get() {
                unsafe { *effect = DROPEFFECT_NONE };
                return Ok(());
            }
            unsafe { self.inner.DragOver(keys, *pt, effect) }
        }

        fn DragLeave(&self) -> Result<()> {
            if self.refused.replace(false) {
                return Ok(());
            }
            self.virtual_data.borrow_mut().take();
            unsafe { self.inner.DragLeave() }
        }

        fn Drop(&self, pdataobj: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
            if self.refused.replace(false) {
                unsafe { *effect = DROPEFFECT_NONE };
                return Ok(());
            }
            let obj = pdataobj.ok()?;
            // by now archive managers have extracted their files: read the paths again
            let real = if has(obj, CF_HDROP.0) {
                read_hdrop(obj)
            } else {
                virtual_files(obj).map(|files| save_virtual(obj, &files)).unwrap_or_default()
            };
            let pass = self.virtual_data.borrow_mut().take().unwrap_or_else(|| obj.clone());
            DROPPED.with(|d| *d.borrow_mut() = Some(real));
            let result = unsafe { self.inner.Drop(&pass, keys, *pt, effect) };
            DROPPED.with(|d| d.borrow_mut().take());
            result
        }
    }

    /// Forwards everything to the source, but also answers CF_HDROP with placeholder paths so
    /// GPUI treats a virtual-file drag like any file drag.
    #[implement(IDataObject)]
    struct VirtualAsPaths {
        inner: IDataObject,
        names: Vec<PathBuf>,
    }

    impl IDataObject_Impl for VirtualAsPaths_Impl {
        fn GetData(&self, format: *const FORMATETC) -> Result<STGMEDIUM> {
            unsafe {
                if !format.is_null() && (*format).cfFormat == CF_HDROP.0 {
                    let h = make_hdrop(&self.names)?;
                    return Ok(STGMEDIUM {
                        tymed: TYMED_HGLOBAL.0 as u32,
                        u: STGMEDIUM_0 { hGlobal: h },
                        pUnkForRelease: ManuallyDrop::new(None),
                    });
                }
                self.inner.GetData(format)
            }
        }

        fn GetDataHere(&self, format: *const FORMATETC, medium: *mut STGMEDIUM) -> Result<()> {
            unsafe { self.inner.GetDataHere(format, medium) }
        }

        fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
            unsafe {
                if format.is_null() {
                    return DV_E_FORMATETC;
                }
                if (*format).cfFormat == CF_HDROP.0 {
                    return S_OK;
                }
                self.inner.QueryGetData(format)
            }
        }

        fn GetCanonicalFormatEtc(&self, format_in: *const FORMATETC, format_out: *mut FORMATETC) -> HRESULT {
            unsafe { self.inner.GetCanonicalFormatEtc(format_in, format_out) }
        }

        fn SetData(&self, format: *const FORMATETC, medium: *const STGMEDIUM, release: BOOL) -> Result<()> {
            unsafe { self.inner.SetData(format, medium, release.as_bool()) }
        }

        fn EnumFormatEtc(&self, direction: u32) -> Result<IEnumFORMATETC> {
            unsafe { self.inner.EnumFormatEtc(direction) }
        }

        fn DAdvise(&self, format: *const FORMATETC, advf: u32, sink: Ref<IAdviseSink>) -> Result<u32> {
            unsafe { self.inner.DAdvise(format, advf, sink.as_ref()) }
        }

        fn DUnadvise(&self, connection: u32) -> Result<()> {
            unsafe { self.inner.DUnadvise(connection) }
        }

        fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
            unsafe { self.inner.EnumDAdvise() }
        }
    }
}

/// Rows dragged out of the window and dropped in Explorer (or on the desktop).
///
/// Local rows are dragged as the real files with "move" preferred, like Ctrl+X then Ctrl+V;
/// Explorer does the move itself. Remote rows have no local file yet, and Explorer never tells
/// the source where a drop landed. So we drag a hidden, empty marker file, watch the shell's
/// change notifications for the copy Explorer makes of it, and download into that folder.
#[cfg(windows)]
mod out {
    use std::mem::ManuallyDrop;
    use std::os::windows::fs::OpenOptionsExt;
    use std::path::PathBuf;
    use std::time::Duration;

    use anyhow::{Context as _, anyhow};
    use futures::channel::oneshot;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Com::{IDataObject, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_MOVE, DROPEFFECT_NONE};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        ILCreateFromPathW, ILFindLastID, ILFree, SHCNE_CREATE, SHCNE_RENAMEITEM, SHCNE_UPDATEDIR, SHCNRF_NewDelivery, SHCNRF_ShellLevel,
        SHChangeNotification_Lock, SHChangeNotification_Unlock, SHChangeNotifyDeregister, SHChangeNotifyEntry, SHChangeNotifyRegister,
        SHCreateDataObject, SHDoDragDrop, SHGetPathFromIDListW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_WNDPROC, GetMessageW, HWND_MESSAGE, MSG, PostMessageW,
        PostQuitMessage, PostThreadMessageW, SetWindowLongPtrW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_LBUTTONUP, WM_QUIT,
    };
    use windows::core::{BOOL, HSTRING, w};

    use super::drop_dir;
    use super::win::fmt;

    const WM_SHELL_CHANGE: u32 = WM_APP + 0x51;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

    /// The native handle of `window`, to start a drag from.
    pub fn window_handle(window: &gpui::Window) -> Option<isize> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
            _ => None,
        }
    }

    /// Marks our remote-files drag, so our own window refuses it.
    pub fn cf_remote_drag() -> u16 {
        unsafe { RegisterClipboardFormatW(w!("SmartFTP.RemoteDrag")) as u16 }
    }

    fn cf_preferred_effect() -> u16 {
        unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) as u16 }
    }

    /// Drag local files out of the window; returns once they are dropped (or the drag ends).
    /// Explorer may still be moving them in the background.
    pub fn drag_out_local(hwnd: isize, paths: &[PathBuf]) -> anyhow::Result<()> {
        let hwnd = HWND(hwnd as *mut _);
        let result = (|| {
            let obj = shell_data(paths)?;
            set_dword(&obj, cf_preferred_effect(), DROPEFFECT_MOVE.0)?;
            unsafe { SHDoDragDrop(Some(hwnd), &obj, None, DROPEFFECT_COPY | DROPEFFECT_MOVE) }.context("drag and drop")
        })();
        release_mouse(hwnd);
        result.map(|_| ())
    }

    /// A remote-files drag that was dropped somewhere: `dir` gets the folder it landed in.
    pub struct RemoteDrop {
        pub dir: oneshot::Receiver<PathBuf>,
        watcher: u32,
        source: PathBuf,
    }

    impl Drop for RemoteDrop {
        fn drop(&mut self) {
            unsafe {
                let _ = PostThreadMessageW(self.watcher, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            let _ = std::fs::remove_dir_all(&self.source);
        }
    }

    /// Drag remote files out of the window as a marker file. `None` when the drag was cancelled.
    pub fn drag_out_remote(hwnd: isize) -> anyhow::Result<Option<RemoteDrop>> {
        let hwnd = HWND(hwnd as *mut _);
        let id = uuid::Uuid::new_v4().simple().to_string();
        let source = drop_dir().join(format!("out-{id}"));
        let name = format!("SmartFTP-{}.download", &id[..12]);
        let marker = source.join(&name);
        let made = std::fs::create_dir_all(&source)
            .and_then(|_| std::fs::OpenOptions::new().write(true).create_new(true).attributes(FILE_ATTRIBUTE_HIDDEN).open(&marker));
        let watched = made.map_err(anyhow::Error::from).and_then(|_| watch_for(name));
        let (watcher, dir) = match watched {
            Ok(w) => w,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&source);
                release_mouse(hwnd);
                return Err(e);
            }
        };
        let pending = RemoteDrop { dir, watcher, source };
        let result = (|| {
            let obj = shell_data(std::slice::from_ref(&marker))?;
            set_dword(&obj, cf_remote_drag(), 1)?;
            unsafe { SHDoDragDrop(Some(hwnd), &obj, None, DROPEFFECT_COPY) }.context("drag and drop")
        })();
        release_mouse(hwnd);
        Ok((result? != DROPEFFECT_NONE).then_some(pending))
    }

    /// OLE ate the button release: send GPUI one (off-window) so it forgets the pressed row.
    fn release_mouse(hwnd: HWND) {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_LBUTTONUP, WPARAM(0), LPARAM(0xFFFF_FFFF));
        }
    }

    /// A shell data object for `paths` (all in one folder, as the local pane lists them).
    fn shell_data(paths: &[PathBuf]) -> anyhow::Result<IDataObject> {
        let parent = paths.first().and_then(|p| p.parent()).ok_or_else(|| anyhow!("nothing to drag"))?;
        unsafe {
            let folder = ILCreateFromPathW(&HSTRING::from(parent.as_os_str()));
            if folder.is_null() {
                anyhow::bail!("{} not found", parent.display());
            }
            let full: Vec<*mut ITEMIDLIST> = paths.iter().map(|p| ILCreateFromPathW(&HSTRING::from(p.as_os_str()))).collect();
            let result = if full.iter().any(|p| p.is_null()) || paths.iter().any(|p| p.parent() != Some(parent)) {
                Err(anyhow!("the dragged files were not found"))
            } else {
                let children: Vec<*const ITEMIDLIST> = full.iter().map(|p| ILFindLastID(*p) as *const _).collect();
                SHCreateDataObject(Some(folder), Some(&children), None::<&IDataObject>).map_err(anyhow::Error::from)
            };
            for p in full {
                ILFree(Some(p));
            }
            ILFree(Some(folder));
            result
        }
    }

    fn set_dword(obj: &IDataObject, cf: u16, value: u32) -> anyhow::Result<()> {
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, 4)?;
            *(GlobalLock(h) as *mut u32) = value;
            let _ = GlobalUnlock(h);
            let medium =
                STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: h }, pUnkForRelease: ManuallyDrop::new(None) };
            obj.SetData(&fmt(cf, TYMED_HGLOBAL.0, -1), &medium, true)?;
        }
        Ok(())
    }

    /// Start a thread that watches the whole shell namespace for a file called `name` and
    /// sends the folder it appears in. Returns the thread id (post WM_QUIT to stop it).
    fn watch_for(name: String) -> anyhow::Result<(u32, oneshot::Receiver<PathBuf>)> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (found_tx, found_rx) = oneshot::channel();
        std::thread::Builder::new().name("drop-watch".into()).spawn(move || watch_thread(&name, ready_tx, found_tx))?;
        let tid = ready_rx.recv_timeout(Duration::from_secs(5)).map_err(|_| anyhow!("drop watcher did not start"))??;
        Ok((tid, found_rx))
    }

    thread_local! {
        /// The marker the watcher thread looks for, and the folder it was found in.
        static WATCH: std::cell::RefCell<(String, Option<PathBuf>)> = const { std::cell::RefCell::new((String::new(), None)) };
    }

    /// The shell sends change notifications straight to the window procedure (not through the
    /// message queue), so the watcher needs one of its own.
    unsafe extern "system" fn watch_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            if msg != WM_SHELL_CHANGE {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let changes = changed_paths(wparam, lparam);
            WATCH.with(|w| {
                let mut w = w.borrow_mut();
                if w.1.is_none()
                    && let Some(dir) = changes.into_iter().find_map(|(event, path)| landed_in(&w.0, event, path))
                {
                    w.1 = Some(dir);
                    PostQuitMessage(0);
                }
            });
            LRESULT(0)
        }
    }

    fn watch_thread(name: &str, ready: std::sync::mpsc::Sender<anyhow::Result<u32>>, found: oneshot::Sender<PathBuf>) {
        WATCH.with(|w| *w.borrow_mut() = (name.to_string(), None));
        unsafe {
            let hwnd = match CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("SmartFTP drop watcher"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            ) {
                Ok(h) => h,
                Err(e) => {
                    let _ = ready.send(Err(e.into()));
                    return;
                }
            };
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, watch_proc as *const () as isize);
            // the empty ID list is the desktop, the root of the whole namespace
            let root = [0u16; 2];
            let entry = SHChangeNotifyEntry { pidl: root.as_ptr() as *mut ITEMIDLIST, fRecursive: BOOL(1) };
            let reg = SHChangeNotifyRegister(
                hwnd,
                SHCNRF_ShellLevel | SHCNRF_NewDelivery,
                (SHCNE_CREATE.0 | SHCNE_RENAMEITEM.0 | SHCNE_UPDATEDIR.0) as i32,
                WM_SHELL_CHANGE,
                1,
                &entry,
            );
            if reg == 0 {
                let _ = DestroyWindow(hwnd);
                let _ = ready.send(Err(anyhow!("could not watch for the drop")));
                return;
            }
            let _ = ready.send(Ok(GetCurrentThreadId()));
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
            let _ = SHChangeNotifyDeregister(reg);
            let _ = DestroyWindow(hwnd);
        }
        if let Some(dir) = WATCH.with(|w| w.borrow_mut().1.take()) {
            // the marker was only there to find the folder
            let copy = dir.join(name);
            for _ in 0..30 {
                if std::fs::remove_file(&copy).is_ok() || !copy.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = found.send(dir);
        }
    }

    /// The folder a change shows the marker `name` was copied into (never our own temp folder).
    fn landed_in(name: &str, event: u32, path: PathBuf) -> Option<PathBuf> {
        let dir = if event & SHCNE_UPDATEDIR.0 != 0 {
            path.join(name).is_file().then_some(path)?
        } else {
            if path.file_name().is_none_or(|n| n != name) {
                return None;
            }
            path.parent()?.to_path_buf()
        };
        (!dir.starts_with(drop_dir()) && dir.is_dir()).then_some(dir)
    }

    /// The (event, path) pairs of one shell change notification.
    unsafe fn changed_paths(wparam: WPARAM, lparam: LPARAM) -> Vec<(u32, PathBuf)> {
        let mut out = vec![];
        unsafe {
            let mut pidls: *mut *mut ITEMIDLIST = std::ptr::null_mut();
            let mut event = 0i32;
            let lock = SHChangeNotification_Lock(HANDLE(wparam.0 as *mut _), lparam.0 as u32, Some(&mut pidls), Some(&mut event));
            if lock.is_invalid() || pidls.is_null() {
                return out;
            }
            for i in 0..2 {
                let pidl = *pidls.add(i);
                if pidl.is_null() {
                    continue;
                }
                let mut buf = [0u16; 260];
                if SHGetPathFromIDListW(pidl, &mut buf).as_bool() {
                    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
                    out.push((event as u32, PathBuf::from(String::from_utf16_lossy(&buf[..len]))));
                }
            }
            let _ = SHChangeNotification_Unlock(lock);
        }
        out
    }

    /// Copies a marker the way Explorer does and checks that the watcher finds the folder.
    /// Ignored: it drives the real shell (run with `cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn watcher_finds_where_the_shell_copied_the_marker() {
        let name = format!("SmartFTP-test-{}.download", uuid::Uuid::new_v4().simple());
        let src_dir = std::env::temp_dir().join(format!("smartftp-src-{}", uuid::Uuid::new_v4().simple()));
        let dst_dir = std::env::temp_dir().join(format!("smartftp-dst-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&dst_dir).unwrap();
        std::fs::write(src_dir.join(&name), b"").unwrap();
        let (tid, mut rx) = watch_for(name.clone()).unwrap();
        let script = format!(
            "(New-Object -ComObject Shell.Application).Namespace('{}').CopyHere('{}', 16); Start-Sleep -Seconds 2",
            dst_dir.display(),
            src_dir.join(&name).display()
        );
        std::process::Command::new("powershell").args(["-NoProfile", "-Command", &script]).status().unwrap();
        let mut got = None;
        for _ in 0..100 {
            if let Ok(Some(dir)) = rx.try_recv() {
                got = Some(dir);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        unsafe {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        assert_eq!(got.as_deref().map(|p| p.canonicalize().unwrap()), Some(dst_dir.canonicalize().unwrap()));
        assert!(!dst_dir.join(&name).exists(), "the marker copy is removed");
        let _ = std::fs::remove_dir_all(&src_dir);
        let _ = std::fs::remove_dir_all(&dst_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_inside_the_drop_folder() {
        let base = Path::new("C:/drops/x");
        assert_eq!(safe_join(base, "a\\b.txt"), Some(base.join("a").join("b.txt")));
        assert_eq!(safe_join(base, "..\\evil"), None);
        assert_eq!(safe_join(base, "C:\\Windows\\x"), None);
        assert_eq!(safe_join(base, "\\\\"), None);
        assert_eq!(safe_join(base, "we?ird*.txt"), Some(base.join("we_ird_.txt")));
    }

    /// Drives the real shell copy engine (run with `cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn move_into_moves_and_copies() {
        let root = std::env::temp_dir().join(format!("smartftp-mv-{}", uuid::Uuid::new_v4().simple()));
        let (src, dst) = (root.join("src"), root.join("dst"));
        std::fs::create_dir_all(src.join("folder")).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("a.txt"), b"a").unwrap();
        std::fs::write(src.join("folder").join("b.txt"), b"b").unwrap();
        assert!(move_into(&[src.join("a.txt"), src.join("folder")], &dst, false).unwrap());
        assert!(dst.join("a.txt").is_file() && dst.join("folder").join("b.txt").is_file());
        assert!(!src.join("a.txt").exists() && !src.join("folder").exists());
        assert!(move_into(&[dst.join("a.txt")], &src, true).unwrap());
        assert!(src.join("a.txt").is_file() && dst.join("a.txt").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }
}
