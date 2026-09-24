use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{Components, System};

const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const STORAGE_KEY: &str = "config-v1";
const MAX_HISTORY_SECONDS: u64 = 60 * 60;
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
struct MetricConfig {
    #[serde(default = "default_true")]
    show_topbar: bool,
    #[serde(default)]
    show_tray: bool,
    #[serde(default = "default_true")]
    sound: bool,
    #[serde(default = "default_true")]
    notification: bool,
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
struct Config {
    sample_seconds: u64,
    default_window_minutes: u64,
    cpu: MetricConfig,
    temperature: MetricConfig,
    memory: MetricConfig,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            sample_seconds: DEFAULT_SAMPLE_SECONDS,
            default_window_minutes: 15,
            cpu: metric_config(60.0, 90.0),
            temperature: metric_config(60.0, 90.0),
            memory: metric_config(70.0, 90.0),
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
    Ok(c)
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
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TemperatureSensor {
    label: String,
    value_c: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelperTemperatureSensor {
    hardware: String,
    name: String,
    value_c: f64,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelperSensorResponse {
    #[serde(default)]
    sensors: Vec<HelperTemperatureSensor>,
    #[serde(default)]
    error: Option<String>,
}

struct LibreHardwareMonitorProvider {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}
impl LibreHardwareMonitorProvider {
    fn new() -> Result<Self, String> {
        let current = std::env::current_exe().map_err(|e| format!("cannot locate plugin executable: {e}"))?;
        let helper = current.parent().ok_or_else(|| "plugin executable has no parent directory".to_string())?
            .join("remotehand-system-monitor-sensors.exe");
        if !helper.is_file() {
            return Err(format!("temperature helper not found: {}", helper.display()));
        }
        let mut command = Command::new(&helper);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(|e| format!("cannot start temperature helper: {e}"))?;
        let stdin = child.stdin.take().ok_or_else(|| "temperature helper stdin unavailable".to_string())?;
        let stdout = child.stdout.take().ok_or_else(|| "temperature helper stdout unavailable".to_string())?;
        // Dropping Child does not terminate the process; the open pipe handles keep the helper alive.
        drop(child);
        Ok(Self { stdin, stdout: BufReader::new(stdout) })
    }
    fn sample(&mut self) -> Result<HelperSensorResponse, String> {
        self.stdin.write_all(b"sample\n").and_then(|_| self.stdin.flush())
            .map_err(|e| format!("temperature helper write failed: {e}"))?;
        let mut line = String::new();
        self.stdout.read_line(&mut line).map_err(|e| format!("temperature helper read failed: {e}"))?;
        if line.trim().is_empty() { return Err("temperature helper closed without a response".into()); }
        serde_json::from_str(&line).map_err(|e| format!("invalid temperature helper response: {e}"))
    }
}

struct WindowsSystemProvider {
    system: System,
    components: Components,
    lhm: Option<LibreHardwareMonitorProvider>,
    lhm_error: Option<String>,
}
impl WindowsSystemProvider {
    fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_cpu_usage();
        system.refresh_memory();
        let components = Components::new_with_refreshed_list();
        let (lhm, lhm_error) = match LibreHardwareMonitorProvider::new() {
            Ok(provider) => (Some(provider), None),
            Err(error) => (None, Some(error)),
        };
        Self { system, components, lhm, lhm_error }
    }
    fn sample(&mut self) -> Sample {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();

        let mut provider_name = "LibreHardwareMonitor".to_string();
        let mut provider_error = self.lhm_error.clone();
        let mut sensors = Vec::<TemperatureSensor>::new();

        if let Some(lhm) = self.lhm.as_mut() {
            match lhm.sample() {
                Ok(response) => {
                    provider_error = response.error;
                    sensors = response.sensors.into_iter()
                        .filter(|s| s.value_c.is_finite() && s.value_c >= -20.0 && s.value_c <= 150.0)
                        .map(|s| TemperatureSensor {
                            label: if s.hardware.trim().is_empty() { s.name } else { format!("{} · {}", s.hardware, s.name) },
                            value_c: s.value_c,
                        })
                        .collect();
                }
                Err(error) => {
                    provider_error = Some(error);
                    self.lhm = None;
                }
            }
        }

        if sensors.is_empty() {
            self.components.refresh(false);
            let fallback = self.components.iter().filter_map(|component| {
                let value = component.temperature()? as f64;
                if !value.is_finite() || value < -20.0 || value > 150.0 { return None; }
                Some(TemperatureSensor { label: component.label().to_string(), value_c: value })
            }).collect::<Vec<_>>();
            if !fallback.is_empty() {
                provider_name = "sysinfo fallback".to_string();
                sensors = fallback;
            }
        }
        sensors.sort_by(|a,b| a.label.cmp(&b.label));

        fn rank(label: &str) -> u8 {
            let l = label.to_ascii_lowercase();
            if l.contains("cpu package") { 0 }
            else if l.contains("core average") { 1 }
            else if l.contains("core max") { 2 }
            else if l.contains("package") { 3 }
            else if l.contains("cpu") || l.contains("core") { 4 }
            else { 5 }
        }
        let preferred = sensors.iter().min_by_key(|s| rank(&s.label));

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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Health { Healthy, Warning, Critical, Unavailable }
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

struct Store {
    config: Config,
    history: VecDeque<Sample>,
    cpu_tracker: TransitionTracker,
    temp_tracker: TransitionTracker,
    memory_tracker: TransitionTracker,
}
impl Store {
    fn new() -> Self {
        Self { config: Config::default(), history: VecDeque::new(), cpu_tracker:Default::default(), temp_tracker:Default::default(), memory_tracker:Default::default() }
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
        out
    }
}

#[derive(Clone)]
struct AlertTransition {
    metric: &'static str,
    title: &'static str,
    value: Option<f64>,
    from: Health,
    to: Health,
    config: MetricConfig,
}
impl AlertTransition {
    fn new(metric:&'static str,title:&'static str,value:Option<f64>,from:Health,to:Health,config:MetricConfig)->Self{Self{metric,title,value,from,to,config}}
    fn actionable(&self)->bool{matches!(self.to,Health::Warning|Health::Critical)}
}

#[cfg(windows)]
fn play_alert_sound() {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBeep(u_type: u32) -> i32;
    }
    const MB_ICONEXCLAMATION: u32 = 0x00000030;
    unsafe {
        let _ = MessageBeep(MB_ICONEXCLAMATION);
    }
}
#[cfg(not(windows))]
fn play_alert_sound() {}

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

fn recent_series(history:&VecDeque<Sample>,selector:fn(&Sample)->Option<f64>,seconds:u64)->Vec<Value>{
    let cutoff=now_ms().saturating_sub(seconds*1000);
    history.iter().filter(|s|s.timestamp>=cutoff).filter_map(|s|selector(s).map(|v|json!({"value":v}))).collect()
}
fn metric_topbar(id:&str,title:&str,value:Option<f64>,unit:&str,series:Vec<Value>,h:Health,cfg:&MetricConfig)->Value{
    let shown=value.map(|v|format!("{:.0}{unit}",v)).unwrap_or_else(||"—".into());
    json!({
        "id":format!("system-monitor:{id}"),"title":title,
        "tooltip":format!("{title}: {shown} · {}",health_name(h)),
        "content":{"type":"metric-sparkline","value":shown,"series":series,"color":health_color(h,cfg)},
        "action":{"type":"open-panel","panel":"system-monitor","context":{"metric":id}}
    })
}
fn surfaces(store:&Store)->(Value,Value){
    let latest=store.history.back();
    let cpu_value=latest.map(|s|s.cpu_percent);
    let temp_value=latest.and_then(|s|s.temperature_c);
    let mem_value=latest.map(|s|s.memory_percent);
    let cpu_h=health(cpu_value,&store.config.cpu);let temp_h=health(temp_value,&store.config.temperature);let mem_h=health(mem_value,&store.config.memory);
    let mut top=Vec::new();
    if store.config.cpu.show_topbar{top.push(metric_topbar("cpu","CPU",cpu_value,"%",recent_series(&store.history,|s|Some(s.cpu_percent),60),cpu_h,&store.config.cpu));}
    if store.config.temperature.show_topbar{top.push(metric_topbar("temperature","CPU Temp",temp_value,"°",recent_series(&store.history,|s|s.temperature_c,60),temp_h,&store.config.temperature));}
    if store.config.memory.show_topbar{top.push(metric_topbar("memory","Memory",mem_value,"%",recent_series(&store.history,|s|Some(s.memory_percent),60),mem_h,&store.config.memory));}
    let tray=vec![
        tray_item("cpu","CPU utilization",cpu_value,"%",cpu_h,&store.config.cpu),
        tray_item("temperature","CPU temperature",temp_value,"°C",temp_h,&store.config.temperature),
        tray_item("memory","Memory utilization",mem_value,"%",mem_h,&store.config.memory),
    ];
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
fn status_value(store:&Store)->Value{
    json!({"config":&store.config,"latest":store.history.back(),"history":&store.history})
}
fn emit_alert(output:&mpsc::Sender<Value>,transition:&AlertTransition){
    if !transition.actionable(){return;}
    if transition.config.sound{play_alert_sound();}
    let state=health_name(transition.to);
    let value=transition.value.map(|v|format!("{v:.1}")).unwrap_or_else(||"unavailable".into());
    let payload=json!({"metric":transition.metric,"title":transition.title,"value":transition.value,"state":state,"from":health_name(transition.from),"sound":transition.config.sound,"notification":transition.config.notification});
    let _=send_outbound(output,PluginEvent{message_type:"plugin.event",event:"system.alert",payload});
    if transition.config.notification{
        let _=send_outbound(output,PluginEvent{message_type:"plugin.event",event:"notification.show",payload:json!({
            "itemId":format!("system-monitor-{}",transition.metric),
            "title":format!("{} · {}",transition.title,if transition.to==Health::Critical{"Critical"}else{"Warning"}),
            "body":format!("Current value: {value}"),
            "tone":if transition.to==Health::Critical{"danger"}else{"warning"},
            "silent":true,
            "action":{"type":"open-panel","panel":"system-monitor","context":{"metric":transition.metric}}
        })});
    }
}
fn csv_for(history:&VecDeque<Sample>,minutes:u64)->String{
    let cutoff=now_ms().saturating_sub(minutes.clamp(1,60)*60*1000);
    let max_cores=history.iter().map(|s|s.core_percent.len()).max().unwrap_or(0);
    let mut out=String::from("timestamp_ms,cpu_percent,cpu_temperature_c,temperature_source,memory_percent,memory_used_bytes,memory_available_bytes,memory_total_bytes");
    for i in 0..max_cores{out.push_str(&format!(",core_{i}_percent"));}out.push('\n');
    for s in history.iter().filter(|s|s.timestamp>=cutoff){
        out.push_str(&format!("{},{:.2},",s.timestamp,s.cpu_percent));
        if let Some(v)=s.temperature_c{out.push_str(&format!("{v:.2}"));}
        out.push(',');
        let source=s.temperature_source.as_deref().unwrap_or("").replace('"', "\"\"");
        out.push('"');out.push_str(&source);out.push_str("\",");
        out.push_str(&format!("{:.2},{},{},{}",s.memory_percent,s.memory_used_bytes,s.memory_available_bytes,s.memory_total_bytes));
        for i in 0..max_cores{out.push(',');if let Some(v)=s.core_percent.get(i){out.push_str(&format!("{v:.2}"));}}
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
    let store=Arc::new(Mutex::new(Store::new()));
    store.lock().map_err(|_|"state lock poisoned".to_string())?.config=config;

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

    send_outbound(&output_tx,PluginEvent{message_type:"plugin.event",event:"system.ready",payload:json!({"version":env!("CARGO_PKG_VERSION")})})?;

    loop{
        let payload=read_frame(&mut input)?;
        let request:HostRequest=match serde_json::from_slice(&payload){Ok(v)=>v,Err(_)=>continue};
        let response=match request.method.as_str(){
            "system.status"=>{
                let guard=store.lock().map_err(|_|"state lock poisoned".to_string())?;
                PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:status_value(&guard),error:String::new()}
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
                let csv={let guard=store.lock().map_err(|_|"state lock poisoned".to_string())?;csv_for(&guard.history,minutes)};
                match host_request(&mut input,&output_tx,&mut host_request_id,"file.save",json!({"suggestedName":"remotehand-system-monitor.csv","content":csv,"filters":[{"name":"CSV","extensions":["csv"]}]})){
                    Ok(result)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result,error:String::new()},
                    Err(error)=>PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:false,result:Value::Null,error}
                }
            }
            "system.alert.test"=>{
                let metric=request.params.get("metric").and_then(Value::as_str).unwrap_or("temperature");
                let guard=store.lock().map_err(|_|"state lock poisoned".to_string())?;
                let cfg=match metric{"cpu"=>&guard.config.cpu,"memory"=>&guard.config.memory,_=>&guard.config.temperature};
                if cfg.sound{play_alert_sound();}
                if cfg.notification{
                    let _=send_outbound(&output_tx,PluginEvent{message_type:"plugin.event",event:"notification.show",payload:json!({
                        "itemId":format!("system-monitor-{metric}"),"title":"System Monitor · Test","body":format!("{metric} alert test"),"tone":"warning","silent":true,
                        "action":{"type":"open-panel","panel":"system-monitor","context":{"metric":metric}}
                    })});
                }
                PluginResponse{message_type:"plugin.response",request_id:request.request_id,ok:true,result:json!({"tested":metric}),error:String::new()}
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
    #[test] fn thresholds_are_three_state(){let c=metric_config(60.0,90.0);assert_eq!(health(Some(60.0),&c),Health::Healthy);assert_eq!(health(Some(70.0),&c),Health::Warning);assert_eq!(health(Some(90.0),&c),Health::Critical);}
    #[test] fn unavailable_is_not_zero(){let c=metric_config(60.0,90.0);assert_eq!(health(None,&c),Health::Unavailable);}
    #[test] fn transitions_require_two_samples(){let mut t=TransitionTracker::default();assert!(t.observe(Health::Healthy).is_none());assert_eq!(t.observe(Health::Healthy),Some((Health::Unavailable,Health::Healthy)));assert!(t.observe(Health::Critical).is_none());assert_eq!(t.observe(Health::Critical),Some((Health::Healthy,Health::Critical)));}
    #[test] fn config_rejects_overlapping_thresholds(){let mut c=Config::default();c.temperature.healthy_max=95.0;c.temperature.critical_min=90.0;assert!(normalize_config(c).is_err());}
}
