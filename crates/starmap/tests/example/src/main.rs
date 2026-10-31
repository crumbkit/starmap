//! The source of the example application, which `starmap check` and
//! `starmap init` scan in the CLI's tests. Cargo never builds it.
//!
//! A doc comment that names env::var("SHELF_IN_A_DOC") reads nothing.

mod books;
mod env;

fn main() {
    // A comment that names std::env::var("SHELF_IN_A_COMMENT") reads nothing.
    let env = env::Env::load().unwrap();
    let label = "SHELF_LABEL";
    let name = String::from("SHELF_DYNAMIC");
    let _ = std::env::var(&name);
    let _ = std::env::var("shelf_lowercase");
    println!(
        "{label}: {:?} on port {:?}",
        env,
        std::env::var("SHELF_PORT"),
    );
    books::remove::state_dir();
}
