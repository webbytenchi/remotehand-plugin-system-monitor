use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::io::{self, Read, Write};
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{Disks, Pid, ProcessesToUpdate, System, Users};

mod gpu_windows;
mod process_windows;
use gpu_windows::{GpuAdapterInfo, GpuSampler, GpuTelemetry};

const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const STORAGE_KEY: &str = "config-v1";
const PROCESS_UI_STORAGE_KEY: &str = "process-ui-v1";
const ALERT_HISTORY_STORAGE_KEY: &str = "alert-history-v1";
const MAX_HISTORY_SECONDS: u64 = 60 * 60;
const MAX_ALERT_HISTORY_EVENTS: usize = 10_000;
const DEFAULT_SAMPLE_SECONDS: u64 = 2;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostReady {
    #[serde(rename = "type")]
    message_type: String,
    plugin_id: String,
    api_version: u32,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginHello<'a> {
    #[serde(rename = "type")]
    message_type: &'static str,
    plugin_id: &'a str,
    api_version: u32,
    plugin_version: &'a str,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostRequest {
    #[serde(rename = "type")]
    _message_type: String,
    request_id: u64,
    method: String,
    #[serde(default)]
    params: Value,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginResponse {
    #[serde(rename = "type")]
    message_type: &'static str,
    request_id: u64,
    ok: bool,
    result: Value,
    error: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginEvent {
    #[serde(rename = "type")]
    message_type: &'static str,
    event: &'static str,
    payload: Value,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginHostRequest {
    #[serde(rename = "type")]
    message_type: &'static str,
    request_id: u64,
    method: &'static str,
    params: Value,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostResponse {
    #[serde(rename = "type")]
    _message_type: String,
    request_id: u64,
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlertDelivery {
    #[serde(default = "default_true")]
    sound: bool,
    #[serde(default = "default_true")]
    notification: bool,
    #[serde(default)]
    sound_file: Option<Value>,
}
fn alert_delivery(sound: bool, notification: bool) -> AlertDelivery {
    AlertDelivery { sound, notification, sound_file: None }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetricConfig {
    #[serde(default = "default_true")]
    show_topbar: bool,
    #[serde(default)]
    show_tray: bool,
    #[serde(default = "default_true")]
    sound: bool,
    #[serde(default = "default_true")]
    notification: bool,
    #[serde(default)]
    sound_file: Option<Value>,
    #[serde(default)]
    warning_alert: Option<AlertDelivery>,
    #[serde(default)]
    critical_alert: Option<AlertDelivery>,
    #[serde(default)]
    recovered_alert: Option<AlertDelivery>,
    #[serde(default)]
    overview_visualization: Option<String>,
    #[serde(default)]
    topbar_visualization: Option<String>,
    #[serde(default)]
    topbar_title: Option<String>,
    #[serde(default)]
    topbar_title_color: Option<String>,
    #[serde(default)]
    topbar_value_color: Option<String>,
    #[serde(default)]
    topbar_value_alert_color: Option<String>,
    #[serde(default)]
    topbar_background_color: Option<String>,
    #[serde(default)]
    topbar_border_color: Option<String>,
    healthy_max: f64,
    critical_min: f64,
    healthy_color: String,
    warning_color: String,
    critical_color: String,
    unavailable_color: String,
    text_color: String,
    border_color: String,
}
fn default_true() -> bool { true }
fn metric_config(healthy_max: f64, critical_min: f64) -> MetricConfig {
    MetricConfig {
        show_topbar: true,
        show_tray: false,
        sound: true,
        notification: true,
        sound_file: None,
        warning_alert: Some(alert_delivery(true, true)),
        critical_alert: Some(alert_delivery(true, true)),
        recovered_alert: Some(alert_delivery(true, false)),
        overview_visualization: None,
        topbar_visualization: None,
        topbar_title: None,
        topbar_title_color: None,
        topbar_value_color: None,
        topbar_value_alert_color: None,
        topbar_background_color: None,
        topbar_border_color: None,
        healthy_max,
        critical_min,
        healthy_color: "#2f9e62".into(),
        warning_color: "#c58a24".into(),
        critical_color: "#c84c4c".into(),
        unavailable_color: "#596473".into(),
        text_color: "#ffffff".into(),
        border_color: "#d2dae4".into(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiskConfig {
    usage: MetricConfig,
}
fn disk_config(label:&str)->DiskConfig{
    let mut usage=metric_config(80.0,95.0);
    usage.overview_visualization=Some("bar".into());
    usage.topbar_visualization=Some("bar".into());
    usage.topbar_title=Some(format!("{} Disk",label.trim_end_matches(|ch| ch=='\\' || ch=='/')));
    usage.show_topbar=false;
    DiskConfig{usage}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GpuConfig {
    utilization: MetricConfig,
    dedicated_memory: MetricConfig,
    shared_memory: MetricConfig,
    temperature: MetricConfig,
}
fn gpu_config(_index:u32,name:&str)->GpuConfig{
    let mut utilization=metric_config(70.0,95.0);
    utilization.overview_visualization=Some("graph".into());
    utilization.topbar_visualization=Some("graph".into());
    utilization.topbar_title=Some(name.to_string());

    let mut dedicated_memory=metric_config(80.0,95.0);
    dedicated_memory.overview_visualization=Some("bar".into());
    dedicated_memory.topbar_visualization=Some("bar".into());
    dedicated_memory.topbar_title=Some(format!("{name} VRAM"));
    dedicated_memory.show_topbar=false;

    let mut shared_memory=metric_config(80.0,95.0);
    shared_memory.overview_visualization=Some("bar".into());
    shared_memory.topbar_visualization=Some("bar".into());
    shared_memory.topbar_title=Some(format!("{name} Shared"));
    shared_memory.show_topbar=false;

    let mut temperature=metric_config(70.0,90.0);
    temperature.overview_visualization=Some("graph".into());
    temperature.topbar_visualization=Some("graph".into());
    temperature.topbar_title=Some(format!("{name} Temp"));
    temperature.show_topbar=false;

    GpuConfig{utilization,dedicated_memory,shared_memory,temperature}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    sample_seconds: u64,
    default_window_minutes: u64,
    #[serde(default = "default_true")]
    alert_history_enabled: bool,
    #[serde(default = "default_true")]
    alert_sounds_enabled: bool,
    cpu: MetricConfig,
    temperature: MetricConfig,
    memory: MetricConfig,
    #[serde(default)]
    gpus: BTreeMap<String,GpuConfig>,
    #[serde(default)]
    disks: BTreeMap<String,DiskConfig>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            sample_seconds: DEFAULT_SAMPLE_SECONDS,
            default_window_minutes: 15,
            alert_history_enabled: true,
            alert_sounds_enabled: true,
            cpu: {
                let mut m=metric_config(70.0,95.0);
                m.overview_visualization=Some("graph".into());
                m.topbar_visualization=Some("graph".into());
                m.topbar_title=Some("CPU".into());
                m
            },
            temperature: metric_config(60.0, 90.0),
            memory: {
                let mut m=metric_config(70.0,90.0);
                m.overview_visualization=Some("bar".into());
                m.topbar_visualization=Some("bar".into());
                m.topbar_title=Some("Memory".into());
                m
            },
            gpus:BTreeMap::new(),
            disks:BTreeMap::new(),
        }
    }
}
fn normalize_config(mut c: Config) -> Result<Config, String> {
    c.sample_seconds = c.sample_seconds.clamp(1, 30);
    if !matches!(c.default_window_minutes, 5 | 15 | 30 | 60) { c.default_window_minutes = 15; }
    for (name, m) in [("CPU",&c.cpu),("Temperature",&c.temperature),("Memory",&c.memory)] {
        if !m.healthy_max.is_finite() || !m.critical_min.is_finite() || m.healthy_max >= m.critical_min {
            return Err(format!("{name} thresholds must be finite and healthy < critical"));
        }
    }
    for (id,disk) in &c.disks {
        let m=&disk.usage;
        if !m.healthy_max.is_finite() || !m.critical_min.is_finite() || m.healthy_max >= m.critical_min {
            return Err(format!("Disk {id} usage thresholds must be finite and healthy < critical"));
        }
    }
    for (luid,gpu) in &c.gpus {
        for (name,m) in [
            ("utilization",&gpu.utilization),
            ("dedicated memory",&gpu.dedicated_memory),
            ("shared memory",&gpu.shared_memory),
            ("temperature",&gpu.temperature),
        ] {
            if !m.healthy_max.is_finite() || !m.critical_min.is_finite() || m.healthy_max >= m.critical_min {
                return Err(format!("GPU {luid} {name} thresholds must be finite and healthy < critical"));
            }
        }
    }
    Ok(c)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiskTelemetry {
    id: String,
    name: String,
    mount_point: String,
    file_system: String,
    kind: String,
    removable: bool,
    total_bytes: u64,
    used_bytes: u64,
    free_bytes: u64,
    usage_percent: f64,
    read_bytes_per_second: f64,
    write_bytes_per_second: f64,
}

fn disk_metric_id(disk_id:&str)->String{format!("disk-{disk_id}-usage")}
fn gpu_metric_id(index:u32,kind:&str)->String{format!("gpu{index}-{kind}")}
fn disk_rate_bytes_per_second(delta_bytes:u64,elapsed_ms:u64)->f64{
    if elapsed_ms==0{0.0}else{delta_bytes as f64/(elapsed_ms as f64/1000.0)}
}
fn csv_disk_key(id:&str)->String{
    let mut out=id.chars().map(|ch|if ch.is_ascii_alphanumeric(){ch.to_ascii_lowercase()}else{'_'}).collect::<String>();
    while out.contains("__"){out=out.replace("__","_");}
    out.trim_matches('_').to_string()
}

fn stable_disk_id(mount_point: &str, name: &str) -> String {
    let normalized = mount_point
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    if normalized.is_empty() {
        name.trim().to_ascii_lowercase()
    } else {
        normalized
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Sample {
    timestamp: u64,
    cpu_percent: f64,
    core_percent: Vec<f64>,
    temperature_c: Option<f64>,
    temperature_source: Option<String>,
    temperature_sensors: Vec<TemperatureSensor>,
    temperature_provider: String,
    temperature_error: Option<String>,
    memory_percent: f64,
    memory_used_bytes: u64,
    memory_available_bytes: u64,
    memory_total_bytes: u64,
    disks: Vec<DiskTelemetry>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TemperatureSensor {
    label: String,
    value_c: f64,
}

struct WindowsSystemProvider {
    system: System,
    disks: Disks,
    last_disk_sample_ms: u64,
}
impl WindowsSystemProvider {
    fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_cpu_usage();
        system.refresh_memory();
        Self {
            system,
            disks: Disks::new_with_refreshed_list(),
            last_disk_sample_ms: now_ms(),
        }
    }
    fn sample(&mut self) -> Sample {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.disks.refresh(true);

        let disk_timestamp = now_ms();
        let elapsed_ms = disk_timestamp.saturating_sub(self.last_disk_sample_ms);
        self.last_disk_sample_ms = disk_timestamp;
        let disks = self.disks.list().iter().map(|disk| {
            let total_bytes = disk.total_space();
            let free_bytes = disk.available_space().min(total_bytes);
            let used_bytes = total_bytes.saturating_sub(free_bytes);
            let usage_percent = if total_bytes == 0 { 0.0 } else { used_bytes as f64 / total_bytes as f64 * 100.0 };
            let usage = disk.usage();
            let name = disk.name().to_string_lossy().into_owned();
            let mount_point = disk.mount_point().to_string_lossy().into_owned();
            DiskTelemetry {
                id: stable_disk_id(&mount_point, &name),
                name,
                mount_point,
                file_system: disk.file_system().to_string_lossy().into_owned(),
                kind: format!("{:?}", disk.kind()),
                removable: disk.is_removable(),
                total_bytes,
                used_bytes,
                free_bytes,
                usage_percent,
                read_bytes_per_second: disk_rate_bytes_per_second(usage.read_bytes,elapsed_ms),
                write_bytes_per_second: disk_rate_bytes_per_second(usage.written_bytes,elapsed_ms),
            }
        }).collect::<Vec<_>>();

        // CPU temperature is intentionally deferred for the v0.1 dependency-free baseline.
        // Keep the normalized fields unavailable so a future provider can be added without
        // changing the surrounding sample model.
        let sensors = Vec::<TemperatureSensor>::new();
        let preferred: Option<&TemperatureSensor> = None;
        let provider_name = "Deferred".to_string();
        let provider_error = None;

        let total = self.system.total_memory();
        let used = self.system.used_memory();
        let available = self.system.available_memory();
        let memory_percent = if total == 0 { 0.0 } else { (used as f64 / total as f64) * 100.0 };

        Sample {
            timestamp: now_ms(),
            cpu_percent: self.system.global_cpu_usage() as f64,
            core_percent: self.system.cpus().iter().map(|c| c.cpu_usage() as f64).collect(),
            temperature_c: preferred.map(|s| s.value_c),
            temperature_source: preferred.map(|s| s.label.clone()),
            temperature_sensors: sensors,
            temperature_provider: provider_name,
            temperature_error: provider_error,
            memory_percent,
            memory_used_bytes: used,
            memory_available_bytes: available,
            memory_total_bytes: total,
            disks,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Health { Healthy, Warning, Critical, Unavailable }
fn percent(used:Option<u64>,total:u64)->Option<f64>{used.filter(|_|total>0).map(|u|(u as f64/total as f64)*100.0)}
fn health(value: Option<f64>, cfg: &MetricConfig) -> Health {
    let Some(v)=value.filter(|v|v.is_finite()) else { return Health::Unavailable; };
    if v >= cfg.critical_min { Health::Critical }
    else if v <= cfg.healthy_max { Health::Healthy }
    else { Health::Warning }
}
fn health_name(h: Health) -> &'static str {
    match h { Health::Healthy=>"healthy",Health::Warning=>"warning",Health::Critical=>"critical",Health::Unavailable=>"unavailable" }
}
fn health_color(h: Health, cfg: &MetricConfig) -> &str {
    match h {
        Health::Healthy=>&cfg.healthy_color,
        Health::Warning=>&cfg.warning_color,
        Health::Critical=>&cfg.critical_color,
        Health::Unavailable=>&cfg.unavailable_color,
    }
}

fn alert_delivery_for(cfg: &MetricConfig, state: Health) -> AlertDelivery {
    match state {
        Health::Warning => cfg.warning_alert.clone().unwrap_or_else(|| AlertDelivery { sound: cfg.sound, notification: cfg.notification, sound_file: cfg.sound_file.clone() }),
        Health::Critical => cfg.critical_alert.clone().unwrap_or_else(|| AlertDelivery { sound: cfg.sound, notification: cfg.notification, sound_file: cfg.sound_file.clone() }),
        Health::Healthy => cfg.recovered_alert.clone().unwrap_or_else(|| alert_delivery(true, false)),
        Health::Unavailable => alert_delivery(false, false),
    }
}

#[derive(Clone)]
struct TransitionTracker {
    committed: Health,
    pending: Health,
    count: u8,
}
impl Default for TransitionTracker {
    fn default() -> Self { Self { committed: Health::Unavailable, pending: Health::Unavailable, count: 0 } }
}
impl TransitionTracker {
    fn observe(&mut self, next: Health) -> Option<(Health,Health)> {
        if next == self.committed { self.pending=next; self.count=0; return None; }
        if next != self.pending { self.pending=next; self.count=1; return None; }
        self.count=self.count.saturating_add(1);
        if self.count < 2 { return None; }
        let previous=self.committed;
        self.committed=next; self.count=0;
        Some((previous,next))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GpuSnapshot {
    timestamp: u64,
    gpus: Vec<GpuTelemetry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessSnapshot {
    pid: u32,
    name: String,
    cpu_percent: f64,
    memory_bytes: u64,
    gpu_percent: Option<f64>,
    user: String,
    start_time: u64,
    parent_pid: Option<u32>,
    path: String,
    command_line: String,
    status: String,
    window_visible: bool,
}

fn process_snapshot(system: &System, users: &Users, visible: &std::collections::HashSet<u32>) -> Vec<ProcessSnapshot> {
    let cpu_count = system.cpus().len().max(1) as f64;
    system.processes().values().map(|p| {
        let pid = p.pid().as_u32();
        let user = p.user_id()
            .and_then(|id| users.get_user_by_id(id))
            .map(|u| u.name().to_string())
            .unwrap_or_default();
        ProcessSnapshot {
            pid,
            name: p.name().to_string_lossy().into_owned(),
            cpu_percent: (p.cpu_usage() as f64 / cpu_count).clamp(0.0, 100.0),
            memory_bytes: p.memory(),
            gpu_percent: None,
            user,
            start_time: p.start_time(),
            parent_pid: p.parent().map(|x| x.as_u32()),
            path: p.exe().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default(),
            command_line: p.cmd().iter().map(|x| x.to_string_lossy()).collect::<Vec<_>>().join(" "),
            status: format!("{:?}", p.status()),
            window_visible: visible.contains(&pid),
        }
    }).collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessNode {
    #[serde(flatten)]
    process: ProcessSnapshot,
    children: Vec<ProcessNode>,
    aggregate_cpu_percent: f64,
    aggregate_memory_bytes: u64,
}

fn process_matches(p: &ProcessSnapshot, query: &str, filter: &str) -> bool {
    let query_ok = query.is_empty()
        || p.name.to_ascii_lowercase().contains(query)
        || p.path.to_ascii_lowercase().contains(query)
        || p.command_line.to_ascii_lowercase().contains(query)
        || p.user.to_ascii_lowercase().contains(query)
        || p.pid.to_string().contains(query);
    let filter_ok = match filter {
        "background" => !p.window_visible,
        "visible" => p.window_visible,
        "highCpu" => p.cpu_percent >= 10.0,
        "highMemory" => p.memory_bytes >= 512 * 1024 * 1024,
        "gpuActive" => p.gpu_percent.unwrap_or(0.0) > 0.0,
        _ => true,
    };
    query_ok && filter_ok
}

fn build_process_node(
    pid: u32,
    by_pid: &HashMap<u32, ProcessSnapshot>,
    children_by_parent: &HashMap<u32, Vec<u32>>,
    included: &HashSet<u32>,
    sort: &str,
    direction: &str,
) -> Option<ProcessNode> {
    if !included.contains(&pid) { return None; }
    let process=by_pid.get(&pid)?.clone();
    let mut children:Vec<ProcessNode>=children_by_parent.get(&pid).into_iter().flatten()
        .filter_map(|child|build_process_node(*child,by_pid,children_by_parent,included,sort,direction))
        .collect();
    sort_process_nodes(&mut children,sort,direction);
    let aggregate_cpu_percent=process.cpu_percent+children.iter().map(|x|x.aggregate_cpu_percent).sum::<f64>();
    let aggregate_memory_bytes=process.memory_bytes+children.iter().map(|x|x.aggregate_memory_bytes).sum::<u64>();
    Some(ProcessNode{process,children,aggregate_cpu_percent,aggregate_memory_bytes})
}

fn sort_process_nodes(nodes:&mut [ProcessNode], sort:&str, direction:&str){
    nodes.sort_by(|a,b|{
        let ord=match sort{
            "name"=>a.process.name.to_ascii_lowercase().cmp(&b.process.name.to_ascii_lowercase()),
            "pid"=>a.process.pid.cmp(&b.process.pid),
            "memory"=>a.aggregate_memory_bytes.cmp(&b.aggregate_memory_bytes),
            "gpu"=>a.process.gpu_percent.unwrap_or(0.0).total_cmp(&b.process.gpu_percent.unwrap_or(0.0)),
            "user"=>a.process.user.to_ascii_lowercase().cmp(&b.process.user.to_ascii_lowercase()),
            "started"=>a.process.start_time.cmp(&b.process.start_time),
            _=>a.aggregate_cpu_percent.total_cmp(&b.aggregate_cpu_percent),
        };
        let ord=if direction=="asc"{ord}else{ord.reverse()};
        ord.then_with(||a.process.pid.cmp(&b.process.pid))
    });
}

fn process_list_value(rows: &[ProcessSnapshot], params: &Value, updated_at: u64) -> Value {
    let query = params.get("query").and_then(Value::as_str).unwrap_or("").trim().to_ascii_lowercase();
    let filter = params.get("filter").and_then(Value::as_str).unwrap_or("all");
    let sort = params.get("sort").and_then(Value::as_str).unwrap_or("cpu");
    let direction = params.get("direction").and_then(Value::as_str).unwrap_or("desc");
    let by_pid:HashMap<u32,ProcessSnapshot>=rows.iter().cloned().map(|p|(p.pid,p)).collect();
    let mut children_by_parent:HashMap<u32,Vec<u32>>=HashMap::new();
    for p in rows{
        if let Some(parent)=p.parent_pid{
            if parent!=p.pid && by_pid.contains_key(&parent){
                children_by_parent.entry(parent).or_default().push(p.pid);
            }
        }
    }

    let direct:HashSet<u32>=rows.iter().filter(|p|process_matches(p,&query,filter)).map(|p|p.pid).collect();
    let mut included=direct.clone();
    for pid in direct{
        let mut cursor=by_pid.get(&pid).and_then(|p|p.parent_pid);
        let mut guard=0usize;
        while let Some(parent)=cursor{
            if guard>64 || !by_pid.contains_key(&parent){break;}
            included.insert(parent);
            cursor=by_pid.get(&parent).and_then(|p|p.parent_pid);
            guard+=1;
        }
    }

    let mut roots:Vec<ProcessNode>=rows.iter()
        .filter(|p|included.contains(&p.pid))
        .filter(|p|p.parent_pid.is_none() || !included.contains(&p.parent_pid.unwrap_or_default()))
        .filter_map(|p|build_process_node(p.pid,&by_pid,&children_by_parent,&included,sort,direction))
        .collect();
    sort_process_nodes(&mut roots,sort,direction);

    fn count_nodes(nodes:&[ProcessNode])->usize{nodes.iter().map(|n|1+count_nodes(&n.children)).sum()}
    let total_processes=count_nodes(&roots);
    let total_groups=roots.len();

    json!({
        "rows":roots,
        "total":total_processes,
        "totalGroups":total_groups,
        "page":1,
        "pageCount":1,
        "sort":sort,
        "direction":direction,
        "updatedAt":updated_at,
        "searchActive":!query.is_empty() || filter!="all"
    })
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlertHistoryEvent {
    id: String,
    timestamp: u64,
    metric: String,
    title: String,
    status: String,
    value: String,
    threshold: String,
    sound: bool,
    notification: bool,
    details: String,
}
#[derive(Debug, Clone)]
struct AlertHistoryState {
    events: Arc<Mutex<Vec<AlertHistoryEvent>>>,
    dirty: Arc<AtomicBool>,
}
impl Default for AlertHistoryState {
    fn default() -> Self {
        Self { events: Arc::new(Mutex::new(Vec::new())), dirty: Arc::new(AtomicBool::new(false)) }
    }
}
impl AlertHistoryState {
    fn record(&self,event:AlertHistoryEvent){
        if let Ok(mut events)=self.events.lock(){
            events.push(event);
            if events.len()>MAX_ALERT_HISTORY_EVENTS {
                let remove=events.len()-MAX_ALERT_HISTORY_EVENTS;
                events.drain(0..remove);
            }
            self.dirty.store(true,Ordering::Release);
        }
    }
    fn snapshot(&self)->Vec<AlertHistoryEvent>{self.events.lock().map(|events|events.clone()).unwrap_or_default()}
    fn replace(&self,mut events:Vec<AlertHistoryEvent>){
        if events.len()>MAX_ALERT_HISTORY_EVENTS{events.drain(0..events.len()-MAX_ALERT_HISTORY_EVENTS);}
        if let Ok(mut current)=self.events.lock(){*current=events;}
        self.dirty.store(false,Ordering::Release);
    }
    fn clear(&self){
        if let Ok(mut events)=self.events.lock(){events.clear();}
        self.dirty.store(true,Ordering::Release);
    }
}

struct Store {
    config: Config,
    history: VecDeque<Sample>,
    gpu_history: VecDeque<GpuSnapshot>,
    alert_history: AlertHistoryState,
    gpu_latest: Vec<GpuTelemetry>,
    gpu_adapters: Vec<GpuAdapterInfo>,
    gpu_discovery_state: String,
    gpu_discovery_error: Option<String>,
    processes: Vec<ProcessSnapshot>,
    processes_updated_at: u64,
    process_icon_cache: BTreeMap<String, Option<String>>,
    cpu_tracker: TransitionTracker,
    temp_tracker: TransitionTracker,
    memory_tracker: TransitionTracker,
    gpu_trackers: BTreeMap<String,TransitionTracker>,
    disk_trackers: BTreeMap<String,TransitionTracker>,
}
impl Store {
    fn new() -> Self {
        Self {
            config: Config::default(),
            history: VecDeque::new(),
            gpu_history: VecDeque::new(),
            alert_history: AlertHistoryState::default(),
            gpu_latest: Vec::new(),
            gpu_adapters: Vec::new(),
            gpu_discovery_state: "pending".into(),
            gpu_discovery_error: None,
            processes: Vec::new(),
            processes_updated_at: 0,
            process_icon_cache: BTreeMap::new(),
            cpu_tracker: Default::default(),
            temp_tracker: Default::default(),
            memory_tracker: Default::default(),
            gpu_trackers: BTreeMap::new(),
            disk_trackers: BTreeMap::new(),
        }
    }
    fn record(&mut self, sample: Sample) -> Vec<AlertTransition> {
        self.history.push_back(sample.clone());
        let cutoff=sample.timestamp.saturating_sub(MAX_HISTORY_SECONDS*1000);
        while self.history.front().is_some_and(|s|s.timestamp<cutoff){ self.history.pop_front(); }
        let mut out=Vec::new();
        let cpu=health(Some(sample.cpu_percent),&self.config.cpu);
        if let Some((from,to))=self.cpu_tracker.observe(cpu){ out.push(AlertTransition::new("cpu","CPU utilization",Some(sample.cpu_percent),from,to,self.config.cpu.clone())); }
        let temp=health(sample.temperature_c,&self.config.temperature);
        if let Some((from,to))=self.temp_tracker.observe(temp){ out.push(AlertTransition::new("temperature","CPU temperature",sample.temperature_c,from,to,self.config.temperature.clone())); }
        let mem=health(Some(sample.memory_percent),&self.config.memory);
        if let Some((from,to))=self.memory_tracker.observe(mem){ out.push(AlertTransition::new("memory","Memory utilization",Some(sample.memory_percent),from,to,self.config.memory.clone())); }
        for disk in &sample.disks {
            let label=disk.mount_point.trim_end_matches(|ch| ch=='\\' || ch=='/');
            let cfg=self.config.disks.entry(disk.id.clone()).or_insert_with(||disk_config(label)).clone();
            let h=health(Some(disk.usage_percent),&cfg.usage);
            let tracker=self.disk_trackers.entry(disk.id.clone()).or_default();
            if let Some((from,to))=tracker.observe(h){
                out.push(AlertTransition::new(
                    disk_metric_id(&disk.id),
                    format!("{} disk usage",if label.is_empty(){&disk.name}else{label}),
                    Some(disk.usage_percent),
                    from,
                    to,
                    cfg.usage,
                ));
            }
        }
        self.record_alert_transitions(&out);
        out
    }

    fn record_gpu(&mut self, gpus:Vec<GpuTelemetry>)->Vec<AlertTransition>{
        let timestamp=now_ms();
        self.gpu_latest=gpus.clone();
        self.gpu_history.push_back(GpuSnapshot{timestamp,gpus:gpus.clone()});
        let cutoff=timestamp.saturating_sub(MAX_HISTORY_SECONDS*1000);
        while self.gpu_history.front().is_some_and(|s|s.timestamp<cutoff){self.gpu_history.pop_front();}

        let mut out=Vec::new();
        for gpu in gpus {
            let cfg=self.config.gpus.entry(gpu.luid.clone()).or_insert_with(||gpu_config(gpu.index,&gpu.name)).clone();
            let dedicated_percent=percent(gpu.dedicated_used_bytes,gpu.dedicated_total_bytes);
            let shared_percent=percent(gpu.shared_used_bytes,gpu.shared_total_bytes);
            for (kind,title,value,metric_cfg) in [
                ("utilization",format!("GPU {} utilization",gpu.index),gpu.utilization_percent,cfg.utilization),
                ("dedicatedMemory",format!("GPU {} dedicated memory",gpu.index),dedicated_percent,cfg.dedicated_memory),
                ("sharedMemory",format!("GPU {} shared memory",gpu.index),shared_percent,cfg.shared_memory),
                ("temperature",format!("GPU {} temperature",gpu.index),gpu.temperature_c,cfg.temperature),
            ] {
                let h=health(value,&metric_cfg);
                let key=format!("{}:{kind}",gpu.luid);
                let tracker=self.gpu_trackers.entry(key.clone()).or_default();
                if let Some((from,to))=tracker.observe(h){
                    out.push(AlertTransition::new(gpu_metric_id(gpu.index,kind),title,value,from,to,metric_cfg));
                }
            }
        }
        self.record_alert_transitions(&out);
        out
    }
    fn record_alert_transitions(&self,transitions:&[AlertTransition]){
        if !self.config.alert_history_enabled{return;}
        for transition in transitions.iter().filter(|transition|transition.actionable()){
            self.alert_history.record(alert_history_event(transition));
        }
    }
}

#[derive(Clone)]
struct AlertTransition {
    metric: String,
    title: String,
    value: Option<f64>,
    from: Health,
    to: Health,
    config: MetricConfig,
}
impl AlertTransition {
    fn new(metric:impl Into<String>,title:impl Into<String>,value:Option<f64>,from:Health,to:Health,config:MetricConfig)->Self{Self{metric:metric.into(),title:title.into(),value,from,to,config}}
    fn actionable(&self)->bool{
        matches!(self.to,Health::Warning|Health::Critical)
            || (self.to==Health::Healthy && matches!(self.from,Health::Warning|Health::Critical))
    }
}
fn alert_history_event(transition:&AlertTransition)->AlertHistoryEvent{
    let status=match transition.to{Health::Warning=>"Warning",Health::Critical=>"Critical",Health::Healthy=>"Recovered",Health::Unavailable=>"Unavailable"};
    let threshold=match transition.to{
        Health::Warning=>format!("> {:.1}",transition.config.healthy_max),
        Health::Critical=>format!("≥ {:.1}",transition.config.critical_min),
        Health::Healthy=>format!("≤ {:.1}",transition.config.healthy_max),
        Health::Unavailable=>String::new(),
    };
    let unit=if transition.title.to_ascii_lowercase().contains("temperature"){"°C"}else{"%"};
    let value=transition.value.map(|v|format!("{v:.1}{unit}")).unwrap_or_else(||"—".into());
    let delivery=alert_delivery_for(&transition.config,transition.to);
    AlertHistoryEvent{
        id:format!("alert-{}-{}",now_ms(),std::process::id()),
        timestamp:now_ms(),
        metric:transition.metric.clone(),
        title:transition.title.clone(),
        status:status.into(),
        value,
        threshold,
        sound:delivery.sound,
        notification:delivery.notification,
        details:if status=="Recovered"{format!("Recovered from {}",health_name(transition.from))}else{format!("State changed from {} to {}",health_name(transition.from),health_name(transition.to))},
    }
}

fn read_frame(reader:&mut impl Read)->Result<Vec<u8>,String>{
    let mut header=[0u8;4]; reader.read_exact(&mut header).map_err(|e|e.to_string())?;
    let len=u32::from_le_bytes(header) as usize;
    if len==0||len>MAX_FRAME_BYTES{return Err("invalid frame length".into());}
    let mut payload=vec![0u8;len];reader.read_exact(&mut payload).map_err(|e|e.to_string())?;Ok(payload)
}
fn write_frame_raw(writer:&mut impl Write,value:&impl Serialize)->Result<(),String>{
    let payload=serde_json::to_vec(value).map_err(|e|e.to_string())?;
    if payload.len()>MAX_FRAME_BYTES{return Err("frame too large".into());}
    writer.write_all(&(payload.len() as u32).to_le_bytes()).and_then(|_|writer.write_all(&payload)).and_then(|_|writer.flush()).map_err(|e|e.to_string())
}
fn send_outbound<T:Serialize>(output:&mpsc::Sender<Value>,value:T)->Result<(),String>{
    output.send(serde_json::to_value(value).map_err(|e|e.to_string())?).map_err(|e|e.to_string())
}
fn host_request(input:&mut impl Read,output:&mpsc::Sender<Value>,request_id:&mut u64,method:&'static str,params:Value)->Result<Value,String>{
    let id=*request_id;*request_id+=1;
    send_outbound(output,PluginHostRequest{message_type:"plugin.request",request_id:id,method,params})?;
    loop{
        let payload=read_frame(input)?;
        if let Ok(response)=serde_json::from_slice::<HostResponse>(&payload){
            if response.request_id==id{return if response.ok{Ok(response.result)}else{Err(response.error)};}
        }
    }
}

fn pick_alert_sound(input:&mut impl Read,output:&mpsc::Sender<Value>,request_id:&mut u64)->Result<Value,String>{
    let picked=host_request(input,output,request_id,"file.open",json!({"filters":[{"name":"Alert sound","extensions":["mp3","wav"]}]}))?;
    if picked.get("opened")!=Some(&Value::Bool(true)){return Ok(json!({"selected":false}));}
    let path_text=picked.get("path").and_then(Value::as_str).ok_or_else(||"host file.open did not return a path".to_string())?;
    let path=Path::new(path_text);
    let metadata=fs::metadata(path).map_err(|e|format!("cannot inspect alert sound: {e}"))?;
    if !metadata.is_file(){return Err("selected sound is not a regular file".to_string());}
    if metadata.len()==0||metadata.len()>2*1024*1024{return Err("Sound must be between 1 byte and 2 MiB.".to_string());}
    let ext=path.extension().and_then(|v|v.to_str()).unwrap_or("").to_ascii_lowercase();
    let mime=match ext.as_str(){"mp3"=>"audio/mpeg","wav"=>"audio/wav",_=>return Err("Select an MP3 or WAV file.".to_string())};
    let bytes=fs::read(path).map_err(|e|format!("cannot read alert sound: {e}"))?;
    let filename=path.file_name().and_then(|v|v.to_str()).unwrap_or("alert-sound").to_string();
    Ok(json!({"selected":true,"filename":filename,"mime":mime,"dataUrl":format!("data:{mime};base64,{}",BASE64_STANDARD.encode(bytes)),"sizeBytes":metadata.len()}))
}

fn load_config(input:&mut impl Read,output:&mpsc::Sender<Value>,request_id:&mut u64)->Result<Config,String>{
    let result=host_request(input,output,request_id,"storage.get",json!({"key":STORAGE_KEY}))?;
    match result.get("value"){
        None|Some(Value::Null)=>Ok(Config::default()),
        Some(v)=>normalize_config(serde_json::from_value(v.clone()).map_err(|e|format!("invalid stored config: {e}"))?)
    }
}
fn save_config(input:&mut impl Read,output:&mpsc::Sender<Value>,request_id:&mut u64,config:&Config)->Result<(),String>{
    host_request(input,output,request_id,"storage.set",json!({"key":STORAGE_KEY,"value":config}))?;Ok(())
}
fn load_alert_history(input:&mut impl Read,output:&mpsc::Sender<Value>,request_id:&mut u64)->Result<Vec<AlertHistoryEvent>,String>{
    let result=host_request(input,output,request_id,"storage.get",json!({"key":ALERT_HISTORY_STORAGE_KEY}))?;
    match result.get("value"){None|Some(Value::Null)=>Ok(Vec::new()),Some(value)=>serde_json::from_value(value.clone()).map_err(|e|format!("invalid stored System Monitor alert history: {e}"))}
}
fn save_alert_history(input:&mut impl Read,output:&mpsc::Sender<Value>,request_id:&mut u64,history:&AlertHistoryState)->Result<(),String>{
    if !history.dirty.load(Ordering::Acquire){return Ok(());}
    let events=history.snapshot();
    host_request(input,output,request_id,"storage.set",json!({"key":ALERT_HISTORY_STORAGE_KEY,"value":events}))?;
    history.dirty.store(false,Ordering::Release);
    Ok(())
}
fn alert_history_text(event:&AlertHistoryEvent)->String{
    format!("{} {} {} {} {} {}",event.metric,event.title,event.status,event.value,event.threshold,event.details).to_ascii_lowercase()
}
fn alert_history_rows(events:&[AlertHistoryEvent],params:&Value)->Vec<AlertHistoryEvent>{
    let search=params.get("search").and_then(Value::as_str).unwrap_or("").trim().to_ascii_lowercase();
    let metric=params.get("metric").and_then(Value::as_str).unwrap_or("");
    let status=params.get("status").and_then(Value::as_str).unwrap_or("");
    let from=params.get("from").and_then(Value::as_u64).unwrap_or(0);
    let to=params.get("to").and_then(Value::as_u64).unwrap_or(u64::MAX);
    let mut rows=events.iter().filter(|event|{
        (search.is_empty()||alert_history_text(event).contains(&search))
        &&(metric.is_empty()||event.metric==metric)
        &&(status.is_empty()||event.status==status)
        &&event.timestamp>=from&&event.timestamp<=to
    }).cloned().collect::<Vec<_>>();
    let sort_by=params.get("sortBy").and_then(Value::as_str).unwrap_or("timestamp");
    let descending=params.get("sortDir").and_then(Value::as_str).unwrap_or("desc")!="asc";
    rows.sort_by(|a,b|{
        let order=match sort_by{
            "metric"=>a.title.cmp(&b.title),
            "status"=>a.status.cmp(&b.status),
            _=>a.timestamp.cmp(&b.timestamp),
        };
        if descending{order.reverse()}else{order}
    });
    rows
}
fn alert_history_query(events:&[AlertHistoryEvent],params:&Value)->Value{
    let rows=alert_history_rows(events,params);
    let mut metrics=events.iter().map(|event|(event.metric.clone(),event.title.clone())).collect::<Vec<_>>();
    metrics.sort_by(|a,b|a.1.cmp(&b.1));metrics.dedup_by(|a,b|a.0==b.0);
    let mut statuses=events.iter().map(|event|event.status.clone()).collect::<Vec<_>>();statuses.sort();statuses.dedup();
    let total=rows.len();
    let page_size=params.get("pageSize").and_then(Value::as_u64).unwrap_or(25).clamp(1,100) as usize;
    let page=params.get("page").and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
    let start=(page-1).saturating_mul(page_size).min(total);
    let end=(start+page_size).min(total);
    json!({"rows":rows[start..end].to_vec(),"total":total,"page":page,"pageSize":page_size,"facets":{"metrics":metrics.into_iter().map(|(id,name)|json!({"id":id,"name":name})).collect::<Vec<_>>(),"statuses":statuses}})
}
fn csv_cell(value:&str)->String{
    if value.contains(',')||value.contains('"')||value.contains('\r')||value.contains('\n'){format!("\"{}\"",value.replace('"',"\"\""))}else{value.to_string()}
}
fn alert_history_csv(events:&[AlertHistoryEvent],params:&Value)->String{
    let rows=alert_history_rows(events,params);
    let mut out=String::from("timestamp_ms,metric,event,value,threshold,sound,notification,details\n");
    for event in rows{
        let fields=[
            event.timestamp.to_string(),
            event.title,
            event.status,
            event.value,
            event.threshold,
            if event.sound{"Yes".into()}else{"No".into()},
            if event.notification{"Yes".into()}else{"No".into()},
            event.details,
        ];
        out.push_str(&fields.iter().map(|v|csv_cell(v)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

fn recent_series(history:&VecDeque<Sample>,selector:fn(&Sample)->Option<f64>,seconds:u64,cfg:&MetricConfig)->Vec<Value>{
    let cutoff=now_ms().saturating_sub(seconds*1000);
    history.iter().filter(|s|s.timestamp>=cutoff).filter_map(|s|{
        selector(s).map(|v|{
            let h=health(Some(v),cfg);
            json!({"value":v,"tone":health_name(h),"color":health_color(h,cfg)})
        })
    }).collect()
}
fn gpu_value(gpu:&GpuTelemetry,kind:&str)->Option<f64>{
    match kind{
        "utilization"=>gpu.utilization_percent,
        "dedicatedMemory"=>percent(gpu.dedicated_used_bytes,gpu.dedicated_total_bytes),
        "sharedMemory"=>percent(gpu.shared_used_bytes,gpu.shared_total_bytes),
        "temperature"=>gpu.temperature_c,
        _=>None,
    }
}
fn recent_disk_series(history:&VecDeque<Sample>,disk_id:&str,limit:usize,cfg:&MetricConfig)->Vec<Value>{
    history.iter().rev().take(limit).collect::<Vec<_>>().into_iter().rev().filter_map(|s|{
        s.disks.iter().find(|d|d.id==disk_id).map(|d|{
            let h=health(Some(d.usage_percent),cfg);
            json!({"value":d.usage_percent,"color":health_color(h,cfg)})
        })
    }).collect()
}
fn recent_gpu_series(history:&VecDeque<GpuSnapshot>,luid:&str,kind:&str,seconds:u64,cfg:&MetricConfig)->Vec<Value>{
    let cutoff=now_ms().saturating_sub(seconds*1000);
    history.iter().filter(|s|s.timestamp>=cutoff).filter_map(|s|{
        let gpu=s.gpus.iter().find(|g|g.luid==luid)?;
        let v=gpu_value(gpu,kind)?;
        let h=health(Some(v),cfg);
        Some(json!({"value":v,"tone":health_name(h),"color":health_color(h,cfg)}))
    }).collect()
}
fn gpu_telemetry_present(gpu:&GpuTelemetry)->bool{
    gpu.utilization_percent.is_some()
        || gpu.dedicated_used_bytes.is_some()
        || gpu.shared_used_bytes.is_some()
        || gpu.temperature_c.is_some()
        || !gpu.engines.is_empty()
}

fn gpu_identity(adapter:&GpuAdapterInfo)->String{
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        adapter.name.to_ascii_lowercase(),
        adapter.vendor_id,
        adapter.device_id,
        adapter.subsystem_id,
        adapter.revision,
        adapter.dedicated_video_memory_bytes,
        adapter.dedicated_system_memory_bytes,
        adapter.shared_system_memory_bytes,
    )
}

fn visible_gpu_luids(adapters:&[GpuAdapterInfo],latest:&[GpuTelemetry])->HashSet<String>{
    let mut groups:BTreeMap<String,Vec<&GpuAdapterInfo>>=BTreeMap::new();
    for adapter in adapters{
        groups.entry(gpu_identity(adapter)).or_default().push(adapter);
    }

    let mut visible=HashSet::new();
    for group in groups.values(){
        if group.len()==1{
            visible.insert(group[0].luid.clone());
            continue;
        }

        let active:Vec<&GpuAdapterInfo>=group.iter().copied().filter(|adapter|{
            latest.iter().find(|gpu|gpu.luid==adapter.luid).is_some_and(gpu_telemetry_present)
        }).collect();

        if active.len()==1{
            visible.insert(active[0].luid.clone());
        }else{
            for adapter in group{
                visible.insert(adapter.luid.clone());
            }
        }
    }
    visible
}

fn topbar_alert_color<'a>(h:Health,cfg:&'a MetricConfig)->Option<&'a str>{
    match h{
        Health::Warning=>Some(&cfg.warning_color),
        Health::Critical=>Some(&cfg.critical_color),
        _=>None,
    }
}
fn metric_topbar(id:&str,default_title:&str,value:Option<f64>,unit:&str,progress_percent:Option<f64>,series:Vec<Value>,h:Health,cfg:&MetricConfig,default_visualization:&str)->Value{
    let shown=value.map(|v|format!("{:.0}{unit}",v)).unwrap_or_else(||"—".into());
    let title=cfg.topbar_title.as_deref().filter(|v|!v.trim().is_empty()).unwrap_or(default_title);
    let visualization=cfg.topbar_visualization.as_deref().unwrap_or(default_visualization);
    let state_color=health_color(h,cfg);
    let value_normal=cfg.topbar_value_color.as_deref().unwrap_or("#dce4ee");
    let value_alert=cfg.topbar_value_alert_color.as_deref().unwrap_or("#efaaaa");
    let value_color=if matches!(h,Health::Warning|Health::Critical){value_alert}else{value_normal};
    let title_color=cfg.topbar_title_color.as_deref().unwrap_or("#a8b4c2");
    let alert_color=topbar_alert_color(h,cfg);
    let visual=if visualization=="bar"{
        json!({"type":"progress","label":"","value":progress_percent.unwrap_or(0.0),"color":state_color})
    }else{
        json!({"type":"sparkline","series":series,"color":state_color})
    };
    json!({
        "id":format!("system-monitor:{id}"),"title":title,
        "tooltip":format!("{title}: {shown} · {}",health_name(h)),
        "icon":alert_color.map(|_|"alert"),
        "content":{
            "type":"stack",
            "color":alert_color,
            "backgroundColor":cfg.topbar_background_color,
            "borderColor":cfg.topbar_border_color,
            "children":[
                {"type":"text","label":title,"fontSize":9,"color":title_color},
                {"type":"metric","value":shown,"fontSize":14,"color":value_color},
                visual
            ]
        },
        "action":{"type":"open-panel","panel":"system-monitor","context":{"metric":id}}
    })
}
fn surfaces(store:&Store)->(Value,Value){
    let latest=store.history.back();
    let cpu_value=latest.map(|s|s.cpu_percent);
    let mem_value=latest.map(|s|s.memory_percent);
    let cpu_h=health(cpu_value,&store.config.cpu);let mem_h=health(mem_value,&store.config.memory);
    let mut top=Vec::new();
    let mut tray=vec![
        tray_item("cpu","CPU utilization",cpu_value,"%",cpu_h,&store.config.cpu),
        tray_item("memory","Memory utilization",mem_value,"%",mem_h,&store.config.memory),
    ];
    if store.config.cpu.show_topbar{top.push(metric_topbar("cpu","CPU",cpu_value,"%",cpu_value,recent_series(&store.history,|s|Some(s.cpu_percent),60,&store.config.cpu),cpu_h,&store.config.cpu,"graph"));}
    if store.config.memory.show_topbar{top.push(metric_topbar("memory","Memory",mem_value,"%",mem_value,recent_series(&store.history,|s|Some(s.memory_percent),60,&store.config.memory),mem_h,&store.config.memory,"bar"));}

    if let Some(sample)=latest {
        for disk in &sample.disks {
            let Some(cfg)=store.config.disks.get(&disk.id) else {continue};
            let label=disk.mount_point.trim_end_matches(|ch| ch=='\\' || ch=='/');
            let title=format!("{} Disk",if label.is_empty(){&disk.name}else{label});
            let value=Some(disk.usage_percent);
            let h=health(value,&cfg.usage);
            let id=disk_metric_id(&disk.id);
            if cfg.usage.show_topbar {
                top.push(metric_topbar(
                    &id,&title,value,"%",value,
                    recent_disk_series(&store.history,&disk.id,60,&cfg.usage),
                    h,&cfg.usage,"bar"
                ));
            }
            tray.push(tray_item(&id,&title,value,"%",h,&cfg.usage));
        }
    }

    let visible_gpu_luids=visible_gpu_luids(&store.gpu_adapters,&store.gpu_latest);
    for gpu in &store.gpu_latest {
        if !visible_gpu_luids.contains(&gpu.luid){continue;}
        let Some(cfg)=store.config.gpus.get(&gpu.luid) else {continue};
        let mut metrics=vec![
            ("utilization","%",gpu.utilization_percent,&cfg.utilization,"graph"),
            ("dedicatedMemory","%",percent(gpu.dedicated_used_bytes,gpu.dedicated_total_bytes),&cfg.dedicated_memory,"bar"),
            ("sharedMemory","%",percent(gpu.shared_used_bytes,gpu.shared_total_bytes),&cfg.shared_memory,"bar"),
        ];
        if gpu.temperature_c.is_some(){metrics.push(("temperature","°C",gpu.temperature_c,&cfg.temperature,"graph"));}
        for (kind,unit,value,metric_cfg,default_visual) in metrics {
            let h=health(value,metric_cfg);
            let id=gpu_metric_id(gpu.index,kind);
            let default_title=match kind{
                "utilization"=>format!("GPU {}",gpu.index),
                "dedicatedMemory"=>format!("GPU {} VRAM",gpu.index),
                "sharedMemory"=>format!("GPU {} Shared",gpu.index),
                _=>format!("GPU {} Temp",gpu.index),
            };
            if metric_cfg.show_topbar{
                top.push(metric_topbar(&id,&default_title,value,unit,value,recent_gpu_series(&store.gpu_history,&gpu.luid,kind,60,metric_cfg),h,metric_cfg,default_visual));
            }
            tray.push(tray_item(&id,&default_title,value,unit,h,metric_cfg));
        }
    }
    (json!({"items":top}),json!({"items":tray}))
}
fn tray_item(id:&str,title:&str,value:Option<f64>,unit:&str,h:Health,cfg:&MetricConfig)->Value{
    let label=value.map(|v|format!("{:.0}",v)).unwrap_or_else(||"--".into());
    let tooltip=value.map(|v|format!("{title}: {:.1}{unit} · {}",v,health_name(h))).unwrap_or_else(||format!("{title}: unavailable"));
    json!({
        "id":format!("system-monitor-{id}"),"title":title,"tooltip":tooltip,"tone":health_name(h),
        "visible":cfg.show_tray,"label":label,"textColor":cfg.text_color,
        "backgroundColor":health_color(h,cfg),"borderColor":cfg.border_color,"series":[],
        "action":{"type":"open-panel","panel":"system-monitor","context":{"metric":id}}
    })
}
fn emit_surfaces(output:&mpsc::Sender<Value>,store:&Store)->Result<(),String>{
    let (top,tray)=surfaces(store);
    send_outbound(output,PluginEvent{message_type:"plugin.event",event:"topbar.update",payload:top})?;
    send_outbound(output,PluginEvent{message_type:"plugin.event",event:"tray.update",payload:tray})
}
// These IDs correlate dashboard devices and alert items without exposing mount
// paths or Windows GPU LUIDs. GPU indices are scoped to current enumeration.
fn api_disk_id(disk: &DiskTelemetry) -> String {
    // Non-cryptographic fingerprint for an opaque local dashboard key.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in disk.id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("disk-{hash:016x}")
}

fn api_disk_label(disk: &DiskTelemetry) -> String {
    let name = disk.name.trim();
    // Expose only root drive-letter labels on Windows, not filesystem paths.
    let mount = disk.mount_point.as_bytes();
    let drive = if mount.len() >= 2 && mount[0].is_ascii_alphabetic() && mount[1] == b':' {
        Some(format!("{}:", (mount[0] as char).to_ascii_uppercase()))
    } else { None };
    match (drive, name.is_empty()) {
        (Some(drive), false) if name != drive => format!("{drive} · {name}"),
        (Some(drive), _) => drive,
        (None, false) => name.to_string(),
        (None, true) => "Disk".to_string(),
    }
}

fn api_catalog() -> Value {
    // Stable field names and raw units; no locale-dependent formatted numbers.
    json!({
        "schemaVersion": 1,
        "timestampUnit": "unixMilliseconds",
        "nullMeaning": "unavailable",
        "deviceIdScope": "per-host; GPU IDs follow enumeration and may change when adapters change",
        "metrics": [
            {"key":"cpuPercent","label":"CPU Usage","unit":"percent","source":"metrics","kind":"gauge","min":0,"max":100},
            {"key":"corePercent","label":"CPU Core Usage","unit":"percent","source":"metrics","kind":"series","min":0,"max":100},
            {"key":"memoryPercent","label":"Memory Usage","unit":"percent","source":"metrics","kind":"gauge","min":0,"max":100},
            {"key":"memoryUsedBytes","label":"Used Memory","unit":"bytes","source":"metrics","kind":"number"},
            {"key":"memoryAvailableBytes","label":"Available Memory","unit":"bytes","source":"metrics","kind":"number"},
            {"key":"memoryTotalBytes","label":"Total Memory","unit":"bytes","source":"metrics","kind":"number"},
            {"key":"temperatureC","label":"CPU Temperature","unit":"celsius","source":"metrics","kind":"gauge"},
            {"key":"utilizationPercent","label":"GPU Usage","unit":"percent","source":"gpus.items","deviceType":"gpu","kind":"gauge","min":0,"max":100},
            {"key":"dedicatedUsedBytes","label":"Dedicated GPU Memory Used","unit":"bytes","source":"gpus.items","deviceType":"gpu","kind":"number"},
            {"key":"dedicatedTotalBytes","label":"Dedicated GPU Memory Total","unit":"bytes","source":"gpus.items","deviceType":"gpu","kind":"number"},
            {"key":"sharedUsedBytes","label":"Shared GPU Memory Used","unit":"bytes","source":"gpus.items","deviceType":"gpu","kind":"number"},
            {"key":"sharedTotalBytes","label":"Shared GPU Memory Total","unit":"bytes","source":"gpus.items","deviceType":"gpu","kind":"number"},
            {"key":"temperatureC","label":"GPU Temperature","unit":"celsius","source":"gpus.items","deviceType":"gpu","kind":"gauge"},
            {"key":"usagePercent","label":"Disk Usage","unit":"percent","source":"disks.items","deviceType":"disk","kind":"gauge","min":0,"max":100},
            {"key":"usedBytes","label":"Used Disk Capacity","unit":"bytes","source":"disks.items","deviceType":"disk","kind":"number"},
            {"key":"freeBytes","label":"Free Disk Capacity","unit":"bytes","source":"disks.items","deviceType":"disk","kind":"number"},
            {"key":"totalBytes","label":"Total Disk Capacity","unit":"bytes","source":"disks.items","deviceType":"disk","kind":"number"},
            {"key":"readBytesPerSecond","label":"Disk Read Rate","unit":"bytesPerSecond","source":"disks.items","deviceType":"disk","kind":"number"},
            {"key":"writeBytesPerSecond","label":"Disk Write Rate","unit":"bytesPerSecond","source":"disks.items","deviceType":"disk","kind":"number"}
        ]
    })
}

// Explicit public API projection. Never send status_value() to HTTP clients:
// it contains private configuration, hardware identity and unbounded history.
fn api_active_alert(metric: &str, tracker: &TransitionTracker) -> Option<Value> {
    match tracker.committed {
        Health::Warning | Health::Critical => Some(json!({
            "metric": metric, "state": health_name(tracker.committed)
        })),
        Health::Healthy | Health::Unavailable => None,
    }
}

fn api_read_value(store: &Store, view: &str) -> Option<Value> {
    let latest = store.history.back();
    Some(match view {
        "summary" => json!({
            "sampleTimestamp": latest.map(|s| s.timestamp),
            "cpuPercent": latest.map(|s| s.cpu_percent),
            "memoryPercent": latest.map(|s| s.memory_percent),
            "temperatureC": latest.and_then(|s| s.temperature_c),
            "gpuCount": store.gpu_latest.len(),
            "diskCount": latest.map_or(0, |s| s.disks.len())
        }),
        "metrics" => json!({
            "timestamp": latest.map(|s| s.timestamp),
            "cpuPercent": latest.map(|s| s.cpu_percent),
            "corePercent": latest.map(|s| &s.core_percent),
            "temperatureC": latest.and_then(|s| s.temperature_c),
            "memoryPercent": latest.map(|s| s.memory_percent),
            "memoryUsedBytes": latest.map(|s| s.memory_used_bytes),
            "memoryAvailableBytes": latest.map(|s| s.memory_available_bytes),
            "memoryTotalBytes": latest.map(|s| s.memory_total_bytes)
        }),
        "gpus" => json!({
            "timestamp": store.gpu_history.back().map(|s| s.timestamp),
            "items": store.gpu_latest.iter().map(|g| json!({
                "index": g.index,
                "id": format!("gpu-{}", g.index),
                "name": g.name,
                "displayName": format!("GPU {} · {}", g.index, g.name),
                "utilizationPercent": g.utilization_percent,
                "dedicatedUsedBytes": g.dedicated_used_bytes,
                "dedicatedTotalBytes": g.dedicated_total_bytes,
                "sharedUsedBytes": g.shared_used_bytes,
                "sharedTotalBytes": g.shared_total_bytes,
                "temperatureC": g.temperature_c
            })).collect::<Vec<_>>()
        }),
        "disks" => json!({
            "timestamp": latest.map(|s| s.timestamp),
            // Add recognizable display metadata without exposing full mount paths,
            // raw device identifiers, filesystem details or serial numbers.
            "items": latest.map(|s| s.disks.iter().enumerate().map(|(index,d)| json!({
                "index": index,
                "id": api_disk_id(d),
                "name": api_disk_label(d),
                "displayName": api_disk_label(d),
                "totalBytes": d.total_bytes,
                "usedBytes": d.used_bytes,
                "freeBytes": d.free_bytes,
                "usagePercent": d.usage_percent,
                "readBytesPerSecond": d.read_bytes_per_second,
                "writeBytesPerSecond": d.write_bytes_per_second
            })).collect::<Vec<_>>()).unwrap_or_default()
        }),
        "history" => json!({
            // At most 120 sampled points; excludes per-process and device identity data.
            "items": store.history.iter()
                .skip(store.history.len().saturating_sub(120))
                .map(|s| json!({
                    "timestamp": s.timestamp,
                    "cpuPercent": s.cpu_percent,
                    "memoryPercent": s.memory_percent,
                    "temperatureC": s.temperature_c
                })).collect::<Vec<_>>()
        }),
        "catalog" => api_catalog(),
        "alerts" => {
            let mut items = Vec::new();
            for (metric, tracker) in [
                ("cpu", &store.cpu_tracker),
                ("temperature", &store.temp_tracker),
                ("memory", &store.memory_tracker)
            ] {
                if let Some(alert) = api_active_alert(metric, tracker) { items.push(alert); }
            }
            // Link live alert trackers to their device records using the same IDs
            // exposed in /gpus and /disks. Never expose GPU LUIDs or raw mount paths.
            for gpu in &store.gpu_latest {
                for (kind, key, label) in [
                    ("utilization", "GPU Usage", "percent"),
                    ("dedicatedMemory", "Dedicated GPU Memory", "percent"),
                    ("sharedMemory", "Shared GPU Memory", "percent"),
                    ("temperature", "GPU Temperature", "celsius")
                ] {
                    let tracker_key = format!("{}:{kind}", gpu.luid);
                    if let Some(tracker) = store.gpu_trackers.get(&tracker_key) {
                        if let Some(mut alert) = api_active_alert(&gpu_metric_id(gpu.index,kind), tracker) {
                            alert["deviceId"] = json!(format!("gpu-{}",gpu.index));
                            alert["name"] = json!(format!("GPU {} · {} — {key}",gpu.index,gpu.name));
                            alert["unit"] = json!(label);
                            items.push(alert);
                        }
                    }
                }
            }
            if let Some(sample) = latest {
                for disk in &sample.disks {
                    if let Some(tracker) = store.disk_trackers.get(&disk.id) {
                        if let Some(mut alert) = api_active_alert(&disk_metric_id(&disk.id),tracker) {
                            alert["deviceId"] = json!(api_disk_id(disk));
                            alert["name"] = json!(format!("{} — Disk Usage",api_disk_label(disk)));
                            alert["unit"] = json!("percent");
                            items.push(alert);
                        }
                    }
                }
            }
            json!({"items": items})
        },
        _ => return None,
    })
}

fn status_value(store:&Store)->Value{
    json!({
        "config":&store.config,
        "latest":store.history.back(),
        "history":&store.history,
        "gpu":{
            "state":store.gpu_discovery_state,
            "error":store.gpu_discovery_error,
            "adapters":store.gpu_adapters,
            "latest":store.gpu_latest,
            "history":store.gpu_history
        }
    })
}
fn emit_alert(output:&mpsc::Sender<Value>,transition:&AlertTransition){
    if !transition.actionable(){return;}
    let state=health_name(transition.to);
    let delivery=alert_delivery_for(&transition.config,transition.to);
    let value=transition.value.map(|v|format!("{v:.1}")).unwrap_or_else(||"unavailable".into());
    let payload=json!({"metric":transition.metric,"title":transition.title,"value":transition.value,"state":state,"from":health_name(transition.from),"sound":delivery.sound,"soundFile":delivery.sound_file,"notification":delivery.notification});
    let _=send_outbound(output,PluginEvent{message_type:"plugin.event",event:"system.alert",payload});
    if delivery.notification{
        let label=match transition.to{Health::Critical=>"Critical",Health::Warning=>"Warning",Health::Healthy=>"Recovered",Health::Unavailable=>"Unavailable"};
        let tone=match transition.to{Health::Critical=>"danger",Health::Warning=>"warning",Health::Healthy=>"success",Health::Unavailable=>"info"};
        let _=send_outbound(output,PluginEvent{message_type:"plugin.event",event:"notification.show",payload:json!({
            "itemId":format!("system-monitor-{}",transition.metric),
            "title":format!("{} · {}",transition.title,label),
            "body":format!("Current value: {value}"),
            "tone":tone,
            "silent":true,
            "action":{"type":"open-panel","panel":"system-monitor","context":{"metric":transition.metric}}
        })});
    }
}
fn csv_for(history:&VecDeque<Sample>,gpu_history:&VecDeque<GpuSnapshot>,minutes:u64)->String{
    let cutoff=now_ms().saturating_sub(minutes.clamp(1,60)*60*1000);
    let max_cores=history.iter().map(|s|s.core_percent.len()).max().unwrap_or(0);
    let adapters:Vec<GpuTelemetry>=gpu_history.back().map(|s|s.gpus.clone()).unwrap_or_default();
    let disks:Vec<DiskTelemetry>=history.back().map(|s|s.disks.clone()).unwrap_or_default();
    let mut out=String::from("timestamp_ms,cpu_percent,memory_percent,memory_used_bytes,memory_available_bytes,memory_total_bytes");
    for i in 0..max_cores{out.push_str(&format!(",core_{i}_percent"));}
    for gpu in &adapters{
        out.push_str(&format!(",gpu_{}_utilization_percent,gpu_{}_dedicated_used_bytes,gpu_{}_shared_used_bytes,gpu_{}_temperature_c",gpu.index,gpu.index,gpu.index,gpu.index));
    }
    for disk in &disks{
        let key=csv_disk_key(&disk.id);
        out.push_str(&format!(
            ",disk_{key}_usage_percent,disk_{key}_used_bytes,disk_{key}_free_bytes,disk_{key}_total_bytes,disk_{key}_read_bytes_per_second,disk_{key}_write_bytes_per_second"
        ));
    }
    out.push('\n');
    for s in history.iter().filter(|s|s.timestamp>=cutoff){
        out.push_str(&format!("{},{:.2},",s.timestamp,s.cpu_percent));
        out.push_str(&format!("{:.2},{},{},{}",s.memory_percent,s.memory_used_bytes,s.memory_available_bytes,s.memory_total_bytes));
        for i in 0..max_cores{out.push(',');if let Some(v)=s.core_percent.get(i){out.push_str(&format!("{v:.2}"));}}
        let gpu_sample=gpu_history.iter().rev().find(|g|g.timestamp<=s.timestamp);
        for adapter in &adapters{
            let gpu=gpu_sample.and_then(|g|g.gpus.iter().find(|x|x.luid==adapter.luid));
            out.push(',');
            if let Some(v)=gpu.and_then(|g|g.utilization_percent){out.push_str(&format!("{v:.2}"));}
            out.push(',');
            if let Some(v)=gpu.and_then(|g|g.dedicated_used_bytes){out.push_str(&v.to_string());}
            out.push(',');
            if let Some(v)=gpu.and_then(|g|g.shared_used_bytes){out.push_str(&v.to_string());}
            out.push(',');
            if let Some(v)=gpu.and_then(|g|g.temperature_c){out.push_str(&format!("{v:.1}"));}
        }
        for disk_def in &disks{
            let disk=s.disks.iter().find(|d|d.id==disk_def.id);
            out.push(',');
            if let Some(d)=disk{out.push_str(&format!("{:.2}",d.usage_percent));}
            out.push(',');
            if let Some(d)=disk{out.push_str(&d.used_bytes.to_string());}
            out.push(',');
            if let Some(d)=disk{out.push_str(&d.free_bytes.to_string());}
            out.push(',');
            if let Some(d)=disk{out.push_str(&d.total_bytes.to_string());}
            out.push(',');
            if let Some(d)=disk{out.push_str(&format!("{:.2}",d.read_bytes_per_second));}
            out.push(',');
            if let Some(d)=disk{out.push_str(&format!("{:.2}",d.write_bytes_per_second));}
        }
        out.push('\n');
    }
    out
}

fn main()->Result<(),String>{
    let plugin_id=std::env::var("REMOTEHAND_PLUGIN_ID").map_err(|_|"REMOTEHAND_PLUGIN_ID is missing".to_string())?;
    let api_version:u32=std::env::var("REMOTEHAND_PLUGIN_API_VERSION").map_err(|_|"REMOTEHAND_PLUGIN_API_VERSION is missing".to_string())?.parse().map_err(|e|format!("invalid API version: {e}"))?;
    let stdin=io::stdin();let mut input=stdin.lock();let stdout=io::stdout();let mut handshake_output=stdout.lock();
    write_frame_raw(&mut handshake_output,&PluginHello{message_type:"plugin.hello",plugin_id:&plugin_id,api_version,plugin_version:env!("CARGO_PKG_VERSION")})?;
    let ready:HostReady=serde_json::from_slice(&read_frame(&mut input)?).map_err(|e|format!("invalid host.ready: {e}"))?;
    if ready.message_type!="host.ready"||ready.plugin_id!=plugin_id||ready.api_version!=api_version{return Err("host.ready mismatch".into());}
    drop(handshake_output);

    let (output_tx,output_rx)=mpsc::channel::<Value>();
    let _writer=thread::spawn(move||{let stdout=io::stdout();let mut output=stdout.lock();while let Ok(value)=output_rx.recv(){write_frame_raw(&mut output,&value)?;}Ok::<(),String>(())});
    let mut host_request_id=1u64;
    let config=load_config(&mut input,&output_tx,&mut host_request_id).unwrap_or_default();
    let stored_alert_history=load_alert_history(&mut input,&output_tx,&mut host_request_id).unwrap_or_default();
    let store=Arc::new(Mutex::new(Store::new()));
    {
        let mut guard=store.lock().map_err(|_|"state lock poisoned".to_string())?;
        guard.config=config;
        guard.alert_history.replace(stored_alert_history);
    }

    // GPU discovery/telemetry stays entirely off the startup/handshake path.
    {
        let gpu_store=Arc::clone(&store);
        let gpu_output=output_tx.clone();
        thread::spawn(move||{
            let adapters=match gpu_windows::enumerate_gpus(){
                Ok(adapters)=>adapters,
                Err(error)=>{
                    if let Ok(mut guard)=gpu_store.lock(){
                        guard.gpu_discovery_state="error".into();
                        guard.gpu_discovery_error=Some(error.clone());
                    }
                    let _=send_outbound(&gpu_output,PluginEvent{message_type:"plugin.event",event:"system.gpu.discovered",payload:json!({"state":"error","error":error,"adapters":[]})});
                    return;
                }
            };

            if let Ok(mut guard)=gpu_store.lock(){
                guard.gpu_adapters=adapters.clone();
                guard.gpu_discovery_state="ready".into();
                guard.gpu_discovery_error=None;
                for adapter in &adapters{
                    let cfg=guard.config.gpus.entry(adapter.luid.clone()).or_insert_with(||gpu_config(adapter.index,&adapter.name));
                    let old_util=format!("GPU {}",adapter.index);
                    let old_vram=format!("GPU {} VRAM",adapter.index);
                    let old_shared=format!("GPU {} Shared",adapter.index);
                    let old_temp=format!("GPU {} Temp",adapter.index);
                    if cfg.utilization.topbar_title.as_deref()==Some(old_util.as_str()){cfg.utilization.topbar_title=Some(adapter.name.clone());}
                    if cfg.dedicated_memory.topbar_title.as_deref()==Some(old_vram.as_str()){cfg.dedicated_memory.topbar_title=Some(format!("{} VRAM",adapter.name));}
                    if cfg.shared_memory.topbar_title.as_deref()==Some(old_shared.as_str()){cfg.shared_memory.topbar_title=Some(format!("{} Shared",adapter.name));}
                    if cfg.temperature.topbar_title.as_deref()==Some(old_temp.as_str()){cfg.temperature.topbar_title=Some(format!("{} Temp",adapter.name));}
                }
            }
            let _=send_outbound(&gpu_output,PluginEvent{
                message_type:"plugin.event",
                event:"system.gpu.discovered",
                payload:json!({"state":"ready","error":null,"adapters":adapters})
            });

            if adapters.is_empty(){return;}
            let sampler=GpuSampler::new(adapters);
            GpuSampler::prime_delay();
            loop{
                let telemetry=sampler.sample();
                let transitions={
                    let mut guard=match gpu_store.lock(){Ok(g)=>g,Err(_)=>return};
                    let transitions=guard.record_gpu(telemetry.clone());
                    let _=emit_surfaces(&gpu_output,&guard);
                    transitions
                };
                let _=send_outbound(&gpu_output,PluginEvent{
                    message_type:"plugin.event",
                    event:"system.gpu.sampled",
                    payload:json!({"timestamp":now_ms(),"gpus":telemetry})
                });
                for t in transitions{emit_alert(&gpu_output,&t);}
                let seconds=match gpu_store.lock(){Ok(g)=>g.config.sample_seconds,Err(_)=>return};
                thread::sleep(Duration::from_secs(seconds));
            }
        });
    }

    {
        let sampler_store=Arc::clone(&store);let sampler_output=output_tx.clone();
        thread::spawn(move||{
            let mut provider=WindowsSystemProvider::new();
            thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            loop{
                let sample=provider.sample();
                let transitions={
                    let mut guard=match sampler_store.lock(){Ok(g)=>g,Err(_)=>return};
                    let transitions=guard.record(sample.clone());
                    let _=emit_surfaces(&sampler_output,&guard);
                    transitions
                };
                let _=send_outbound(&sampler_output,PluginEvent{message_type:"plugin.event",event:"system.sampled",payload:serde_json::to_value(sample).unwrap_or(Value::Null)});
                for t in transitions{emit_alert(&sampler_output,&t);}
                let seconds=match sampler_store.lock(){Ok(g)=>g.config.sample_seconds,Err(_)=>return};
                thread::sleep(Duration::from_secs(seconds));
            }
        });
    }

    {
        let process_store=Arc::clone(&store);
        let process_output=output_tx.clone();
        thread::spawn(move||{
            let mut system=System::new_all();
            let mut users=Users::new_with_refreshed_list();
            thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            loop{
                system.refresh_processes(ProcessesToUpdate::All,true);
                users.refresh();
                let visible=process_windows::visible_process_ids();
                let snapshots=process_snapshot(&system,&users,&visible);
                let count=snapshots.len();
                let updated_at=now_ms();
                if let Ok(mut guard)=process_store.lock(){
                    guard.processes=snapshots;
                    guard.processes_updated_at=updated_at;
                }else{return;}
                let _=send_outbound(&process_output,PluginEvent{
                    message_type:"plugin.event",
                    event:"system.processes.updated",
                    payload:json!({"count":count,"updatedAt":updated_at})
                });
                let seconds=match process_store.lock(){Ok(g)=>g.config.sample_seconds,Err(_)=>return};
                thread::sleep(Duration::from_secs(seconds));
            }
        });
    }

    send_outbound(&output_tx,PluginEvent{message_type:"plugin.event",event:"system.ready",payload:json!({"version":env!("CARGO_PKG_VERSION")})})?;

    loop{
        let payload=read_frame(&mut input)?;
        let request:HostRequest=match serde_json::from_slice(&payload){Ok(v)=>v,Err(_)=>continue};
        let response=match request.method.as_str(){
            "api.read" | "system.api.read"=>{
                let view=request.params.get("view").and_then(Value::as_str).unwrap_or("");
                let guard=store.lock().map_err(|_|"state lock poisoned".to_string())?;
                match api_read_value(&guard,view){
                    Some(result)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result,error:String::new()},
                    None=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error:"unknown system API view".into()},
                }
            }
            "system.status"=>{
                let history=store.lock().map_err(|_|"state lock poisoned".to_string())?.alert_history.clone();
                let _=save_alert_history(&mut input,&output_tx,&mut host_request_id,&history);
                let guard=store.lock().map_err(|_|"state lock poisoned".to_string())?;
                PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:status_value(&guard),error:String::new()}
            }
            "system.history.query"=>{
                let history=store.lock().map_err(|_|"state lock poisoned".to_string())?.alert_history.clone();
                let _=save_alert_history(&mut input,&output_tx,&mut host_request_id,&history);
                let rows=history.snapshot();
                PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:alert_history_query(&rows,&request.params),error:String::new()}
            }
            "system.history.export"=>{
                let history=store.lock().map_err(|_|"state lock poisoned".to_string())?.alert_history.clone();
                let _=save_alert_history(&mut input,&output_tx,&mut host_request_id,&history);
                let rows=history.snapshot();
                let csv=alert_history_csv(&rows,&request.params);
                let suggested_name=request.params.get("suggestedName").and_then(Value::as_str).filter(|name|!name.trim().is_empty()).unwrap_or("system-monitor-alert-history.csv");
                match host_request(&mut input,&output_tx,&mut host_request_id,"file.save",json!({"suggestedName":suggested_name,"content":csv,"filters":[{"name":"CSV","extensions":["csv"]}]})){
                    Ok(result)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result,error:String::new()},
                    Err(error)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error}
                }
            }
            "system.history.clear"=>{
                let history=store.lock().map_err(|_|"state lock poisoned".to_string())?.alert_history.clone();
                history.clear();
                match save_alert_history(&mut input,&output_tx,&mut host_request_id,&history){
                    Ok(())=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:json!({"cleared":true}),error:String::new()},
                    Err(error)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error}
                }
            }
            "system.processes.list"=>{
                let guard=store.lock().map_err(|_|"state lock poisoned".to_string())?;
                PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:process_list_value(&guard.processes,&request.params,guard.processes_updated_at),error:String::new()}
            }
            "system.process.icon"=>{
                let path=request.params.get("path").and_then(Value::as_str).unwrap_or("").trim().to_string();
                if path.is_empty(){
                    PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:json!({"dataUrl":null}),error:String::new()}
                }else{
                    let cached={store.lock().map_err(|_|"state lock poisoned".to_string())?.process_icon_cache.get(&path).cloned()};
                    let data_url=match cached{
                        Some(v)=>v,
                        None=>{
                            let v=process_windows::icon_data_url(&path);
                            if let Ok(mut guard)=store.lock(){guard.process_icon_cache.insert(path.clone(),v.clone());}
                            v
                        }
                    };
                    PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:json!({"dataUrl":data_url}),error:String::new()}
                }
            }
            "system.process.icons"=>{
                let paths=request.params.get("paths").and_then(Value::as_array).cloned().unwrap_or_default();
                let mut icons=serde_json::Map::new();
                for path in paths.into_iter().filter_map(|v|v.as_str().map(str::trim).filter(|s|!s.is_empty()).map(str::to_string)).take(100){
                    let cached={store.lock().map_err(|_|"state lock poisoned".to_string())?.process_icon_cache.get(&path).cloned()};
                    let data_url=match cached{
                        Some(v)=>v,
                        None=>{
                            let v=process_windows::icon_data_url(&path);
                            if let Ok(mut guard)=store.lock(){guard.process_icon_cache.insert(path.clone(),v.clone());}
                            v
                        }
                    };
                    icons.insert(path,data_url.map(Value::String).unwrap_or(Value::Null));
                }
                PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:Value::Object(icons),error:String::new()}
            }
            "system.process.open_file_location"=>{
                let path=request.params.get("path").and_then(Value::as_str).unwrap_or("").trim();
                match process_windows::open_file_location(path){
                    Ok(())=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:json!({"opened":true}),error:String::new()},
                    Err(error)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error}
                }
            }
            "system.process.copy"=>{
                let text=request.params.get("text").and_then(Value::as_str).unwrap_or("");
                if text.is_empty(){
                    PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error:"nothing to copy".to_string()}
                }else{
                    match host_request(&mut input,&output_tx,&mut host_request_id,"clipboard.write",json!({"text":text})){
                        Ok(result)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result,error:String::new()},
                        Err(error)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error}
                    }
                }
            }
            "system.sound.pick"=>{
                match pick_alert_sound(&mut input,&output_tx,&mut host_request_id){
                    Ok(result)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result,error:String::new()},
                    Err(error)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error}
                }
            }
            "system.ui.process.get"=>{
                match host_request(&mut input,&output_tx,&mut host_request_id,"storage.get",json!({"key":PROCESS_UI_STORAGE_KEY})){
                    Ok(result)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:result.get("value").cloned().unwrap_or(Value::Null),error:String::new()},
                    Err(error)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error}
                }
            }
            "system.ui.process.set"=>{
                let value=&request.params;
                let compact=value.get("compactTree").and_then(Value::as_bool);
                let widths=value.get("columnWidths").and_then(Value::as_object);
                let valid_widths=widths.map(|map|{
                    map.len()<=16 && map.values().all(|v|v.as_f64().is_some_and(|n|n>=60.0&&n<=800.0))
                }).unwrap_or(false);
                if compact.is_none() || !valid_widths {
                    PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error:"invalid process UI preferences".to_string()}
                } else {
                    match host_request(&mut input,&output_tx,&mut host_request_id,"storage.set",json!({"key":PROCESS_UI_STORAGE_KEY,"value":value})){
                        Ok(_)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:json!({"stored":true}),error:String::new()},
                        Err(error)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error}
                    }
                }
            }
            "system.process.end"=>{
                let pid=request.params.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32;
                let expected_name=request.params.get("name").and_then(Value::as_str).unwrap_or("");
                let outcome=(||{
                    if pid<=4{return Err("refusing to terminate a critical system process".to_string());}
                    if pid==std::process::id(){return Err("refusing to terminate System Monitor itself".to_string());}
                    let mut system=System::new_all();
                    system.refresh_processes(ProcessesToUpdate::All,true);
                    let process=system.process(Pid::from_u32(pid)).ok_or_else(||format!("process {pid} is no longer running"))?;
                    let actual_name=process.name().to_string_lossy();
                    if !expected_name.is_empty() && !actual_name.eq_ignore_ascii_case(expected_name){
                        return Err("process changed before it could be ended; refresh and try again".to_string());
                    }
                    if process.kill(){Ok(())}else{Err(format!("Windows refused to terminate process {pid}"))}
                })();
                match outcome{
                    Ok(())=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:json!({"ended":pid}),error:String::new()},
                    Err(error)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error}
                }
            }
            "system.config.set"=>{
                let parsed:Result<Config,_>=serde_json::from_value(request.params.clone());
                match parsed.map_err(|e|format!("invalid config: {e}")).and_then(normalize_config){
                    Err(error)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error},
                    Ok(config)=>{
                        let previous=store.lock().map_err(|_|"state lock poisoned".to_string())?.config.clone();
                        store.lock().map_err(|_|"state lock poisoned".to_string())?.config=config.clone();
                        match save_config(&mut input,&output_tx,&mut host_request_id,&config){
                            Ok(())=>{let guard=store.lock().map_err(|_|"state lock poisoned".to_string())?;let _=emit_surfaces(&output_tx,&guard);PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:status_value(&guard),error:String::new()}},
                            Err(error)=>{store.lock().map_err(|_|"state lock poisoned".to_string())?.config=previous;PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error}}
                        }
                    }
                }
            }
            "system.export.csv"=>{
                let minutes=request.params.get("minutes").and_then(Value::as_u64).unwrap_or(15).clamp(1,60);
                let suggested_name=request.params.get("suggestedName").and_then(Value::as_str).filter(|name| !name.trim().is_empty()).unwrap_or("system-monitor-history.csv");
                let csv={let guard=store.lock().map_err(|_|"state lock poisoned".to_string())?;csv_for(&guard.history,&guard.gpu_history,minutes)};
                match host_request(&mut input,&output_tx,&mut host_request_id,"file.save",json!({"suggestedName":suggested_name,"content":csv,"filters":[{"name":"CSV","extensions":["csv"]}]})){
                    Ok(result)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result,error:String::new()},
                    Err(error)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error}
                }
            }
            "system.alert.test"=>{
                let metric=request.params.get("metric").and_then(Value::as_str).unwrap_or("cpu");
                let item_id=request.params.get("itemId").and_then(Value::as_str).unwrap_or(metric);
                let state_name=request.params.get("state").and_then(Value::as_str).unwrap_or("warning");
                let notification=request.params.get("notification").and_then(Value::as_bool).unwrap_or(false);
                let tone=if state_name=="critical"{"danger"}else if state_name=="recovered"{"success"}else{"warning"};
                if notification{
                    let _=send_outbound(&output_tx,PluginEvent{message_type:"plugin.event",event:"notification.show",payload:json!({
                        "itemId":format!("system-monitor-{item_id}"),"title":format!("System Monitor · {} test",if state_name=="recovered"{"Recovered"}else if state_name=="critical"{"Critical"}else{"Warning"}),"body":format!("{metric} alert test"),"tone":tone,"silent":true,
                        "action":{"type":"open-panel","panel":"system-monitor","context":{"metric":metric}}
                    })});
                }
                PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:json!({"tested":metric,"itemId":item_id,"state":state_name}),error:String::new()}
            }
            _=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error:format!("unknown method: {}",request.method)}
        };
        send_outbound(&output_tx,response)?;
    }

    #[allow(unreachable_code)]
    { drop(output_tx); let _=_writer.join(); Ok(()) }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn core_and_gpu_alert_metrics_match_surface_item_ids(){
        assert_eq!(format!("system-monitor-{}","cpu"),"system-monitor-cpu");
        assert_eq!(format!("system-monitor-{}","memory"),"system-monitor-memory");
        assert_eq!(gpu_metric_id(0,"utilization"),"gpu0-utilization");
        assert_eq!(gpu_metric_id(1,"dedicatedMemory"),"gpu1-dedicatedMemory");
        assert_eq!(format!("system-monitor-{}",gpu_metric_id(0,"utilization")),"system-monitor-gpu0-utilization");
    }
    #[test] fn disk_alert_metric_matches_surface_item_id(){
        assert_eq!(disk_metric_id("c:"),"disk-c:-usage");
        assert_eq!(format!("system-monitor-{}",disk_metric_id("c:")),"system-monitor-disk-c:-usage");
    }
    #[test] fn disk_rate_conversion_is_stable(){
        assert_eq!(disk_rate_bytes_per_second(0,2000),0.0);
        assert_eq!(disk_rate_bytes_per_second(2048,2000),1024.0);
        assert_eq!(disk_rate_bytes_per_second(2048,0),0.0);
    }
    #[test] fn csv_disk_key_is_header_safe(){
        assert_eq!(csv_disk_key("c:"),"c");
        assert_eq!(csv_disk_key("Portable SSD"),"portable_ssd");
    }
    #[test] fn disk_identity_is_stable_for_equivalent_windows_mount_paths(){
        assert_eq!(stable_disk_id("C:\\", "System"), "c:");
        assert_eq!(stable_disk_id("c:/", "Renamed"), "c:");
    }
    #[test] fn disk_identity_falls_back_to_name_without_mount_point(){
        assert_eq!(stable_disk_id("", "Portable SSD"), "portable ssd");
    }
    #[test] fn disk_usage_defaults_are_capacity_oriented_and_hidden_from_topbar(){
        let d=disk_config("C:");
        assert_eq!(d.usage.healthy_max,80.0);
        assert_eq!(d.usage.critical_min,95.0);
        assert!(!d.usage.show_topbar);
        assert_eq!(d.usage.overview_visualization.as_deref(),Some("bar"));
        assert_eq!(d.usage.topbar_visualization.as_deref(),Some("bar"));
    }
    #[test] fn config_rejects_invalid_disk_usage_thresholds(){
        let mut c=Config::default();
        let mut d=disk_config("C:");
        d.usage.healthy_max=95.0;
        d.usage.critical_min=90.0;
        c.disks.insert("c:".into(),d);
        assert!(normalize_config(c).is_err());
    }
    #[test] fn disk_overview_uses_usage_donut_with_capacity(){
        let ui=include_str!("../ui/index.html");
        assert!(ui.contains("function diskUsageDonut("));
        assert!(ui.contains("conic-gradient("));
        assert!(ui.contains("capacity"));
        assert!(ui.contains("used"));
        assert!(ui.contains("free"));
        assert!(!ui.contains("addMetric(\"Capacity\""));
    }
    #[test] fn disk_overview_graphs_read_and_write_throughput(){
        let ui=include_str!("../ui/index.html");
        assert!(ui.contains("function drawThroughput("));
        assert!(ui.contains("\"readBytesPerSecond\""));
        assert!(ui.contains("\"writeBytesPerSecond\""));
        assert!(ui.contains("canvas.id=\"disk-\"+safeId+\"-\"+throughputKey"));
    }
    #[test] fn overview_keeps_cores_near_cpu_and_metrics_include_disks(){
        let ui=include_str!("../ui/index.html");
        let cpu=ui.find("CPU Utilization").unwrap();
        let cores=ui.find("Per-core activity").unwrap();
        let gpus=ui.find("<h2>GPUs</h2>").unwrap();
        let disks=ui.find("<h2>Disks</h2>").unwrap();
        let metrics=ui.find("<h2>Metrics</h2>").unwrap();
        assert!(cpu<cores && cores<gpus && gpus<disks && disks<metrics);
        assert!(ui.contains("Disk Usage"));
        assert!(ui.contains("bytes(d.totalBytes)"));
    }
    #[test] fn ui_exposes_per_disk_usage_settings(){
        let ui=include_str!("../ui/index.html");
        assert!(ui.contains(r#"metricBox("disks."+disk.id+".usage""#));
        assert!(ui.contains("disk usage"));
        assert!(ui.contains("kind===\"usage\""));
    }
    #[test] fn thresholds_are_three_state(){let c=metric_config(60.0,90.0);assert_eq!(health(Some(60.0),&c),Health::Healthy);assert_eq!(health(Some(70.0),&c),Health::Warning);assert_eq!(health(Some(90.0),&c),Health::Critical);}
    #[test] fn unavailable_is_not_zero(){let c=metric_config(60.0,90.0);assert_eq!(health(None,&c),Health::Unavailable);}
    #[test] fn transitions_require_two_samples(){let mut t=TransitionTracker::default();assert!(t.observe(Health::Healthy).is_none());assert_eq!(t.observe(Health::Healthy),Some((Health::Unavailable,Health::Healthy)));assert!(t.observe(Health::Critical).is_none());assert_eq!(t.observe(Health::Critical),Some((Health::Healthy,Health::Critical)));}
    #[test] fn config_rejects_overlapping_thresholds(){let mut c=Config::default();c.temperature.healthy_max=95.0;c.temperature.critical_min=90.0;assert!(normalize_config(c).is_err());}
    #[test] fn alert_history_records_only_actionable_transitions_and_respects_disable(){
        let mut store=Store::new();
        let cfg=metric_config(70.0,95.0);
        store.record_alert_transitions(&[
            AlertTransition::new("cpu","CPU utilization",Some(80.0),Health::Healthy,Health::Warning,cfg.clone()),
            AlertTransition::new("cpu","CPU utilization",Some(60.0),Health::Unavailable,Health::Healthy,cfg.clone()),
        ]);
        assert_eq!(store.alert_history.snapshot().len(),1);
        assert_eq!(store.alert_history.snapshot()[0].status,"Warning");
        store.config.alert_history_enabled=false;
        store.record_alert_transitions(&[AlertTransition::new("cpu","CPU utilization",Some(96.0),Health::Warning,Health::Critical,cfg)]);
        assert_eq!(store.alert_history.snapshot().len(),1);
    }
    #[test] fn alert_history_csv_escapes_commas_and_quotes(){
        let event=AlertHistoryEvent{id:"1".into(),timestamp:42,metric:"cpu".into(),title:"CPU, utilization".into(),status:"Warning".into(),value:"81.0%".into(),threshold:"> 70.0".into(),sound:true,notification:false,details:"State changed from \"healthy\"".into()};
        let csv=alert_history_csv(&[event],&json!({}));
        assert!(csv.contains("\"CPU, utilization\""));
        assert!(csv.contains("\"State changed from \"\"healthy\"\"\""));
    }
    #[test] fn alert_history_clear_preserves_disabled_setting(){
        let store=Store::new();
        store.record_alert_transitions(&[AlertTransition::new("cpu","CPU utilization",Some(80.0),Health::Healthy,Health::Warning,metric_config(70.0,95.0))]);
        assert_eq!(store.alert_history.snapshot().len(),1);
        store.alert_history.clear();
        assert!(store.alert_history.snapshot().is_empty());
        assert!(store.config.alert_history_enabled);
    }

    #[test] fn recovered_transition_is_actionable_but_initial_healthy_is_not(){let c=metric_config(70.0,95.0);assert!(!AlertTransition::new("cpu","CPU",Some(10.0),Health::Unavailable,Health::Healthy,c.clone()).actionable());assert!(AlertTransition::new("cpu","CPU",Some(10.0),Health::Warning,Health::Healthy,c).actionable());}
    #[test] fn default_recovered_alert_sounds_without_notification(){let c=metric_config(70.0,95.0);let d=alert_delivery_for(&c,Health::Healthy);assert!(d.sound);assert!(!d.notification);}
    #[test] fn ui_persists_alert_delivery_checkboxes_separately(){
        let ui=include_str!("../ui/index.html");
        assert!(ui.contains("box.querySelectorAll(\"[data-path][data-alert][data-alert-field]\")"));
        assert!(!ui.contains("if(i.dataset.alert){var alert=target[i.dataset.alert]"));
    }
    #[test] fn process_list_defaults_to_cpu_descending(){
        let rows=vec![
            ProcessSnapshot{pid:1,name:"low.exe".into(),cpu_percent:2.0,memory_bytes:10,gpu_percent:None,user:"u".into(),start_time:1,parent_pid:None,path:String::new(),command_line:String::new(),status:"Run".into(),window_visible:false},
            ProcessSnapshot{pid:2,name:"high.exe".into(),cpu_percent:80.0,memory_bytes:20,gpu_percent:None,user:"u".into(),start_time:2,parent_pid:None,path:String::new(),command_line:String::new(),status:"Run".into(),window_visible:true},
        ];
        let value=process_list_value(&rows,&json!({"page":1,"pageSize":25}),123);
        assert_eq!(value["rows"][0]["pid"],2);
        assert_eq!(value["total"],2);
        assert_eq!(value["page"],1);
        assert_eq!(value["direction"],"desc");
    }
    #[test] fn process_list_keeps_parent_context_for_matching_child(){
        let rows=vec![
            ProcessSnapshot{pid:100,name:"parent.exe".into(),cpu_percent:1.0,memory_bytes:10,gpu_percent:None,user:"u".into(),start_time:1,parent_pid:None,path:String::new(),command_line:String::new(),status:"Run".into(),window_visible:true},
            ProcessSnapshot{pid:101,name:"remote-child.exe".into(),cpu_percent:2.0,memory_bytes:20,gpu_percent:None,user:"u".into(),start_time:2,parent_pid:Some(100),path:String::new(),command_line:String::new(),status:"Run".into(),window_visible:false},
        ];
        let value=process_list_value(&rows,&json!({"query":"remote"}),123);
        assert_eq!(value["total"],2);
        assert_eq!(value["totalGroups"],1);
        assert_eq!(value["rows"][0]["pid"],100);
        assert_eq!(value["rows"][0]["children"][0]["pid"],101);
    }
    #[test] fn process_list_searches_background_processes(){
        let rows=vec![
            ProcessSnapshot{pid:11,name:"visible.exe".into(),cpu_percent:1.0,memory_bytes:1,gpu_percent:None,user:"Papa".into(),start_time:1,parent_pid:None,path:"C:\\Visible.exe".into(),command_line:String::new(),status:"Run".into(),window_visible:true},
            ProcessSnapshot{pid:12,name:"worker.exe".into(),cpu_percent:1.0,memory_bytes:1,gpu_percent:None,user:"Papa".into(),start_time:1,parent_pid:None,path:"C:\\Tools\\worker.exe".into(),command_line:"worker.exe --quiet".into(),status:"Run".into(),window_visible:false},
        ];
        let value=process_list_value(&rows,&json!({"filter":"background","query":"worker"}),123);
        assert_eq!(value["total"],1);
        assert_eq!(value["rows"][0]["pid"],12);
    }
    #[test] fn visualization_defaults_match_metric_behavior(){let c=Config::default();assert_eq!(c.cpu.overview_visualization.as_deref(),Some("graph"));assert_eq!(c.cpu.topbar_visualization.as_deref(),Some("graph"));assert_eq!(c.memory.overview_visualization.as_deref(),Some("bar"));assert_eq!(c.memory.topbar_visualization.as_deref(),Some("bar"));}
    #[test] fn gpu_defaults_are_per_adapter_and_optional_temperature(){
        let g=gpu_config(2,"Example GPU");
        assert_eq!(g.utilization.topbar_title.as_deref(),Some("Example GPU"));
        assert!(g.utilization.show_topbar);
        assert!(!g.dedicated_memory.show_topbar);
        assert!(!g.shared_memory.show_topbar);
        assert!(!g.temperature.show_topbar);
        assert_eq!(g.dedicated_memory.overview_visualization.as_deref(),Some("bar"));
        assert_eq!(g.dedicated_memory.topbar_title.as_deref(),Some("Example GPU VRAM"));
        assert_eq!(g.temperature.overview_visualization.as_deref(),Some("graph"));
    }
    #[test] fn gpu_memory_percent_requires_capacity(){
        assert_eq!(percent(Some(4),8),Some(50.0));
        assert_eq!(percent(Some(4),0),None);
        assert_eq!(percent(None,8),None);
    }
    #[test] fn duplicate_gpu_identity_hides_only_the_single_telemetry_empty_luid(){
        fn adapter(index:u32,luid:&str)->GpuAdapterInfo{
            GpuAdapterInfo{
                index,
                name:"Intel(R) Iris(R) Xe Graphics".into(),
                luid:luid.into(),
                vendor_id:0x8086,
                device_id:0x1234,
                subsystem_id:0x5678,
                revision:1,
                dedicated_video_memory_bytes:0,
                dedicated_system_memory_bytes:0,
                shared_system_memory_bytes:8*1024*1024*1024,
                software:false,
            }
        }
        fn telemetry(index:u32,luid:&str,util:Option<f64>)->GpuTelemetry{
            GpuTelemetry{
                index,
                name:"Intel(R) Iris(R) Xe Graphics".into(),
                luid:luid.into(),
                utilization_percent:util,
                engines:vec![],
                dedicated_used_bytes:None,
                dedicated_total_bytes:0,
                shared_used_bytes:None,
                shared_total_bytes:8*1024*1024*1024,
                temperature_c:None,
                temperature_provider:None,
                telemetry_error:None,
            }
        }

        let adapters=vec![adapter(0,"A"),adapter(1,"B")];
        let latest=vec![telemetry(0,"A",Some(0.0)),telemetry(1,"B",None)];
        let visible=visible_gpu_luids(&adapters,&latest);
        assert!(visible.contains("A"));
        assert!(!visible.contains("B"));
    }

    #[test] fn duplicate_gpu_identity_keeps_both_when_visibility_is_ambiguous(){
        let mk_adapter=|index,luid:&str|GpuAdapterInfo{
            index,
            name:"Example GPU".into(),
            luid:luid.into(),
            vendor_id:1,
            device_id:2,
            subsystem_id:3,
            revision:4,
            dedicated_video_memory_bytes:5,
            dedicated_system_memory_bytes:6,
            shared_system_memory_bytes:7,
            software:false,
        };
        let adapters=vec![mk_adapter(0,"A"),mk_adapter(1,"B")];
        let visible=visible_gpu_luids(&adapters,&[]);
        assert!(visible.contains("A"));
        assert!(visible.contains("B"));
    }

    #[test] fn hidden_duplicate_gpu_is_not_emitted_to_topbar_or_tray(){
        fn adapter(index:u32,luid:&str)->GpuAdapterInfo{
            GpuAdapterInfo{
                index,
                name:"Intel(R) Iris(R) Xe Graphics".into(),
                luid:luid.into(),
                vendor_id:0x8086,
                device_id:0x1234,
                subsystem_id:0x5678,
                revision:1,
                dedicated_video_memory_bytes:0,
                dedicated_system_memory_bytes:0,
                shared_system_memory_bytes:8*1024*1024*1024,
                software:false,
            }
        }
        fn telemetry(index:u32,luid:&str,util:Option<f64>)->GpuTelemetry{
            GpuTelemetry{
                index,
                name:"Intel(R) Iris(R) Xe Graphics".into(),
                luid:luid.into(),
                utilization_percent:util,
                engines:vec![],
                dedicated_used_bytes:None,
                dedicated_total_bytes:0,
                shared_used_bytes:None,
                shared_total_bytes:8*1024*1024*1024,
                temperature_c:None,
                temperature_provider:None,
                telemetry_error:None,
            }
        }

        let mut store=Store::new();
        store.gpu_adapters=vec![adapter(0,"A"),adapter(1,"B")];
        store.gpu_latest=vec![telemetry(0,"A",Some(0.0)),telemetry(1,"B",None)];
        store.config.gpus.insert("A".into(),gpu_config(0,"Intel(R) Iris(R) Xe Graphics"));
        store.config.gpus.insert("B".into(),gpu_config(1,"Intel(R) Iris(R) Xe Graphics"));
        let (top,tray)=surfaces(&store);
        let top=top.to_string();
        let tray=tray.to_string();
        assert!(top.contains("gpu0-utilization"));
        assert!(!top.contains("gpu1-utilization"));
        assert!(tray.contains("gpu0-utilization"));
        assert!(!tray.contains("gpu1-utilization"));
    }

    #[test] fn topbar_live_alert_icon_uses_metric_warning_and_critical_colors(){
        let mut cfg=metric_config(60.0,90.0);
        cfg.warning_color="#123456".into();
        cfg.critical_color="#abcdef".into();

        let warning=metric_topbar("cpu","CPU",Some(75.0),"%",Some(75.0),vec![],Health::Warning,&cfg,"graph");
        assert_eq!(warning["icon"],json!("alert"));
        assert_eq!(warning["content"]["color"],json!("#123456"));

        let critical=metric_topbar("cpu","CPU",Some(95.0),"%",Some(95.0),vec![],Health::Critical,&cfg,"graph");
        assert_eq!(critical["icon"],json!("alert"));
        assert_eq!(critical["content"]["color"],json!("#abcdef"));

        let healthy=metric_topbar("cpu","CPU",Some(40.0),"%",Some(40.0),vec![],Health::Healthy,&cfg,"graph");
        assert!(healthy["icon"].is_null());
        assert!(healthy["content"]["color"].is_null());
    }

    #[test] fn topbar_series_colors_each_sample_by_its_own_threshold_state(){
        let cfg=metric_config(60.0,90.0);
        let now=now_ms();
        let mut history=VecDeque::new();
        history.push_back(Sample{timestamp:now-4000,cpu_percent:40.0,core_percent:vec![],temperature_c:None,temperature_source:None,temperature_sensors:vec![],temperature_provider:String::new(),temperature_error:None,memory_percent:0.0,memory_used_bytes:0,memory_available_bytes:0,memory_total_bytes:0,disks:vec![]});
        history.push_back(Sample{timestamp:now-2000,cpu_percent:75.0,core_percent:vec![],temperature_c:None,temperature_source:None,temperature_sensors:vec![],temperature_provider:String::new(),temperature_error:None,memory_percent:0.0,memory_used_bytes:0,memory_available_bytes:0,memory_total_bytes:0,disks:vec![]});
        history.push_back(Sample{timestamp:now,cpu_percent:95.0,core_percent:vec![],temperature_c:None,temperature_source:None,temperature_sensors:vec![],temperature_provider:String::new(),temperature_error:None,memory_percent:0.0,memory_used_bytes:0,memory_available_bytes:0,memory_total_bytes:0,disks:vec![]});
        let series=recent_series(&history,|s|Some(s.cpu_percent),60,&cfg);
        assert_eq!(series[0]["color"],cfg.healthy_color);
        assert_eq!(series[1]["color"],cfg.warning_color);
        assert_eq!(series[2]["color"],cfg.critical_color);
    }

    #[test] fn global_alert_sound_setting_defaults_enabled_and_round_trips(){
        let defaults=Config::default();
        assert!(defaults.alert_sounds_enabled);

        let serialized=serde_json::to_value(&defaults).unwrap();
        assert_eq!(serialized["alertSoundsEnabled"],json!(true));

        let mut muted=serialized.clone();
        muted["alertSoundsEnabled"]=json!(false);
        let restored:Config=serde_json::from_value(muted).unwrap();
        assert!(!restored.alert_sounds_enabled);

        let mut legacy=serialized;
        legacy.as_object_mut().unwrap().remove("alertSoundsEnabled");
        let restored_legacy:Config=serde_json::from_value(legacy).unwrap();
        assert!(restored_legacy.alert_sounds_enabled);
    }
    #[test]
    fn api_projection_does_not_expose_config_hardware_identity_or_processes() {
        let mut store = Store::new();
        store.config.alert_sounds_enabled = false;
        let private = ["config","luid","mountPoint","fileSystem","processes","soundFile","path","temperatureError"];
        for view in ["summary","metrics","gpus","disks","history","alerts","catalog"] {
            let value=api_read_value(&store,view).expect("supported view");
            let encoded=value.to_string();
            for field in private {
                assert!(!encoded.contains(&format!("\"{field}\"")), "{view} exposed {field}");
            }
        }
        assert!(api_read_value(&store,"status").is_none());
        assert!(api_read_value(&store,"processes").is_none());
    }

    #[test]
    fn api_alerts_only_report_active_warning_or_critical_states() {
        let mut store=Store::new();
        store.cpu_tracker.committed=Health::Warning;
        store.memory_tracker.committed=Health::Healthy;
        store.temp_tracker.committed=Health::Critical;
        let alerts=api_read_value(&store,"alerts").unwrap();
        let items=alerts["items"].as_array().unwrap();
        assert_eq!(items.len(),2);
        assert_eq!(items[0]["metric"],"cpu");
        assert_eq!(items[1]["metric"],"temperature");
    }

    #[test]
    fn api_history_is_bounded_to_120_points() {
        let mut store=Store::new();
        for timestamp in 0..200 {
            store.history.push_back(Sample {
                timestamp,cpu_percent:10.0,core_percent:vec![],temperature_c:None,
                temperature_source:None,temperature_sensors:vec![],temperature_provider:String::new(),
                temperature_error:None,memory_percent:20.0,memory_used_bytes:1,
                memory_available_bytes:2,memory_total_bytes:3,disks:vec![]
            });
        }
        let history=api_read_value(&store,"history").unwrap();
        let rows=history["items"].as_array().unwrap();
        assert_eq!(rows.len(),120);
        assert_eq!(rows[0]["timestamp"],80);
        assert_eq!(rows[119]["timestamp"],199);
    }

}
