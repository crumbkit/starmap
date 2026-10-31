//! Tests for the parser of `starmap.toml`, its canonical form and its
//! digest, and the goldens of every spec problem.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use snapbox::assert::DEFAULT_ACTION_ENV;
use snapbox::{Assert, Data};

use super::{Show, Source, Spec, SpecError, SpecProblemKind, Unset};
use crate::{Choice, Form, IntegerRange, Mode, Name, Placeholder, SecretBytes, SecretRef, Value};

/// The illustrative `starmap.toml` of the build plan.
const EXAMPLE: &str = include_str!("../../tests/example/starmap.toml");

/// The directory of the spec-problem fixtures and their goldens.
const PROBLEMS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/spec");

/// The canonical form of the example, as a golden file.
const CANONICAL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/example/canonical.toml");

/// Every problem kind. A kind added to `SpecProblemKind` belongs here too,
/// with a fixture or a test of its own that writes its golden.
const KINDS: [SpecProblemKind; 17] = [
    SpecProblemKind::TooLarge,
    SpecProblemKind::Syntax,
    SpecProblemKind::UnknownField,
    SpecProblemKind::MissingField,
    SpecProblemKind::BadValue,
    SpecProblemKind::BadName,
    SpecProblemKind::Guess,
    SpecProblemKind::BadDefault,
    SpecProblemKind::BadRange,
    SpecProblemKind::EmptyEnumeration,
    SpecProblemKind::ShownSecret,
    SpecProblemKind::SecretForm,
    SpecProblemKind::BindMismatch,
    SpecProblemKind::BadBytes,
    SpecProblemKind::UnknownPlaceholder,
    SpecProblemKind::ModeUndeclared,
    SpecProblemKind::ModeSource,
];

/// Returns the path of each `.toml` fixture under `tests/spec`, in name
/// order.
fn fixtures() -> Vec<PathBuf> {
    let mut fixtures: Vec<PathBuf> = fs::read_dir(PROBLEMS)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension() == Some(OsStr::new("toml")))
        .collect();
    fixtures.sort();
    fixtures
}

/// Parses `text`, which is a valid spec.
fn spec(text: &str) -> Spec {
    match Spec::parse(text) {
        Ok(spec) => spec,
        Err(error) => panic!("the spec did not parse:\n{}", render(text, &error)),
    }
}

/// Parses `text`, which is not a valid spec, and returns the kind of each
/// problem.
fn kinds(text: &str) -> Vec<SpecProblemKind> {
    Spec::parse(text)
        .unwrap_err()
        .problems()
        .iter()
        .map(super::SpecProblem::kind)
        .collect()
}

/// Returns the name `text`.
fn name(text: &str) -> Name {
    text.parse().unwrap()
}

/// Returns a spec with one variable whose fields are `fields`.
fn one(fields: &str) -> String {
    format!("[variables.SHELF_X]\n{fields}\n")
}

/// Returns the 1-based line and column of byte `offset` in `text`,
/// counting columns in characters.
fn position(text: &str, offset: usize) -> (usize, usize) {
    let before = text.get(..offset).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |start| start.chars().count())
        + 1;
    (line, column)
}

/// Renders each problem of `error` with its code, its message, its span
/// and the line of `text` it starts on, underlined.
fn render(text: &str, error: &SpecError) -> String {
    let mut out = String::new();
    for problem in error.problems() {
        let span = problem.span();
        let (line, column) = position(text, span.start);
        let (end_line, end_column) = position(text, span.end);
        out.push_str(&format!("{}: {problem}\n", problem.kind().code()));
        out.push_str(&format!(
            "  at: {line}:{column} to {end_line}:{end_column}, bytes {span:?}\n"
        ));
        // A line is shown up to 100 characters, so the generated line of
        // the too-large fixture does not fill its golden.
        if let Some(source) = text.lines().nth(line - 1) {
            let source: String = source.chars().take(100).collect();
            let width = if end_line == line {
                (end_column - column).max(1)
            } else {
                source.chars().count().saturating_sub(column - 1).max(1)
            };
            out.push_str(&format!("{line:>4} | {source}\n"));
            out.push_str(&format!(
                "     | {}{}\n",
                " ".repeat(column - 1),
                "^".repeat(width)
            ));
        }
    }
    out
}

/// Compares `actual` with the golden file at `path`, which
/// `SNAPSHOTS=overwrite` rewrites.
fn golden(actual: String, path: &Path) {
    Assert::new()
        .action_env(DEFAULT_ACTION_ENV)
        .eq(actual, Data::read_from(path, None));
}

// The example spec

#[test]
fn the_example_spec_declares_its_variables_in_file_order() {
    let spec = spec(EXAMPLE);

    let names: Vec<&str> = spec.variables().map(|(name, _)| name.as_str()).collect();

    assert_eq!(
        names,
        [
            "SHELF_ENV",
            "SHELF_PORT",
            "SHELF_CATALOGUE_URL",
            "SHELF_DATABASE",
            "SHELF_DEBUG",
            "SHELF_PAGE_SIZE",
            "SHELF_MIRROR_TOKEN",
            "SHELF_SESSION_KEY",
            "XDG_STATE_HOME",
        ]
    );
}

#[test]
fn the_example_spec_reads_its_mode_rule() {
    let spec = spec(EXAMPLE);

    let mode = spec.mode().unwrap();

    assert_eq!(mode.variable().as_str(), "SHELF_ENV");
    assert_eq!(mode.decide(Some(OsStr::new("prod"))), Mode::Production);
    assert_eq!(mode.decide(Some(OsStr::new("dev"))), Mode::Local);
}

#[test]
fn the_example_spec_reads_each_type() {
    let spec = spec(EXAMPLE);
    let form = |text: &str| spec.variable(&name(text)).unwrap().form().clone();

    assert_eq!(form("SHELF_PORT"), Form::Port);
    assert_eq!(form("SHELF_CATALOGUE_URL"), Form::Url);
    assert_eq!(form("SHELF_DATABASE"), Form::Path { absolute: false });
    assert_eq!(form("XDG_STATE_HOME"), Form::Path { absolute: true });
    assert_eq!(form("SHELF_DEBUG"), Form::Boolean);
    assert_eq!(form("SHELF_MIRROR_TOKEN"), Form::String);
    assert_eq!(
        form("SHELF_PAGE_SIZE"),
        Form::Integer(IntegerRange::new(1, 100).unwrap())
    );
    let Form::Enumeration(choices) = form("SHELF_ENV") else {
        panic!("SHELF_ENV is not an enumeration");
    };
    let values: Vec<&str> = choices.iter().map(Choice::as_str).collect();
    assert_eq!(values, ["dev", "prod"]);
}

#[test]
fn the_example_spec_reads_each_unset_rule_in_both_modes() {
    let spec = spec(EXAMPLE);
    let unset = |text: &str, mode| {
        spec.variable(&name(text))
            .unwrap()
            .unset()
            .in_mode(mode)
            .clone()
    };

    assert_eq!(unset("SHELF_PORT", Mode::Production), Unset::Required);
    assert_eq!(unset("SHELF_CATALOGUE_URL", Mode::Local), Unset::Optional);
    assert_eq!(
        unset("SHELF_PAGE_SIZE", Mode::Production),
        Unset::Default(Value::Integer(20))
    );
    assert_eq!(
        unset("SHELF_DEBUG", Mode::Local),
        Unset::Default(Value::Boolean(false))
    );
    assert_eq!(unset("SHELF_MIRROR_TOKEN", Mode::Local), Unset::Optional);
    assert_eq!(
        unset("SHELF_MIRROR_TOKEN", Mode::Production),
        Unset::Required
    );
}

#[test]
fn the_example_spec_reads_each_show() {
    let spec = spec(EXAMPLE);
    let show = |text: &str| spec.variable(&name(text)).unwrap().show();

    assert_eq!(show("SHELF_PORT"), Show::Shown);
    assert_eq!(show("SHELF_MIRROR_TOKEN"), Show::Withheld);
    assert_eq!(show("SHELF_SESSION_KEY"), Show::Withheld);
}

#[test]
fn the_example_spec_merges_each_source_with_its_bind() {
    let spec = spec(EXAMPLE);
    let source = |text: &str| spec.variable(&name(text)).unwrap().source().clone();

    assert_eq!(source("SHELF_ENV"), Source::Setting { bind: None });
    assert_eq!(source("XDG_STATE_HOME"), Source::Inherited);

    let Source::Setting {
        bind: Some(template),
    } = source("SHELF_DATABASE")
    else {
        panic!("SHELF_DATABASE has no path bind");
    };
    assert_eq!(template.as_str(), "{workspace}/books.db");
    assert_eq!(
        template.placeholders().collect::<Vec<_>>(),
        [Placeholder::Workspace]
    );

    let Source::Provided(port) = source("SHELF_PORT") else {
        panic!("SHELF_PORT is not provided");
    };
    assert_eq!(port.kind().as_str(), "multiverse/port");
    assert_eq!(port.qualifier("process"), Some("api"));

    let Source::Secret {
        bind: Some(SecretRef::OnePassword(reference)),
    } = source("SHELF_MIRROR_TOKEN")
    else {
        panic!("SHELF_MIRROR_TOKEN has no one-password bind");
    };
    assert_eq!(reference.as_str(), "op://Personal/shelf mirror/token");
}

#[test]
fn a_generated_variable_reads_its_bytes() {
    let spec = spec(EXAMPLE);

    assert_eq!(
        spec.variable(&name("SHELF_SESSION_KEY")).unwrap().source(),
        &Source::Generated {
            bytes: SecretBytes::new(48).unwrap()
        }
    );
}

#[test]
fn a_generated_variable_without_bytes_has_32() {
    let spec = spec(&one(
        "purpose = \"Signs cookies.\"\ntype = \"string\"\nunset = \"required\"\n\
         show = \"withheld\"\nsource = \"generated\"",
    ));

    assert_eq!(
        spec.variable(&name("SHELF_X")).unwrap().source(),
        &Source::Generated {
            bytes: SecretBytes::DEFAULT
        }
    );
}

#[test]
fn the_example_spec_reads_its_purposes() {
    let spec = spec(EXAMPLE);

    assert_eq!(
        spec.variable(&name("SHELF_DEBUG")).unwrap().purpose(),
        "Logs each catalogue query."
    );
}

#[test]
fn a_spec_without_variables_is_empty() {
    let spec = spec("");

    assert_eq!(spec.variables().count(), 0);
    assert!(spec.mode().is_none());
    assert_eq!(spec.to_toml(), "");
}

// Field rules

#[test]
fn a_production_rule_without_its_own_word_equals_the_local_one() {
    let spec = spec(&one(
        "purpose = \"A port.\"\ntype = \"port\"\nunset = { default = \"8080\" }\n\
         show = \"shown\"\nsource = \"setting\"",
    ));
    let unset = spec.variable(&name("SHELF_X")).unwrap().unset();

    assert_eq!(unset.in_mode(Mode::Local), unset.in_mode(Mode::Production));
}

#[test]
fn a_default_takes_a_production_rule_of_its_own() {
    let spec = spec(&one("purpose = \"A port.\"\ntype = \"port\"\n\
         unset = { default = \"8080\", production = \"required\" }\n\
         show = \"shown\"\nsource = \"setting\""));
    let unset = spec.variable(&name("SHELF_X")).unwrap().unset();

    assert_eq!(
        unset.in_mode(Mode::Local),
        &Unset::Default(Value::Port("8080".parse().unwrap()))
    );
    assert_eq!(unset.in_mode(Mode::Production), &Unset::Required);
}

#[test]
fn an_unset_table_takes_local_or_default_and_not_both() {
    let text = one("purpose = \"A port.\"\ntype = \"port\"\n\
         unset = { local = \"optional\", default = \"8080\" }\n\
         show = \"shown\"\nsource = \"setting\"");

    assert_eq!(kinds(&text), [SpecProblemKind::BadValue]);
}

#[test]
fn a_production_rule_takes_no_default() {
    let text = one("purpose = \"A port.\"\ntype = \"port\"\n\
         unset = { local = \"optional\", production = \"8080\" }\n\
         show = \"shown\"\nsource = \"setting\"");

    assert_eq!(kinds(&text), [SpecProblemKind::BadValue]);
}

#[test]
fn a_relative_default_of_an_absolute_path_is_a_bad_default() {
    let text = one(
        "purpose = \"A directory.\"\ntype = { path = { absolute = true } }\n\
         unset = { default = \"state\" }\nshow = \"shown\"\nsource = \"inherited\"",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::BadDefault]);
}

#[test]
fn a_default_written_as_a_toml_number_is_refused() {
    let text = one(
        "purpose = \"A size.\"\ntype = { integer = { min = 1, max = 9 } }\n\
         unset = { default = 5 }\nshow = \"shown\"\nsource = \"setting\"",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::BadValue]);
}

#[test]
fn an_empty_enumeration_is_refused() {
    let text = one(
        "purpose = \"A stage.\"\ntype = { enumeration = [] }\nunset = \"required\"\n\
         show = \"shown\"\nsource = \"setting\"",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::EmptyEnumeration]);
}

#[test]
fn an_empty_purpose_is_refused() {
    let text = one(
        "purpose = \" \"\ntype = \"port\"\nunset = \"required\"\nshow = \"shown\"\n\
         source = \"setting\"",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::BadValue]);
}

#[test]
fn a_purpose_that_asks_a_question_is_not_a_guess() {
    spec(&one(
        "purpose = \"Which port does the api process listen on?\"\ntype = \"port\"\n\
         unset = \"required\"\nshow = \"shown\"\nsource = \"setting\"",
    ));
}

#[test]
fn a_generated_variable_is_shown_secret_when_it_is_shown() {
    let text = one(
        "purpose = \"Signs cookies.\"\ntype = \"string\"\nunset = \"required\"\n\
         show = \"shown\"\nsource = \"generated\"",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::ShownSecret]);
}

#[test]
fn a_withheld_setting_must_have_the_type_string() {
    let text = one(
        "purpose = \"A port.\"\ntype = \"port\"\nunset = \"required\"\n\
         show = \"withheld\"\nsource = \"setting\"",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::SecretForm]);
}

#[test]
fn each_source_refuses_the_binds_the_rules_forbid() {
    let binds = [
        ("provided", "{ provided = \"multiverse/port\" }"),
        ("path", "{ path = \"{workspace}/x\" }"),
        (
            "one-password",
            "{ one-password = \"op://Personal/shelf/token\" }",
        ),
    ];
    let allowed = [
        ("setting", "path"),
        ("secret", "one-password"),
        ("provided", "provided"),
    ];
    for source in ["setting", "secret", "generated", "inherited", "provided"] {
        let show = if matches!(source, "secret" | "generated") {
            "withheld"
        } else {
            "shown"
        };
        for (bind, written) in binds {
            let text = one(&format!(
                "purpose = \"A value.\"\ntype = \"string\"\nunset = \"optional\"\n\
                 show = \"{show}\"\nsource = \"{source}\"\nbind = {written}"
            ));
            let fits = allowed.contains(&(source, bind));

            assert_eq!(
                Spec::parse(&text).is_ok(),
                fits,
                "a {source} variable with a {bind} bind"
            );
            if !fits {
                assert_eq!(kinds(&text), [SpecProblemKind::BindMismatch]);
            }
        }
    }
}

#[test]
fn a_provided_variable_without_a_bind_is_a_bind_mismatch() {
    let text = one(
        "purpose = \"A port.\"\ntype = \"port\"\nunset = \"required\"\nshow = \"shown\"\n\
         source = \"provided\"",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::BindMismatch]);
}

#[test]
fn bytes_on_a_variable_that_is_not_generated_are_bad_bytes() {
    let text = one(
        "purpose = \"A token.\"\ntype = \"string\"\nunset = \"optional\"\n\
         show = \"withheld\"\nsource = \"secret\"\nbytes = 32",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::BadBytes]);
}

#[test]
fn bytes_from_16_to_1024_fit_a_generated_variable() {
    for (bytes, fits) in [
        (15, false),
        (16, true),
        (1024, true),
        (1025, false),
        (-1, false),
    ] {
        let text = one(&format!(
            "purpose = \"Signs cookies.\"\ntype = \"string\"\nunset = \"required\"\n\
             show = \"withheld\"\nsource = \"generated\"\nbytes = {bytes}"
        ));

        assert_eq!(Spec::parse(&text).is_ok(), fits, "bytes = {bytes}");
    }
}

#[test]
fn a_path_bind_with_a_lone_brace_is_an_unknown_placeholder() {
    let text = one(
        "purpose = \"A file.\"\ntype = \"path\"\nunset = \"required\"\nshow = \"shown\"\n\
         source = \"setting\"\nbind = { path = \"{workspace/books.db\" }",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::UnknownPlaceholder]);
}

#[test]
fn an_inherited_mode_variable_decides_the_mode() {
    let spec = spec(
        "[mode]\nvariable = \"APP_STAGE\"\nproduction = [\"prod\", \"live\"]\n\n\
         [variables.APP_STAGE]\npurpose = \"The stage.\"\ntype = \"string\"\n\
         unset = \"optional\"\nshow = \"shown\"\nsource = \"inherited\"\n",
    );

    let mode = spec.mode().unwrap();
    assert_eq!(mode.decide(Some(OsStr::new("live"))), Mode::Production);
}

#[test]
fn a_secret_mode_variable_is_refused() {
    let text = "[mode]\nvariable = \"APP_STAGE\"\nproduction = [\"prod\"]\n\n\
         [variables.APP_STAGE]\npurpose = \"The stage.\"\ntype = \"string\"\n\
         unset = \"optional\"\nshow = \"withheld\"\nsource = \"secret\"\n";

    assert_eq!(kinds(text), [SpecProblemKind::ModeSource]);
}

#[test]
fn a_withheld_mode_variable_is_refused() {
    let text = "[mode]\nvariable = \"APP_STAGE\"\nproduction = [\"prod\"]\n\n\
         [variables.APP_STAGE]\npurpose = \"The stage.\"\ntype = \"string\"\n\
         unset = \"optional\"\nshow = \"withheld\"\nsource = \"setting\"\n";

    assert_eq!(kinds(text), [SpecProblemKind::ModeSource]);
}

#[test]
fn an_empty_production_list_is_refused() {
    let text = "[mode]\nvariable = \"APP_STAGE\"\nproduction = []\n\n\
         [variables.APP_STAGE]\npurpose = \"The stage.\"\ntype = \"string\"\n\
         unset = \"optional\"\nshow = \"shown\"\nsource = \"setting\"\n";

    assert_eq!(kinds(text), [SpecProblemKind::EmptyEnumeration]);
}

#[test]
fn the_parser_reports_every_problem_at_once() {
    let text = "[variables.A]\npurpose = \"?\"\ntype = \"port\"\nunset = \"required\"\n\
         show = \"shown\"\nsource = \"setting\"\n\n\
         [variables.b]\npurpose = \"B.\"\ntype = \"port\"\nunset = \"required\"\n\
         show = \"shown\"\nsource = \"setting\"\n";

    assert_eq!(
        kinds(text),
        [SpecProblemKind::Guess, SpecProblemKind::BadName]
    );
}

#[test]
fn variables_written_as_inline_tables_parse_as_tables_do() {
    let inline = "variables.SHELF_X = { purpose = \"A port.\", type = \"port\", \
         unset = \"required\", show = \"shown\", source = \"setting\" }\n";
    let table = one(
        "purpose = \"A port.\"\ntype = \"port\"\nunset = \"required\"\nshow = \"shown\"\n\
         source = \"setting\"",
    );

    assert_eq!(spec(inline), spec(&table));
}

// The size limit

#[test]
fn a_file_of_exactly_the_limit_parses() {
    let mut text = "#".repeat(Spec::MAX_BYTES - 1);
    text.push('\n');

    assert_eq!(text.len(), Spec::MAX_BYTES);
    assert!(Spec::parse(&text).is_ok());
}

#[test]
fn a_file_one_byte_over_the_limit_is_too_large() {
    let text = "#".repeat(Spec::MAX_BYTES + 1);

    assert_eq!(kinds(&text), [SpecProblemKind::TooLarge]);
}

// The canonical form and the digest

#[test]
fn the_canonical_form_of_the_example_matches_its_golden() {
    golden(spec(EXAMPLE).to_toml(), Path::new(CANONICAL));
}

#[test]
fn the_canonical_form_of_the_example_parses_to_the_same_spec() {
    let spec = spec(EXAMPLE);

    assert_eq!(super::Spec::parse(&spec.to_toml()).unwrap(), spec);
}

#[test]
fn a_digest_prints_as_sha256_and_64_hex_digits() {
    let digest = spec(EXAMPLE).digest().to_string();

    let hex = digest.strip_prefix("sha256:").unwrap();
    assert_eq!(hex.len(), 64);
    assert!(hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn comments_layout_and_equal_spellings_leave_the_digest_unchanged() {
    let first = one(
        "purpose = \"Logs.\"\ntype = \"boolean\"\nunset = { default = \"yes\" }\n\
         show = \"shown\"\nsource = \"setting\"",
    );
    let second = "# A comment.\n[variables.SHELF_X]\nsource = 'setting'\nshow = 'shown'\n\
         unset.default = 'TRUE'\ntype = 'boolean'\npurpose = 'Logs.'\n";

    assert_eq!(spec(&first).digest(), spec(second).digest());
}

#[test]
fn equal_path_defaults_have_one_digest() {
    let with = |default: &str| {
        spec(&one(&format!(
            "purpose = \"A file.\"\ntype = \"path\"\nunset = {{ default = \"{default}\" }}\n\
             show = \"shown\"\nsource = \"setting\""
        )))
    };

    for (first, second) in [("a//b", "a/b"), ("a/b/", "a/b"), ("/var//lib/", "/var/lib")] {
        assert_eq!(with(first), with(second), "{first} and {second}");
        assert_eq!(
            with(first).digest(),
            with(second).digest(),
            "{first} and {second}"
        );
    }
}

#[test]
fn the_digest_is_the_sha256_of_the_canonical_form() {
    use sha2::Digest as _;

    let spec = spec(EXAMPLE);
    let hash = sha2::Sha256::digest(spec.to_toml().as_bytes());
    let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();

    assert_eq!(spec.digest().to_string(), format!("sha256:{hex}"));
}

#[test]
fn a_changed_value_changes_the_digest() {
    let first = one(
        "purpose = \"A port.\"\ntype = \"port\"\nunset = { default = \"8080\" }\n\
         show = \"shown\"\nsource = \"setting\"",
    );
    let second = first.replace("8080", "8081");

    assert_ne!(spec(&first).digest(), spec(&second).digest());
}

// The goldens of every spec problem

#[test]
fn each_problem_fixture_matches_its_golden_and_shows_its_kind_first() {
    for fixture in fixtures() {
        let text = fs::read_to_string(&fixture).unwrap();
        let error = Spec::parse(&text).unwrap_err();
        let stem = fixture.file_stem().unwrap().to_str().unwrap();

        assert_eq!(
            error
                .problems()
                .first()
                .map(|problem| problem.kind().code()),
            Some(stem),
            "the first problem of {stem}.toml"
        );
        golden(render(&text, &error), &fixture.with_extension("txt"));
    }
}

#[test]
fn the_too_large_problem_matches_its_golden() {
    let text = format!("{}\n", "#".repeat(Spec::MAX_BYTES));
    let error = Spec::parse(&text).unwrap_err();

    golden(
        render(&text, &error),
        &Path::new(PROBLEMS).join("too_large.txt"),
    );
}

#[test]
fn the_fixtures_and_the_too_large_test_cover_every_problem_kind() {
    let mut shown: Vec<String> = fixtures()
        .iter()
        .map(|fixture| fixture.file_stem().unwrap().to_str().unwrap().to_owned())
        .collect();
    shown.push("too_large".to_owned());
    shown.sort();
    let mut codes: Vec<String> = KINDS.iter().map(|kind| kind.code().to_owned()).collect();
    codes.sort();

    assert_eq!(shown, codes);
}

// Properties

/// Quotes `text` as a TOML string.
fn quote(text: &str) -> String {
    toml_edit::Value::from(text).to_string().trim().to_owned()
}

/// A type with a default that fits it, both written as TOML.
fn form_with_default() -> impl Strategy<Value = (String, String)> {
    let integer = (any::<i64>(), any::<i64>()).prop_flat_map(|(a, b)| {
        let (min, max) = (a.min(b), a.max(b));
        (min..=max).prop_map(move |number| {
            (
                format!("{{ integer = {{ min = {min}, max = {max} }} }}"),
                number.to_string(),
            )
        })
    });
    let enumeration =
        prop::collection::btree_set("[a-z][a-z0-9-]{0,5}", 1..4).prop_flat_map(|values| {
            let values: Vec<String> = values.into_iter().collect();
            let listed = values
                .iter()
                .map(|value| quote(value))
                .collect::<Vec<_>>()
                .join(", ");
            prop::sample::select(values)
                .prop_map(move |value| (format!("{{ enumeration = [{listed}] }}"), value))
        });
    prop_oneof![
        "[a-z]{1,8}(/[a-z]{1,8}){0,2}".prop_map(|path| ("\"path\"".to_owned(), path)),
        "(/[a-z]{1,8}){1,3}".prop_map(|path| ("{ path = { absolute = true } }".to_owned(), path)),
        (1_u16..).prop_map(|port| ("\"port\"".to_owned(), port.to_string())),
        "[a-z]{1,10}".prop_map(|host| ("\"url\"".to_owned(), format!("https://{host}.test"))),
        prop::sample::select(vec!["1", "0", "TRUE", "false", "Yes", "no", "on", "OFF"])
            .prop_map(|word| ("\"boolean\"".to_owned(), word.to_owned())),
        integer,
        enumeration,
        "\\PC{0,12}".prop_map(|text| ("\"string\"".to_owned(), text)),
    ]
}

/// An unset field written as TOML, which may use `default`.
fn unset_with(default: String) -> impl Strategy<Value = String> {
    let default = quote(&default);
    prop::sample::select(vec![
        "\"required\"".to_owned(),
        "\"optional\"".to_owned(),
        format!("{{ default = {default} }}"),
        format!("{{ default = {default}, production = \"required\" }}"),
        "{ local = \"optional\", production = \"required\" }".to_owned(),
        "{ local = \"required\", production = \"optional\" }".to_owned(),
        "{ local = \"required\" }".to_owned(),
    ])
}

/// A source, the show it allows and its bind, written as TOML lines.
fn source() -> impl Strategy<Value = (&'static str, bool, String)> {
    let template = prop::collection::vec(
        (
            "[a-z./]{0,4}",
            prop::sample::select(vec!["{workspace}", "{checkout}", "{home}"]),
        ),
        0..3,
    )
    .prop_map(|parts| {
        let mut text: String = parts
            .into_iter()
            .map(|(before, placeholder)| format!("{before}{placeholder}"))
            .collect();
        text.push_str("/books.db");
        text
    });
    prop_oneof![
        prop::option::of(template).prop_map(|bind| {
            let bind = bind.map_or(String::new(), |path| {
                format!("bind = {{ path = {} }}\n", quote(&path))
            });
            ("setting", false, bind)
        }),
        prop::option::of("[A-Za-z]{1,6}( [a-z]{1,4})?").prop_map(|item| {
            let bind = item.map_or(String::new(), |item| {
                let reference = format!("op://Personal/{item}/token");
                format!("bind = {{ one-password = {} }}\n", quote(&reference))
            });
            ("secret", true, bind)
        }),
        prop::option::of(16_u16..=1024).prop_map(|bytes| {
            let bytes = bytes.map_or(String::new(), |bytes| format!("bytes = {bytes}\n"));
            ("generated", true, bytes)
        }),
        Just(("inherited", false, String::new())),
        (
            "[a-z]{1,6}/[a-z][a-z-]{0,6}",
            prop::option::of("[a-z]{1,6}")
        )
            .prop_map(|(kind, app)| {
                let app = app.map_or(String::new(), |app| format!(", app = {}", quote(&app)));
                let bind = format!("bind = {{ provided = {}{app} }}\n", quote(&kind));
                ("provided", false, bind)
            }),
    ]
}

/// One variable's table body and its source word.
fn variable() -> impl Strategy<Value = (&'static str, String)> {
    (source(), any::<bool>(), "[A-Za-z][a-z ]{0,20}\\.").prop_flat_map(
        |((source, secret, bind), withheld, purpose)| {
            let withheld = secret || withheld;
            let form = if withheld {
                "\\PC{0,12}"
                    .prop_map(|text| ("\"string\"".to_owned(), text))
                    .boxed()
            } else {
                form_with_default().boxed()
            };
            form.prop_flat_map(move |(form, default)| {
                let purpose = purpose.clone();
                let bind = bind.clone();
                unset_with(default).prop_map(move |unset| {
                    let show = if withheld { "withheld" } else { "shown" };
                    let body = format!(
                        "purpose = {}\ntype = {form}\nunset = {unset}\nshow = \"{show}\"\n\
                         source = \"{source}\"\n{bind}",
                        quote(&purpose)
                    );
                    (source, body)
                })
            })
        },
    )
}

/// The text of a valid spec, with a mode table when one of its variables
/// may decide the mode: a shown setting or inherited variable.
fn valid_spec() -> impl Strategy<Value = String> {
    (
        prop::collection::btree_map("[A-Z_][A-Z0-9_]{0,12}", variable(), 0..6),
        any::<bool>(),
        prop::collection::btree_set("[a-z]{1,6}", 1..3),
    )
        .prop_map(|(variables, with_mode, production)| {
            let mut text = String::new();
            let decider = variables.iter().find(|(_, (source, body))| {
                matches!(*source, "setting" | "inherited") && body.contains("show = \"shown\"")
            });
            if let (true, Some((name, _))) = (with_mode, decider) {
                let production: Vec<String> = production.iter().map(|value| quote(value)).collect();
                text.push_str(&format!(
                    "[mode]\nvariable = {}\nproduction = [{}]\n\n",
                    quote(name),
                    production.join(", ")
                ));
            }
            for (name, (_, body)) in &variables {
                text.push_str(&format!("# {name}\n[variables.{name}]\n{body}\n"));
            }
            text
        })
}

/// Text made of pieces of `starmap.toml`, which reaches deeper into the
/// parser than random text does.
fn spec_like_text() -> impl Strategy<Value = String> {
    let pieces = vec![
        "[mode]\n",
        "[variables.A]\n",
        "[variables.a]\n",
        "variables.B = ",
        "variable = \"A\"\n",
        "production = [\"p\"]\n",
        "purpose = \"P.\"\n",
        "purpose = \"?\"\n",
        "type = ",
        "unset = ",
        "show = ",
        "source = ",
        "bind = ",
        "bytes = ",
        "\"port\"",
        "\"string?\"",
        "\"setting\"",
        "\"provided\"",
        "\"withheld\"",
        "{ integer = { min = 9, max = 1 } }",
        "{ enumeration = [\"a\", \"a\"] }",
        "{ path = { absolute = true } }",
        "{ default = \"x\" }",
        "{ local = \"optional\", production = \"required\" }",
        "{ provided = \"a/b\", app = \"c\" }",
        "{ path = \"{nowhere}\" }",
        "{ one-password = \"op://a/b\" }",
        "4096",
        "{",
        "}",
        "[",
        "]",
        "\"",
        "=",
        "\n",
    ];
    prop::collection::vec(prop::sample::select(pieces), 0..40).prop_map(|parts| parts.concat())
}

proptest! {
    #[test]
    fn parsing_the_canonical_form_of_a_valid_spec_returns_the_same_spec_and_digest(
        text in valid_spec(),
    ) {
        let spec = Spec::parse(&text).map_err(|error| {
            TestCaseError::fail(format!("the generated spec did not parse:\n{text}\n{error}"))
        })?;

        let canonical = spec.to_toml();
        let again = Spec::parse(&canonical).map_err(|error| {
            TestCaseError::fail(format!("the canonical form did not parse:\n{canonical}\n{error}"))
        })?;

        prop_assert_eq!(&again, &spec);
        prop_assert_eq!(again.digest(), spec.digest());
        prop_assert_eq!(again.to_toml(), canonical);
    }

    #[test]
    fn parsing_any_text_returns_a_spec_or_an_error_and_never_panics(text in "\\PC{0,512}") {
        let _ = Spec::parse(&text);
    }

    #[test]
    fn parsing_text_made_of_spec_pieces_never_panics(text in spec_like_text()) {
        if let Err(error) = Spec::parse(&text) {
            prop_assert!(!error.problems().is_empty());
            for problem in error.problems() {
                prop_assert!(problem.span().end <= text.len());
            }
        }
    }
}
