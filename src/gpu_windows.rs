#[cfg(windows)]
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuAdapterInfo {
    pub index: u32,
    pub name: String,
    pub luid: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub subsystem_id: u32,
    pub revision: u32,
    pub dedicated_video_memory_bytes: u64,
    pub dedicated_system_memory_bytes: u64,
    pub shared_system_memory_bytes: u64,
}

#[cfg(windows)]
fn wide_nul_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

#[cfg(windows)]
fn luid_string(luid: windows::Win32::Foundation::LUID) -> String {
    format!("{:08X}:{:08X}", luid.HighPart as u32, luid.LowPart)
}

#[cfg(windows)]
pub fn enumerate_gpus() -> Result<Vec<GpuAdapterInfo>, String> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()
            .map_err(|e| format!("CreateDXGIFactory1 failed: {e}"))?;
        let mut out = Vec::new();
        let mut index = 0u32;

        loop {
            let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(index) {
                Ok(adapter) => adapter,
                Err(_) => break,
            };
            let desc = adapter.GetDesc1()
                .map_err(|e| format!("IDXGIAdapter1::GetDesc1 failed for adapter {index}: {e}"))?;
            out.push(GpuAdapterInfo {
                index,
                name: wide_nul_to_string(&desc.Description),
                luid: luid_string(desc.AdapterLuid),
                vendor_id: desc.VendorId,
                device_id: desc.DeviceId,
                subsystem_id: desc.SubSysId,
                revision: desc.Revision,
                dedicated_video_memory_bytes: desc.DedicatedVideoMemory as u64,
                dedicated_system_memory_bytes: desc.DedicatedSystemMemory as u64,
                shared_system_memory_bytes: desc.SharedSystemMemory as u64,
            });
            index += 1;
        }

        Ok(out)
    }
}

#[cfg(not(windows))]
pub fn enumerate_gpus() -> Result<Vec<GpuAdapterInfo>, String> {
    Ok(Vec::new())
}
