use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::ffi::{c_char, c_void, CStr};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
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
    pub software: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuEngineUsage {
    pub name: String,
    pub percent: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTelemetry {
    pub index: u32,
    pub name: String,
    pub luid: String,
    pub utilization_percent: Option<f64>,
    pub engines: Vec<GpuEngineUsage>,
    pub dedicated_used_bytes: Option<u64>,
    pub dedicated_total_bytes: u64,
    pub shared_used_bytes: Option<u64>,
    pub shared_total_bytes: u64,
    pub temperature_c: Option<f64>,
    pub temperature_provider: Option<String>,
    pub telemetry_error: Option<String>,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::mem::{size_of, zeroed};
    use std::thread;
    use std::time::Duration;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
        PdhGetFormattedCounterArrayW, PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W,
        PDH_FMT_DOUBLE, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    };

    fn wide_nul_to_string(buf: &[u16]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }

    fn pwstr_to_string(ptr: *mut u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        unsafe {
            let mut len = 0usize;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
        }
    }

    fn luid_string(luid: windows::Win32::Foundation::LUID) -> String {
        format!("{:08X}:{:08X}", luid.HighPart as u32, luid.LowPart)
    }

    pub fn enumerate_gpus() -> Result<Vec<GpuAdapterInfo>, String> {
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1()
                .map_err(|e| format!("CreateDXGIFactory1 failed: {e}"))?;
            let mut out = Vec::new();
            let mut dxgi_index = 0u32;

            loop {
                let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(dxgi_index) {
                    Ok(adapter) => adapter,
                    Err(_) => break,
                };
                let desc = adapter.GetDesc1()
                    .map_err(|e| format!("IDXGIAdapter1::GetDesc1 failed for adapter {dxgi_index}: {e}"))?;
                let name = wide_nul_to_string(&desc.Description);
                let software = (desc.Flags & 2) != 0 || name.eq_ignore_ascii_case("Microsoft Basic Render Driver");
                if !software {
                    let visible_index = out.len() as u32;
                    out.push(GpuAdapterInfo {
                        index: visible_index,
                        name,
                        luid: luid_string(desc.AdapterLuid),
                        vendor_id: desc.VendorId,
                        device_id: desc.DeviceId,
                        subsystem_id: desc.SubSysId,
                        revision: desc.Revision,
                        dedicated_video_memory_bytes: desc.DedicatedVideoMemory as u64,
                        dedicated_system_memory_bytes: desc.DedicatedSystemMemory as u64,
                        shared_system_memory_bytes: desc.SharedSystemMemory as u64,
                        software,
                    });
                }
                dxgi_index += 1;
            }
            Ok(out)
        }
    }

    fn parse_luid(instance: &str) -> Option<String> {
        let lower = instance.to_ascii_lowercase();
        let start = lower.find("luid_0x")? + "luid_0x".len();
        let rest = &lower[start..];
        let split = rest.find("_0x")?;
        let hi = u32::from_str_radix(&rest[..split], 16).ok()?;
        let rest2 = &rest[split + 3..];
        let end = rest2.find('_').unwrap_or(rest2.len());
        let lo = u32::from_str_radix(&rest2[..end], 16).ok()?;
        Some(format!("{hi:08X}:{lo:08X}"))
    }

    fn engine_key(instance: &str) -> String {
        if let Some(pos) = instance.to_ascii_lowercase().find("_phys_") {
            instance[pos + 1..].to_string()
        } else {
            instance.to_string()
        }
    }

    fn engine_type(instance: &str) -> String {
        let lower = instance.to_ascii_lowercase();
        if let Some(pos) = lower.find("engtype_") {
            let rest = &instance[pos + "engtype_".len()..];
            return rest.split('_').next().unwrap_or(rest).to_string();
        }
        "Other".to_string()
    }

    struct PdhQuery {
        query: PDH_HQUERY,
        engine: PDH_HCOUNTER,
        dedicated: PDH_HCOUNTER,
        shared: PDH_HCOUNTER,
    }

    impl PdhQuery {
        fn new() -> Result<Self, String> {
            unsafe {
                let mut query = PDH_HQUERY::default();
                let status = PdhOpenQueryW(PCWSTR::null(), 0, &mut query);
                if status != 0 {
                    return Err(format!("PdhOpenQueryW failed: 0x{status:08X}"));
                }

                let mut engine = PDH_HCOUNTER::default();
                let mut dedicated = PDH_HCOUNTER::default();
                let mut shared = PDH_HCOUNTER::default();
                for (path, counter) in [
                    (r"\GPU Engine(*)\Utilization Percentage", &mut engine),
                    (r"\GPU Adapter Memory(*)\Dedicated Usage", &mut dedicated),
                    (r"\GPU Adapter Memory(*)\Shared Usage", &mut shared),
                ] {
                    let hs = HSTRING::from(path);
                    let rc = PdhAddEnglishCounterW(query, &hs, 0, counter);
                    if rc != 0 {
                        let _ = PdhCloseQuery(query);
                        return Err(format!("PdhAddEnglishCounterW failed for {path}: 0x{rc:08X}"));
                    }
                }

                let rc = PdhCollectQueryData(query);
                if rc != 0 {
                    let _ = PdhCloseQuery(query);
                    return Err(format!("PdhCollectQueryData prime failed: 0x{rc:08X}"));
                }
                Ok(Self { query, engine, dedicated, shared })
            }
        }

        fn collect(&self) -> Result<(), String> {
            let rc = unsafe { PdhCollectQueryData(self.query) };
            if rc == 0 { Ok(()) } else { Err(format!("PdhCollectQueryData failed: 0x{rc:08X}")) }
        }

        fn doubles(&self, counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
            unsafe {
                let mut bytes = 0u32;
                let mut count = 0u32;
                let first = PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut bytes, &mut count, None);
                if first != PDH_MORE_DATA || bytes == 0 {
                    return Vec::new();
                }
                let units = (bytes as usize + size_of::<usize>() - 1) / size_of::<usize>();
                let mut storage = vec![0usize; units];
                let ptr = storage.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                let rc = PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut bytes, &mut count, Some(ptr));
                if rc != 0 {
                    return Vec::new();
                }
                std::slice::from_raw_parts(ptr, count as usize).iter().filter_map(|item| {
                    if item.FmtValue.CStatus != 0 {
                        return None;
                    }
                    let value = item.FmtValue.Anonymous.doubleValue;
                    if !value.is_finite() {
                        return None;
                    }
                    Some((pwstr_to_string(item.szName.0), value))
                }).collect()
            }
        }

        fn larges(&self, counter: PDH_HCOUNTER) -> Vec<(String, u64)> {
            unsafe {
                let mut bytes = 0u32;
                let mut count = 0u32;
                let first = PdhGetFormattedCounterArrayW(counter, PDH_FMT_LARGE, &mut bytes, &mut count, None);
                if first != PDH_MORE_DATA || bytes == 0 {
                    return Vec::new();
                }
                let units = (bytes as usize + size_of::<usize>() - 1) / size_of::<usize>();
                let mut storage = vec![0usize; units];
                let ptr = storage.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                let rc = PdhGetFormattedCounterArrayW(counter, PDH_FMT_LARGE, &mut bytes, &mut count, Some(ptr));
                if rc != 0 {
                    return Vec::new();
                }
                std::slice::from_raw_parts(ptr, count as usize).iter().filter_map(|item| {
                    if item.FmtValue.CStatus != 0 {
                        return None;
                    }
                    let value = item.FmtValue.Anonymous.largeValue;
                    if value < 0 {
                        return None;
                    }
                    Some((pwstr_to_string(item.szName.0), value as u64))
                }).collect()
            }
        }
    }

    impl Drop for PdhQuery {
        fn drop(&mut self) {
            unsafe { let _ = PdhCloseQuery(self.query); }
        }
    }

    #[repr(C)]
    struct NvmlUtilization {
        gpu: u32,
        memory: u32,
    }

    #[repr(C)]
    struct NvmlMemory {
        total: u64,
        free: u64,
        used: u64,
    }

    type NvmlDevice = *mut c_void;
    type NvmlInit = unsafe extern "C" fn() -> i32;
    type NvmlShutdown = unsafe extern "C" fn() -> i32;
    type NvmlGetCount = unsafe extern "C" fn(*mut u32) -> i32;
    type NvmlGetHandle = unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32;
    type NvmlGetName = unsafe extern "C" fn(NvmlDevice, *mut c_char, u32) -> i32;
    type NvmlGetTemp = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32;
    type NvmlGetUtil = unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> i32;
    type NvmlGetMem = unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> i32;

    struct Nvml {
        _lib: libloading::Library,
        shutdown: NvmlShutdown,
        handles: Vec<(String, NvmlDevice)>,
        get_temp: NvmlGetTemp,
        get_util: NvmlGetUtil,
        get_mem: NvmlGetMem,
    }

    impl Nvml {
        unsafe fn try_open() -> Option<Self> {
            let mut candidates = vec![PathBuf::from("nvml.dll")];
            if let Some(root) = std::env::var_os("ProgramW6432") {
                candidates.push(PathBuf::from(root).join("NVIDIA Corporation").join("NVSMI").join("nvml.dll"));
            }
            let lib = candidates.into_iter().find_map(|p| libloading::Library::new(p).ok())?;
            let init: NvmlInit = *lib.get::<NvmlInit>(b"nvmlInit_v2\0").ok()?;
            let shutdown: NvmlShutdown = *lib.get::<NvmlShutdown>(b"nvmlShutdown\0").ok()?;
            let get_count: NvmlGetCount = *lib.get::<NvmlGetCount>(b"nvmlDeviceGetCount_v2\0").ok()?;
            let get_handle: NvmlGetHandle = *lib.get::<NvmlGetHandle>(b"nvmlDeviceGetHandleByIndex_v2\0").ok()?;
            let get_name: NvmlGetName = *lib.get::<NvmlGetName>(b"nvmlDeviceGetName\0").ok()?;
            let get_temp: NvmlGetTemp = *lib.get::<NvmlGetTemp>(b"nvmlDeviceGetTemperature\0").ok()?;
            let get_util: NvmlGetUtil = *lib.get::<NvmlGetUtil>(b"nvmlDeviceGetUtilizationRates\0").ok()?;
            let get_mem: NvmlGetMem = *lib.get::<NvmlGetMem>(b"nvmlDeviceGetMemoryInfo\0").ok()?;
            if init() != 0 {
                return None;
            }
            let mut count = 0u32;
            if get_count(&mut count) != 0 {
                let _ = shutdown();
                return None;
            }
            let mut handles = Vec::new();
            for index in 0..count {
                let mut handle: NvmlDevice = std::ptr::null_mut();
                if get_handle(index, &mut handle) != 0 || handle.is_null() {
                    continue;
                }
                let mut buf = [0i8; 128];
                let name = if get_name(handle, buf.as_mut_ptr(), buf.len() as u32) == 0 {
                    CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
                } else {
                    String::new()
                };
                handles.push((name, handle));
            }
            Some(Self { _lib: lib, shutdown, handles, get_temp, get_util, get_mem })
        }

        fn for_adapter(&self, adapter: &GpuAdapterInfo) -> Option<(Option<f64>, Option<u64>, Option<f64>)> {
            if adapter.vendor_id != 0x10DE {
                return None;
            }
            let needle = adapter.name.to_ascii_lowercase();
            let (_, handle) = self.handles.iter().find(|(name, _)| {
                let n = name.to_ascii_lowercase();
                needle.contains(&n) || n.contains(&needle)
            }).or_else(|| self.handles.first())?;
            unsafe {
                let mut temp = 0u32;
                let temp = if (self.get_temp)(*handle, 0, &mut temp) == 0 { Some(temp as f64) } else { None };
                let mut util: NvmlUtilization = zeroed();
                let util = if (self.get_util)(*handle, &mut util) == 0 { Some(util.gpu as f64) } else { None };
                let mut mem: NvmlMemory = zeroed();
                let used = if (self.get_mem)(*handle, &mut mem) == 0 { Some(mem.used) } else { None };
                Some((temp, used, util))
            }
        }
    }

    impl Drop for Nvml {
        fn drop(&mut self) {
            unsafe { let _ = (self.shutdown)(); }
        }
    }

    pub struct GpuSampler {
        adapters: Vec<GpuAdapterInfo>,
        pdh: Option<PdhQuery>,
        nvml: Option<Nvml>,
        init_error: Option<String>,
    }

    impl GpuSampler {
        pub fn new(adapters: Vec<GpuAdapterInfo>) -> Self {
            let (pdh, init_error) = match PdhQuery::new() {
                Ok(pdh) => (Some(pdh), None),
                Err(error) => (None, Some(error)),
            };
            let nvml = unsafe { Nvml::try_open() };
            Self { adapters, pdh, nvml, init_error }
        }

        pub fn prime_delay() {
            thread::sleep(Duration::from_millis(1100));
        }

        pub fn sample(&self) -> Vec<GpuTelemetry> {
            let mut engine_rows = Vec::new();
            let mut dedicated_rows = Vec::new();
            let mut shared_rows = Vec::new();
            let mut collect_error = self.init_error.clone();

            if let Some(pdh) = &self.pdh {
                if let Err(error) = pdh.collect() {
                    collect_error = Some(error);
                } else {
                    engine_rows = pdh.doubles(pdh.engine);
                    dedicated_rows = pdh.larges(pdh.dedicated);
                    shared_rows = pdh.larges(pdh.shared);
                }
            }

            let mut engine_groups: HashMap<(String, String), f64> = HashMap::new();
            let mut type_groups: HashMap<(String, String), f64> = HashMap::new();
            for (instance, value) in engine_rows {
                let Some(luid) = parse_luid(&instance) else { continue };
                let key = engine_key(&instance);
                *engine_groups.entry((luid, key)).or_insert(0.0) += value.max(0.0);
            }
            for ((luid, key), value) in engine_groups {
                let kind = engine_type(&key);
                let entry = type_groups.entry((luid, kind)).or_insert(0.0);
                *entry = entry.max(value.min(100.0));
            }

            let mut dedicated: BTreeMap<String, u64> = BTreeMap::new();
            for (instance, value) in dedicated_rows {
                if let Some(luid) = parse_luid(&instance) {
                    *dedicated.entry(luid).or_insert(0) += value;
                }
            }
            let mut shared: BTreeMap<String, u64> = BTreeMap::new();
            for (instance, value) in shared_rows {
                if let Some(luid) = parse_luid(&instance) {
                    *shared.entry(luid).or_insert(0) += value;
                }
            }

            self.adapters.iter().map(|adapter| {
                let mut engines: Vec<GpuEngineUsage> = type_groups.iter()
                    .filter(|((luid, _), _)| luid == &adapter.luid)
                    .map(|((_, kind), value)| GpuEngineUsage { name: kind.clone(), percent: *value })
                    .collect();
                engines.sort_by(|a, b| b.percent.partial_cmp(&a.percent).unwrap_or(std::cmp::Ordering::Equal));
                let pdh_util = engines.iter().map(|e| e.percent).fold(None, |m, v| Some(m.map_or(v, |x: f64| x.max(v))));
                let mut temperature_c = None;
                let mut temperature_provider = None;
                let mut dedicated_used = dedicated.get(&adapter.luid).copied();
                let mut utilization = pdh_util;

                if let Some(nvml) = &self.nvml {
                    if let Some((temp, nvml_used, nvml_util)) = nvml.for_adapter(adapter) {
                        if temp.is_some() {
                            temperature_c = temp;
                            temperature_provider = Some("NVIDIA NVML".to_string());
                        }
                        if dedicated_used.is_none() {
                            dedicated_used = nvml_used;
                        }
                        if utilization.is_none() {
                            utilization = nvml_util;
                        }
                    }
                }

                GpuTelemetry {
                    index: adapter.index,
                    name: adapter.name.clone(),
                    luid: adapter.luid.clone(),
                    utilization_percent: utilization,
                    engines,
                    dedicated_used_bytes: dedicated_used,
                    dedicated_total_bytes: adapter.dedicated_video_memory_bytes,
                    shared_used_bytes: shared.get(&adapter.luid).copied(),
                    shared_total_bytes: adapter.shared_system_memory_bytes,
                    temperature_c,
                    temperature_provider,
                    telemetry_error: collect_error.clone(),
                }
            }).collect()
        }
    }
}

#[cfg(windows)]
pub use imp::{enumerate_gpus, GpuSampler};

#[cfg(not(windows))]
pub fn enumerate_gpus() -> Result<Vec<GpuAdapterInfo>, String> {
    Ok(Vec::new())
}

#[cfg(not(windows))]
pub struct GpuSampler;
#[cfg(not(windows))]
impl GpuSampler {
    pub fn new(_: Vec<GpuAdapterInfo>) -> Self { Self }
    pub fn prime_delay() {}
    pub fn sample(&self) -> Vec<GpuTelemetry> { Vec::new() }
}
