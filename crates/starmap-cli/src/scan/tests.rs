use starmap::Name;

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
    let text = "const NAME: &str = \"A\";\nfn f(name: &str) {\n    env::var(NAME);\n    env::var(name);\n    env::var(\"B\".to_owned());\n    env::var(b\"C\");\n    let var = 1;\n    f(\"E\");\n    var[\"F\"];\n    let text = \"D\";\n}\n";
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
fn a_missing_source_directory_has_no_reads() {
    let dir = tempfile::tempdir().unwrap();
    let reads = scan(&dir.path().join("src")).unwrap();
    assert!(reads.is_empty());
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
        format!(
            "{} is over 1 MiB, so the scan does not read it",
            path.display()
        )
    );
}

#[test]
fn a_method_named_var_is_not_a_read() {
    let text = "fn f(ctx: Ctx) { ctx.var(\"A\"); ctx.inner().var_os(\"B\"); }";
    assert_eq!(reads(text), []);
}

#[test]
fn the_item_after_cfg_test_is_not_read_and_the_items_around_it_are() {
    let text = "fn a() { env::var(\"A\"); }\n#[cfg(test)]\n#[expect(unused)]\nmod tests {\n    fn t() { env::var(\"T\"); }\n}\n#[cfg(test)]\nuse std::env::var as T;\nfn b() { env::var(\"B\"); }\n";
    assert_eq!(reads(text), [read("A", 1), read("B", 9)]);
}

#[test]
fn a_file_of_unit_tests_is_not_scanned() {
    let dir = tempfile::tempdir().unwrap();
    let read_in = "fn f() { std::env::var(\"A\"); }";
    for file in ["main.rs", "tests.rs", "render_tests.rs"] {
        std::fs::write(
            dir.path().join(file),
            read_in.replace('A', &file[..1].to_uppercase()),
        )
        .unwrap();
    }
    let reads = scan(dir.path()).unwrap();
    let names: Vec<&str> = reads.keys().map(Name::as_str).collect();
    assert_eq!(names, ["M"]);
}
