# RemoteHand System Monitor

Status: **v0.1.0 release candidate**

RemoteHand System Monitor is a Windows-first plugin for local CPU and memory monitoring. It keeps hardware-monitoring semantics inside the plugin while RemoteHand core provides only generic plugin infrastructure.

## v0.1.0 scope

The first release provides:

- overall CPU utilization
- per-logical-core CPU utilization
- memory used, available, total, and utilization
- 2-second sampling
- one hour of in-memory history
- 5 / 15 / 30 / 60 minute history views
- live CPU and memory graphs
- current / minimum / average / maximum summaries
- timestamped CSV export
- optional CPU and memory top-bar widgets
- optional CPU and memory system-tray tiles
- configurable warning and critical thresholds and colors
- transition-based sound and native notification alerts
- two-sample anti-flap confirmation before state transitions
- plugin-scoped settings persistence
- tray and notification click-through to the System Monitor panel

Default CPU thresholds are 70% healthy maximum and 95% critical minimum. Default memory thresholds are 70% healthy maximum and 90% critical minimum.

Top-bar sparklines retain per-sample health coloring, so only samples that crossed warning or critical thresholds use those colors. Tray tiles represent the current state.

## CSV export

CSV files are suggested using the local-time pattern:

```text
system-monitor-history-YYYY-MM-DD-HHmm.csv
```

Exports contain sampled telemetry for the selected history window, including timestamp, CPU utilization, memory values, and per-core CPU utilization.

## Platform direction

v0.1.0 is validated on Windows 11.

The implementation keeps a provider boundary so Linux support and additional hardware providers can be added later without moving hardware-specific behavior into RemoteHand core.

## CPU temperature

CPU temperature is deliberately deferred beyond v0.1.0.

The v0.1.0 package does **not** require .NET, PawnIO, LibreHardwareMonitor, another separately installed hardware-monitoring application, or a third-party driver solely for temperature telemetry.

Earlier temperature-provider experiments remain available in Git history but are not part of the v0.1.0 source tree or package.

## Architecture rule

RemoteHand core owns generic infrastructure.

System Monitor owns system-monitoring semantics, including sampling, metric normalization, thresholds, colors, alerts, history, CSV formatting, and presentation.

Do not add CPU-, GPU-, sensor-, vendor-, or hardware-specific logic to RemoteHand core merely to support this plugin.

## Current non-goals

Not in v0.1.0:

- CPU temperature
- NVIDIA, Intel, or AMD GPU monitoring
- Linux implementation
- fan control
- voltage, tuning, or overclocking
- process management
- benchmarking or stress testing
- persistent telemetry database
- motherboard-wide sensor-suite replacement

See `V0.1.0-DESIGN.md` for the accepted v0.1.0 design and acceptance criteria.
