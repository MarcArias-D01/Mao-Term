//! End-to-end: spawn a real process in a PTY (ConPTY on Windows) and read its output.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use mao_terminal::{GridSize, Palette, SpawnOptions, Terminal, TerminalEvent};

const SIZE: GridSize = GridSize {
    cols: 80,
    rows: 24,
    cell_width: 8,
    cell_height: 16,
};

fn palette() -> Palette {
    Palette::new([[0; 3]; 16], [255; 3], [0; 3], [128; 3], [64; 3])
}

fn command(text: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        ("cmd.exe".into(), vec!["/C".into(), format!("echo {text}")])
    } else {
        ("/bin/sh".into(), vec!["-c".into(), format!("echo {text}")])
    }
}

fn screen_text(term: &mut Terminal) -> String {
    let mut text = String::new();
    term.render(true, true, |_, cells| {
        text.extend(cells.iter().map(|c| c.ch));
        text.push('\n');
    });
    text
}

/// The command's output is a line of its own (the typed command may wrap).
fn has_output_line(text: &str) -> bool {
    text.lines().any(|l| l.trim_end() == "round-trip-ok")
}

#[test]
fn echo_output_reaches_the_grid() {
    let (program, args) = command("mao-term-says-hello");
    let (tx, rx) = mpsc::channel();
    let mut term = Terminal::spawn(
        SpawnOptions {
            program: Some(program),
            args,
            working_directory: None,
            scrollback: 100,
            palette: palette(),
        },
        SIZE,
        move |event| {
            let _ = tx.send(event);
        },
    )
    .expect("spawn");

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut exit = None;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(TerminalEvent::Exited(code)) => {
                exit = Some(code);
                break;
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let text = screen_text(&mut term);
    assert!(exit.is_some(), "process did not exit; screen:\n{text}");
    assert!(
        text.contains("mao-term-says-hello"),
        "output not found; screen:\n{text}"
    );
}

#[test]
fn input_round_trips_through_the_shell() {
    // `cat` / `more` echo stdin back; this checks the write path.
    let (program, args): (String, Vec<String>) = if cfg!(windows) {
        ("cmd.exe".into(), vec!["/Q".into()])
    } else {
        ("/bin/sh".into(), vec![])
    };
    let (tx, rx) = mpsc::channel();
    let mut term = Terminal::spawn(
        SpawnOptions {
            program: Some(program),
            args,
            working_directory: None,
            scrollback: 100,
            palette: palette(),
        },
        SIZE,
        move |event| {
            let _ = tx.send(event);
        },
    )
    .expect("spawn");

    term.write(b"echo round-trip-ok\r".as_slice());

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut text = String::new();
    while Instant::now() < deadline {
        let _ = rx.recv_timeout(Duration::from_millis(100));
        text = screen_text(&mut term);
        if has_output_line(&text) {
            break;
        }
    }
    assert!(
        has_output_line(&text),
        "echo output not found; screen:\n{text}"
    );
    term.write(b"exit\r".as_slice());
}
