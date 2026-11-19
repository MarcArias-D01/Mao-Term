---
name: rust-conventions
description: Mao-Term Rust code conventions (structure, errors, logging, concurrency, dependencies, tests). Use when writing or reviewing any Rust code in the repo.
---

# Rust conventions

## Toolchain and workspace

- Stable Rust, **edition 2024**, toolchain pinned in `rust-toolchain.toml` and `rust-version` in the workspace.
- Shared dependencies declared in `[workspace.dependencies]`; crates use `dep.workspace = true`.
- Workspace lints in `[workspace.lints]` (`unsafe_code = "deny"` except justified `platform/` modules).

## Structure

- One crate = one responsibility (see `docs/design.md`). No new crates without an ADR.
- `lib.rs` exposes a small API; the rest is `pub(crate)`.
- OS code in `src/platform/{windows,unix}.rs` (`cross-platform` skill).

## Errors

- Libraries: error enums with `thiserror`. Binary (`mao-app`): `anyhow` with `.context("…")`.
- No `unwrap()` outside tests. `expect("invariant: …")` only for real invariants.
- No `panic!` on recoverable paths. A failing panel shows its error inside the panel.

## Logging

- `tracing` (`info!`, `debug!`, `trace!`); never `println!`.
- Spans (`#[tracing::instrument(skip_all)]`) on operations worth profiling, **not** on per-cell/per-byte functions.

## Concurrency

- Dedicated named threads (`thread::Builder::new().name("pty-reader-3")`) + `crossbeam-channel`. No `tokio` without an ADR.
- Always wake the UI via `EventLoopProxy`.
- Locks: never across I/O or GPU calls; short critical sections. `parking_lot` for mutexes.

## Performance (summary; see the `performance` skill)

- No per-frame/per-byte allocations in hot paths: reuse buffers (`clear()` instead of `Vec::new()`).
- Avoid `String` where `&str`/`Cow` suffices; `SmallVec` for small hot collections.

## Dependencies

Before adding a crate, check and mention in the commit/PR:

- Compatible license (MIT, Apache-2.0, BSD, Zlib, ISC, MPL-2.0, OFL for fonts). `cargo deny check` must pass.
- Maintained (releases within the last year) and builds on Windows and Linux.
- Cost: compile time and binary size; disable unused `default-features`.
- Read the docs of **the exact version** you are using (docs.rs); don't assume the API from memory.

## Tests

- Unit tests next to the code (`#[cfg(test)] mod tests`); integration tests in `crates/<x>/tests/`.
- Domain logic must be testable without a GPU or window.
- For the emulator: tests with recorded byte sequences → expected grid state.
- OS-dependent tests marked `#[cfg(windows)]` / `#[cfg(unix)]`.

## Style

- `cargo fmt` (default config). English names.
- Comments explain the *why*, not the *what*. Doc comments (`///`) on all public APIs.
