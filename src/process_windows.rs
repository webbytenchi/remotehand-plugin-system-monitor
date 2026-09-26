use std::collections::HashSet;

#[cfg(windows)]
mod imp {
    use super::*;
    use base64::Engine as _;
    use image::{ImageBuffer, ImageEncoder, Rgba};
    use std::ffi::c_void;
    use std::io::Cursor;
    use std::mem::{size_of, zeroed};
    use std::path::Path;
    use windows::core::{BOOL, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON};
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, DestroyIcon, DrawIconEx, EnumWindows,
        GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        SetForegroundWindow, ShowWindow, DI_NORMAL, SW_RESTORE,
    };

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

    pub fn icon_data_url(path: &str) -> Option<String> {
        if path.is_empty() || !Path::new(path).exists() { return None; }
        let wide:Vec<u16>=path.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let mut info:SHFILEINFOW=zeroed();
            let ok=SHGetFileInfoW(
                PCWSTR(wide.as_ptr()),
                Default::default(),
                Some(&mut info),
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON|SHGFI_SMALLICON,
            );
            if ok==0 || info.hIcon.is_invalid(){return None;}
            let hicon=info.hIcon;
            let dc=CreateCompatibleDC(None);
            if dc.is_invalid(){let _=DestroyIcon(hicon);return None;}
            let mut bmi:BITMAPINFO=zeroed();
            bmi.bmiHeader=BITMAPINFOHEADER{
                biSize:size_of::<BITMAPINFOHEADER>() as u32,
                biWidth:32,
                biHeight:-32,
                biPlanes:1,
                biBitCount:32,
                biCompression:BI_RGB.0,
                ..Default::default()
            };
            let mut bits:*mut c_void=std::ptr::null_mut();
            let bitmap=CreateDIBSection(Some(dc),&bmi,DIB_RGB_COLORS,&mut bits,None,0).ok()?;
            let old=SelectObject(dc,HGDIOBJ(bitmap.0));
            let drawn=DrawIconEx(dc,0,0,hicon,32,32,0,None,DI_NORMAL).is_ok();
            let mut rgba=vec![0u8;32*32*4];
            if drawn && !bits.is_null(){
                let bgra=std::slice::from_raw_parts(bits as *const u8,rgba.len());
                for (dst,src) in rgba.chunks_exact_mut(4).zip(bgra.chunks_exact(4)){
                    dst[0]=src[2];dst[1]=src[1];dst[2]=src[0];dst[3]=src[3];
                }
            }
            let _=SelectObject(dc,old);
            let _=DeleteObject(HGDIOBJ(bitmap.0));
            let _=DeleteDC(dc);
            let _=DestroyIcon(hicon);
            if !drawn{return None;}
            let image:ImageBuffer<Rgba<u8>,Vec<u8>>=ImageBuffer::from_raw(32,32,rgba)?;
            let mut png=Cursor::new(Vec::new());
            image::codecs::png::PngEncoder::new(&mut png)
                .write_image(image.as_raw(),32,32,image::ExtendedColorType::Rgba8)
                .ok()?;
            Some(format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(png.into_inner())))
        }
    }

    struct FocusTarget {
        pid: u32,
        preferred: Option<HWND>,
        fallback: Option<HWND>,
    }

    unsafe extern "system" fn enum_focus_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let target = &mut *(lparam.0 as *mut FocusTarget);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != target.pid {
            return BOOL(1);
        }
        if target.fallback.is_none() {
            target.fallback = Some(hwnd);
        }
        let unowned = GetWindow(hwnd, GW_OWNER).map(|owner| owner.0.is_null()).unwrap_or(true);
        let titled = GetWindowTextLengthW(hwnd) > 0;
        if unowned && titled {
            target.preferred = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    pub fn focus_process_window(pid: u32) -> Result<(), String> {
        if pid == 0 {
            return Err("process has no focusable window".to_string());
        }
        let mut target = FocusTarget { pid, preferred: None, fallback: None };
        unsafe {
            let _ = EnumWindows(Some(enum_focus_window), LPARAM((&mut target as *mut FocusTarget) as isize));
            let hwnd = target.preferred.or(target.fallback).ok_or_else(|| "process has no visible window".to_string())?;
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }

            // First make the target visibly rise even on systems that enforce
            // foreground-lock restrictions, then ask Windows for keyboard focus.
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
            let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);

            let foreground = GetForegroundWindow();
            let current_thread = GetCurrentThreadId();
            let target_thread = GetWindowThreadProcessId(hwnd, None);
            let foreground_thread = if foreground.0.is_null() { 0 } else { GetWindowThreadProcessId(foreground, None) };
            if foreground_thread != 0 && foreground_thread != current_thread {
                let _ = AttachThreadInput(current_thread, foreground_thread, true);
            }
            if target_thread != 0 && target_thread != current_thread {
                let _ = AttachThreadInput(current_thread, target_thread, true);
            }
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            if target_thread != 0 && target_thread != current_thread {
                let _ = AttachThreadInput(current_thread, target_thread, false);
            }
            if foreground_thread != 0 && foreground_thread != current_thread {
                let _ = AttachThreadInput(current_thread, foreground_thread, false);
            }
        }
        Ok(())
    }

    pub fn open_file_location(path: &str) -> Result<(), String> {
        let path = Path::new(path);
        if !path.is_file() {
            return Err("process executable path is unavailable".to_string());
        }
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map_err(|error| format!("failed to open file location: {error}"))?;
        Ok(())
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
pub use imp::{focus_process_window, icon_data_url, open_file_location, visible_process_ids};

#[cfg(not(windows))]
pub fn visible_process_ids() -> HashSet<u32> {
    HashSet::new()
}

#[cfg(not(windows))]
pub fn icon_data_url(_: &str) -> Option<String> {
    None
}

#[cfg(not(windows))]
pub fn focus_process_window(_: u32) -> Result<(), String> {
    Err("process focus is only supported on Windows".to_string())
}

#[cfg(not(windows))]
pub fn open_file_location(_: &str) -> Result<(), String> {
    Err("opening process file locations is only supported on Windows".to_string())
}
