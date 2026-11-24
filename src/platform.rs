//! Small OS queries that have no crate in the dependency tree.

/// Whether this process runs elevated (as administrator) on Windows.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use std::ffi::c_void;
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(process: isize, access: u32, token: *mut isize) -> i32;
        fn GetTokenInformation(token: isize, class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn CloseHandle(handle: isize) -> i32;
    }
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_ELEVATION: u32 = 20;
    // SAFETY: plain Win32 calls on the current process token with correctly sized out-buffers.
    unsafe {
        let mut token = 0isize;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevated: u32 = 0;
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TOKEN_ELEVATION, (&mut elevated as *mut u32).cast(), 4, &mut len);
        CloseHandle(token);
        ok != 0 && elevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}
