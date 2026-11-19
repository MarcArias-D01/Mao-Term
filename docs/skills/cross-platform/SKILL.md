---
name: cross-platform
description: How to write Mao-Term code that works on Windows and Linux (paths, shells, processes, windowing, fonts, CI) and where to isolate OS-specific code.
---

# Cross-platform (Windows + Linux)

## Main rule

OS-specific code lives in `platform/` modules and exposes **one common API**:

```
crates/mao-xxx/src/platform/mod.rs      // common trait or functions + cfg
crates/mao-xxx/src/platform/windows.rs  // #[cfg(windows)]
crates/mao-xxx/src/platform/unix.rs     // #[cfg(unix)]
```

No `#[cfg(windows)]` scattered through business logic.

## Differences to keep in mind

| Topic | Windows | Linux |
|---|---|---|
| Paths | `\`, drives `C:\`, UNC `\\srv\share`, case-insensitive | `/`, case-sensitive |
| Paths in code | Always `Path`/`PathBuf`; `dunce::canonicalize` to avoid `\\?\` | `std::fs::canonicalize` |
| Shell quoting | pwsh: `'…'` with `''` escape; cmd: `"…"` | POSIX: `'…'` with `'\''` |
| Default shell | `pwsh` → `powershell` → `cmd` | `$SHELL` → `/bin/bash` |
| WSL | Per-distro profiles (`wsl.exe -d <distro>`) | — |
| Another process's cwd | Unreliable (we rely on OSC 7) | `/proc/<pid>/cwd` |
| Kill process tree | Job Object | `kill(-pgid, SIGHUP)` |
| System fonts | `C:\Windows\Fonts`, per-user folder | fontconfig |
| Windowing | Win32 | X11 and **Wayland** (test both) |
| GPU | DX12 (Vulkan fallback) | Vulkan (GL fallback) |
| Config/data paths | `directories` crate | `directories` crate (XDG) |
| Open with default app | `open` crate | `open` crate (`xdg-open`) |
| Line endings | Accept `\r\n` when reading user files | `\n` |

## CI

- Matrix `windows-latest` + `ubuntu-latest` for fmt, clippy and test.
- Run `cargo clippy` on each OS (code under the other OS's `cfg` isn't checked locally).

## Checklist

- [ ] Builds on both OSes? If you could only test one, say so explicitly when done.
- [ ] Paths with spaces, Unicode and (on Windows) another drive?
- [ ] Default behavior is natural for each OS (shortcuts, right click, clipboard)?
- [ ] On Linux, tested on Wayland and X11 (or noted as pending)?
