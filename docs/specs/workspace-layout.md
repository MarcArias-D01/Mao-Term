# Spec: Workspace — tabs, splits, sidebar and palette

- **Status:** In progress
- **Milestone:** M2 (sidebar in M4)
- **Crates:** `mao-ui`, `mao-app`
- **Related:** [file-tree](file-tree.md), [agents-panel](agents-panel.md), [system-monitor](system-monitor.md)

## Goal

Open several terminals (and documents) effortlessly, organize them in tabs and panes, and reach the side panels with one click or shortcut.

## Scope

- **Tabs**: create, close, reorder (drag), rename, duplicate (same cwd), activity/bell indicator.
- **Splits**: binary tree per tab; split right/down, resize by dragging, close, temporarily maximize a pane.
- **Panes**: `Terminal` or `Document`. A new pane inherits the cwd of the focused pane.
- **Activity bar**: Files, Agents, Monitor, Settings buttons (with badges: number of agents, system alerts).
- **Sidebar**: shows the active panel, resizable, collapsible; remembers its width.
- **Status bar**: cwd, git branch, compact CPU/RAM, agent count.
- **Command palette**: fuzzy search over all registered actions + their shortcuts.
- Restore layout (tabs, splits, cwd) on reopen — optional, configurable.

## Out of scope

- Multiple synchronized windows, dragging tabs between windows (Future).
- Custom title bar (M7).

## Behavior

| Action | Shortcut |
|---|---|
| New tab | `Ctrl+Shift+T` |
| Close pane/tab | `Ctrl+Shift+W` |
| Next / previous tab | `Ctrl+Tab` / `Ctrl+Shift+Tab` |
| Go to tab N | `Alt+1…9` |
| Split right / down | `Ctrl+Shift+D` / `Ctrl+Shift+E` |
| Move focus between panes | `Alt+Arrows` |
| Maximize pane | `Ctrl+Shift+Enter` |
| Command palette | `Ctrl+Shift+P` |
| Toggle sidebar | `Ctrl+Shift+B` |
| Files / Agents / Monitor panel | `Ctrl+Shift+1` / `2` / `3` |

## Technical design

```rust
enum PaneKind { Terminal(SessionId), Document(DocId) }
enum SplitNode { Leaf(PaneId), Split { dir: Axis, ratio: f32, a: Box<SplitNode>, b: Box<SplitNode> } }
struct Tab { title: String, root: SplitNode, focused: PaneId }
struct Workspace { tabs: Vec<Tab>, active: usize, sidebar: SidebarState }
```

- **Action registry**: each action (`Action::NewTab`, `Action::SplitRight`…) has an id, name and default shortcut. The palette, keybindings and menus all use the same registry.
- Chrome layout via `taffy`; each pane's interior is drawn by its owner.
- Non-visible panes (other tabs) keep processing their PTY but are not rendered.

## Performance

- Switching tabs < 1 frame.
- 20 open tabs at idle: ~0% CPU.

## Acceptance criteria

- [ ] Every action in the table works via shortcut and from the palette.
- [ ] Resizing a split resizes the PTYs on both sides.
- [ ] Closing the last pane of a tab closes the tab; the last tab closes the window (configurable).
- [ ] A new pane inherits cwd when shell integration is active.

## Open questions

- Tab bar on top or vertical (side)?
- Session restore enabled by default?
