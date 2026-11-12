// A family field cannot be deferred, so its member type is unsupported.
//
// trybuild builds the case in a project of its own, whose root holds no
// starmap.toml, so the spec path names the Cargo.toml that trybuild writes
// there. The derive only embeds the file's text, so any file compiles.
#[derive(starmap::Env)]
#[starmap(spec = "Cargo.toml")]
struct DeferredFamily {
    #[starmap(family = "SHELF_DB_URL")]
    databases: starmap::Family<starmap::Deferred<starmap::url::Url>>,
}

fn main() {}
