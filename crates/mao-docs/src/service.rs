//! Background loading of open documents, and reloading them when they change on disk.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

use crate::Theme;
use crate::load::Loader;
use crate::model::Document;

enum Request {
    Load(PathBuf),
    /// Something changed at this path: reload it if it is open.
    Changed(PathBuf),
    Forget(PathBuf),
}

/// Editors save in bursts (truncate, write, rename): wait for the dust to settle.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Loads documents on a worker thread and watches their folders for changes.
pub struct DocService {
    tx: Sender<Request>,
    watcher: Option<notify::RecommendedWatcher>,
    /// Open documents (with how many panes show each).
    open: HashMap<PathBuf, usize>,
    /// Watched folders (with how many open documents are in each).
    dirs: HashMap<PathBuf, usize>,
}

impl DocService {
    /// `max_texture` is the GPU's largest texture side; bigger images are downscaled.
    pub fn new(
        theme: Theme,
        max_texture: u32,
        notify: impl Fn(Arc<Document>) + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<Request>();
        let _ = std::thread::Builder::new()
            .name("doc-loader".into())
            .spawn(move || {
                let loader = Loader::new(theme, max_texture);
                let mut wanted: HashSet<PathBuf> = HashSet::new();
                while let Ok(first) = rx.recv() {
                    let mut pending: Vec<PathBuf> = Vec::new();
                    let mut handle = |req: Request, pending: &mut Vec<PathBuf>| match req {
                        Request::Load(p) => {
                            wanted.insert(p.clone());
                            if !pending.contains(&p) {
                                pending.push(p);
                            }
                        }
                        Request::Changed(p) => {
                            if wanted.contains(&p) && !pending.contains(&p) {
                                pending.push(p);
                            }
                        }
                        Request::Forget(p) => {
                            wanted.remove(&p);
                            pending.retain(|x| *x != p);
                        }
                    };
                    let debounce = matches!(first, Request::Changed(_));
                    handle(first, &mut pending);
                    if debounce {
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
                    for path in pending {
                        loader.load(&path, &mut |doc| notify(Arc::new(doc)));
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
                let _ = watch_tx.send(Request::Changed(path));
            }
        })
        .map_err(|e| tracing::warn!("document watching unavailable: {e}"))
        .ok();

        Self {
            tx,
            watcher,
            open: HashMap::new(),
            dirs: HashMap::new(),
        }
    }

    /// Starts showing `path`: loads it and reloads it on change until [`Self::close`].
    pub fn open(&mut self, path: &Path) {
        let count = self.open.entry(path.to_path_buf()).or_default();
        *count += 1;
        if *count > 1 {
            // Already loaded for another pane: load again so this one gets it too.
            let _ = self.tx.send(Request::Load(path.to_path_buf()));
            return;
        }
        let _ = self.tx.send(Request::Load(path.to_path_buf()));
        if let Some(dir) = path.parent() {
            let n = self.dirs.entry(dir.to_path_buf()).or_default();
            *n += 1;
            if *n == 1
                && let Some(w) = self.watcher.as_mut()
                && let Err(e) = w.watch(dir, RecursiveMode::NonRecursive)
            {
                tracing::debug!("cannot watch {}: {e}", dir.display());
            }
        }
    }

    /// Loads `path` again (it must be open).
    pub fn reload(&self, path: &Path) {
        let _ = self.tx.send(Request::Load(path.to_path_buf()));
    }

    /// One pane stopped showing `path`.
    pub fn close(&mut self, path: &Path) {
        let Some(count) = self.open.get_mut(path) else {
            return;
        };
        *count -= 1;
        if *count > 0 {
            return;
        }
        self.open.remove(path);
        let _ = self.tx.send(Request::Forget(path.to_path_buf()));
        if let Some(dir) = path.parent()
            && let Some(n) = self.dirs.get_mut(dir)
        {
            *n -= 1;
            if *n == 0 {
                self.dirs.remove(dir);
                if let Some(w) = self.watcher.as_mut() {
                    let _ = w.unwatch(dir);
                }
            }
        }
    }

    /// Whether any document is open.
    pub fn is_idle(&self) -> bool {
        self.open.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Content;

    #[test]
    fn loads_and_reloads_on_change() {
        let dir = std::env::temp_dir().join(format!("mao-docs-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.txt");
        std::fs::write(&file, "one\n").unwrap();

        let (tx, rx) = mpsc::channel();
        let mut docs = DocService::new(Theme::default(), 4096, move |d| {
            let _ = tx.send(d);
        });
        docs.open(&file);
        let first = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let Content::Text(t) = &first.content else {
            panic!("{:?}", first.content)
        };
        assert_eq!(t.lines.len(), 1);

        std::fs::write(&file, "one\ntwo\n").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut seen = false;
        while std::time::Instant::now() < deadline {
            if let Ok(d) = rx.recv_timeout(Duration::from_millis(200))
                && let Content::Text(t) = &d.content
                && t.lines.len() == 2
            {
                seen = true;
                break;
            }
        }
        assert!(seen, "the change was not picked up");
        docs.close(&file);
        assert!(docs.is_idle());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
