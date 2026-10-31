// Each field below has a type that starmap does not read.
//
// trybuild builds the case in a project of its own, whose root holds no
// starmap.toml, so the spec path names the Cargo.toml that trybuild writes
// there. The derive only embeds the file's text, so any file compiles.
#[derive(starmap::Env)]
#[starmap(spec = "Cargo.toml")]
struct Unsupported {
    #[starmap("SHELF_PAGE_SIZE")]
    page_size: f64,
    #[starmap("SHELF_DATABASE")]
    database: Option<Vec<u8>>,
    #[starmap("SHELF_SESSION_KEY")]
    session_key: starmap::Secret<Vec<u8>>,
}

fn main() {}
