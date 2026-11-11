//! Reads SHELF_PORT, which starmap.toml declares. Cargo never builds this
//! file.

pub(crate) fn port() {
    let _port = std::env::var("SHELF_PORT");
}
