# RemoteHand System Monitor

Status: **v0.3.1**

RemoteHand System Monitor is a Windows-first plugin for local CPU, memory, and GPU monitoring. It keeps hardware-monitoring semantics inside the plugin while RemoteHand core provides only generic plugin infrastructure.

## v0.3.1 scope

v0.3.1 is a focused GPU-surface consistency bugfix release.

- applies the existing duplicate/inactive GPU-adapter visibility rule to top-bar and system-tray contributions
- keeps Overview, Settings, top-bar, and tray GPU visibility aligned
- preserves legitimate idle GPUs at 0% utilization
- suppresses only the duplicate logical adapter when an otherwise identical adapter is the sole instance with meaningful telemetry
- adds regression coverage for duplicate-LUID filtering and emitted top-bar/tray surfaces

This fixes the case where a silent duplicate Intel GPU LUID could be hidden correctly in Overview and Settings but still remain visible in the RemoteHand top bar without a corresponding Settings control.

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

CSV files are suggested using the local-time pattern:

```text
system-monitor-history-YYYY-MM-DD-HHmm.csv
```

Exports contain sampled telemetry for the selected history window, including timestamp, CPU utilization, memory values, per-core CPU utilization, and available per-GPU utilization, memory, and temperature fields.

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

Not in v0.3.0:

- CPU temperature
- Linux implementation
- fan control
- voltage, tuning, or overclocking
- benchmarking or stress testing
- persistent telemetry database
- motherboard-wide sensor-suite replacement

See `V0.1.0-DESIGN.md` for the original baseline design. v0.2.0 extended that baseline with native Windows GPU monitoring, and v0.3.0 adds process inspection and selected process actions.
