# Spec: Configuration, themes and keybindings

- **Status:** In progress
- **Milestone:** M0 (minimal) → M7 (complete)
- **Crates:** `mao-config`
- **Related:** all

## Goal

Everything configurable through a readable text file, with hot reload, and good defaults so nothing needs touching.

## Scope

- `config.toml` file:
  - Windows: `%APPDATA%\mao-term\config.toml`
  - Linux: `$XDG_CONFIG_HOME/mao-term/config.toml` (`~/.config/…`)
- Hot reload on save; config errors are shown in a notice without breaking anything (last valid config is kept).
- Sections: `font`, `theme`, `window`, `terminal` (scrollback, cursor), `shell` (profiles: program, args, env, integration on/off), `input` (editor on/off, auto menu), `sidebar`, `agents` (detectors), `sysmon`, `keybindings`.
- **Themes** in TOML files (`themes/<name>.toml`): 16 ANSI colors + background, foreground, cursor, selection + UI colors. Bundled themes: default dark and light + a few popular ones. Optionally follow the OS light/dark theme.
- **Keybindings**: `[[keybindings]] key = "ctrl+shift+t", action = "NewTab"`; disable with `action = "None"`.
- Shell profiles selectable when creating a tab (pwsh, cmd, WSL, bash…). On Windows, detect WSL distros.
- Open the config from the palette (`Preferences: Open config`).

## Implemented (themes)

- `theme = "<name>"` at the top level of `config.toml` (built-in or `<config dir>/themes/<name>.toml`, matched ignoring case/spaces/dashes) + optional `[colors]` overrides (`background`, `foreground`, `cursor`, `selection`, `ansi`). A `[theme]` table of colors (older configs) still works.
- Theme file = `name` (optional, defaults to the file name) + the five color keys. Built-in: Mao Dark, Tokyo Night, Catppuccin Mocha, Dracula, Nord, Gruvbox Dark, One Dark, Rosé Pine, Solarized Dark, Solarized Light, GitHub Light (`crates/mao-config/themes/`).
- Palette → *Select Theme…*: swatches, live preview while moving (terminals, chrome, editor, documents), Enter writes `theme = "…"` into `config.toml` keeping the rest of the file, Esc restores. *Open Themes Folder* creates/opens the folder.
- UI colors are derived from the theme (accent = cursor color); no separate UI keys yet.
- Pending: hot reload of the whole config file, follow OS light/dark, keybindings, shell profiles.

## Out of scope

- Graphical settings UI (M7).
- Cloud sync.

## Example

```toml
[font]
family = "JetBrains Mono"
size = 13.0

theme = "Tokyo Night"

[colors]          # optional overrides
cursor = "#ff9e64"

[shell]
default = "pwsh"

[[shell.profile]]
name = "pwsh"
program = "pwsh.exe"
args = ["-NoLogo"]

[input]
editor = true
auto_menu = false

[[keybindings]]
key = "ctrl+shift+t"
action = "NewTab"
```

## Technical design

- `serde` with `#[serde(default)]` everywhere: an empty file is valid.
- Validation with messages pointing to line and key.
- `notify` with debounce on the file and the themes folder.
- Generated JSON schema (`schemars`) for editor autocomplete — optional.

## Acceptance criteria

- [ ] Starts with defaults when there is no config file.
- [ ] Changing `font.size` and saving applies without restarting.
- [ ] A syntax error shows a notice with the line and keeps the previous config.

## Open questions

- Default theme name and brand palette.
