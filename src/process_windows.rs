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
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, DrawIconEx, EnumWindows, GetWindowThreadProcessId,
        IsWindowVisible, DI_NORMAL,
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
pub use imp::{icon_data_url, open_file_location, visible_process_ids};

#[cfg(not(windows))]
pub fn visible_process_ids() -> HashSet<u32> {
    HashSet::new()
}

#[cfg(not(windows))]
pub fn icon_data_url(_: &str) -> Option<String> {
    None
}

#[cfg(not(windows))]
pub fn open_file_location(_: &str) -> Result<(), String> {
    Err("opening process file locations is only supported on Windows".to_string())
}
