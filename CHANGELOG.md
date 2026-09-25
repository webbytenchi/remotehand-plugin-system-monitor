# Changelog

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
