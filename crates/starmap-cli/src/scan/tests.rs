use std::path::Path;

use super::{lines_of_reads, scan};

/// Returns each read in `text` as its name and line.
fn reads(text: &str) -> Vec<(String, usize)> {
    lines_of_reads(text)
        .unwrap()
        .into_iter()
        .map(|(name, line)| (name.as_str().to_owned(), line))
        .collect()
}

/// Returns `name` read on `line`.
fn read(name: &str, line: usize) -> (String, usize) {
    (name.to_owned(), line)
}

#[test]
fn a_literal_name_passed_to_env_var_is_a_read_on_its_line() {
    let text = "fn main() {\n    let port = std::env::var(\"SHELF_PORT\");\n}\n";
    assert_eq!(reads(text), [read("SHELF_PORT", 2)]);
}

#[test]
fn var_os_an_imported_var_and_a_trailing_comma_are_reads() {
    let text = "use std::env::var;\nfn f() {\n    env::var_os(\"A\");\n    var(\"B\",);\n}\n";
    assert_eq!(reads(text), [read("A", 3), read("B", 4)]);
}

#[test]
fn a_raw_string_literal_is_a_read() {
    let text = "fn f() { std::env::var(r\"A\"); std::env::var(r#\"B\"#); }";
    assert_eq!(reads(text), [read("A", 1), read("B", 1)]);
}

#[test]
fn a_read_inside_a_macro_call_is_a_read() {
    let text = "fn f() {\n    println!(\"{:?}\", std::env::var(\"SHELF_PORT\"));\n}\n";
    assert_eq!(reads(text), [read("SHELF_PORT", 2)]);
}

#[test]
fn a_starmap_field_attribute_is_a_read() {
    let text = "#[derive(starmap::Env)]\n#[starmap(spec = \"config/starmap.toml\")]\nstruct Env {\n    #[starmap(\"SHELF_PORT\")]\n    port: u16,\n}\n";
    assert_eq!(reads(text), [read("SHELF_PORT", 4)]);
}

#[test]
fn a_name_in_a_comment_or_a_doc_comment_is_not_a_read() {
    let text = "//! env::var(\"A\")\n/// env::var(\"B\")\n// env::var(\"C\")\n/* env::var(\"D\") */\nfn f() {}\n";
    assert_eq!(reads(text), []);
}

#[test]
fn a_name_that_is_not_a_literal_argument_is_not_a_read() {
    let text = "const NAME: &str = \"A\";\nfn f(name: &str) {\n    env::var(NAME);\n    env::var(name);\n    env::var(\"B\".to_owned());\n    env::var(b\"C\");\n    let text = \"D\";\n}\n";
    assert_eq!(reads(text), []);
}

#[test]
fn a_literal_that_is_not_a_variable_name_is_not_a_read() {
    let text = "fn f() { env::var(\"lower_case\"); env::var(\"A B\"); env::var(\"A\\u{42}\"); }";
    assert_eq!(reads(text), []);
}

#[test]
fn text_that_is_not_rust_tokens_is_refused() {
    assert!(lines_of_reads("fn f() { \"unterminated").is_err());
    assert!(lines_of_reads("fn f() {").is_err());
}

#[test]
fn the_example_application_reads_each_variable_at_its_places() {
    // Unit tests run in the package's directory.
    let reads = scan(Path::new("../starmap/tests/example/src")).unwrap();
    let found: Vec<(&str, Vec<&str>)> = reads
        .iter()
        .map(|(name, places)| {
            let places = places
                .iter()
                .map(|place| place.strip_prefix("../starmap/tests/example/").unwrap());
            (name.as_str(), places.collect())
        })
        .collect();
    assert_eq!(
        found,
        [
            ("SHELF_CATALOGUE_URL", vec!["src/env.rs:9"]),
            ("SHELF_DATABASE", vec!["src/env.rs:11"]),
            ("SHELF_DEBUG", vec!["src/env.rs:13"]),
            ("SHELF_ENV", vec!["src/env.rs:5"]),
            ("SHELF_MIRROR_TOKEN", vec!["src/env.rs:17"]),
            ("SHELF_PAGE_SIZE", vec!["src/env.rs:15"]),
            ("SHELF_PORT", vec!["src/env.rs:7", "src/main.rs:19"]),
            ("SHELF_SESSION_KEY", vec!["src/env.rs:19"]),
            ("XDG_STATE_HOME", vec!["src/books/remove.rs:8"]),
        ]
    );
}

#[test]
fn a_missing_source_directory_has_no_reads() {
    let dir = tempfile::tempdir().unwrap();
    let reads = scan(&dir.path().join("src")).unwrap();
    assert!(reads.is_empty());
}

#[test]
fn a_source_file_that_is_not_rust_fails_and_names_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("broken.rs"), "fn f() {").unwrap();
    let failure = scan(&src).unwrap_err();
    let expected = format!(
        "{} is not valid Rust at line 1",
        src.join("broken.rs").display()
    );
    assert!(failure.to_string().starts_with(&expected), "{failure}");
}

#[test]
fn a_source_file_over_the_limit_fails_as_too_large() {
    let dir = tempfile::tempdir().unwrap();
    let text = "/".repeat(super::MAX_SOURCE_BYTES + 1);
    let path = dir.path().join("long.rs");
    std::fs::write(&path, text).unwrap();
    let failure = scan(dir.path()).unwrap_err();
    assert_eq!(
        failure.to_string(),
        format!("{} is over 1 MiB", path.display())
    );
}
