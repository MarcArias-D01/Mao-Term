# Spec: System monitor

- **Status:** Draft
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

## Acceptance criteria

- [ ] Values roughly match (±5%) Task Manager / `htop`.
- [ ] Closing the panel stops sampling (verifiable in logs).
- [ ] Works without administrator privileges.

## Open questions

- Panel in the sidebar or as a full tab? (proposal: sidebar, with an option to open in a tab)
