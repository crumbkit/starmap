//! Reads SHELF_PORT, which starmap.toml declares. Cargo never builds this
//! file.

fn main() {
    let _port = std::env::var("SHELF_PORT");
}
