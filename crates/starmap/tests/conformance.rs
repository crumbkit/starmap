//! Runs the conformance cases under the repository's `conformance/`
//! directory, whose README states the layout, and checks the runner against
//! small case directories that each test writes.

#![cfg(test)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use starmap::{Environment, Problem, Spec, SpecError, Verdict};
use toml_edit::{DocumentMut, Item, TableLike};

/// The repository's conformance cases.
const CONFORMANCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../conformance");

/// A file that parses, with a defaulted port.
const PORT_SPEC: &str = r#"
[variables.SHELF_PORT]
purpose = "The port the catalogue listens on."
type = "port"
unset = { default = "8080" }
show = "shown"
source = "setting"
"#;

/// A file whose default does not fit its type.
const BAD_DEFAULT_SPEC: &str = r#"
[variables.SHELF_PORT]
purpose = "The port the catalogue listens on."
type = "port"
unset = { default = "eighty" }
show = "shown"
source = "setting"
"#;

/// The source file whose `SpecProblemKind::code` lists every
/// `starmap::spec::*` code. `SpecProblemKind` is non-exhaustive and has no
/// list of its kinds, so the convention test reads the codes from the
/// source text.
const SPEC_SOURCE: &str = include_str!("../src/spec.rs");

/// The codes that no `parse` case covers. A file over 256 KiB is refused
/// with `too_large`, and such an input is too large to keep in the
/// repository.
const UNCOVERED: &[&str] = &["starmap::spec::too_large"];

/// The result of one case, or the reason it fails.
type Outcome = Result<(), String>;

/// A function that runs one case directory of a kind.
type CaseRunner = fn(&Path) -> Outcome;

/// Runs every case under `root` and returns one line for each failing case,
/// which starts with the case's path relative to the directory above
/// `root`, such as `conformance/parse/port`.
fn run(root: &Path) -> Vec<String> {
    let base = root.parent().unwrap_or(root);
    let shown = |path: &Path| {
        path.strip_prefix(base)
            .unwrap_or(path)
            .display()
            .to_string()
    };
    let mut failures = Vec::new();
    let kinds: [(&str, CaseRunner); 2] = [("parse", parse_case), ("check", check_case)];
    for (kind, run_case) in kinds {
        let dir = root.join(kind);
        match case_dirs(&dir) {
            Ok(cases) => {
                for case in cases {
                    if let Err(reason) = run_case(&case) {
                        failures.push(format!("{}: {reason}", shown(&case)));
                    }
                }
            }
            Err(reason) => failures.push(format!("{}: {reason}", shown(&dir))),
        }
    }
    failures
}

/// Returns each case directory in `dir`, in name order, or the reason
/// `dir` holds no cases the layout allows.
fn case_dirs(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(dir).map_err(|error| format!("cannot be read: {error}"))?;
    let mut cases = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("cannot be read: {error}"))?
            .path();
        if !path.is_dir() {
            return Err(format!(
                "holds {}, and the layout holds only case directories here",
                path.display()
            ));
        }
        cases.push(path);
    }
    cases.sort();
    Ok(cases)
}

/// Reads the files of the case directory `case`: each of `required`, and
/// each of `optional` that is present. A file the layout does not name is
/// refused, and so is a missing required file.
fn read_files(
    case: &Path,
    required: &[&str],
    optional: &[&str],
) -> Result<BTreeMap<String, String>, String> {
    let mut files = BTreeMap::new();
    let entries = fs::read_dir(case).map_err(|error| format!("cannot be read: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot be read: {error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !required.contains(&name.as_str()) && !optional.contains(&name.as_str()) {
            return Err(format!(
                "has the file {name}, which the layout does not name"
            ));
        }
        let text = fs::read_to_string(entry.path())
            .map_err(|error| format!("cannot read {name} as UTF-8 text: {error}"))?;
        files.insert(name, text);
    }
    if let Some(missing) = required.iter().find(|name| !files.contains_key(**name)) {
        return Err(format!("lacks the file {missing}, which the layout needs"));
    }
    Ok(files)
}

/// Refuses a key of `table`, whose dotted path is `path`, that `known` does
/// not list.
fn known(table: &dyn TableLike, path: &str, known: &[&str]) -> Outcome {
    for (key, _) in table.iter() {
        if !known.contains(&key) {
            let at = if path.is_empty() {
                format!("`{key}`")
            } else {
                format!("`{path}.{key}`")
            };
            return Err(format!(
                "case.toml has the key {at}, which the layout does not name"
            ));
        }
    }
    Ok(())
}

/// Parses `case.toml`, checks its top-level keys against `keys`, checks its
/// description, and returns the document and its `expect` table.
fn case_toml(text: &str, keys: &[&str]) -> Result<DocumentMut, String> {
    let document: DocumentMut = text
        .parse()
        .map_err(|error| format!("case.toml is not valid TOML: {error}"))?;
    known(document.as_table(), "", keys)?;
    match document.get("description").and_then(Item::as_str) {
        Some(description) if !description.trim().is_empty() => {}
        _ => return Err("case.toml has no `description` sentence".to_owned()),
    }
    if document
        .get("expect")
        .and_then(Item::as_table_like)
        .is_none()
    {
        return Err("case.toml has no `[expect]` table".to_owned());
    }
    Ok(document)
}

/// Returns the `expect` table of a document that [`case_toml`] checked.
fn expect_table(document: &DocumentMut) -> &dyn TableLike {
    document
        .get("expect")
        .and_then(Item::as_table_like)
        .unwrap_or_else(|| document.as_table())
}

/// Returns the text of `item`, whose dotted path is `path`.
fn text<'a>(item: &'a Item, path: &str) -> Result<&'a str, String> {
    item.as_str()
        .ok_or_else(|| format!("case.toml's `{path}` is not text"))
}

/// Returns the texts of the array `item`, whose dotted path is `path`.
fn texts(item: &Item, path: &str) -> Result<Vec<String>, String> {
    let array = item
        .as_array()
        .ok_or_else(|| format!("case.toml's `{path}` is not an array"))?;
    array
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("case.toml's `{path}` holds a value that is not text"))
        })
        .collect()
}

/// Returns each key of the table `item`, whose dotted path is `path`, with
/// its text.
fn text_table(item: &Item, path: &str) -> Result<Vec<(String, String)>, String> {
    let table = item
        .as_table_like()
        .ok_or_else(|| format!("case.toml's `{path}` is not a table"))?;
    table
        .iter()
        .map(|(key, value)| {
            let value = text(value, &format!("{path}.{key}"))?;
            Ok((key.to_owned(), value.to_owned()))
        })
        .collect()
}

/// Returns the code of each problem of `error`, in the parser's order.
fn codes(error: &SpecError) -> Vec<String> {
    error
        .problems()
        .iter()
        .map(|problem| problem.kind().code().to_owned())
        .collect()
}

/// Runs the parse case in `case`.
fn parse_case(case: &Path) -> Outcome {
    let files = read_files(case, &["starmap.toml", "case.toml"], &["canonical.toml"])?;
    let document = case_toml(&files["case.toml"], &["description", "expect"])?;
    let expect = expect_table(&document);
    known(expect, "expect", &["digest", "problems"])?;
    let parsed = Spec::parse(&files["starmap.toml"]);
    match (expect.get("digest"), expect.get("problems")) {
        (Some(digest), None) => {
            let digest = text(digest, "expect.digest")?;
            let canonical = files.get("canonical.toml").ok_or(
                "lacks the file canonical.toml, which a case that expects a digest needs",
            )?;
            let spec = parsed.map_err(|error| {
                format!(
                    "expected the digest {digest}, and the file is refused with {}",
                    codes(&error).join(", ")
                )
            })?;
            let actual = spec.digest().to_string();
            if actual != digest {
                return Err(format!("expected the digest {digest}, and the digest is {actual}"));
            }
            let written = spec.to_toml();
            if written != *canonical {
                return Err(format!(
                    "expected the canonical form in canonical.toml:\n{canonical}\nand the canonical form is:\n{written}"
                ));
            }
            let again = Spec::parse(canonical)
                .map_err(|error| format!("canonical.toml is refused with {}", codes(&error).join(", ")))?;
            if again.digest() != spec.digest() {
                return Err(format!(
                    "canonical.toml has the digest {}, and its own digest is the case's digest",
                    again.digest()
                ));
            }
            Ok(())
        }
        (None, Some(problems)) => {
            if files.contains_key("canonical.toml") {
                return Err(
                    "has canonical.toml, and a case that expects problems has none".to_owned(),
                );
            }
            let expected = texts(problems, "expect.problems")?;
            if expected.is_empty() {
                return Err("case.toml's `expect.problems` is empty".to_owned());
            }
            let actual = match parsed {
                Ok(spec) => {
                    return Err(format!(
                        "expected the problems {}, and the file parses with the digest {}",
                        expected.join(", "),
                        spec.digest()
                    ));
                }
                Err(error) => codes(&error),
            };
            if actual != expected {
                return Err(format!(
                    "expected the problems {}, and the problems are {}",
                    expected.join(", "),
                    actual.join(", ")
                ));
            }
            Ok(())
        }
        _ => Err(
            "case.toml's `expect` has neither or both of `digest` and `problems`, and a parse case expects one"
                .to_owned(),
        ),
    }
}

/// What a check gives: the mode and the text each variable gives the
/// child, or the variable and code of each refusal.
#[derive(Debug, PartialEq, Eq)]
enum CheckResult {
    Ready {
        mode: String,
        values: BTreeMap<String, String>,
    },
    Refused(Vec<(String, String)>),
}

impl fmt::Display for CheckResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready { mode, values } => {
                write!(formatter, "mode = \"{mode}\", values = {{ ")?;
                for (index, (name, value)) in values.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{name} = {value:?}")?;
                }
                formatter.write_str(" }")
            }
            Self::Refused(refused) => {
                formatter.write_str("refused = [")?;
                for (index, (variable, code)) in refused.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(
                        formatter,
                        "{{ variable = \"{variable}\", code = \"{code}\" }}"
                    )?;
                }
                formatter.write_str("]")
            }
        }
    }
}

/// Returns the code that `starmap run` reports for `problem`.
fn problem_code(problem: &Problem) -> &'static str {
    match problem {
        Problem::Missing { .. } => "starmap::variable::missing",
        Problem::GroupIncomplete { .. } => "starmap::group::incomplete",
        Problem::UnlistedKey { .. } => "starmap::family::unlisted_key",
        // Invalid, ShowMismatch and ModeMismatch, and any later problem,
        // are reported as an invalid value.
        _ => "starmap::variable::invalid",
    }
}

/// Returns a command that the runner applies an environment to and never
/// starts.
#[expect(
    clippy::disallowed_methods,
    reason = "the runner reads the command's environment and never starts it"
)]
fn command() -> Command {
    Command::new("conformance")
}

/// Returns the text each variable of `environment` gives a child, leaving
/// out each variable that stays unset.
fn child_values(environment: &Environment) -> Result<BTreeMap<String, String>, String> {
    let mut command = command();
    environment.apply(&mut command);
    let mut values = BTreeMap::new();
    for (name, value) in command.get_envs() {
        let Some(value) = value else {
            continue;
        };
        let name = name.to_str().ok_or("a name is not Unicode")?;
        let value = value
            .to_str()
            .ok_or_else(|| format!("the value of {name} is not Unicode"))?;
        values.insert(name.to_owned(), value.to_owned());
    }
    Ok(values)
}

/// Returns the result that the `expect` table of a check case states.
fn expected_check(expect: &dyn TableLike) -> Result<CheckResult, String> {
    known(expect, "expect", &["mode", "values", "refused"])?;
    match (expect.get("mode"), expect.get("values"), expect.get("refused")) {
        (Some(mode), Some(values), None) => {
            let mode = text(mode, "expect.mode")?;
            if mode != "local" && mode != "production" {
                return Err(format!(
                    "case.toml's `expect.mode` is \"{mode}\", and it is \"local\" or \"production\""
                ));
            }
            Ok(CheckResult::Ready {
                mode: mode.to_owned(),
                values: text_table(values, "expect.values")?.into_iter().collect(),
            })
        }
        (None, None, Some(refused)) => {
            let path = "expect.refused";
            let tables: Vec<&dyn TableLike> = match refused {
                Item::ArrayOfTables(tables) => tables.iter().map(|table| table as &dyn TableLike).collect(),
                Item::Value(toml_edit::Value::Array(array)) => array
                    .iter()
                    .map(|value| {
                        value
                            .as_inline_table()
                            .map(|table| table as &dyn TableLike)
                            .ok_or_else(|| format!("case.toml's `{path}` holds a value that is not a table"))
                    })
                    .collect::<Result<_, _>>()?,
                _ => return Err(format!("case.toml's `{path}` is not an array of tables")),
            };
            if tables.is_empty() {
                return Err(format!("case.toml's `{path}` is empty"));
            }
            let mut refusals = Vec::new();
            for table in tables {
                known(table, path, &["variable", "code"])?;
                let field = |key: &str| {
                    let item = table
                        .get(key)
                        .ok_or_else(|| format!("case.toml's `{path}` has an entry without `{key}`"))?;
                    text(item, &format!("{path}.{key}")).map(str::to_owned)
                };
                refusals.push((field("variable")?, field("code")?));
            }
            Ok(CheckResult::Refused(refusals))
        }
        _ => Err(
            "case.toml's `expect` has neither `mode` and `values` nor `refused`, and a check case expects one of them"
                .to_owned(),
        ),
    }
}

/// Runs the check case in `case`.
fn check_case(case: &Path) -> Outcome {
    let files = read_files(case, &["starmap.toml", "case.toml"], &[])?;
    let document = case_toml(
        &files["case.toml"],
        &["description", "environment", "expect"],
    )?;
    let expected = expected_check(expect_table(&document))?;
    let environment = match document.get("environment") {
        Some(item) => text_table(item, "environment")?,
        None => Vec::new(),
    };
    let spec = Spec::parse(&files["starmap.toml"])
        .map_err(|error| format!("the file is refused with {}", codes(&error).join(", ")))?;
    let vars = environment
        .into_iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    let bindings = starmap::capture(&spec, vars);
    let mode = starmap::mode(&spec, &bindings);
    let actual = match starmap::check(&spec, &bindings, mode) {
        Verdict::Ready(environment) => CheckResult::Ready {
            mode: mode.to_string(),
            values: child_values(&environment)?,
        },
        Verdict::Refused(refusals) => CheckResult::Refused(
            refusals
                .iter()
                .map(|finding| {
                    (
                        finding.name.to_string(),
                        problem_code(&finding.problem).to_owned(),
                    )
                })
                .collect(),
        ),
    };
    if actual == expected {
        Ok(())
    } else {
        Err(format!("expected {expected}, and the check gives {actual}"))
    }
}

/// Returns each `starmap::spec::*` code that `SpecProblemKind::code`
/// returns, read from the source text of `spec.rs`.
fn spec_codes() -> Vec<String> {
    let prefix = "\"starmap::spec::";
    let mut codes = Vec::new();
    let mut rest = SPEC_SOURCE;
    while let Some(start) = rest.find(prefix) {
        let after = &rest[start + 1..];
        let Some(end) = after.find('"') else {
            break;
        };
        let code = &after[..end];
        if !codes.iter().any(|known: &String| known == code) {
            codes.push(code.to_owned());
        }
        rest = &after[end..];
    }
    codes
}

/// Returns each `starmap::spec::*` code that no `parse` case under `root`
/// expects, other than the codes the layout leaves out.
fn uncovered(root: &Path) -> Vec<String> {
    let mut covered = Vec::new();
    for case in case_dirs(&root.join("parse")).unwrap_or_default() {
        let Ok(text) = fs::read_to_string(case.join("case.toml")) else {
            continue;
        };
        let Ok(document) = text.parse::<DocumentMut>() else {
            continue;
        };
        if let Some(problems) = document
            .get("expect")
            .and_then(|expect| expect.get("problems"))
            .and_then(|problems| texts(problems, "expect.problems").ok())
        {
            covered.extend(problems);
        }
    }
    spec_codes()
        .into_iter()
        .filter(|code| !UNCOVERED.contains(&code.as_str()) && !covered.contains(code))
        .collect()
}

/// Returns a new, empty `conformance` directory with `parse` and `check`
/// inside it, under a directory named after `test`.
fn cases(test: &str) -> PathBuf {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("conformance")
        .join(test);
    if base.exists() {
        fs::remove_dir_all(&base).unwrap();
    }
    let root = base.join("conformance");
    fs::create_dir_all(root.join("parse")).unwrap();
    fs::create_dir_all(root.join("check")).unwrap();
    root
}

/// Writes the case `case`, such as `parse/port`, under `root` with `files`.
fn write(root: &Path, case: &str, files: &[(&str, &str)]) {
    let dir = root.join(case);
    fs::create_dir_all(&dir).unwrap();
    for (name, text) in files {
        fs::write(dir.join(name), text).unwrap();
    }
}

/// Writes a parse case that expects `PORT_SPEC`'s digest and canonical
/// form, with `digest` in place of the digest when it is given.
fn write_digest_case(root: &Path, case: &str, digest: Option<&str>) {
    let spec = Spec::parse(PORT_SPEC).unwrap();
    let digest = digest.map_or_else(|| spec.digest().to_string(), str::to_owned);
    let expect =
        format!("description = \"A defaulted port parses.\"\n[expect]\ndigest = \"{digest}\"\n");
    write(
        root,
        case,
        &[
            ("starmap.toml", PORT_SPEC),
            ("case.toml", &expect),
            ("canonical.toml", &spec.to_toml()),
        ],
    );
}

#[test]
fn every_conformance_case_passes() {
    let failures = run(Path::new(CONFORMANCE));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_spec_code_but_too_large_has_a_parse_case() {
    let codes = spec_codes();
    assert!(
        codes.len() > UNCOVERED.len()
            && UNCOVERED
                .iter()
                .all(|code| codes.iter().any(|found| found == code)),
        "the codes read from spec.rs are {codes:?}"
    );
    let uncovered = uncovered(Path::new(CONFORMANCE));
    assert!(
        uncovered.is_empty(),
        "no parse case expects {}",
        uncovered.join(", ")
    );
}

#[test]
fn the_runner_passes_on_a_correct_case() {
    let root = cases("correct");
    write_digest_case(&root, "parse/port", None);
    write(
        &root,
        "check/empty",
        &[
            ("starmap.toml", PORT_SPEC),
            (
                "case.toml",
                "description = \"An empty value counts as unset, so the default applies.\"\n\
                 [environment]\nSHELF_PORT = \"\"\n\
                 [expect]\nmode = \"local\"\nvalues = { SHELF_PORT = \"8080\" }\n",
            ),
        ],
    );

    assert_eq!(run(&root), Vec::<String>::new());
}

#[test]
fn a_case_whose_digest_is_wrong_fails_and_names_its_path() {
    let root = cases("wrong-digest");
    let wrong = format!("sha256:{}", "0".repeat(64));
    write_digest_case(&root, "parse/port", Some(&wrong));

    let failures = run(&root);

    let actual = Spec::parse(PORT_SPEC).unwrap().digest().to_string();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].starts_with("conformance/parse/port: "),
        "{failures:?}"
    );
    assert!(failures[0].contains(&wrong), "{failures:?}");
    assert!(failures[0].contains(&actual), "{failures:?}");
}

#[test]
fn a_case_that_lists_the_wrong_code_fails() {
    let root = cases("wrong-code");
    write(
        &root,
        "parse/default",
        &[
            ("starmap.toml", BAD_DEFAULT_SPEC),
            (
                "case.toml",
                "description = \"A default that is no port is refused.\"\n\
                 [expect]\nproblems = [\"starmap::spec::bad_range\"]\n",
            ),
        ],
    );

    let failures = run(&root);

    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].starts_with("conformance/parse/default: "),
        "{failures:?}"
    );
    assert!(
        failures[0].contains("starmap::spec::bad_range"),
        "{failures:?}"
    );
    assert!(
        failures[0].contains("starmap::spec::bad_default"),
        "{failures:?}"
    );
}

#[test]
fn a_check_case_whose_result_differs_fails_with_the_expected_and_actual_result() {
    let root = cases("wrong-values");
    write(
        &root,
        "check/default",
        &[
            ("starmap.toml", PORT_SPEC),
            (
                "case.toml",
                "description = \"The default applies.\"\n\
                 [expect]\nmode = \"local\"\nvalues = { SHELF_PORT = \"9090\" }\n",
            ),
        ],
    );

    let failures = run(&root);

    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].starts_with("conformance/check/default: "),
        "{failures:?}"
    );
    assert!(
        failures[0].contains("SHELF_PORT = \"9090\""),
        "{failures:?}"
    );
    assert!(
        failures[0].contains("SHELF_PORT = \"8080\""),
        "{failures:?}"
    );
}

#[test]
fn a_case_with_an_unknown_key_is_refused() {
    let root = cases("unknown-key");
    write(
        &root,
        "check/default",
        &[
            ("starmap.toml", PORT_SPEC),
            (
                "case.toml",
                "description = \"The default applies.\"\n\
                 [expect]\nmode = \"local\"\nvalue = { SHELF_PORT = \"8080\" }\n",
            ),
        ],
    );

    let failures = run(&root);

    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].starts_with("conformance/check/default: "),
        "{failures:?}"
    );
    assert!(failures[0].contains("`expect.value`"), "{failures:?}");
}

#[test]
fn a_case_that_lacks_a_file_is_refused() {
    let root = cases("lacks-file");
    write(
        &root,
        "parse/port",
        &[(
            "case.toml",
            "description = \"A port parses.\"\n[expect]\nproblems = [\"starmap::spec::syntax\"]\n",
        )],
    );

    let failures = run(&root);

    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].starts_with("conformance/parse/port: "),
        "{failures:?}"
    );
    assert!(failures[0].contains("starmap.toml"), "{failures:?}");
}

#[test]
fn the_runner_reports_every_failing_case_in_one_run() {
    let root = cases("every-failure");
    let wrong = format!("sha256:{}", "0".repeat(64));
    write_digest_case(&root, "parse/first", Some(&wrong));
    write_digest_case(&root, "parse/second", None);
    write_digest_case(&root, "parse/third", Some(&wrong));

    let failures = run(&root);

    assert_eq!(failures.len(), 2, "{failures:?}");
    assert!(
        failures[0].starts_with("conformance/parse/first: "),
        "{failures:?}"
    );
    assert!(
        failures[1].starts_with("conformance/parse/third: "),
        "{failures:?}"
    );
}
