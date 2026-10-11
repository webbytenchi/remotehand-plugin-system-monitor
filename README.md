# RemoteHand System Monitor

Status: **v0.5.1**

RemoteHand System Monitor is a Windows-first plugin for local CPU, memory, and GPU monitoring. It keeps hardware-monitoring semantics inside the plugin while RemoteHand core provides only generic plugin infrastructure.

## v0.5.1 scope

v0.5.1 adds a persistent global alert-sound mute while keeping monitoring and other alert delivery channels independent.

- **Enable alert sounds** checkbox in General settings controls alert audio globally.
- The preference is saved with plugin configuration and persists across RemoteHand restarts.
- Existing installations and older configuration files default to sounds enabled unless the user turns the preference off.
- When disabled, alert audio is muted while monitoring, alert evaluation, visual state updates, native notifications, and persistent History continue normally.

## v0.5.0 scope

v0.5.0 adds persistent alert-event History and History controls while keeping telemetry history and alert delivery independent.

- persistent History for real Warning, Critical, and Recovered metric transitions
- Enable Alerts History setting; disabling it stops new persistence without disabling alerts, sound, or notifications
- existing saved History remains available while History is disabled
- dedicated History tab with search, metric/event filters, sortable columns, pagination, and From/To filtering
- in-app calendar/time picker for From and To, with Now, Clear, Cancel, and Apply actions
- calendar icons in the From and To fields
- Clear History with in-app confirmation
- filtered alert-History CSV export
- History layout/navigation aligned with the RemoteHand monitor-family style

The existing Overview telemetry history, graphs, and telemetry CSV export remain separate from persistent alert-event History.

## v0.4.0 scope

v0.4.0 adds per-volume disk monitoring and live severity indicators while keeping hardware-monitoring semantics inside the plugin.

- local disk discovery with stable per-volume identities
- per-volume used, free, total, and utilization telemetry
- live read/write throughput telemetry with history graphs
- compact Used vs Free donut visualization with total capacity
- per-disk usage thresholds with Healthy / Warning / Critical states
- independent Warning, Critical, and Recovered sound/notification delivery
- optional per-disk top-bar and system-tray surfaces
- live warning/critical icon support on top-bar metrics using each metric's configured alert color
- disk usage in the Metrics table with current, minimum, average, peak, thresholds, and capacity
- disk telemetry included in CSV export
- Overview reordered so per-core CPU activity stays adjacent to the CPU summary
- Metrics ordering aligned with Overview: CPU, Memory, GPUs, then Disks
- notification Test actions aligned with their actual tray/surface IDs for CPU, Memory, GPU, and Disk metrics
- GPU live-alert notification IDs aligned with emitted GPU surface IDs

Disk temperature is intentionally deferred because the current sysinfo::Disks provider does not expose it portably on Windows; adding SMART/NVMe temperature would require a separate native provider.

## v0.3.0 scope

v0.3.0 adds a Windows-first Processes view while keeping process-monitoring semantics inside the plugin.

- full Windows process enumeration, including background/headless processes
- searchable process tree with parent/child context preserved
- collapsible hierarchy with compact launcher-chain folding
- process executable icons with lazy/cached loading
- sortable columns for Process, PID, CPU, Memory, GPU, User, and Started
- resizable process columns with persisted widths
- All / Background / Visible / GPU-active filters
- top-level process-group pagination with selectable group counts
- process Details dialog
- Open file location
- Copy process name, PID, executable path, or command line
- guarded End task action with confirmation and PID/name reuse protection
- plugin-owned themed sidebar icon through RemoteHand's generic sidebar asset support

Per-process GPU attribution is not yet implemented, so the Processes GPU column remains unavailable until a suitable Windows provider is added. Publisher/signature metadata is also deferred.

The attempted Open / Focus window action was intentionally removed from v0.3.0 after Windows foreground-window behavior proved unreliable across process/window hosting models.

## v0.2.0 scope

The v0.2.0 release keeps the CPU + memory baseline and adds native Windows GPU monitoring:

- overall CPU utilization
- per-logical-core CPU utilization
- memory used, available, total, and utilization
- native Windows GPU discovery through DXGI
- separate physical GPU entries where Windows exposes distinct hardware adapters
- per-GPU utilization
- dedicated GPU memory used / total
- shared GPU memory used / total
- per-engine GPU activity
- optional NVIDIA GPU temperature through the installed NVIDIA NVML provider
- 2-second sampling
- one hour of in-memory history
- configurable 5 / 15 / 30 / 60 minute default history window
- selectable Graph / Bar overview visualizations for configurable metrics
- current / minimum / average / maximum summaries
- timestamped CSV export including GPU telemetry
- optional CPU, memory, and GPU top-bar widgets with configurable titles, colors, backgrounds, borders, and Graph / Bar visualization
- optional CPU, memory, and GPU system-tray tiles
- configurable warning and critical thresholds and health colors
- independent Warning, Critical, and Recovered sound/notification delivery with custom MP3/WAV support and built-in beep fallback
- two-sample anti-flap confirmation before state transitions
- plugin-scoped settings persistence
- tray and notification click-through to the System Monitor panel

GPU discovery and telemetry are started in the background after the plugin handshake so they do not block RemoteHand startup. Software fallback adapters are filtered, and a silent duplicate logical adapter may be hidden when an otherwise identical adapter has the real telemetry stream.

Temperature is intentionally opportunistic: it is shown only when a native/vendor provider already available on the machine reports it. The plugin does not require a third-party sensor application solely to obtain temperature data.

Default CPU thresholds are 70% healthy maximum and 95% critical minimum. Default memory thresholds are 70% healthy maximum and 90% critical minimum.

Top-bar sparklines retain per-sample health coloring, so only samples that crossed warning or critical thresholds use those colors. Tray tiles represent the current state.

## CSV export

System Monitor has two CSV export paths:

- Overview exports sampled telemetry for the selected in-memory history window.
- History exports persistent alert events matching the active History filters.

Overview CSV files are suggested using the local-time pattern:

```text
system-monitor-history-YYYY-MM-DD-HHmm.csv
```

Overview exports contain sampled telemetry for the selected history window, including timestamp, CPU utilization, memory values, per-core CPU utilization, available per-GPU utilization/memory/temperature fields, and per-disk usage, used/free/total bytes, plus read/write bytes per second.

Alert-History CSV files use the local-time pattern `system-monitor-alert-history-YYYY-MM-DD-HHmm.csv` and contain timestamp, metric, event, value, threshold, delivery flags, and details for the currently filtered persistent History rows.

## Platform direction

v0.2.0 is validated on Windows 11.

The implementation keeps a provider boundary so Linux support and additional hardware providers can be added later without moving hardware-specific behavior into RemoteHand core.

## CPU temperature

CPU temperature remains deliberately deferred beyond v0.2.0.

The v0.2.0 package does **not** require .NET, PawnIO, LibreHardwareMonitor, another separately installed hardware-monitoring application, or a third-party driver solely for temperature telemetry.

Earlier CPU-temperature provider experiments remain available in Git history but are not part of the v0.2.0 source tree or package.

## Architecture rule

RemoteHand core owns generic infrastructure.

System Monitor owns system-monitoring semantics, including sampling, metric normalization, thresholds, colors, alerts, history, CSV formatting, and presentation.

Do not add CPU-, GPU-, sensor-, vendor-, or hardware-specific logic to RemoteHand core merely to support this plugin.

## Current non-goals

Not in v0.5.0:

- CPU temperature
- disk temperature / SMART or NVMe health telemetry
- Linux implementation
- fan control
- voltage, tuning, or overclocking
- benchmarking or stress testing
- persistent telemetry database
- motherboard-wide sensor-suite replacement

See `V0.1.0-DESIGN.md` for the original baseline design. v0.2.0 extended that baseline with native Windows GPU monitoring, v0.3.0 added process inspection and selected process actions, and v0.4.0 added per-volume disk monitoring and disk-aware alert/surface integration, and v0.5.0 adds persistent alert-event History with independent enable/disable, filtering, clearing, and CSV export.
