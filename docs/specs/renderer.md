# Spec: Renderer

- **Status:** Draft
- **Milestone:** M0–M1
- **Crates:** `mao-render`, `mao-ui`
- **Related:** [terminal-core](terminal-core.md), [ADR 0001](../decisions/0001-render-stack.md)

## Goal

Draw the terminal and UI on the GPU with minimal latency and zero cost when nothing changes.

## Scope

- wgpu initialization (low-power adapter by default, configurable), surface, resize, DPI/scale.
- **Instanced quad** pipeline: cell backgrounds, cursor, selection, underlines, borders, UI rects with corner radius.
- **Text** pipeline with `glyphon`: primary monospace font + fallback (emoji, CJK, symbols), bold/italic, optional ligatures.
- **Image** pipeline (for the document viewer and icons).
- `DrawList` abstraction independent of wgpu: `Rect`, `Text`, `Image`, `Clip`.
- Default font embedded in the binary (OFL license) + system fonts.

## Out of scope

- Complex animations, window blur/transparency (Future; Windows acrylic maybe in M7).

## Technical design

- **On-demand rendering**: only on `RedrawRequested` after marking dirty. Cursor blink uses `ControlFlow::WaitUntil`.
- **Per-row cache**: each visible grid row has its own `glyphon::Buffer`; rebuilt only if the row is in `alacritty_terminal`'s damage or theme/font changed.
- Cells are placed on a fixed grid (cell width = font advance); shaping never decides horizontal grid positions.
- Instance buffers reused across frames (no per-frame allocations in steady state).
- `PresentMode::Fifo` by default; `Mailbox`/`Immediate` configurable for low latency.
- Backends: DX12 on Windows, Vulkan on Linux, with automatic fallback.

## Performance

- Typical frame (1 pane, 50 rows, few dirty) < 2 ms CPU.
- Zero allocations per frame in steady state (verified with an alloc counter in debug).
- No changes: 0 frames.

## Acceptance criteria

- [ ] Crisp text at 100%, 125%, 150%, 200% scale.
- [ ] Emoji and CJK render with the fallback font and take 2 cells where appropriate.
- [ ] Moving the window between monitors with different DPI re-rasterizes correctly.
- [ ] Idle with blinking cursor < 0.5% CPU.
- [ ] Works on machines without a discrete GPU and in VMs (software/GL fallback if needed).

## Open questions

- Default font? (candidates: JetBrains Mono, Cascadia Code — both OFL).
- Subpixel (ClearType) or grayscale-only antialiasing?
