//! Keeps a `FileIndex` current: loads the last snapshot, re-walks in the
//! background, applies filesystem change events and rescans periodically.
//!
//! One worker thread owns all mutation; searches only take a read lock.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use notify::{EventKind, RecursiveMode, Watcher};

use crate::index::FileIndex;
use crate::persist;
use crate::reconcile;
use crate::scan::{self, ScanControl};
use crate::scope::Scope;

/// Without change notifications (most of `/` on Linux) this is how stale
/// the index can get.
const PERIODIC_RESCAN: Duration = Duration::from_secs(if cfg!(windows) { 6 * 3600 } else { 3600 });
/// Watcher buffer overflows trigger at most one full rescan per this period.
const MIN_OVERFLOW_RESCAN_GAP: Duration = Duration::from_secs(10 * 60);
const EVENT_DEBOUNCE: Duration = Duration::from_millis(400);
const SAVE_EVERY: Duration = Duration::from_secs(5 * 60);
const COMPACT_GARBAGE_RATIO: f32 = 0.3;
/// How long dropping the indexer waits for the final snapshot save.
const STOP_WAIT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Reading the on-disk snapshot.
    Loading,
    /// Walking the filesystem. Search uses the previous snapshot meanwhile.
    Scanning { found: usize },
    Ready,
}

#[derive(Clone, Debug)]
pub struct Status {
    pub phase: Phase,
    pub entries: usize,
    pub memory_bytes: usize,
    pub last_scan: Option<SystemTime>,
    pub last_scan_took: Option<Duration>,
    /// Folders receiving live change notifications.
    pub watched: Vec<PathBuf>,
}

enum Msg {
    Changed(Vec<PathBuf>),
    Overflow,
    RescanNow,
    Stop,
}

pub(crate) struct Shared {
    pub(crate) index: RwLock<FileIndex>,
    status: Mutex<Status>,
    scan: ScanControl,
    /// Set by the worker once its final save is done.
    finished: (Mutex<bool>, Condvar),
}

pub struct FileIndexer {
    pub(crate) shared: Arc<Shared>,
    tx: Sender<Msg>,
}

impl FileIndexer {
    /// Starts indexing `scope` in the background. Returns immediately.
    pub fn start(scope: Scope, snapshot: PathBuf) -> Self {
        let shared = Arc::new(Shared {
            index: RwLock::new(FileIndex::new()),
            status: Mutex::new(Status {
                phase: Phase::Loading,
                entries: 0,
                memory_bytes: 0,
                last_scan: None,
                last_scan_took: None,
                watched: Vec::new(),
            }),
            scan: ScanControl::default(),
            finished: (Mutex::new(false), Condvar::new()),
        });
        let (tx, rx) = mpsc::channel();
        {
            let shared = shared.clone();
            let tx = tx.clone();
            thread::Builder::new()
                .name("yaltri-index".into())
                .spawn(move || Worker { shared, scope, snapshot, tx, dirty: false }.run(rx))
                .expect("spawning index thread");
        }
        Self { shared, tx }
    }

    pub fn status(&self) -> Status {
        let mut s = self.shared.status.lock().unwrap().clone();
        if let Phase::Scanning { found } = &mut s.phase {
            *found = self.shared.scan.found.load(std::sync::atomic::Ordering::Relaxed);
        }
        s
    }

    pub fn rescan(&self) {
        let _ = self.tx.send(Msg::RescanNow);
    }
}

impl Drop for FileIndexer {
    /// Stops the worker and waits (bounded) for its final snapshot save.
    /// The thread itself is not joined: dropping a Windows `notify` watcher
    /// can block until the next filesystem event, which must not delay quit.
    fn drop(&mut self) {
        self.shared.scan.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = self.tx.send(Msg::Stop);
        let (lock, cvar) = &self.shared.finished;
        let done = lock.lock().unwrap();
        let (done, _) = cvar.wait_timeout_while(done, STOP_WAIT, |finished| !*finished).unwrap();
        if !*done {
            tracing::warn!("index worker did not finish saving in time");
        }
    }
}

struct Worker {
    shared: Arc<Shared>,
    scope: Scope,
    snapshot: PathBuf,
    tx: Sender<Msg>,
    dirty: bool,
}

impl Worker {
    fn run(mut self, rx: Receiver<Msg>) {
        let fingerprint = self.scope.fingerprint();
        let started = Instant::now();
        match persist::load(&self.snapshot, fingerprint) {
            Ok(index) => {
                tracing::info!(entries = index.len(), took = ?started.elapsed(), "file index snapshot loaded");
                *self.shared.index.write().unwrap() = index;
            }
            Err(e) => tracing::info!("no usable index snapshot ({e:#}), full scan needed"),
        }
        self.publish(Phase::Loading);

        // Start watching before the walk so nothing changing mid-scan is missed.
        let _watcher = self.start_watcher();
        self.full_scan();

        let mut last_full = Instant::now();
        let mut last_overflow_scan: Option<Instant> = None;
        let mut last_save = Instant::now();
        loop {
            let timeout = PERIODIC_RESCAN.saturating_sub(last_full.elapsed()).min(SAVE_EVERY);
            let msg = match rx.recv_timeout(timeout) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            match msg {
                Some(Msg::Stop) => break,
                Some(Msg::RescanNow) => {
                    self.full_scan();
                    last_full = Instant::now();
                }
                Some(Msg::Overflow) => {
                    if last_overflow_scan.is_none_or(|t| t.elapsed() > MIN_OVERFLOW_RESCAN_GAP) {
                        tracing::warn!("change notifications overflowed, rescanning");
                        self.full_scan();
                        last_full = Instant::now();
                        last_overflow_scan = Some(Instant::now());
                    }
                }
                Some(Msg::Changed(paths)) => {
                    if self.apply_changes(paths, &rx) {
                        break;
                    }
                }
                None => {}
            }
            if last_full.elapsed() >= PERIODIC_RESCAN
                || self.shared.index.read().unwrap().garbage_ratio() > COMPACT_GARBAGE_RATIO
            {
                self.full_scan();
                last_full = Instant::now();
            }
            if self.dirty && last_save.elapsed() >= SAVE_EVERY {
                self.save();
                last_save = Instant::now();
            }
        }
        if self.dirty {
            self.save();
        }
        let (lock, cvar) = &self.shared.finished;
        *lock.lock().unwrap() = true;
        cvar.notify_all();
        // `_watcher` drops after this, possibly slowly; nobody waits for it.
    }

    fn full_scan(&mut self) {
        use std::sync::atomic::Ordering;
        self.shared.scan.found.store(0, Ordering::Relaxed);
        self.publish(Phase::Scanning { found: 0 });
        let started = Instant::now();
        let rebuild = {
            let index = self.shared.index.read().unwrap();
            index.is_empty() || index.garbage_ratio() > COMPACT_GARBAGE_RATIO
        };
        if rebuild {
            // First run (nothing to compare against) or too many tombstones:
            // build a compact index from scratch. Only here do two indexes
            // coexist briefly; normal rescans reconcile in place below.
            let fresh = scan::full_scan(&self.scope, &self.shared.scan);
            if self.shared.scan.cancel.load(Ordering::Relaxed) {
                return;
            }
            tracing::info!(entries = fresh.len(), mib = fresh.memory_bytes() / (1 << 20), took = ?started.elapsed(), "full scan done (rebuilt)");
            *self.shared.index.write().unwrap() = fresh;
            self.dirty = true;
        } else {
            // Compare under a read lock (searches keep working), then apply
            // the few differences under a short write lock.
            let edits = {
                let index = self.shared.index.read().unwrap();
                reconcile::diff(&index, &self.scope, &self.shared.scan)
            };
            let Some(edits) = edits else { return };
            let changed = edits.len();
            let apply_started = Instant::now();
            reconcile::apply(&mut self.shared.index.write().unwrap(), edits);
            tracing::info!(changed, apply = ?apply_started.elapsed(), took = ?started.elapsed(), "full scan done (reconciled)");
            self.dirty |= changed > 0;
        }
        let took = started.elapsed();
        {
            let mut s = self.shared.status.lock().unwrap();
            s.last_scan = Some(SystemTime::now());
            s.last_scan_took = Some(took);
        }
        self.publish(Phase::Ready);
        if self.dirty {
            self.save();
        }
    }

    /// Debounces a burst of change events, then reconciles each affected
    /// folder. Returns true if a Stop arrived meanwhile.
    fn apply_changes(&mut self, first: Vec<PathBuf>, rx: &Receiver<Msg>) -> bool {
        let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
        let collect = |paths: Vec<PathBuf>, dirs: &mut BTreeSet<PathBuf>| {
            for p in paths {
                if self.scope.is_inside_excluded(&p) {
                    continue;
                }
                if let Some(parent) = p.parent() {
                    dirs.insert(parent.to_path_buf());
                }
            }
        };
        collect(first, &mut dirs);
        let deadline = Instant::now() + EVENT_DEBOUNCE;
        let mut other = Vec::new();
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(Msg::Changed(paths)) => collect(paths, &mut dirs),
                Ok(Msg::Stop) => return true,
                Ok(m) => other.push(m),
                Err(_) => break,
            }
        }
        // Re-queue anything that was not a change event.
        for m in other {
            let _ = self.tx.send(m);
        }
        if dirs.is_empty() {
            return false;
        }

        let dirs: Vec<PathBuf> = dirs.into_iter().collect();
        let mut index = self.shared.index.write().unwrap();
        let resolved = {
            let refs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
            scan::resolve_dirs(&index, &refs)
        };
        let mut done: BTreeSet<u32> = BTreeSet::new();
        for (dir, id) in dirs.iter().zip(resolved) {
            // A brand-new folder is not in the index yet: reconcile the
            // closest known ancestor instead, which will walk it.
            let (dir, id) = match id {
                Some(id) => (dir.clone(), id),
                None => match nearest_known_ancestor(&index, dir) {
                    Some(found) => found,
                    None => continue,
                },
            };
            if done.insert(id) {
                scan::rescan_dir(&mut index, id, &dir, &self.scope);
            }
        }
        drop(index);
        tracing::debug!(dirs = done.len(), "applied file changes");
        self.dirty = true;
        self.publish(Phase::Ready);
        false
    }

    fn start_watcher(&self) -> Option<notify::RecommendedWatcher> {
        let tx = self.tx.clone();
        let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
            Ok(event) => {
                if event.need_rescan() {
                    let _ = tx.send(Msg::Overflow);
                    return;
                }
                let relevant = matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(notify::event::ModifyKind::Name(_)) | EventKind::Any
                );
                if relevant && !event.paths.is_empty() {
                    let _ = tx.send(Msg::Changed(event.paths));
                }
            }
            Err(e) => tracing::debug!(%e, "watch error"),
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(%e, "file watching unavailable; relying on periodic rescans");
                return None;
            }
        };

        let mut watched = Vec::new();
        for root in watch_targets(&self.scope) {
            match watcher.watch(&root, RecursiveMode::Recursive) {
                Ok(()) => watched.push(root),
                Err(e) => tracing::warn!(root = %root.display(), %e, "cannot watch folder"),
            }
        }
        tracing::info!(?watched, "watching for file changes");
        self.shared.status.lock().unwrap().watched = watched;
        Some(watcher)
    }

    fn publish(&self, phase: Phase) {
        let (entries, memory) = {
            let index = self.shared.index.read().unwrap();
            (index.len(), index.memory_bytes())
        };
        let mut s = self.shared.status.lock().unwrap();
        s.phase = phase;
        s.entries = entries;
        s.memory_bytes = memory;
    }

    fn save(&mut self) {
        let started = Instant::now();
        let index = self.shared.index.read().unwrap();
        match persist::save(&index, self.scope.fingerprint(), &self.snapshot) {
            Ok(()) => {
                self.dirty = false;
                tracing::debug!(took = ?started.elapsed(), "index snapshot saved");
            }
            Err(e) => tracing::warn!("saving index snapshot failed: {e:#}"),
        }
    }
}

fn nearest_known_ancestor(index: &FileIndex, dir: &Path) -> Option<(PathBuf, u32)> {
    let mut cur = dir.parent();
    while let Some(p) = cur {
        if let Some(id) = scan::resolve_dirs(index, &[p])[0] {
            return Some((p.to_path_buf(), id));
        }
        cur = p.parent();
    }
    None
}

/// Windows can watch whole drives cheaply (one handle per root). inotify
/// needs one watch per folder and has a per-user limit, so on Linux only the
/// home folder is watched live; the rest relies on periodic rescans.
fn watch_targets(scope: &Scope) -> Vec<PathBuf> {
    if cfg!(windows) {
        return scope.roots.clone();
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    if scope.roots.iter().any(|r| home.starts_with(r)) { vec![home] } else { Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !cond() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn count(indexer: &FileIndexer, q: &str) -> usize {
        indexer.shared.index.read().unwrap().search(q, 10, &|_| 0).len()
    }

    #[test]
    fn indexes_then_follows_changes_and_reloads_snapshot() {
        let base = std::env::temp_dir().join(format!("yaltri-indexer-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("root");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/first.txt"), "").unwrap();
        let snapshot = base.join("files.idx");
        let scope = Scope::for_test(vec![root.clone()], vec![], &[]);

        let indexer = FileIndexer::start(scope.clone(), snapshot.clone());
        wait_for("initial scan", || indexer.status().phase == Phase::Ready);
        assert_eq!(count(&indexer, "first"), 1);

        // Live change notifications (watched on Windows; on Linux only $HOME is
        // watched, so trigger an explicit rescan there).
        fs::write(root.join("sub/second.txt"), "").unwrap();
        if !cfg!(windows) {
            indexer.rescan();
        }
        wait_for("change event", || count(&indexer, "second") == 1);
        fs::remove_file(root.join("sub/first.txt")).unwrap();
        if !cfg!(windows) {
            indexer.rescan();
        }
        wait_for("removal event", || count(&indexer, "first") == 0);
        drop(indexer);

        // Restart: the snapshot is loaded before any rescan finishes.
        let indexer = FileIndexer::start(scope, snapshot);
        wait_for("snapshot", || count(&indexer, "second") == 1);
        drop(indexer);
        fs::remove_dir_all(base).unwrap();
    }
}
