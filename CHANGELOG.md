# Changelog

## v0.2.0

First GPU-enabled System Monitor release.

### Added
- Native Windows GPU discovery through DXGI with hardware-adapter filtering.
- Per-GPU utilization telemetry from Windows GPU performance counters.
- Dedicated and shared GPU memory usage/capacity telemetry.
- Per-engine GPU activity with stable engine ordering.
- Optional NVIDIA GPU temperature through the installed NVIDIA NVML provider; temperature UI is omitted when no native/vendor reading is available.
- Dynamic per-GPU Settings tabs with independent thresholds, colors, alerts, overview visualization, top-bar visualization, titles, and tray/top-bar visibility.
- GPU telemetry in the Metrics table and CSV export.
- GPU monitoring status and configurable default history window in General settings.

### Changed
- GPU display names and default top-bar titles use the reported adapter name instead of generic GPU numbers.
- Saved default history window is applied immediately to Overview.
- GPU discovery and sampling run off the startup critical path.
- General settings layout was rebalanced for wider panels.
- Overview History label/dropdown spacing was polished.

### Fixed
- Filtered the Microsoft Basic Render Driver from hardware GPU discovery.
- Suppressed silent duplicate logical GPU adapters when an otherwise identical adapter has the real telemetry stream.
- Prevented GPU engine cards from changing positions as utilization changes.
- Hid unavailable GPU temperature cards, settings, and surfaces.
- Kept genuinely customized GPU titles when migrating old generated defaults to adapter-name-based titles.

## v0.1.1

System Monitor v0.1.1 is a polish and usability release for the Windows CPU + memory baseline.

### Changed
- Added selectable Graph / Bar visualization for CPU and Memory overview cards.
- Added selectable Graph / Bar visualization for CPU and Memory top-bar widgets.
- Added configurable top-bar title, title color, normal value color, alert value color, background, and border.
- Added independent Warning, Critical, and Recovered alert delivery settings.
- Added custom MP3/WAV alert sounds with built-in beep fallback.
- Kept alerts transition-based with two-sample anti-flap confirmation and no reminder spam.
- Improved Settings layout and compactness.
- Added Target Monitor-style floating notices.
- Removed the redundant Memory Used metric as a separate configurable surface while retaining used / total / available memory data in the Memory view and CSV export.
- Fixed Overview Graph / Bar switching so only the selected visualization is shown.
- Removed duplicate value labels from Overview bars.

### Defaults
- CPU overview: Graph.
- CPU top bar: Graph.
- Memory overview: Bar.
- Memory top bar: Bar.
- Warning: Sound on, Notification on.
- Critical: Sound on, Notification on.
- Recovered: Sound on, Notification off.

## v0.1.0

Initial Windows-first System Monitor baseline with CPU utilization, per-core activity, memory telemetry, history, CSV export, top-bar and tray contributions, configurable thresholds/colors, alerts, and plugin-scoped settings.
