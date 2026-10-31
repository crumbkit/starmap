//! Reads SHELF_PORT, which starmap.toml declares, and SHELF_THEME, which it
//! does not. Cargo never builds this file.

pub(crate) fn theme() {
    let _port = std::env::var("SHELF_PORT");
    let _theme = std::env::var("SHELF_THEME");
}
