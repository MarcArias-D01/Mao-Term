# Spec: File tree (sidebar)

- **Status:** Draft
- **Milestone:** M4
- **Crates:** `mao-fs`, `mao-ui`, `mao-app`
- **Related:** [workspace-layout](workspace-layout.md), [shell-integration](shell-integration.md), [document-viewer](document-viewer.md)

## Goal

See the files of the directory you are in (in the focused terminal) in the sidebar, and act on them without typing paths.

## Scope

- Root = cwd of the focused pane (from OSC 7). Updates on `cd` or pane switch. Option to "pin" the root.
- If the cwd is inside a git repo, option to use the repo root and highlight the cwd.
- Lazy folder expansion; respects `.gitignore` (toggle to show hidden/ignored).
- File type icons; order: folders first, natural alphabetical.
- Live updates (create/delete/rename) with watchers only on expanded folders.
- Quick filter by typing with the sidebar focused.
- **Actions**:
  - Click a file → open in the document viewer (new or reused pane).
  - Double-click a folder / `Enter` → `cd` in the focused terminal (if at the prompt).
  - Drag onto the terminal → inserts the path (quoted per shell).
  - Context menu: copy path (absolute/relative), open in new terminal, reveal in OS file explorer, open with default app.

## Out of scope

- Creating/renaming/deleting files from the sidebar (Future).
- Per-file git status (M4 if time allows; otherwise Future).

## Technical design

- `mao-fs::Tree`: nodes with `path`, `kind`, `children: Option<Vec<NodeId>>` (None = not loaded).
- Directory reads on a background thread; results via channel; the UI shows "loading" on the node.
- `notify` watcher with debounce (~100 ms); watches removed on collapse.
- Virtualized list: only visible rows are drawn.

## Performance

- Opening a folder with 10,000 entries doesn't block the UI; shown in batches.
- Sidebar closed → no watchers or reads.

## Acceptance criteria

- [ ] After `cd` in the terminal, the tree changes root in < 200 ms.
- [ ] `node_modules` and `target` are hidden via `.gitignore`.
- [ ] Creating a file from the terminal shows it in the tree without manual refresh.
- [ ] Works on network paths and Windows drives, and with symlinks without loops.

## Open questions

- Without shell integration (cmd), which root do we show? (proposal: last known or home)
