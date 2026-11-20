//! Incremental scanner for the shell-integration OSC sequences that
//! `alacritty_terminal` ignores: OSC 133 (prompt/command marks), OSC 7 (cwd)
//! and Mao-Term's own OSC 7770 (`mao open <file>`).
//! See `docs/specs/shell-integration.md`.
//!
//! Runs on the PTY I/O thread over every byte read, so the common path (no ESC
//! in the chunk) must stay a single fast scan.

use std::path::PathBuf;

/// Something the shell told us through shell integration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    /// `OSC 133;A` — the prompt starts.
    PromptStart,
    /// `OSC 133;B` — the prompt ended; the shell waits for input.
    PromptEnd,
    /// `OSC 133;C` — a command started executing.
    CommandStart,
    /// `OSC 133;D[;code]` — the command finished.
    CommandEnd(Option<i32>),
    /// `OSC 7;file://host/path` — current directory.
    Cwd(PathBuf),
    /// `OSC 7770;open;file://host/path` — show a file in the document viewer.
    Open(PathBuf),
}

/// Longest OSC payload we keep; longer ones are not ours and are skipped.
const MAX_OSC: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    Osc,
    /// Inside an OSC, after an ESC (expecting `\` for ST).
    OscEscape,
}

#[derive(Debug)]
pub struct OscScanner {
    state: State,
    buf: Vec<u8>,
    overflow: bool,
}

impl Default for OscScanner {
    fn default() -> Self {
        Self {
            state: State::Ground,
            buf: Vec::with_capacity(256),
            overflow: false,
        }
    }
}

impl OscScanner {
    /// Feeds a chunk of PTY output, calling `on_event` for each complete sequence.
    pub fn feed(&mut self, bytes: &[u8], mut on_event: impl FnMut(ShellEvent)) {
        let mut i = 0;
        while i < bytes.len() {
            if self.state == State::Ground {
                // Fast path: jump to the next ESC.
                match bytes[i..].iter().position(|&b| b == 0x1b) {
                    Some(off) => {
                        i += off + 1;
                        self.state = State::Escape;
                    }
                    None => return,
                }
                continue;
            }
            let b = bytes[i];
            i += 1;
            match self.state {
                State::Ground => unreachable!("handled above"),
                State::Escape => {
                    self.state = if b == b']' {
                        self.buf.clear();
                        self.overflow = false;
                        State::Osc
                    } else if b == 0x1b {
                        State::Escape
                    } else {
                        State::Ground
                    };
                }
                State::Osc => match b {
                    0x07 => self.finish(&mut on_event),
                    0x1b => self.state = State::OscEscape,
                    // CAN/SUB abort the sequence.
                    0x18 | 0x1a => self.state = State::Ground,
                    _ => self.push(b),
                },
                State::OscEscape => {
                    if b == b'\\' {
                        self.finish(&mut on_event);
                    } else {
                        // ESC inside an OSC that isn't ST: the OSC was abandoned
                        // and a new escape sequence started.
                        self.state = if b == b']' {
                            self.buf.clear();
                            self.overflow = false;
                            State::Osc
                        } else {
                            State::Ground
                        };
                    }
                }
            }
        }
    }

    fn push(&mut self, b: u8) {
        if self.buf.len() < MAX_OSC {
            self.buf.push(b);
        } else {
            self.overflow = true;
        }
    }

    fn finish(&mut self, on_event: &mut impl FnMut(ShellEvent)) {
        self.state = State::Ground;
        if self.overflow {
            return;
        }
        if let Some(event) = parse(&self.buf) {
            on_event(event);
        }
    }
}

fn parse(payload: &[u8]) -> Option<ShellEvent> {
    let text = std::str::from_utf8(payload).ok()?;
    let (code, rest) = text.split_once(';').unwrap_or((text, ""));
    match code {
        "133" => {
            let mut parts = rest.split(';');
            match parts.next()? {
                "A" => Some(ShellEvent::PromptStart),
                "B" => Some(ShellEvent::PromptEnd),
                "C" => Some(ShellEvent::CommandStart),
                "D" => Some(ShellEvent::CommandEnd(
                    parts.next().and_then(|c| c.trim().parse().ok()),
                )),
                _ => None,
            }
        }
        "7" => parse_file_url(rest).map(ShellEvent::Cwd),
        "7770" => match rest.split_once(';')? {
            ("open", url) => parse_file_url(url).map(ShellEvent::Open),
            _ => None,
        },
        _ => None,
    }
}

/// Parses `file://host/path` (percent-encoded) into a local path.
/// On Windows, `file:///C:/x` → `C:\x` and `file://server/share` → `\\server\share`.
fn parse_file_url(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let path = percent_decode(path)?;
    if cfg!(windows) {
        let bytes = path.as_bytes();
        let has_drive = bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[2] == b':'
            && bytes[1].is_ascii_alphabetic();
        let p = if has_drive {
            path[1..].replace('/', "\\")
        } else if !host.is_empty()
            && !host.eq_ignore_ascii_case("localhost")
            && !is_local_host(host)
        {
            format!("\\\\{host}{}", path.replace('/', "\\"))
        } else {
            path.replace('/', "\\")
        };
        Some(PathBuf::from(p))
    } else {
        Some(PathBuf::from(path))
    }
}

#[cfg(windows)]
fn is_local_host(host: &str) -> bool {
    std::env::var("COMPUTERNAME").is_ok_and(|name| name.eq_ignore_ascii_case(host))
}

#[cfg(not(windows))]
fn is_local_host(_host: &str) -> bool {
    true
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(chunks: &[&[u8]]) -> Vec<ShellEvent> {
        let mut s = OscScanner::default();
        let mut out = Vec::new();
        for c in chunks {
            s.feed(c, |e| out.push(e));
        }
        out
    }

    #[test]
    fn prompt_marks_with_bel_and_st() {
        let ev = scan(&[b"hi\x1b]133;A\x07PS> \x1b]133;B\x1b\\", b"\x1b]133;D;1\x07"]);
        assert_eq!(
            ev,
            vec![
                ShellEvent::PromptStart,
                ShellEvent::PromptEnd,
                ShellEvent::CommandEnd(Some(1)),
            ]
        );
    }

    #[test]
    fn split_across_reads() {
        let ev = scan(&[b"\x1b", b"]13", b"3;", b"B", b"\x1b", b"\\"]);
        assert_eq!(ev, vec![ShellEvent::PromptEnd]);
    }

    #[test]
    fn ignores_other_sequences() {
        let ev = scan(&[b"\x1b[31mred\x1b[0m\x1b]0;title\x07\x1b]8;;http://x\x1b\\"]);
        assert!(ev.is_empty());
    }

    #[test]
    fn command_end_without_code() {
        assert_eq!(
            scan(&[b"\x1b]133;D\x07"]),
            vec![ShellEvent::CommandEnd(None)]
        );
    }

    #[test]
    fn cwd_url() {
        let ev = scan(&[b"\x1b]7;file:///C:/Users/Marc%20A/dev\x07"]);
        let expected = if cfg!(windows) {
            PathBuf::from(r"C:\Users\Marc A\dev")
        } else {
            PathBuf::from("/C:/Users/Marc A/dev")
        };
        assert_eq!(ev, vec![ShellEvent::Cwd(expected)]);
    }

    #[test]
    fn open_request() {
        let ev = scan(&[b"\x1b]7770;open;file://localhost/tmp/a%20b.md\x07"]);
        let expected = if cfg!(windows) {
            PathBuf::from(r"\tmp\a b.md")
        } else {
            PathBuf::from("/tmp/a b.md")
        };
        assert_eq!(ev, vec![ShellEvent::Open(expected)]);
        assert!(scan(&[b"\x1b]7770;close;file:///x\x07"]).is_empty());
    }

    #[test]
    fn unix_cwd_with_host() {
        let ev = scan(&[b"\x1b]7;file://localhost/home/m/%C3%B1\x07"]);
        let expected = if cfg!(windows) {
            PathBuf::from(r"\home\m\ñ")
        } else {
            PathBuf::from("/home/m/ñ")
        };
        assert_eq!(ev, vec![ShellEvent::Cwd(expected)]);
    }
}
