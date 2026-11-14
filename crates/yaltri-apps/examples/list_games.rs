//! Prints games found per launcher: `cargo run -p yaltri-apps --example list_games`
fn main() {
    let t = std::time::Instant::now();
    let games = yaltri_apps::games::discover(&Default::default());
    println!("{} games in {:.2?}", games.len(), t.elapsed());
    for g in games {
        println!("  [{}] {} -> {:?}\n      icon={:?} icon_from={:?}", g.location, g.name, g.action, g.icon, g.icon_from);
    }
}
