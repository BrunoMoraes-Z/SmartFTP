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
        CF_HDROP, DROPEFFECT, IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
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
            let wrapper: IDropTarget = DropFix { inner: inner.clone(), virtual_data: RefCell::new(None) }.into();
            if let Err(e) = RegisterDragDrop(hwnd, &wrapper) {
                tracing::warn!("drop fix not installed: {e}");
                let _ = RegisterDragDrop(hwnd, &inner);
            }
        }
    }

    fn fmt(cf: u16, tymed: i32, lindex: i32) -> FORMATETC {
        FORMATETC { cfFormat: cf, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex, tymed: tymed as u32 }
    }

    fn cf_group() -> u16 {
        unsafe { RegisterClipboardFormatW(w!("FileGroupDescriptorW")) as u16 }
    }

    fn cf_contents() -> u16 {
        unsafe { RegisterClipboardFormatW(w!("FileContents")) as u16 }
    }

    fn has(obj: &IDataObject, cf: u16) -> bool {
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
            let pass = self.for_gpui(obj);
            unsafe { self.inner.DragEnter(&pass, keys, *pt, effect) }
        }

        fn DragOver(&self, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
            unsafe { self.inner.DragOver(keys, *pt, effect) }
        }

        fn DragLeave(&self) -> Result<()> {
            self.virtual_data.borrow_mut().take();
            unsafe { self.inner.DragLeave() }
        }

        fn Drop(&self, pdataobj: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
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
}
