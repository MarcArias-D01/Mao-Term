//! Background listing + file-system watching.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

use crate::list::{Entry, list_dir};

/// Results delivered to the UI (through the callback given to [`FsService::new`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsEvent {
    Listed { dir: PathBuf, entries: Vec<Entry> },
    Failed { dir: PathBuf, error: String },
}

enum Request {
    List {
        dir: PathBuf,
        respect_ignore: bool,
    },
    /// Something changed inside `dir`: re-list it if it is still wanted.
    Changed(PathBuf),
    Forget(PathBuf),
}

/// Lists directories on a worker thread and watches the ones the UI shows.
/// Dropping it stops the worker and every watcher (zero cost when closed).
pub struct FsService {
    tx: Sender<Request>,
    watcher: Option<notify::RecommendedWatcher>,
    watched: HashSet<PathBuf>,
}

/// Bursts of changes (a build writing many files) are coalesced for this long.
const DEBOUNCE: Duration = Duration::from_millis(120);

impl FsService {
    pub fn new(notify: impl Fn(FsEvent) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<Request>();
        let _ = std::thread::Builder::new()
            .name("fs-worker".into())
            .spawn(move || {
                // Last settings each directory was listed with (to re-list on change).
                let mut wanted: HashMap<PathBuf, bool> = HashMap::new();
                while let Ok(first) = rx.recv() {
                    let mut pending: HashMap<PathBuf, bool> = HashMap::new();
                    let mut handle = |req: Request, pending: &mut HashMap<PathBuf, bool>| match req
                    {
                        Request::List {
                            dir,
                            respect_ignore,
                        } => {
                            wanted.insert(dir.clone(), respect_ignore);
                            pending.insert(dir, respect_ignore);
                        }
                        Request::Changed(dir) => {
                            if let Some(&r) = wanted.get(&dir) {
                                pending.insert(dir, r);
                            }
                        }
                        Request::Forget(dir) => {
                            wanted.remove(&dir);
                            pending.remove(&dir);
                        }
                    };
                    let debounce = matches!(first, Request::Changed(_));
                    handle(first, &mut pending);
                    if debounce {
                        // Collect the rest of the burst.
                        loop {
                            match rx.recv_timeout(DEBOUNCE) {
                                Ok(req) => handle(req, &mut pending),
                                Err(RecvTimeoutError::Timeout) => break,
                                Err(RecvTimeoutError::Disconnected) => return,
                            }
                        }
                    } else {
                        while let Ok(req) = rx.try_recv() {
                            handle(req, &mut pending);
                        }
                    }
                    for (dir, respect_ignore) in pending {
                        notify(match list_dir(&dir, respect_ignore) {
                            Ok(entries) => FsEvent::Listed { dir, entries },
                            Err(e) => FsEvent::Failed {
                                dir,
                                error: e.to_string(),
                            },
                        });
                    }
                }
            });

        let watch_tx = tx.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else {
                return;
            };
            if matches!(event.kind, notify::EventKind::Access(_)) {
                return;
            }
            for path in event.paths {
                if let Some(parent) = path.parent() {
                    let _ = watch_tx.send(Request::Changed(parent.to_path_buf()));
                }
                // The path itself may be a watched directory (renamed/deleted contents).
                let _ = watch_tx.send(Request::Changed(path));
            }
        })
        .map_err(|e| tracing::warn!("file watching unavailable: {e}"))
        .ok();

        Self {
            tx,
            watcher,
            watched: HashSet::new(),
        }
    }

    /// Lists `dir` in the background and starts watching it.
    pub fn list(&mut self, dir: &Path, respect_ignore: bool) {
        let _ = self.tx.send(Request::List {
            dir: dir.to_path_buf(),
            respect_ignore,
        });
        if self.watched.insert(dir.to_path_buf())
            && let Some(w) = self.watcher.as_mut()
            && let Err(e) = w.watch(dir, RecursiveMode::NonRecursive)
        {
            tracing::debug!("cannot watch {}: {e}", dir.display());
        }
    }

    /// Stops watching `dir` (collapsed or no longer shown).
    pub fn forget(&mut self, dir: &Path) {
        let _ = self.tx.send(Request::Forget(dir.to_path_buf()));
        if self.watched.remove(dir)
            && let Some(w) = self.watcher.as_mut()
        {
            let _ = w.unwatch(dir);
        }
    }

    /// Directories currently watched.
    pub fn watched(&self) -> impl Iterator<Item = &Path> {
        self.watched.iter().map(PathBuf::as_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_and_notices_new_files() {
        let dir = std::env::temp_dir().join(format!("mao-fs-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "").unwrap();

        let (tx, rx) = mpsc::channel();
        let mut fs = FsService::new(move |e| {
            let _ = tx.send(e);
        });
        fs.list(&dir, true);
        let first = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let FsEvent::Listed { entries, .. } = first else {
            panic!("unexpected {first:?}");
        };
        assert_eq!(entries.len(), 1);

        std::fs::write(dir.join("b.txt"), "").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut seen = false;
        while std::time::Instant::now() < deadline {
            if let Ok(FsEvent::Listed { entries, .. }) = rx.recv_timeout(Duration::from_millis(200))
                && entries.len() == 2
            {
                seen = true;
                break;
            }
        }
        assert!(seen, "watcher did not report the new file");
    }
}
