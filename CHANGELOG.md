# Changelog

## v0.3.2

Host-API cleanup and persistence release.

### Changed
- Process UI preferences now persist through RemoteHand plugin-scoped storage instead of browser `localStorage`.
- Custom alert-sound selection now uses RemoteHand's native `files.open` host API.
- Selected alert sounds are read by the plugin backend and returned to the UI as encoded audio.
- Removed browser-side file reading and FileReader fallbacks for custom alert sounds.

### Compatibility
- Plugin API remains **v1**.
- Existing monitoring, GPU visibility, process behavior, alerts, and saved settings remain compatible.

### Verification
- Rust tests passed with **17 tests and 0 failures**.
- Windows release build passed.
- Manual Windows acceptance confirmed process preference persistence, native sound picking, custom sound playback, and persistence after a full restart.

## v0.3.1

Focused GPU-surface consistency bugfix.

### Fixed
- Applied the existing duplicate/inactive GPU-adapter visibility rule to top-bar and system-tray contributions.
- Kept Overview, Settings, top-bar, and tray GPU visibility consistent.
- Prevented a silent duplicate GPU LUID from remaining visible in the top bar without a corresponding Settings control.
- Preserved legitimate idle GPUs at 0% utilization; only the duplicate logical adapter without meaningful telemetry is suppressed when the match is unambiguous.
- Added regression tests for duplicate-LUID filtering and emitted top-bar/tray surfaces.

## v0.3.0

Processes milestone for the Windows System Monitor plugin.

### Added
- Processes tab with full Windows process enumeration, including background/headless processes.
- Search across process name, PID, path, command line, and user while retaining matching parent context.
- Expandable parent/child process hierarchy with optional compact launcher-chain folding.
- Lazy/cached executable icons.
- Sortable and resizable process columns with persisted widths.
- All, Background, Visible, and GPU-active process filters.
- Top-level process-group pagination with selectable group counts.
- Process Details dialog.
- Open file location action.
- Copy submenu for process name, PID, executable path, and command line.
- Guarded End task action with a custom confirmation dialog and PID/name reuse protection.
- Plugin-owned themed sidebar icon packaged with the plugin.

### Changed
- Process tree pagination is based on top-level groups so expanded trees remain structurally coherent.
- Process search results remain collapsible and expandable.
- Compact-tree status reports hidden launcher wrappers.

### Removed
- Removed the experimental Open / Focus window action because Windows foreground activation was not reliable enough across window-hosting models.

### Deferred
- Per-process GPU attribution.
- Publisher and digital-signature metadata.
- End process tree.

## v0.2.1

Bugfix release for alert delivery setting persistence.

### Fixed
- Fixed Warning, Critical, and Recovered Sound/Notification checkbox changes reverting after Save Settings.
- Alert delivery checkbox choices now persist across Settings tab changes and full RemoteHand restarts.
- Added a regression test covering the alert-delivery settings collector.

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
