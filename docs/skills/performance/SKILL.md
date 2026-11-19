---
name: performance
description: Mao-Term performance rules and procedures. Use when touching rendering, PTY, parser, input, autocomplete or any hot path, when adding anything that runs in the background, or to measure latency, throughput, idle CPU or memory.
---

# Performance

Budgets live in `docs/design.md` → "Performance budgets". Breaking them is a bug.

## Rules

1. **Render on demand.** No `ControlFlow::Poll` and no `request_redraw()` every frame. Mark dirty only if something visible changed.
2. **Zero cost when closed.** Inactive panels/features have no running threads, timers or watchers.
3. **The UI thread is sacred.** No disk, processes, network or long lock waits.
4. **No allocations in steady state** in the frame loop and in PTY byte processing.
5. **Work proportional to change.** Grid damage tracking, virtualized lists, reshape only dirty rows.
6. **Coalesce.** PTY wakeups, watcher events (debounce) and sampling are batched.
7. **Slow timers.** Nothing below 1 s in the background except cursor blink.

## How to measure

Always in `--release` (a `profiling` profile with symbols: `debug = "line-tables-only"`).

| What | Tool |
|---|---|
| CPU profiling (Win + Linux) | `samply record target/profiling/mao-app` |
| Spans / per-frame time | `tracing` + `tracing-tracy` (`tracy` feature) with Tracy |
| Throughput | `vtebench`; `cat` of a 100 MB file with `time`/`Measure-Command` |
| Key → pixel latency | Typometer, or per-event timestamps in `trace` logs |
| Idle CPU | Task Manager / `top -p <pid>` for 60 s |
| Memory | Working set (Windows) / RSS (Linux) after 1 tab and 10k lines |
| Allocations per frame | global allocator counter in debug builds (`alloc-count` feature) |

## Before closing a hot-path change

- [ ] Measured before and after; numbers in the PR description.
- [ ] Idle is still ~0% CPU.
- [ ] No regression > 5% in throughput or frame time.

## Anti-patterns to reject in review

- `format!`/`to_string()` per cell or per byte.
- Cloning the whole grid every frame.
- Recomputing chrome layout when neither size nor state changed.
- `thread::sleep` in polling loops (use blocking channels or event loop timers).
- Holding the `Term` lock while submitting to the GPU.
