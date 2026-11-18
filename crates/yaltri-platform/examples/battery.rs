//! Prints the battery of every supported mouse and earbuds:
//! `cargo run -p yaltri-platform --example battery`.

fn main() {
    let readings = yaltri_platform::battery::read_all();
    if readings.is_empty() {
        println!("no supported device found");
    }
    for r in readings {
        println!("{:?} {}: {:?}", r.kind, r.name, r.level);
    }
}
