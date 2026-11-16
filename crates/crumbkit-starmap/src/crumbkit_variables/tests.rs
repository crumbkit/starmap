use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use super::spec;

/// The crumbkit crate whose reads happen in every tool.
const CORE: &str = "crumbkit";

/// The crumbkit crate whose reads happen only in a tool that uses it.
const USAGE: &str = "crumbkit-usage";

/// A variable that the crates read and the listing leaves out, with why.
const LEFT_OUT: &[(&str, &str)] = &[
    (
        "HOME",
        "only `completion install` and `skills install` read it",
    ),
    (
        "USERPROFILE",
        "only the Windows form of `completion install` reads it",
    ),
    ("SHELL", "only `completion install` reads it"),
    ("XDG_CONFIG_HOME", "only `completion install` reads it"),
    ("XDG_DATA_HOME", "only `completion install` reads it"),
    ("FPATH", "only `completion install` reads it"),
    (
        "BASH_COMPLETION_USER_DIR",
        "only `completion install` reads it",
    ),
    (
        "WT_SESSION",
        "only the Windows build of the usage log reads it",
    ),
    (
        "SHELF_COMPLETE",
        "a documentation example of `declare_completion_variable`",
    ),
];

/// The tool whose variables the tests list, and the start of their names.
const TOOL: &str = "shelf";

/// Returns the version of the package `name` that Cargo.lock holds.
fn locked_version(name: &str) -> String {
    let lock =
        fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock")).unwrap();
    let header = format!("name = \"{name}\"\nversion = \"");
    let start = lock
        .find(&header)
        .unwrap_or_else(|| panic!("Cargo.lock has no {name}"))
        + header.len();
    let rest = &lock[start..];
    let version = &rest[..rest.find('"').unwrap()];
    let entry = &rest[..rest.find("\n\n").unwrap_or(rest.len())];
    assert!(
        entry.contains("source = \"registry+"),
        "{name} {version} does not come from a registry, so this test cannot read its source; point the test at the checkout that provides {name}"
    );
    version.to_owned()
}

/// Returns the directory of the registry source of `name` at `version`.
#[expect(
    clippy::disallowed_methods,
    reason = "the test finds cargo's registry from the environment that cargo gives it"
)]
fn registry_source(name: &str, version: &str) -> PathBuf {
    let home = std::env::var_os("CARGO_HOME").map_or_else(
        || {
            PathBuf::from(std::env::var_os("HOME").expect("HOME or CARGO_HOME names cargo's home"))
                .join(".cargo")
        },
        PathBuf::from,
    );
    fs::read_dir(home.join("registry/src"))
        .unwrap()
        .map(|index| index.unwrap().path().join(format!("{name}-{version}")))
        .find(|path| path.is_dir())
        .unwrap_or_else(|| panic!("the registry holds no source of {name} {version}"))
}

/// Returns the text of each production `.rs` file under `dir`, which is every
/// file except those named `tests.rs` and those under a `tests` directory.
fn sources(dir: &Path) -> Vec<String> {
    let mut texts = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        if path.is_dir() && name != "tests" {
            texts.extend(sources(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") && name != "tests.rs"
        {
            texts.push(fs::read_to_string(&path).unwrap());
        }
    }
    texts
}

/// Returns the text of every production file of the crate `name` at the
/// version that Cargo.lock holds.
fn crate_sources(name: &str) -> Vec<String> {
    let version = locked_version(name);
    sources(&registry_source(name, &version).join("src"))
}

/// Returns the name of each variable that the list declares for a tool with
/// or without `crumbkit-usage`, without the start that the tool's name gives
/// to some.
fn declared(usage: bool) -> Vec<String> {
    let prefix = format!("{}_", TOOL.to_uppercase());
    spec(TOOL, usage)
        .unwrap()
        .variables()
        .map(|(name, _)| name.as_str().trim_start_matches(&prefix).to_owned())
        .collect()
}

/// Returns the name of each variable that only `crumbkit-usage` adds.
fn usage_only() -> Vec<String> {
    let core = declared(false);
    declared(true)
        .into_iter()
        .filter(|name| !core.contains(name))
        .collect()
}

#[test]
fn every_variable_of_the_crumbkit_crate_appears_in_its_source() {
    let sources = crate_sources(CORE);
    for name in declared(false) {
        let literal = format!("\"{name}\"");
        assert!(
            sources.iter().any(|text| text.contains(&literal)),
            "{name} is declared and the {CORE} source names none, so the list is out of date"
        );
    }
}

#[test]
fn every_variable_that_only_crumbkit_usage_adds_appears_in_its_source() {
    let sources = crate_sources(USAGE);
    let added = usage_only();
    assert!(!added.is_empty());
    for name in added {
        let literal = format!("\"{name}\"");
        assert!(
            sources.iter().any(|text| text.contains(&literal)),
            "{name} is declared and the {USAGE} source names none, so the list is out of date"
        );
    }
}

#[test]
fn every_variable_that_the_crumbkit_crate_reads_is_declared_or_left_out_with_a_reason() {
    every_read_is_declared_or_left_out(CORE, &declared(false));
}

#[test]
fn every_variable_that_crumbkit_usage_reads_is_declared_or_left_out_with_a_reason() {
    every_read_is_declared_or_left_out(USAGE, &declared(true));
}

/// Checks that each variable the crate `krate` reads is in `declared` or in
/// `LEFT_OUT`.
fn every_read_is_declared_or_left_out(krate: &str, declared: &[String]) {
    // A read through a call with a literal, or through a constant whose name
    // ends in VARIABLE, such as `const VARIABLE: &str = "OUTPUT_BUDGET";`.
    let read = Regex::new(concat!(
        r#"(?:var|var_os|env|read|colour_variable|variable)\(\s*"([A-Za-z_][A-Za-z0-9_]*)""#,
        r#"|const\s+[A-Z_]*VARIABLE\s*:\s*&(?:'static\s+)?str\s*=\s*"([A-Za-z_][A-Za-z0-9_]*)""#,
    ))
    .unwrap();
    for text in crate_sources(krate) {
        for found in read.captures_iter(&text) {
            let name = found
                .get(1)
                .or_else(|| found.get(2))
                .map_or("", |name| name.as_str());
            assert!(
                declared.iter().any(|declared| declared == name)
                    || LEFT_OUT.iter().any(|(left_out, _)| left_out == &name),
                "{krate} reads {name}: declare it in CORE or USAGE, or add it to LEFT_OUT with a reason"
            );
        }
    }
}

#[test]
fn the_list_names_the_variables_that_start_with_the_tools_name() {
    let names: Vec<String> = spec(TOOL, true)
        .unwrap()
        .variables()
        .map(|(name, _)| name.as_str().to_owned())
        .collect();

    assert!(names.contains(&"SHELF_OUTPUT_BUDGET".to_owned()));
    assert!(names.contains(&"SHELF_PROGRAM".to_owned()));
    assert!(names.contains(&"NO_COLOR".to_owned()));
}

#[test]
fn a_tool_without_crumbkit_usage_has_only_the_five_variables_of_the_crumbkit_crate() {
    let mut names = declared(false);
    names.sort();

    assert_eq!(
        names,
        [
            "CLICOLOR",
            "CLICOLOR_FORCE",
            "NO_COLOR",
            "OUTPUT_BUDGET",
            "TERM"
        ]
    );
}

#[test]
fn a_tool_name_with_a_dash_or_a_leading_digit_gives_variable_names_a_shell_accepts() {
    for (tool, budget) in [
        ("my-tool", "MY_TOOL_PROGRAM"),
        ("2fa", "_2FA_PROGRAM"),
        ("my-tool", "MY_TOOL_OUTPUT_BUDGET"),
        ("2fa", "_2FA_OUTPUT_BUDGET"),
    ] {
        let spec = spec(tool, true).unwrap();
        assert!(
            spec.variables().any(|(name, _)| name.as_str() == budget),
            "{tool} should declare {budget}"
        );
    }
}
