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

fn main() {}
