# RemoteHand System Monitor

Status: **Planned / parked**

This repository is reserved for a future RemoteHand plugin that monitors local system hardware and exposes useful CPU, memory, and GPU telemetry through RemoteHand.

No implementation work has started yet. The notes below capture the current design direction only; they are not frozen scope.

## Purpose

System Monitor should provide a compact, native-feeling view of local machine health without turning RemoteHand core into a hardware-monitoring application.

The plugin should own hardware detection, sampling, metric normalization, thresholds, graphs, alerts, and presentation. RemoteHand core should continue to provide only generic plugin infrastructure such as runtime, storage, top-bar contributions, tray contributions, and native notifications.

## Platform direction

The plugin should be designed as **cross-platform**, not Windows-only.

Development will begin Windows-first because the available test laptop has:

- Intel CPU / integrated graphics
- NVIDIA discrete GPU
- Windows 11

Linux support should be part of the architecture from the beginning and added once the initial Windows metric model and UI are stable.

AMD GPU support is intentionally deferred until there is hardware or another reliable environment available for validation.

## Proposed provider architecture

Keep the UI and normalized metric model independent from operating-system and GPU-vendor implementations.

Conceptually:

```text
System Monitor UI
        |
Normalized metric model
        |
+------------------------+
| Platform / HW providers|
+------------------------+
   |        |        |
 CPU/RAM   Intel   NVIDIA
```

Likely provider families:

- CPU / memory provider
- Intel GPU provider
- NVIDIA GPU provider
- later platform/vendor providers only when they can be properly tested

Providers should degrade gracefully. A missing metric is an unavailable capability, not necessarily an error.

Examples:

- no NVIDIA GPU -> NVIDIA provider is inactive
- sleeping/idle discrete GPU -> valid idle/unavailable state
- temperature not exposed -> show unavailable rather than fabricating a value

## Initial metric ideas

### CPU

Potential metrics:

- overall utilization
- per-core utilization
- frequency
- temperature where reliably available
- system load where appropriate for the platform

### Memory

Potential metrics:

- used memory
- total memory
- usage percentage
- available/free memory

### Intel GPU

Potential metrics, subject to what can be obtained reliably:

- utilization
- frequency
- temperature
- memory information where meaningful and available

Windows and Linux may require different Intel implementations.

### NVIDIA GPU

NVML is the preferred direction because it can provide a relatively consistent NVIDIA path on both Windows and Linux.

Potential metrics:

- GPU utilization
- temperature
- VRAM used / total
- graphics / memory clocks
- fan speed where available
- power usage where available

## UI direction

The first UI should stay focused rather than becoming a full hardware diagnostics suite.

Potential main-panel content:

- CPU summary
- CPU per-core activity
- memory summary
- Intel GPU summary
- NVIDIA GPU summary
- compact recent-history graphs

The interface should adapt to available providers and should not reserve empty sections for hardware that does not exist.

## RemoteHand integration ideas

### Top bar

Allow the user to select compact metrics for a top-bar contribution.

Example:

```text
CPU 18% · 54°C   GPU 6% · 47°C
```

A hover/detail surface could expose additional selected metrics such as:

- CPU frequency
- hottest available CPU sensor
- memory usage
- GPU VRAM usage
- GPU clock
- fan speed

Exact layout should be designed after the underlying metric model is proven.

### System tray

A later version may provide optional tray contributions for selected metrics or status.

This should use RemoteHand's generic tray-contribution infrastructure rather than adding hardware-specific behavior to RemoteHand core.

### Alerts

Potential later alerts include:

- high CPU temperature
- high GPU temperature
- sustained high utilization
- low available memory

Alert semantics and history should remain plugin-owned.

## Cross-platform implementation notes

### Windows

Initial development and validation target.

Use stable native/system APIs where practical. GPU access should remain vendor/provider-specific rather than embedding assumptions into the common metric model.

### Linux

Planned after the Windows baseline.

Potential sources may include:

- `/proc`
- `/sys`
- hwmon
- DRM/sysfs interfaces
- vendor libraries such as NVML

Exact mechanisms should be selected only when implementation begins and can be tested on real Linux systems.

## Suggested development sequence

This is a rough direction, not a release commitment.

### v0.1 — CPU + memory baseline

- common metric model
- sampling lifecycle
- CPU utilization
- per-core utilization
- memory usage
- basic UI
- graceful unavailable-state handling

### v0.2 — NVIDIA GPU

- NVIDIA detection
- NVML integration
- utilization
- temperature
- VRAM
- clocks and other reliable metrics where available

### v0.3 — Intel GPU

- Intel GPU detection
- Windows implementation first
- Linux implementation later where practical
- expose only metrics that can be validated reliably

### Later

Only after real use demonstrates the need:

- configurable top-bar contribution
- richer hover details
- thresholds / alerts
- optional history
- tray contribution
- additional vendors or platforms

## Architecture rule

RemoteHand core owns generic infrastructure.

System Monitor owns system-monitoring semantics.

Do **not** move CPU-, GPU-, sensor-, vendor-, or hardware-specific logic into RemoteHand core merely to support this plugin.

Likewise, do not add speculative host APIs just to make the plugin architecture look complete. New RemoteHand host capabilities should be added only when a concrete plugin need demonstrates them.

## Current non-goals

For the initial work, avoid:

- AMD GPU support without a testable environment
- motherboard-wide sensor-suite replacement
- overclocking or tuning controls
- fan-control features
- process management
- benchmarking
- hardware stress testing
- speculative telemetry collection
- duplicating full tools such as HWiNFO, GPU-Z, or similar utilities

The goal is useful RemoteHand-integrated monitoring, not a complete hardware diagnostics package.

## Open design questions

Before implementation begins, decide:

- exact normalized metric schema
- sampling interval and retention strategy
- whether history belongs in the first release
- which metrics may be selected for the top bar
- how multiple GPUs are represented
- how hybrid Intel + NVIDIA systems expose sleeping/idle GPUs
- which CPU temperature source is sufficiently reliable on Windows
- the minimum Linux baseline to support
- alert model and whether it should reuse patterns proven by Target Monitor

## Development status

**Parked.**

The repository exists so the design has a clean home. Implementation should begin only after the current RemoteHand / Target Monitor work has reached an appropriate stopping point and the first System Monitor milestone is explicitly scoped.
