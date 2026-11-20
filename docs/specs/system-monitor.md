# Spec: System monitor

- **Status:** In progress
- **Milestone:** M6
- **Crates:** `mao-sysmon`, `mao-ui`
- **Related:** [agents-panel](agents-panel.md), [workspace-layout](workspace-layout.md)

## Goal

A button that shows the machine's performance and health at a glance, without opening Task Manager or `htop`.

## Scope

- **CPU**: total + per-core usage, 60 s sparkline, frequency.
- **Memory**: RAM and swap/pagefile used/total.
- **Disks**: usage per drive/mount point, read/write per second.
- **Network**: upload/download per second (total and per interface).
- **Top processes**: top 10 by CPU or RAM, sortable; kill process with confirmation.
- **System**: OS, uptime, battery (if present), temperatures if available.
- **Status bar**: compact CPU % and RAM (can be disabled).
- Configurable alert thresholds (e.g. RAM > 90%) → badge on the activity bar.

## Out of scope

- GPU (Future: NVML on NVIDIA, `/sys` for AMD/Intel on Linux, D3DKMT on Windows).
- Persistent history.

## Technical design

- `mao-sysmon::Sampler` on its own thread using `sysinfo`; publishes a `Snapshot` via channel.
- 60-sample ring buffer for graphs.
- Frequencies: 1 s with the panel open; 2 s for the status bar only; stopped when nothing uses it.
- Shares the process list with `mao-agents` when both are active (a single process refresh).
- Graphs drawn with quads (no chart library).

## Performance

- Sampling cost < 1% of one core with the panel open.
- 0 with the panel closed and the status bar disabled.

## Implementation notes (M6)

- Panel in the sidebar: CPU (60 s graph, per-core bars, frequency), memory (graph) and page file/swap, network (↓ graph with ↑ overlay), disks, uptime and OS, top 12 processes sortable by CPU/memory, context menu (copy PID, kill with confirmation). Colors turn yellow ≥ 75% and red ≥ 90%.
- Sampling cost (Windows, ~400 processes): CPU/memory < 1 ms, network table ~8 ms (refreshed every 2 s), process list ~20 ms (every 3 s with the monitor open). **Measured total with the panel open: ~1.7% of one core** (sampling ≈ 1.2% + one redraw per second) — slightly above the < 1% goal. Closed: 0%, the thread blocks on its channel and `sysinfo` state is dropped.
- `sysinfo`'s `multithread` feature is off (rayon's spinning cost more than it saved).
- Status bar shows CPU/RAM/agents only while a sampling panel is open (no background sampling, to keep idle at 0%).
- Not yet: battery, temperatures, per-interface network, disk I/O rates, alert thresholds.

## Acceptance criteria

- [x] Values roughly match (±5%) Task Manager / `htop` (checked by eye on Windows: memory 85%, page file, disk C:).
- [x] Closing the panel stops sampling (verified: 0% CPU and `sysmon sample` traces stop).
- [x] Works without administrator privileges.

## Open questions

- Panel in the sidebar or as a full tab? (proposal: sidebar, with an option to open in a tab)
