use std::collections::HashSet;

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible};

    unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let set = &mut *(lparam.0 as *mut HashSet<u32>);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 {
            set.insert(pid);
        }
        BOOL(1)
    }

    pub fn visible_process_ids() -> HashSet<u32> {
        let mut out = HashSet::new();
        unsafe {
            let _ = EnumWindows(Some(enum_window), LPARAM((&mut out as *mut HashSet<u32>) as isize));
        }
        out
    }
}

#[cfg(windows)]
pub use imp::visible_process_ids;

#[cfg(not(windows))]
pub fn visible_process_ids() -> HashSet<u32> {
    HashSet::new()
}
