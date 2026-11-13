//! Measures a full scan of the default scope and a few searches.
//! `cargo run --release -p yaltri-files --example scan_bench [query...]`

use std::time::Instant;

use yaltri_core::settings::FilesSettings;
use yaltri_files::Scope;
use yaltri_files::scan::{ScanControl, full_scan};

fn main() {
    let scope = Scope::from_settings(&FilesSettings::default());
    println!("roots: {:?}", scope.roots);
    let started = Instant::now();
    let index = full_scan(&scope, &ScanControl::default());
    println!(
        "scan: {} entries in {:.2?}, ~{} MiB",
        index.len(),
        started.elapsed(),
        index.memory_bytes() / (1 << 20)
    );

    let mut queries: Vec<String> = std::env::args().skip(1).collect();
    if queries.is_empty() {
        queries = ["readme", "cargo toml", "notepad", ".exe", "zz"].map(String::from).to_vec();
    }
    for q in queries {
        let t = Instant::now();
        let hits = index.search(&q, 8, &|_| 0);
        println!("search {q:?}: {} hits in {:.2?}", hits.len(), t.elapsed());
        for h in hits.iter().take(3) {
            println!("    {} ({})", h.path.display(), h.score);
        }
    }
}
