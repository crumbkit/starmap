// Each type below misuses a derive, and each derive names the rule it breaks.
#[derive(starmap::Env)]
struct Tuple(String);

#[derive(starmap::Env)]
struct Unnamed {
    port: starmap::Port,
}

#[derive(starmap::Env)]
#[starmap(path = "starmap.toml")]
struct UnknownKey {
    #[starmap("SHELF_PORT")]
    port: starmap::Port,
}

#[derive(starmap::Env)]
struct Generic<T> {
    #[starmap("SHELF_PORT")]
    port: T,
}

#[derive(starmap::Enumeration)]
enum WithData {
    Dev(String),
}

#[derive(starmap::Enumeration)]
struct NotAnEnum;

#[derive(starmap::Enumeration)]
enum Duplicate {
    Dev,
    #[starmap("dev")]
    Development,
}

#[derive(starmap::Enumeration)]
enum Whitespace {
    #[starmap("local dev")]
    Dev,
}

fn main() {}
