// Each field below names a check whose signature does not fit the field.
//
// A withheld field's check returns `Result<(), &'static str>`, so its reason
// cannot hold the secret, and a shown field's check returns
// `Result<(), String>`. The spec path names the Cargo.toml that trybuild
// writes, as in unsupported_field_type.rs, because the derive only embeds
// the file's text.
#[derive(starmap::Env)]
#[starmap(spec = "Cargo.toml")]
struct Checked {
    #[starmap("SHELF_SESSION_KEY", check = leaks)]
    session_key: starmap::Secret,
    #[starmap("SHELF_PORT", check = wrong_type)]
    port: starmap::Port,
}

fn leaks(key: &starmap::Secret) -> Result<(), String> {
    Err(format!("{key:?} is too short"))
}

fn wrong_type(_port: &u16) -> Result<(), String> {
    Ok(())
}

fn main() {}
