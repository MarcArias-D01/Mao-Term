//! End-to-end shell integration: the injected script must report prompt marks,
//! exit codes and the cwd through the real PTY (ConPTY on Windows).

#![cfg(windows)]

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use mao_terminal::{GridSize, Palette, ShellEvent, SpawnOptions, Terminal, TerminalEvent};

const SIZE: GridSize = GridSize {
    cols: 100,
    rows: 30,
    cell_width: 8,
    cell_height: 16,
};

fn wait_for(
    rx: &mpsc::Receiver<TerminalEvent>,
    seen: &mut Vec<ShellEvent>,
    pred: impl Fn(&ShellEvent) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Ok(TerminalEvent::Shell(e)) = rx.recv_timeout(Duration::from_millis(100)) {
            let hit = pred(&e);
            seen.push(e);
            if hit {
                return true;
            }
        }
    }
    false
}

#[test]
fn powershell_reports_prompt_cwd_and_exit_code() {
    let dir = std::env::temp_dir().join(format!("mao-int-{}", std::process::id()));
    let (tx, rx) = mpsc::channel();
    let term = Terminal::spawn(
        SpawnOptions {
            program: Some("powershell.exe".into()),
            args: vec!["-NoLogo".into(), "-NoProfile".into()],
            working_directory: None,
            scrollback: 100,
            palette: Palette::new([[0; 3]; 16], [255; 3], [0; 3], [128; 3], [64; 3]),
            integration_dir: Some(dir),
        },
        SIZE,
        move |e| {
            let _ = tx.send(e);
        },
    )
    .expect("spawn");

    let mut seen = Vec::new();
    assert!(
        wait_for(&rx, &mut seen, |e| *e == ShellEvent::PromptEnd),
        "no prompt end; events: {seen:?}"
    );
    assert!(seen.contains(&ShellEvent::PromptStart), "events: {seen:?}");

    term.write(b"cd C:\\Windows\r".as_slice());
    assert!(
        wait_for(&rx, &mut seen, |e| *e
            == ShellEvent::Cwd(PathBuf::from(r"C:\Windows"))),
        "no cwd update; events: {seen:?}"
    );

    term.write(b"cmd /c exit 7\r".as_slice());
    assert!(
        wait_for(&rx, &mut seen, |e| *e == ShellEvent::CommandEnd(Some(7))),
        "no exit code; events: {seen:?}"
    );
    term.write(b"exit\r".as_slice());
}
