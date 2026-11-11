use starmap::Name;

use super::{Target, lines_of_reads, pattern, scan};

/// Returns each read of a literal name in `text` as its name and line.
fn reads(text: &str) -> Vec<(String, usize)> {
    lines_of_reads(text)
        .unwrap()
        .into_iter()
        .filter_map(|read| match read.target {
            Target::Variable(name) => Some((name.as_str().to_owned(), read.line)),
            Target::Family(_) | Target::Unresolved => None,
        })
        .collect()
}

/// Returns each read in `text` whose name is built with `format!`, as the
/// family prefix it reads, or `?` when the scan cannot resolve it, and its
/// line.
fn built(text: &str) -> Vec<(String, usize)> {
    lines_of_reads(text)
        .unwrap()
        .into_iter()
        .filter_map(|read| match read.target {
            Target::Variable(_) => None,
            Target::Family(prefix) => Some((prefix.as_str().to_owned(), read.line)),
            Target::Unresolved => Some(("?".to_owned(), read.line)),
        })
        .collect()
}

/// Returns the names in `text` that a field attribute reads with a check.
fn checked(text: &str) -> Vec<String> {
    lines_of_reads(text)
        .unwrap()
        .into_iter()
        .filter(|read| read.checked)
        .filter_map(|read| match read.target {
            Target::Variable(name) => Some(name.as_str().to_owned()),
            Target::Family(_) | Target::Unresolved => None,
        })
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
fn a_starmap_field_attribute_with_a_check_is_a_checked_read() {
    let text = "struct Env {\n    #[starmap(\"SHELF_PORT\", check = unprivileged)]\n    port: u16,\n    #[starmap(\"SHELF_HOST\")]\n    host: String,\n}\n";
    assert_eq!(reads(text), [read("SHELF_PORT", 2), read("SHELF_HOST", 4)]);
    assert_eq!(checked(text), ["SHELF_PORT"]);
}

#[test]
fn env_var_with_more_than_a_name_is_not_a_read() {
    let text = "fn f() { env::var(\"A\", check = x); }";
    assert_eq!(reads(text), []);
}

#[test]
fn a_scan_lists_the_first_place_of_each_checked_field() {
    let dir = tempfile::tempdir().unwrap();
    let text = "struct Env {\n    #[starmap(\"SHELF_PORT\", check = crate::rules::port)]\n    port: u16,\n}\n";
    std::fs::write(dir.path().join("env.rs"), text).unwrap();
    let scanned = scan(dir.path()).unwrap();
    let checks: Vec<(&str, &str)> = scanned
        .checks
        .iter()
        .map(|(name, at)| (name.as_str(), at.as_str()))
        .collect();
    let at = format!("{}:2", dir.path().join("env.rs").display());
    assert_eq!(checks, [("SHELF_PORT", at.as_str())]);
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
    let scanned = scan(&dir.path().join("src")).unwrap();
    assert!(scanned.reads.is_empty());
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
    let scanned = scan(dir.path()).unwrap();
    let names: Vec<&str> = scanned.reads.keys().map(Name::as_str).collect();
    assert_eq!(names, ["M"]);
}

#[test]
fn a_test_only_field_is_skipped_and_the_next_field_is_read() {
    let text = "struct Env {\n    #[cfg(test)]\n    #[starmap(\"T\")]\n    t: String,\n    #[starmap(\"P\")]\n    port: u16,\n}\n";
    assert_eq!(reads(text), [read("P", 5)]);
}

#[test]
fn a_test_only_match_arm_is_skipped_and_the_next_arm_is_read() {
    let text = "fn f() {\n    match x {\n        #[cfg(test)]\n        A => env::var(\"A\"),\n        B => env::var(\"B\"),\n    }\n}\n";
    assert_eq!(reads(text), [read("B", 5)]);
}

#[test]
fn a_format_read_with_a_trailing_placeholder_reads_the_family_of_its_prefix() {
    let text = "fn f(region: &str) {\n    env::var(format!(\"SHELF_DB_URL_{}\", region));\n    env::var_os(format!(\"SHELF_TOKEN_{region}\"));\n}\n";
    assert_eq!(
        built(text),
        [read("SHELF_DB_URL", 2), read("SHELF_TOKEN", 3)]
    );
    assert_eq!(reads(text), []);
}

#[test]
fn a_format_read_behind_a_reference_or_a_path_reads_the_family() {
    let text = "fn f(r: &str) {\n    env::var(&format!(\"SHELF_DB_URL_{r}\"));\n    env::var(std::format!(\"SHELF_DB_URL_{r}\"),);\n    env::var(&::std::format!(\"SHELF_DB_URL_{r}\"));\n}\n";
    assert_eq!(
        built(text),
        [
            read("SHELF_DB_URL", 2),
            read("SHELF_DB_URL", 3),
            read("SHELF_DB_URL", 4)
        ]
    );
}

#[test]
fn a_format_read_with_a_leading_a_middle_or_a_second_placeholder_is_unresolved() {
    let text = "fn f(a: &str, b: &str) {\n    env::var(format!(\"{a}_TOKEN\"));\n    env::var(format!(\"SHELF_{a}_TOKEN\"));\n    env::var(format!(\"SHELF_{a}_{b}\"));\n    env::var(format!(\"SHELF_{a}{b}\"));\n}\n";
    assert_eq!(
        built(text),
        [read("?", 2), read("?", 3), read("?", 4), read("?", 5)]
    );
}

#[test]
fn a_format_read_without_a_placeholder_reads_the_name_it_holds() {
    let text = "fn f() { env::var(format!(\"SHELF_PORT\")); env::var(format!(\"shelf\")); }";
    assert_eq!(reads(text), [read("SHELF_PORT", 1)]);
    assert_eq!(built(text), [read("?", 1)]);
}

#[test]
fn doubled_braces_in_a_format_literal_are_text_and_not_placeholders() {
    assert_eq!(
        pattern("SHELF_{{A}}_{}"),
        Some(("SHELF_{A}_".to_owned(), vec![10]))
    );
    assert_eq!(pattern("SHELF_{{}}"), Some(("SHELF_{}".to_owned(), vec![])));
    assert_eq!(pattern("SHELF_}"), None);
    assert_eq!(pattern("SHELF_{"), None);
    let text = "fn f(r: &str) { env::var(format!(\"SHELF_{{}}_{r}\")); }";
    assert_eq!(built(text), [read("?", 1)]);
}

#[test]
fn a_format_read_in_test_code_is_skipped_as_other_reads_are() {
    let text = "#[cfg(test)]\nfn t(r: &str) { env::var(format!(\"SHELF_DB_URL_{r}\")); }\nfn f(r: &str) { env::var(format!(\"{r}_TOKEN\")); }\n";
    assert_eq!(built(text), [read("?", 3)]);
}

#[test]
fn a_format_call_that_is_not_a_read_is_not_counted() {
    let text = "fn f(r: &str) {\n    println!(\"{}\", format!(\"SHELF_{r}\"));\n    env::var(format!(\"A_{}\", r).as_str());\n    env::var(format!(NAME));\n    #[starmap(format!(\"A_{r}\"))]\n    x: u8,\n}\n";
    assert_eq!(built(text), []);
}

#[test]
fn a_scan_lists_family_reads_by_prefix_and_unresolved_reads_by_place() {
    let dir = tempfile::tempdir().unwrap();
    let text = "fn f(r: &str) {\n    env::var(format!(\"SHELF_DB_URL_{r}\"));\n    env::var(format!(\"{r}_TOKEN\"));\n}\n";
    std::fs::write(dir.path().join("regions.rs"), text).unwrap();
    let scanned = scan(dir.path()).unwrap();
    let file = dir.path().join("regions.rs").display().to_string();
    let families: Vec<(&str, Vec<String>)> = scanned
        .families
        .iter()
        .map(|(prefix, places)| (prefix.as_str(), places.clone()))
        .collect();
    assert_eq!(families, [("SHELF_DB_URL", vec![format!("{file}:2")])]);
    assert_eq!(scanned.unresolved, [format!("{file}:3")]);
    assert!(scanned.reads.is_empty());
}

#[test]
fn a_module_that_starts_with_an_inner_cfg_test_is_not_read() {
    let text = "#![cfg(test)]\nfn f() { env::var(\"A\"); }\n";
    assert_eq!(reads(text), []);
}
