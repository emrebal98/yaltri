//! Prints discovered applications: `cargo run -p yaltri-apps --example list_apps [query]`
use std::time::Instant;
use yaltri_core::SearchProvider;

fn main() {
    let t = Instant::now();
    let apps = yaltri_apps::discover(&Default::default());
    println!("{} apps in {:.2?}", apps.len(), t.elapsed());
    for a in apps.iter().take(8) {
        println!("  {} — {:?}", a.name, a.action);
    }
    if let Some(q) = std::env::args().nth(1) {
        let p = yaltri_apps::AppsProvider::start(std::env::temp_dir().join("yaltri-icon-example"), Default::default());
        while p.count() == 0 { std::thread::sleep(std::time::Duration::from_millis(100)); }
        for item in p.query(&q) {
            println!("{q:?} -> {} ({})", item.title, item.score);
        }
    }
}
