//! Peak heap usage per indexing stage, measured with a counting allocator.
//! `cargo run --release -p yaltri-files --example mem_profile <snapshot.idx>`

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let now = CURRENT.fetch_add(l.size(), Relaxed) + l.size();
        PEAK.fetch_max(now, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        CURRENT.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        // Worst case: old and new block coexist during the copy.
        let now = CURRENT.fetch_add(new, Relaxed) + new;
        PEAK.fetch_max(now, Relaxed);
        let r = unsafe { System.realloc(p, l, new) };
        CURRENT.fetch_sub(l.size(), Relaxed);
        r
    }
}

#[global_allocator]
static A: Counting = Counting;

fn mib(b: usize) -> usize {
    b >> 20
}

static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

fn stage(name: &str) {
    let took = LAST.lock().unwrap().replace(std::time::Instant::now()).map(|t| t.elapsed());
    println!(
        "{name:<28} now {:>4} MiB   peak {:>4} MiB   {:>8.2?}",
        mib(CURRENT.load(Relaxed)),
        mib(PEAK.load(Relaxed)),
        took.unwrap_or_default()
    );
    PEAK.store(CURRENT.load(Relaxed), Relaxed);
}

fn main() {
    use yaltri_core::settings::FilesSettings;
    use yaltri_files::scan::ScanControl;
    use yaltri_files::{Scope, persist, reconcile, scan};

    let scope = Scope::from_settings(&FilesSettings::default());
    let control = ScanControl::default();
    let snapshot = std::env::temp_dir().join("yaltri-memprofile.idx");

    stage("start");
    let fresh = scan::full_scan(&scope, &control);
    stage("fresh full_scan");
    persist::save(&fresh, scope.fingerprint(), &snapshot).unwrap();
    stage("save");
    drop(fresh);
    stage("drop");

    let mut index = persist::load(&snapshot, scope.fingerprint()).unwrap();
    stage("load snapshot");
    let edits = reconcile::diff(&index, &scope, &control).unwrap();
    stage(&format!("reconcile diff ({} edits)", edits.len()));
    reconcile::apply(&mut index, edits);
    stage("reconcile apply");
    let _ = std::fs::remove_file(snapshot);
}
