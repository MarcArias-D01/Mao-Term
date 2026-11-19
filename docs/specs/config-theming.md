# Spec: Configuration, themes and keybindings

- **Status:** Draft
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

## Out of scope

- Graphical settings UI (M7).
- Cloud sync.

## Example

```toml
[font]
family = "JetBrains Mono"
size = 13.0

[theme]
name = "mao-dark"

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
