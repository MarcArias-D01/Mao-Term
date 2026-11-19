//! PTY wrapper that scans the output for shell-integration sequences before
//! `alacritty_terminal` parses it (its parser drops OSC 133/7).

use std::io;
use std::sync::Arc;

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{self, ChildEvent, EventedPty, EventedReadWrite};
use polling::{Event, PollMode, Poller};

use crate::osc::{OscScanner, ShellEvent};

pub(crate) type ShellSink = Arc<dyn Fn(ShellEvent) + Send + Sync>;

pub(crate) struct ScanningPty {
    inner: tty::Pty,
    scanner: OscScanner,
    sink: ShellSink,
}

impl ScanningPty {
    pub(crate) fn new(inner: tty::Pty, sink: ShellSink) -> Self {
        Self {
            inner,
            scanner: OscScanner::default(),
            sink,
        }
    }
}

impl io::Read for ScanningPty {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.reader().read(buf)?;
        let sink = &self.sink;
        self.scanner.feed(&buf[..n], |event| sink(event));
        Ok(n)
    }
}

// SAFETY: `register` is `unsafe` in the trait because the registered sources must
// outlive their registration; we only forward to the wrapped PTY, which owns them
// for as long as this wrapper (and thus the event loop holding it) lives.
#[allow(unsafe_code)]
impl EventedReadWrite for ScanningPty {
    type Reader = Self;
    type Writer = <tty::Pty as EventedReadWrite>::Writer;

    unsafe fn register(
        &mut self,
        poll: &Arc<Poller>,
        event: Event,
        mode: PollMode,
    ) -> io::Result<()> {
        // SAFETY: see the impl comment above.
        unsafe { self.inner.register(poll, event, mode) }
    }

    fn reregister(&mut self, poll: &Arc<Poller>, event: Event, mode: PollMode) -> io::Result<()> {
        self.inner.reregister(poll, event, mode)
    }

    fn deregister(&mut self, poll: &Arc<Poller>) -> io::Result<()> {
        self.inner.deregister(poll)
    }

    fn reader(&mut self) -> &mut Self::Reader {
        self
    }

    fn writer(&mut self) -> &mut Self::Writer {
        self.inner.writer()
    }
}

impl EventedPty for ScanningPty {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.inner.next_child_event()
    }
}

impl OnResize for ScanningPty {
    fn on_resize(&mut self, window_size: WindowSize) {
        self.inner.on_resize(window_size);
    }
}
