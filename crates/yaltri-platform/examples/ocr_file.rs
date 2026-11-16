//! Reads the text in a PNG with the same engine the app uses:
//! `cargo run -p yaltri-platform --example ocr_file -- image.png`

fn main() {
    let path = std::env::args().nth(1).expect("usage: ocr_file <image.png>");
    let image = image::open(&path).expect("reading the image").to_rgba8();
    let (w, h) = image.dimensions();
    let bgra: Vec<u8> = image.pixels().flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    let started = std::time::Instant::now();
    match yaltri_platform::ocr::recognize(w, h, &bgra) {
        Ok(text) => println!("{text}\n--- {:?}", started.elapsed()),
        Err(e) => eprintln!("error: {e}"),
    }
}
