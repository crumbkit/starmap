//! Lets clap read SHELF_PORT, which starmap.toml declares, and SHELF_THEME
//! and SHELF_HOST, which it does not. The bare `env` reads SHELF_HOST, the
//! upper-case form of its field's name. Cargo never builds this file.

#[derive(clap::Parser)]
pub(crate) struct Cli {
    #[arg(long, env = "SHELF_PORT")]
    port: u16,
    #[arg(long, env = "SHELF_THEME", default_value = "plain")]
    theme: String,
    #[arg(long, env)]
    shelf_host: String,
}
