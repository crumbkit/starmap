use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use super::spec;

/// The crumbkit crates whose reads of the environment happen in every
/// tool that uses them: the core crate, and the usage log that each tool
/// that records usage starts.
const CRATES: &[&str] = &["crumbkit", "crumbkit-usage"];

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
    rest[..rest.find('"').unwrap()].to_owned()
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

/// Returns the text of every production file of the crumbkit crates at the
/// versions that Cargo.lock holds.
fn crumbkit_sources() -> Vec<String> {
    CRATES
        .iter()
        .flat_map(|name| {
            let version = locked_version(name);
            sources(&registry_source(name, &version).join("src"))
        })
        .collect()
}

/// Returns the name of each variable that the list declares, without the
/// start that the tool's name gives to some.
fn declared() -> Vec<String> {
    let prefix = format!("{}_", TOOL.to_uppercase());
    spec(TOOL)
        .unwrap()
        .variables()
        .map(|(name, _)| name.as_str().trim_start_matches(&prefix).to_owned())
        .collect()
}

#[test]
fn every_variable_in_the_list_appears_in_crumbkits_source() {
    let sources = crumbkit_sources();
    for name in declared() {
        let literal = format!("\"{name}\"");
        assert!(
            sources.iter().any(|text| text.contains(&literal)),
            "{name} is declared and no crumbkit source names it, so the list is out of date"
        );
    }
}

#[test]
fn every_variable_that_crumbkit_reads_is_in_the_list_or_left_out_with_a_reason() {
    let read = Regex::new(
        r#"(?:var|var_os|env|read|colour_variable|variable)\(\s*"([A-Za-z_][A-Za-z0-9_]*)""#,
    )
    .unwrap();
    let declared = declared();
    for text in crumbkit_sources() {
        for found in read.captures_iter(&text) {
            let name = &found[1];
            assert!(
                declared.iter().any(|declared| declared == name)
                    || LEFT_OUT.iter().any(|(left_out, _)| left_out == &name),
                "crumbkit reads {name}: declare it in TEMPLATE, or add it to LEFT_OUT with a reason"
            );
        }
    }
}

#[test]
fn the_list_names_the_variables_that_start_with_the_tools_name() {
    let names: Vec<String> = spec(TOOL)
        .unwrap()
        .variables()
        .map(|(name, _)| name.as_str().to_owned())
        .collect();

    assert!(names.contains(&"SHELF_PROGRAM".to_owned()));
    assert!(names.contains(&"SHELF_OUTPUT_BUDGET".to_owned()));
    assert!(names.contains(&"NO_COLOR".to_owned()));
}

#[test]
fn a_tool_name_with_a_dash_or_a_leading_digit_gives_variable_names_a_shell_accepts() {
    for (tool, program) in [("my-tool", "MY_TOOL_PROGRAM"), ("2fa", "_2FA_PROGRAM")] {
        let spec = spec(tool).unwrap();
        assert!(
            spec.variables().any(|(name, _)| name.as_str() == program),
            "{tool} should declare {program}"
        );
    }
}
