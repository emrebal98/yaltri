//! Prints games found per launcher (incl. Xbox from the Store listing):
//! `cargo run -p yaltri-apps --example list_games`
fn main() {
    let t = std::time::Instant::now();
    let all = yaltri_apps::discover(&Default::default());
    let games: Vec<_> = all.into_iter().filter(|a| a.launcher.is_some()).collect();
    println!("{} games in {:.2?}", games.len(), t.elapsed());
    for g in games {
        println!("  [{}] {} {:?} -> {:?}\n      icon={:?} icon_from={:?}", g.location, g.name, g.keywords, g.action, g.icon, g.icon_from);
    }
}
