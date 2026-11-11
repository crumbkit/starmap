//! Tests for the parser of `starmap.toml`, its canonical form and its
//! digest, and the goldens of every spec problem.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use snapbox::assert::DEFAULT_ACTION_ENV;
use snapbox::{Assert, Data};

use super::{Pointer, Show, Source, Spec, SpecError, SpecProblemKind, Unset};
use crate::{
    ByteCount, ByteLength, ByteRule, Cache, Choice, Encoding, EnumerationName, IntegerRange, Mode,
    Name, Placeholder, Scheme, SecretRef, Type, Value,
};

/// The example spec in `crates/starmap/tests/example/starmap.toml`.
const EXAMPLE: &str = include_str!("../../tests/example/starmap.toml");

/// The directory of the spec-problem fixtures and their goldens.
const PROBLEMS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/spec");

/// The canonical form of the example, as a golden file.
const CANONICAL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/example/canonical.toml");

/// Every problem kind. A kind added to `SpecProblemKind` belongs here too,
/// with a fixture or a test of its own that writes its golden.
const KINDS: [SpecProblemKind; 24] = [
    SpecProblemKind::TooLarge,
    SpecProblemKind::Syntax,
    SpecProblemKind::UnknownField,
    SpecProblemKind::MissingField,
    SpecProblemKind::BadValue,
    SpecProblemKind::BadName,
    SpecProblemKind::Guess,
    SpecProblemKind::BadDefault,
    SpecProblemKind::BadRange,
    SpecProblemKind::BadPattern,
    SpecProblemKind::EmptyEnumeration,
    SpecProblemKind::ShownSecret,
    SpecProblemKind::WithheldType,
    SpecProblemKind::WithheldDefault,
    SpecProblemKind::BindMismatch,
    SpecProblemKind::BadBytes,
    SpecProblemKind::UnknownPlaceholder,
    SpecProblemKind::ModeUndeclared,
    SpecProblemKind::ModeSource,
    SpecProblemKind::PointerDeclares,
    SpecProblemKind::PointerChain,
    SpecProblemKind::EnumerationUndeclared,
    SpecProblemKind::GroupUndeclared,
    SpecProblemKind::BadGroup,
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
    let ty = |text: &str| spec.variable(&name(text)).unwrap().value_type().clone();

    assert_eq!(ty("SHELF_PORT"), Type::Port);
    assert_eq!(ty("SHELF_CATALOGUE_URL"), Type::Url { schemes: None });
    assert_eq!(ty("SHELF_DATABASE"), Type::Path { absolute: false });
    assert_eq!(ty("XDG_STATE_HOME"), Type::Path { absolute: true });
    assert_eq!(ty("SHELF_DEBUG"), Type::Boolean);
    assert_eq!(ty("SHELF_MIRROR_TOKEN"), Type::String { pattern: None });
    assert_eq!(
        ty("SHELF_PAGE_SIZE"),
        Type::Integer(IntegerRange::new(1, 100).unwrap())
    );
    let Type::Enumeration(choices) = ty("SHELF_ENV") else {
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

    let Source::Secret { bind: Some(bind) } = source("SHELF_MIRROR_TOKEN") else {
        panic!("SHELF_MIRROR_TOKEN has no bind");
    };
    let SecretRef::OnePassword(reference) = bind.reference();
    assert_eq!(reference.as_str(), "op://Personal/shelf mirror/token");
    assert_eq!(bind.cache(), Cache::Allowed);
}

#[test]
fn a_generated_variable_reads_its_bytes() {
    let spec = spec(EXAMPLE);

    assert_eq!(
        spec.variable(&name("SHELF_SESSION_KEY")).unwrap().source(),
        &Source::Generated {
            bytes: ByteCount::generated(48).unwrap(),
            encoding: Encoding::Base64Url,
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
            bytes: ByteCount::DEFAULT,
            encoding: Encoding::Base64Url,
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
fn a_socket_address_type_reads_its_default_as_a_socket_address() {
    let spec = spec(&one("purpose = \"The address the server binds.\"\n\
         type = \"socket-address\"\nunset = { default = \"0.0.0.0:8080\" }\n\
         show = \"shown\"\nsource = \"setting\""));
    let variable = spec.variable(&name("SHELF_X")).unwrap();

    assert_eq!(variable.value_type(), &Type::SocketAddress);
    assert_eq!(
        variable.unset().in_mode(Mode::Local),
        &Unset::Default(Value::SocketAddress(([0, 0, 0, 0], 8080).into()))
    );
}

#[test]
fn a_socket_address_default_that_names_a_host_is_a_bad_default() {
    let text = one("purpose = \"The address the server binds.\"\n\
         type = \"socket-address\"\nunset = { default = \"localhost:8080\" }\n\
         show = \"shown\"\nsource = \"setting\"");

    assert_eq!(kinds(&text), [SpecProblemKind::BadDefault]);
}

#[test]
fn an_unset_rule_describes_itself_for_help_text() {
    assert_eq!(Unset::Required.to_string(), "required");
    assert_eq!(Unset::Optional.to_string(), "optional");
    assert_eq!(
        Unset::Default(Value::SocketAddress(([0, 0, 0, 0], 8080).into())).to_string(),
        "default"
    );
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

/// Returns a spec with one optional shown setting that adds `extra`
/// after its purpose.
fn with_effect(extra: &str) -> String {
    one(&format!(
        "purpose = \"The bucket that recordings are written to.\"\n{extra}\
         type = \"string\"\nunset = \"optional\"\nshow = \"shown\"\nsource = \"setting\""
    ))
}

#[test]
fn a_variable_with_an_effect_parses_and_returns_it() {
    let spec = spec(&with_effect("effect = \"Calls are not recorded.\"\n"));

    let variable = spec.variable(&name("SHELF_X")).unwrap();
    assert_eq!(variable.effect(), Some("Calls are not recorded."));
}

#[test]
fn a_variable_without_an_effect_has_none() {
    let spec = spec(&with_effect(""));

    assert_eq!(spec.variable(&name("SHELF_X")).unwrap().effect(), None);
}

#[test]
fn an_empty_effect_is_refused() {
    for effect in ["\"\"", "\"  \""] {
        let text = with_effect(&format!("effect = {effect}\n"));

        assert_eq!(kinds(&text), [SpecProblemKind::BadValue], "{effect}");
    }
}

#[test]
fn an_effect_that_is_the_guess_of_init_is_refused() {
    let text = with_effect("effect = \"?\"\n");

    assert_eq!(kinds(&text), [SpecProblemKind::Guess]);
}

#[test]
fn an_effect_that_is_not_text_is_refused() {
    let text = with_effect("effect = 3\n");

    assert_eq!(kinds(&text), [SpecProblemKind::BadValue]);
}

#[test]
fn the_canonical_form_writes_an_effect_right_after_the_purpose() {
    let canonical = spec(&with_effect("effect = \"Calls are not recorded.\"\n")).to_toml();

    assert!(
        canonical.contains(
            "purpose = \"The bucket that recordings are written to.\"\n\
             effect = \"Calls are not recorded.\"\ntype = "
        ),
        "{canonical}"
    );
    assert_eq!(
        spec(&canonical),
        spec(&with_effect("effect = \"Calls are not recorded.\"\n"))
    );
}

#[test]
fn the_canonical_form_leaves_out_an_effect_that_a_variable_does_not_have() {
    let canonical = spec(&with_effect("")).to_toml();

    assert!(!canonical.contains("effect"), "{canonical}");
}

#[test]
fn adding_an_effect_changes_the_digest() {
    let without = spec(&with_effect(""));
    let with = spec(&with_effect("effect = \"Calls are not recorded.\"\n"));

    assert_ne!(without.digest(), with.digest());
}

#[test]
fn a_variable_with_a_check_sentence_parses_and_returns_it() {
    let spec = spec(&with_effect("check = \"The bucket is in eu-west-1.\"\n"));

    let variable = spec.variable(&name("SHELF_X")).unwrap();
    assert_eq!(variable.check(), Some("The bucket is in eu-west-1."));
}

#[test]
fn a_variable_without_a_check_sentence_has_none() {
    let spec = spec(&with_effect(""));

    assert_eq!(spec.variable(&name("SHELF_X")).unwrap().check(), None);
}

#[test]
fn an_empty_check_sentence_its_guess_and_one_that_is_not_text_are_refused() {
    for (check, kind) in [
        ("\"\"", SpecProblemKind::BadValue),
        ("\"  \"", SpecProblemKind::BadValue),
        ("\"?\"", SpecProblemKind::Guess),
        ("3", SpecProblemKind::BadValue),
    ] {
        let text = with_effect(&format!("check = {check}\n"));

        assert_eq!(kinds(&text), [kind], "{check}");
    }
}

#[test]
fn the_canonical_form_writes_a_check_sentence_after_the_effect() {
    let fields = "effect = \"Calls are not recorded.\"\ncheck = \"The bucket is in eu-west-1.\"\n";
    let canonical = spec(&with_effect(fields)).to_toml();

    assert!(
        canonical.contains(
            "effect = \"Calls are not recorded.\"\n\
             check = \"The bucket is in eu-west-1.\"\ntype = "
        ),
        "{canonical}"
    );
    assert_eq!(spec(&canonical), spec(&with_effect(fields)));
}

#[test]
fn a_check_sentence_counts_toward_the_digest_and_a_file_without_one_keeps_its_digest() {
    let without = spec(&with_effect(""));
    let with = spec(&with_effect("check = \"The bucket is in eu-west-1.\"\n"));

    assert!(
        !without.to_toml().contains("check"),
        "{}",
        without.to_toml()
    );
    assert_ne!(without.digest(), with.digest());
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

    assert_eq!(kinds(&text), [SpecProblemKind::WithheldType]);
}

/// Returns a spec with one shown setting whose type is `ty` and whose
/// unset rule is `unset`.
fn typed(ty: &str, unset: &str) -> String {
    one(&format!(
        "purpose = \"The revision shelf serves.\"\ntype = {ty}\nunset = {unset}\n\
         show = \"shown\"\nsource = \"setting\""
    ))
}

#[test]
fn a_string_type_table_reads_its_pattern() {
    let spec = spec(&typed(
        "{ string = { pattern = \"[0-9a-f]{40}\" } }",
        "\"required\"",
    ));

    assert_eq!(
        spec.variable(&name("SHELF_X")).unwrap().value_type(),
        &Type::String {
            pattern: Some("[0-9a-f]{40}".parse().unwrap())
        }
    );
}

#[test]
fn an_empty_string_type_table_is_a_string_without_a_pattern() {
    let table = spec(&typed("{ string = {} }", "\"required\""));
    let word = spec(&typed("\"string\"", "\"required\""));

    assert_eq!(
        table.variable(&name("SHELF_X")).unwrap().value_type(),
        &Type::String { pattern: None }
    );
    assert_eq!(table.digest(), word.digest());
}

#[test]
fn a_pattern_that_does_not_compile_is_a_bad_pattern() {
    let text = typed("{ string = { pattern = \"[0-9\" } }", "\"required\"");

    assert_eq!(kinds(&text), [SpecProblemKind::BadPattern]);
}

#[test]
fn a_pattern_over_1024_bytes_is_a_bad_pattern() {
    let ty = format!("{{ string = {{ pattern = \"{}\" }} }}", "a".repeat(1025));

    assert_eq!(
        kinds(&typed(&ty, "\"required\"")),
        [SpecProblemKind::BadPattern]
    );
}

#[test]
fn a_string_type_table_refuses_a_field_other_than_pattern() {
    let text = typed("{ string = { case = \"lower\" } }", "\"required\"");

    assert_eq!(kinds(&text), [SpecProblemKind::UnknownField]);
}

#[test]
fn a_default_that_does_not_match_the_pattern_is_a_bad_default() {
    let ty = "{ string = { pattern = \"[a-z]+-[0-9]\" } }";

    assert_eq!(
        kinds(&typed(ty, "{ default = \"eu-west\" }")),
        [SpecProblemKind::BadDefault]
    );
    spec(&typed(ty, "{ default = \"eu-1\" }"));
}

#[test]
fn a_withheld_variable_may_have_a_pattern() {
    spec(&one("purpose = \"The key shelf calls the mirror with.\"\n\
         type = { string = { pattern = \"sk-[a-z0-9]+\" } }\nunset = \"required\"\n\
         show = \"withheld\"\nsource = \"secret\""));
}

#[test]
fn the_canonical_form_writes_a_string_without_a_pattern_as_its_word() {
    let canonical = spec(&typed("{ string = {} }", "\"required\"")).to_toml();

    assert!(canonical.contains("type = \"string\"\n"), "{canonical}");
}

#[test]
fn the_canonical_form_writes_a_pattern_and_parses_back_to_the_same_spec() {
    let original = spec(&typed(
        "{ string = { pattern = '\\d{4}' } }",
        "{ default = \"2026\" }",
    ));

    let canonical = original.to_toml();

    assert!(
        canonical.contains("type = { string = { pattern = '\\d{4}' } }"),
        "{canonical}"
    );
    assert_eq!(spec(&canonical), original);
}

#[test]
fn a_changed_pattern_changes_the_digest() {
    let four = spec(&typed(
        "{ string = { pattern = \"[0-9]{4}\" } }",
        "\"required\"",
    ));
    let five = spec(&typed(
        "{ string = { pattern = \"[0-9]{5}\" } }",
        "\"required\"",
    ));

    assert_ne!(four.digest(), five.digest());
}

#[test]
fn a_withheld_variable_refuses_a_default() {
    let text = one(
        "purpose = \"The key shelf signs with.\"\ntype = \"string\"\n\
         unset = { default = \"shelf-signing-key\" }\nshow = \"withheld\"\nsource = \"setting\"",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::WithheldDefault]);
}

#[test]
fn a_withheld_variable_refuses_a_default_with_a_production_rule() {
    let text = one(
        "purpose = \"The key shelf signs with.\"\ntype = \"string\"\n\
         unset = { default = \"shelf-signing-key\", production = \"required\" }\n\
         show = \"withheld\"\nsource = \"setting\"",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::WithheldDefault]);
}

#[test]
fn a_withheld_generated_variable_refuses_a_default() {
    let text = one(
        "purpose = \"The key shelf signs with.\"\ntype = \"string\"\n\
         unset = { default = \"shelf-signing-key\" }\n\
         show = \"withheld\"\nsource = \"generated\"",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::WithheldDefault]);
}

#[test]
fn a_withheld_default_that_fails_its_type_is_refused_without_echoing_it() {
    let text = one("purpose = \"The key shelf signs with.\"\ntype = \"port\"\n\
         unset = { default = \"hunter2\" }\nshow = \"withheld\"\nsource = \"setting\"");

    let problems = Spec::parse(&text).unwrap_err();
    let rendered = problems.to_string();
    assert!(
        problems
            .problems()
            .iter()
            .all(|problem| !problem.to_string().contains("hunter2")),
        "{rendered}"
    );
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
            Some(format!("starmap::spec::{stem}").as_str()),
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
        .map(|fixture| {
            let stem = fixture.file_stem().unwrap().to_str().unwrap();
            format!("starmap::spec::{stem}")
        })
        .collect();
    shown.push("starmap::spec::too_large".to_owned());
    shown.sort();
    let mut codes: Vec<String> = KINDS.iter().map(|kind| kind.code().to_owned()).collect();
    codes.sort();

    assert_eq!(shown, codes);
}

#[test]
fn every_problem_kind_code_is_a_full_starmap_spec_code() {
    for kind in KINDS {
        let code = kind.code();
        let reason = code.strip_prefix("starmap::spec::").unwrap_or_default();
        assert!(
            !reason.is_empty() && reason.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{code}"
        );
    }
}

// Tool tables

/// A spec with one variable and tool tables at the top level and in the
/// variable.
const WITH_TOOLS: &str = r#"
[tool.lint]
strict = true

[tool.codegen]
module = "settings"
targets = ["server", "worker"]
nested = { depth = 2 }

[tool.codegen.output]
dir = "generated"

[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = "required"
show = "shown"
source = "setting"
tool.codegen = { field = "port" }
"#;

/// The spec of [`WITH_TOOLS`] without its tool tables.
const WITHOUT_TOOLS: &str = r#"
[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = "required"
show = "shown"
source = "setting"
"#;

#[test]
fn a_file_with_top_level_and_variable_tool_tables_parses() {
    let spec = spec(WITH_TOOLS);
    let name: Name = "SHELF_PORT".parse().unwrap();
    let variable = spec.variable(&name).unwrap();

    let names: Vec<&str> = spec.tools().map(super::ToolTable::name).collect();
    assert_eq!(names, ["codegen", "lint"]);
    assert_eq!(spec.tool("lint").unwrap().as_toml(), "strict = true\n");
    assert_eq!(spec.tool("absent"), None);
    let names: Vec<&str> = variable.tools().map(super::ToolTable::name).collect();
    assert_eq!(names, ["codegen"]);
    assert_eq!(
        variable.tool("codegen").unwrap().as_toml(),
        "field = \"port\"\n"
    );
}

#[test]
fn a_tool_table_round_trips_its_keys_and_values_as_toml_text() {
    let spec = spec(WITH_TOOLS);

    let text = spec.tool("codegen").unwrap().as_toml();
    let reparsed: toml_edit::DocumentMut = text.parse().unwrap();

    assert_eq!(reparsed["module"].as_str(), Some("settings"));
    assert_eq!(reparsed["targets"][1].as_str(), Some("worker"));
    assert_eq!(reparsed["nested"]["depth"].as_integer(), Some(2));
    assert_eq!(reparsed["output"]["dir"].as_str(), Some("generated"));
    assert_eq!(
        text,
        "module = \"settings\"\ntargets = [\"server\", \"worker\"]\n\n[nested]\ndepth = 2\n\n\
         [output]\ndir = \"generated\"\n"
    );
}

#[test]
fn a_key_outside_tool_is_still_refused_and_the_message_lists_tool() {
    let error = Spec::parse(&format!("colour = \"blue\"\n{WITHOUT_TOOLS}")).unwrap_err();

    assert_eq!(error.problems()[0].kind(), SpecProblemKind::UnknownField);
    assert_eq!(
        error.to_string(),
        "the file has the field `colour`, and it takes mode, enumerations, groups, variables and tool"
    );
}

#[test]
fn a_tool_value_that_is_not_a_table_of_tables_is_refused() {
    let variable =
        WITHOUT_TOOLS.replace("source = \"setting\"", "source = \"setting\"\ntool = [1]");
    for (text, message) in [
        (
            format!("tool = 3\n{WITHOUT_TOOLS}"),
            "`tool` is a toml integer, and it takes a table",
        ),
        (
            format!("tool.x = 3\n{WITHOUT_TOOLS}"),
            "`tool.x` is a toml integer, and it takes a table",
        ),
        (
            format!("[[tool.x]]\nkey = 1\n{WITHOUT_TOOLS}"),
            "`tool.x` is a toml array of tables, and it takes a table",
        ),
        (
            format!("tool.\"\" = {{}}\n{WITHOUT_TOOLS}"),
            "`tool` has an empty key, and a tool's name is not empty",
        ),
        (
            variable,
            "`variables.SHELF_PORT.tool` is a toml array, and it takes a table",
        ),
    ] {
        let error = Spec::parse(&text).unwrap_err();
        assert_eq!(error.problems().len(), 1, "{text}");
        assert_eq!(error.problems()[0].kind(), SpecProblemKind::BadValue);
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn files_that_differ_only_in_tool_tables_have_the_same_canonical_form_and_digest() {
    let with = spec(WITH_TOOLS);
    let without = spec(WITHOUT_TOOLS);

    assert_eq!(with.to_toml(), without.to_toml());
    assert_eq!(with.digest(), without.digest());
    assert_eq!(spec(&with.to_toml()), without);
}

// Pointers

/// Returns the kind of each problem of the pointer `text`.
fn pointer_kinds(text: &str) -> Vec<SpecProblemKind> {
    Pointer::parse(text)
        .unwrap_err()
        .problems()
        .iter()
        .map(|problem| problem.kind())
        .collect()
}

#[test]
fn a_file_without_spec_is_not_a_pointer() {
    assert_eq!(Pointer::parse(EXAMPLE).unwrap(), None);
}

#[test]
fn a_pointer_gives_its_paths_as_the_file_writes_them() {
    let pointer = Pointer::parse(
        "spec = \"config/environment.toml\"\nsrc = [\"src\", \"bin\"]\ndotenv = \"config/.env\"\n",
    )
    .unwrap()
    .unwrap();

    assert_eq!(pointer.spec(), Path::new("config/environment.toml"));
    assert_eq!(pointer.src(), [PathBuf::from("src"), PathBuf::from("bin")]);
    assert_eq!(pointer.dotenv(), Path::new("config/.env"));
}

#[test]
fn a_pointer_without_src_or_dotenv_scans_src_and_reads_dot_env() {
    let pointer = Pointer::parse("spec = \"config/environment.toml\"\n")
        .unwrap()
        .unwrap();

    assert_eq!(pointer.src(), [PathBuf::from("src")]);
    assert_eq!(pointer.dotenv(), Path::new(".env"));
}

#[test]
fn a_pointer_that_declares_variables_is_refused() {
    let text = fs::read_to_string(Path::new(PROBLEMS).join("pointer_declares.toml")).unwrap();

    assert_eq!(pointer_kinds(&text), [SpecProblemKind::PointerDeclares]);
}

#[test]
fn a_pointer_with_a_mode_table_is_refused_as_declaring() {
    let text = "spec = \"config/environment.toml\"\n[mode]\nvariable = \"SHELF_ENV\"\nproduction = [\"prod\"]\n";

    assert_eq!(pointer_kinds(text), [SpecProblemKind::PointerDeclares]);
}

#[test]
fn a_pointer_with_an_unknown_key_is_refused() {
    let text = "spec = \"config/environment.toml\"\ncolour = \"blue\"\n";

    assert_eq!(pointer_kinds(text), [SpecProblemKind::UnknownField]);
}

#[test]
fn a_pointer_whose_paths_are_not_text_or_are_empty_is_refused() {
    let text = "spec = 1\nsrc = [\"src\", \"\"]\ndotenv = \"\"\n";

    assert_eq!(
        pointer_kinds(text),
        [
            SpecProblemKind::BadValue,
            SpecProblemKind::BadValue,
            SpecProblemKind::BadValue
        ]
    );
}

#[test]
fn a_pointer_keeps_its_tool_tables_and_refuses_one_that_is_not_a_table() {
    let pointer = Pointer::parse(
        "spec = \"config/environment.toml\"\n[tool.codegen]\nmodule = \"settings\"\n",
    )
    .unwrap()
    .unwrap();

    assert_eq!(
        pointer.tool("codegen").unwrap().as_toml(),
        "module = \"settings\"\n"
    );
    assert_eq!(
        pointer_kinds("spec = \"config/environment.toml\"\ntool = \"codegen\"\n"),
        [SpecProblemKind::BadValue]
    );
}

#[test]
fn a_pointer_whose_src_is_not_a_list_is_refused() {
    let text = "spec = \"config/environment.toml\"\nsrc = \"src\"\n";

    assert_eq!(pointer_kinds(text), [SpecProblemKind::BadValue]);
}

#[test]
fn a_pointer_whose_src_lists_no_directory_scans_nothing() {
    let pointer = Pointer::parse("spec = \"config/environment.toml\"\nsrc = []\n")
        .unwrap()
        .unwrap();

    assert!(pointer.src().is_empty());
}

#[test]
fn a_pointer_that_is_not_toml_fails_as_syntax() {
    assert_eq!(pointer_kinds("spec = "), [SpecProblemKind::Syntax]);
}

#[test]
fn the_spec_parser_refuses_a_pointer_as_a_chain() {
    let error = Spec::parse("spec = \"config/environment.toml\"\n").unwrap_err();

    assert_eq!(error.problems()[0].kind(), SpecProblemKind::PointerChain);
}

// Properties

/// Quotes `text` as a TOML string.
fn quote(text: &str) -> String {
    toml_edit::Value::from(text).to_string().trim().to_owned()
}

/// A type with a default that fits it, both written as TOML.
fn type_with_default() -> impl Strategy<Value = (String, String)> {
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
        prop_oneof![
            any::<std::net::Ipv4Addr>().prop_map(std::net::IpAddr::from),
            any::<std::net::Ipv6Addr>().prop_map(std::net::IpAddr::from),
        ]
        .prop_flat_map(|ip| any::<u16>().prop_map(move |port| {
            (
                "\"socket-address\"".to_owned(),
                std::net::SocketAddr::new(ip, port).to_string(),
            )
        })),
        prop::sample::select(vec!["1", "0", "TRUE", "false", "Yes", "no", "on", "OFF"])
            .prop_map(|word| ("\"boolean\"".to_owned(), word.to_owned())),
        integer,
        enumeration,
        "\\PC{0,12}".prop_map(|text| ("\"string\"".to_owned(), text)),
    ]
}

/// An unset field written as TOML, which uses `default` unless the variable
/// is withheld, because a withheld variable has none.
fn unset_with(default: Option<String>) -> impl Strategy<Value = String> {
    let mut words = vec!["\"required\"".to_owned(), "\"optional\"".to_owned()];
    if let Some(default) = default {
        let default = quote(&default);
        words.push(format!("{{ default = {default} }}"));
        words.push(format!(
            "{{ default = {default}, production = \"required\" }}"
        ));
    }
    words.extend([
        "{ local = \"optional\", production = \"required\" }".to_owned(),
        "{ local = \"required\", production = \"optional\" }".to_owned(),
        "{ local = \"required\" }".to_owned(),
    ]);
    prop::sample::select(words)
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
            let ty = if withheld {
                Just(("\"string\"".to_owned(), None)).boxed()
            } else {
                type_with_default()
                    .prop_map(|(ty, default)| (ty, Some(default)))
                    .boxed()
            };
            ty.prop_flat_map(move |(ty, default)| {
                let purpose = purpose.clone();
                let bind = bind.clone();
                unset_with(default).prop_map(move |unset| {
                    let show = if withheld { "withheld" } else { "shown" };
                    let body = format!(
                        "purpose = {}\ntype = {ty}\nunset = {unset}\nshow = \"{show}\"\n\
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

// A secret's cache policy

/// Returns a spec with one withheld secret whose bind is `bind`.
fn secret(bind: &str) -> String {
    one(&format!(
        "purpose = \"A token.\"\ntype = \"string\"\nunset = \"required\"\n\
         show = \"withheld\"\nsource = \"secret\"\nbind = {bind}"
    ))
}

/// Returns the cache policy of the bind of `SHELF_X` in `text`.
fn cache_of(text: &str) -> Cache {
    let spec = spec(text);
    let Some(Source::Secret { bind: Some(bind) }) =
        spec.variable(&name("SHELF_X")).map(super::Variable::source)
    else {
        panic!("SHELF_X has no bind");
    };
    bind.cache()
}

#[test]
fn a_one_password_bind_with_cache_false_is_never_cached() {
    let text = secret("{ one-password = \"op://Personal/shelf mirror/token\", cache = false }");
    assert_eq!(cache_of(&text), Cache::Never);
}

#[test]
fn a_one_password_bind_without_cache_or_with_cache_true_may_be_cached() {
    for bind in [
        "{ one-password = \"op://Personal/shelf mirror/token\" }",
        "{ one-password = \"op://Personal/shelf mirror/token\", cache = true }",
    ] {
        assert_eq!(cache_of(&secret(bind)), Cache::Allowed, "{bind}");
    }
}

#[test]
fn cache_on_a_path_bind_is_an_unknown_field() {
    let text = one(
        "purpose = \"A file.\"\ntype = \"path\"\nunset = \"optional\"\nshow = \"shown\"\n\
         source = \"setting\"\nbind = { path = \"{workspace}/books.db\", cache = false }",
    );
    assert_eq!(kinds(&text), [SpecProblemKind::UnknownField]);
}

#[test]
fn cache_on_a_provided_bind_is_reserved_and_refused() {
    let provided = |bind: &str| {
        one(&format!(
            "purpose = \"A port.\"\ntype = \"port\"\nunset = \"required\"\nshow = \"shown\"\n\
             source = \"provided\"\nbind = {bind}"
        ))
    };

    assert_eq!(
        kinds(&provided(
            "{ provided = \"multiverse/port\", cache = \"no\" }"
        )),
        [SpecProblemKind::UnknownField]
    );
    assert!(
        Spec::parse(&provided(
            "{ provided = \"multiverse/port\", app = \"no\" }"
        ))
        .is_ok()
    );
}

#[test]
fn cache_that_is_not_true_or_false_is_refused() {
    let text = secret("{ one-password = \"op://Personal/shelf mirror/token\", cache = \"no\" }");
    assert_eq!(kinds(&text), [SpecProblemKind::BadValue]);
}

#[test]
fn the_canonical_form_writes_cache_false_and_parses_back_to_the_same_spec() {
    let never = spec(&secret(
        "{ one-password = \"op://Personal/shelf mirror/token\", cache = false }",
    ));
    let toml = never.to_toml();
    assert!(
        toml.contains(
            "bind = { one-password = \"op://Personal/shelf mirror/token\", cache = false }"
        ),
        "{toml}"
    );
    assert_eq!(Spec::parse(&toml).unwrap(), never);
}

#[test]
fn cache_false_changes_the_digest_and_cache_true_does_not() {
    let reference = "op://Personal/shelf mirror/token";
    let plain = spec(&secret(&format!("{{ one-password = \"{reference}\" }}")));
    let allowed = spec(&secret(&format!(
        "{{ one-password = \"{reference}\", cache = true }}"
    )));
    let never = spec(&secret(&format!(
        "{{ one-password = \"{reference}\", cache = false }}"
    )));

    assert_eq!(plain.digest(), allowed.digest());
    assert_ne!(plain.digest(), never.digest());
}

// The bytes type

/// Returns a shown setting whose type is `ty` and whose unset rule is
/// `unset`.
fn bytes_setting(ty: &str, unset: &str) -> String {
    one(&format!(
        "purpose = \"The salt of the catalogue's hashes.\"\ntype = {ty}\nunset = {unset}\n\
         show = \"shown\"\nsource = \"setting\""
    ))
}

/// Returns the message of each problem of `text`, which is not a valid
/// spec.
fn messages(text: &str) -> Vec<String> {
    Spec::parse(text)
        .unwrap_err()
        .problems()
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_bytes_type_reads_its_encoding_and_length() {
    let cases = [
        (
            "{ bytes = { encoding = \"base64\", length = 32 } }",
            Encoding::Base64,
            ByteLength::Exactly(ByteCount::new(32).unwrap()),
        ),
        (
            "{ bytes = { encoding = \"hex\", min-length = 16 } }",
            Encoding::Hex,
            ByteLength::AtLeast(ByteCount::new(16).unwrap()),
        ),
        (
            "{ bytes = { encoding = \"base64url\", length = 1 } }",
            Encoding::Base64Url,
            ByteLength::Exactly(ByteCount::new(1).unwrap()),
        ),
    ];
    for (ty, encoding, length) in cases {
        let parsed = spec(&bytes_setting(ty, "\"required\""));

        assert_eq!(
            parsed.variable(&name("SHELF_X")).unwrap().value_type(),
            &Type::Bytes(ByteRule::new(encoding, length)),
            "{ty}"
        );
    }
}

#[test]
fn a_bytes_type_without_an_encoding_or_with_an_unknown_one_is_refused() {
    let missing = bytes_setting("{ bytes = { length = 32 } }", "\"required\"");
    let unknown = bytes_setting(
        "{ bytes = { encoding = \"base32\", length = 32 } }",
        "\"required\"",
    );

    assert_eq!(kinds(&missing), [SpecProblemKind::MissingField]);
    assert_eq!(kinds(&unknown), [SpecProblemKind::BadValue]);
}

#[test]
fn a_bytes_type_takes_exactly_one_of_length_and_min_length() {
    let both = bytes_setting(
        "{ bytes = { encoding = \"hex\", length = 32, min-length = 16 } }",
        "\"required\"",
    );
    let neither = bytes_setting("{ bytes = { encoding = \"hex\" } }", "\"required\"");

    assert_eq!(kinds(&both), [SpecProblemKind::BadValue]);
    assert_eq!(kinds(&neither), [SpecProblemKind::MissingField]);
}

#[test]
fn a_bytes_length_outside_1_to_1024_is_bad_bytes() {
    for length in ["0", "1025", "-1", "70000"] {
        let text = bytes_setting(
            &format!("{{ bytes = {{ encoding = \"hex\", length = {length} }} }}"),
            "\"required\"",
        );

        assert_eq!(kinds(&text), [SpecProblemKind::BadBytes], "{length}");
        assert_eq!(
            messages(&text),
            [format!(
                "`variables.SHELF_X.type.bytes.length` is \"{length}\", and a byte length runs \
                 from 1 to 1024 bytes"
            )]
        );
    }
}

#[test]
fn a_shown_bytes_default_must_decode_and_fit() {
    let ty = "{ bytes = { encoding = \"hex\", length = 2 } }";

    let fits = spec(&bytes_setting(ty, "{ default = \"BEEF\" }"));
    assert_eq!(
        fits.variable(&name("SHELF_X"))
            .unwrap()
            .unset()
            .in_mode(Mode::Local),
        &Unset::Default(Value::Bytes {
            bytes: vec![0xbe, 0xef],
            encoding: Encoding::Hex,
        })
    );
    for default in ["\"zz\"", "\"beef00\""] {
        let text = bytes_setting(ty, &format!("{{ default = {default} }}"));
        assert_eq!(kinds(&text), [SpecProblemKind::BadDefault], "{default}");
    }
}

#[test]
fn a_withheld_variable_may_have_a_bytes_type() {
    let text = one("purpose = \"The key that signs session cookies.\"\n\
         type = { bytes = { encoding = \"base64\", length = 32 } }\nunset = \"required\"\n\
         show = \"withheld\"\nsource = \"secret\"");

    spec(&text);
}

/// Returns a spec with one generated variable whose type is `ty` and whose
/// other fields include `extra`.
fn generated_bytes(ty: &str, extra: &str) -> String {
    one(&format!(
        "purpose = \"The key that signs session cookies.\"\ntype = {ty}\n\
         unset = \"required\"\nshow = \"withheld\"\nsource = \"generated\"\n{extra}"
    ))
}

#[test]
fn a_generated_variable_with_each_encoding_takes_its_length_and_encoding_from_the_type() {
    for (word, encoding) in [
        ("hex", Encoding::Hex),
        ("base64", Encoding::Base64),
        ("base64url", Encoding::Base64Url),
    ] {
        for (field, length) in [("length", 32), ("min-length", 24)] {
            let ty = format!("{{ bytes = {{ encoding = \"{word}\", {field} = {length} }} }}");
            let spec = spec(&generated_bytes(&ty, ""));

            assert_eq!(
                spec.variable(&name("SHELF_X")).unwrap().source(),
                &Source::Generated {
                    bytes: ByteCount::generated(length).unwrap(),
                    encoding,
                },
                "{ty}"
            );
        }
    }
}

#[test]
fn bytes_beside_a_bytes_type_on_a_generated_variable_are_bad_bytes() {
    let text = generated_bytes(
        "{ bytes = { encoding = \"hex\", length = 32 } }",
        "bytes = 32",
    );

    assert_eq!(kinds(&text), [SpecProblemKind::BadBytes]);
    assert_eq!(
        messages(&text),
        [
            "`variables.SHELF_X.bytes` is set, and a generated variable with a bytes type takes \
             its length from the type"
        ]
    );
}

#[test]
fn a_generated_bytes_type_under_16_bytes_is_bad_bytes_for_length_and_min_length() {
    for field in ["length", "min-length"] {
        let text = generated_bytes(
            &format!("{{ bytes = {{ encoding = \"hex\", {field} = 8 }} }}"),
            "",
        );

        assert_eq!(kinds(&text), [SpecProblemKind::BadBytes], "{field}");
        assert_eq!(
            messages(&text),
            [
                "`variables.SHELF_X.type` asks for 8 bytes, and a generated value takes from 16 \
                 to 1024 bytes"
            ],
            "{field}"
        );
    }
}

#[test]
fn a_generated_string_variable_keeps_its_bytes_and_base64url() {
    let text = generated_bytes("\"string\"", "bytes = 24");

    assert_eq!(
        spec(&text).variable(&name("SHELF_X")).unwrap().source(),
        &Source::Generated {
            bytes: ByteCount::generated(24).unwrap(),
            encoding: Encoding::Base64Url,
        }
    );
    assert!(spec(&text).to_toml().contains("bytes = 24"));
}

#[test]
fn the_canonical_form_round_trips_a_generated_bytes_variable_without_a_bytes_field() {
    let text = generated_bytes("{ bytes = { encoding = \"base64\", min-length = 32 } }", "");
    let parsed = spec(&text);
    let canonical = parsed.to_toml();

    assert!(
        canonical.contains("type = { bytes = { encoding = \"base64\", min-length = 32 } }"),
        "{canonical}"
    );
    assert!(!canonical.contains("\nbytes ="), "{canonical}");
    let reparsed = spec(&canonical);
    assert_eq!(reparsed, parsed);
    assert_eq!(reparsed.digest(), parsed.digest());
}

#[test]
fn the_canonical_form_round_trips_each_bytes_rule() {
    let text = "[variables.SHELF_A]\npurpose = \"A.\"\n\
                type = { bytes = { encoding = \"hex\", length = 2 } }\n\
                unset = { default = \"BEEF\" }\nshow = \"shown\"\nsource = \"setting\"\n\n\
                [variables.SHELF_B]\npurpose = \"B.\"\n\
                type = { bytes = { min-length = 16, encoding = \"base64url\" } }\n\
                unset = \"required\"\nshow = \"withheld\"\nsource = \"secret\"\n";
    let parsed = spec(text);
    let canonical = parsed.to_toml();

    assert!(
        canonical.contains("type = { bytes = { encoding = \"hex\", length = 2 } }"),
        "{canonical}"
    );
    assert!(
        canonical.contains("unset = { default = \"beef\" }"),
        "{canonical}"
    );
    assert!(
        canonical.contains("type = { bytes = { encoding = \"base64url\", min-length = 16 } }"),
        "{canonical}"
    );
    let reparsed = spec(&canonical);
    assert_eq!(reparsed, parsed);
    assert_eq!(reparsed.digest(), parsed.digest());
}

// url schemes

/// Returns a spec with one shown url setting whose type is `ty` and whose
/// unset rule is `unset`.
fn url_setting(ty: &str, unset: &str) -> String {
    one(&format!(
        "purpose = \"The database shelf keeps its catalogue in.\"\ntype = {ty}\n\
         unset = {unset}\nshow = \"shown\"\nsource = \"setting\""
    ))
}

/// The type of the one variable `SHELF_X` in `text`, which is a valid spec.
fn only_type(text: &str) -> Type {
    spec(text)
        .variable(&name("SHELF_X"))
        .unwrap()
        .value_type()
        .clone()
}

#[test]
fn a_url_type_table_reads_its_schemes_in_order() {
    let ty = only_type(&url_setting(
        "{ url = { schemes = [\"postgresql\", \"postgres\"] } }",
        "\"required\"",
    ));

    let Type::Url {
        schemes: Some(schemes),
    } = ty
    else {
        panic!("{ty:?} has no schemes");
    };
    let words: Vec<&str> = schemes.iter().map(Scheme::as_str).collect();
    assert_eq!(words, ["postgresql", "postgres"]);
}

#[test]
fn an_empty_url_type_table_is_a_url_of_any_scheme_with_the_digest_of_the_word() {
    let table = url_setting("{ url = {} }", "\"required\"");
    let word = url_setting("\"url\"", "\"required\"");

    assert_eq!(only_type(&table), Type::Url { schemes: None });
    assert_eq!(spec(&table).digest(), spec(&word).digest());
}

#[test]
fn an_upper_case_scheme_in_the_file_is_a_bad_value_that_names_it_and_the_rule() {
    let text = url_setting("{ url = { schemes = [\"HTTPS\"] } }", "\"required\"");

    let error = Spec::parse(&text).unwrap_err();
    let problem = &error.problems()[0];
    assert_eq!(problem.kind(), SpecProblemKind::BadValue);
    assert_eq!(
        problem.to_string(),
        "`variables.SHELF_X.type.url.schemes[0]` is \"HTTPS\", and a scheme is a lower case \
         ascii letter followed by lower case letters, digits, +, - and ."
    );
}

#[test]
fn an_empty_or_repeating_scheme_list_is_an_empty_enumeration() {
    for list in ["[]", "[\"https\", \"http\", \"https\"]"] {
        let ty = format!("{{ url = {{ schemes = {list} }} }}");

        assert_eq!(
            kinds(&url_setting(&ty, "\"required\"")),
            [SpecProblemKind::EmptyEnumeration],
            "{list}"
        );
    }
}

#[test]
fn a_url_type_table_refuses_a_field_other_than_schemes() {
    let text = url_setting("{ url = { hosts = [\"shelf.example\"] } }", "\"required\"");

    assert_eq!(kinds(&text), [SpecProblemKind::UnknownField]);
}

#[test]
fn a_default_with_a_scheme_that_is_not_listed_is_a_bad_default() {
    let ty = "{ url = { schemes = [\"postgres\"] } }";

    assert_eq!(
        kinds(&url_setting(
            ty,
            "{ default = \"mysql://localhost/shelf\" }"
        )),
        [SpecProblemKind::BadDefault]
    );
    spec(&url_setting(
        ty,
        "{ default = \"postgres://localhost/shelf\" }",
    ));
}

#[test]
fn the_canonical_form_writes_a_url_without_schemes_as_its_word() {
    let canonical = spec(&url_setting("{ url = {} }", "\"required\"")).to_toml();

    assert!(canonical.contains("type = \"url\"\n"), "{canonical}");
}

#[test]
fn the_canonical_form_keeps_the_order_of_the_schemes_and_parses_back_to_the_same_spec() {
    let original = spec(&url_setting(
        "{ url = { schemes = [\"postgresql\", \"postgres\"] } }",
        "\"required\"",
    ));

    let canonical = original.to_toml();

    assert!(
        canonical.contains("type = { url = { schemes = [\"postgresql\", \"postgres\"] } }"),
        "{canonical}"
    );
    assert_eq!(spec(&canonical), original);
}

#[test]
fn a_changed_scheme_list_changes_the_digest() {
    let one = spec(&url_setting(
        "{ url = { schemes = [\"https\"] } }",
        "\"required\"",
    ));
    let two = spec(&url_setting(
        "{ url = { schemes = [\"https\", \"http\"] } }",
        "\"required\"",
    ));

    assert_ne!(one.digest(), two.digest());
}

/// The named enumeration `log-level`, with three values.
const LOG_LEVEL: &str = "[enumerations.log-level]\n\
     debug = \"Logs every request and its timing.\"\n\
     info = \"Logs each request once.\"\n\
     warn = \"Logs only failures and slow requests.\"\n\n";

/// Returns a setting called `name` whose type is `ty` and whose unset rule
/// is `unset`.
fn enumerated(name: &str, ty: &str, unset: &str) -> String {
    format!(
        "[variables.{name}]\npurpose = \"How much a process logs.\"\ntype = {ty}\n\
         unset = {unset}\nshow = \"shown\"\nsource = \"setting\"\n\n"
    )
}

/// Returns the type of the variable `name` in `spec`.
fn type_of<'s>(spec: &'s Spec, name_text: &str) -> &'s Type {
    spec.variable(&name(name_text)).unwrap().value_type()
}

#[test]
fn a_named_enumeration_parses_and_keeps_the_order_of_its_values() {
    let text = format!(
        "{LOG_LEVEL}{}",
        enumerated(
            "SHELF_LOG_LEVEL",
            "{ enumeration = \"log-level\" }",
            "{ default = \"info\" }"
        )
    );
    let spec = spec(&text);

    let Type::Enumeration(choices) = type_of(&spec, "SHELF_LOG_LEVEL") else {
        panic!("SHELF_LOG_LEVEL is an enumeration");
    };
    let values: Vec<&str> = choices.iter().map(Choice::as_str).collect();
    let warn: Choice = "warn".parse().unwrap();
    assert_eq!(values, ["debug", "info", "warn"]);
    assert_eq!(
        choices.name().map(EnumerationName::as_str),
        Some("log-level")
    );
    assert_eq!(
        choices.description(&warn),
        Some("Logs only failures and slow requests.")
    );
    let names: Vec<&str> = spec
        .enumerations()
        .filter_map(|choices| choices.name().map(EnumerationName::as_str))
        .collect();
    assert_eq!(names, ["log-level"]);
    assert_eq!(
        spec.enumeration(&"log-level".parse().unwrap()),
        Some(choices)
    );
}

#[test]
fn two_variables_share_one_named_enumeration() {
    let ty = "{ enumeration = \"log-level\" }";
    let text = format!(
        "{LOG_LEVEL}{}{}",
        enumerated("SHELF_API_LOG", ty, "\"required\""),
        enumerated("SHELF_WORKER_LOG", ty, "{ default = \"warn\" }")
    );
    let spec = spec(&text);

    assert_eq!(
        type_of(&spec, "SHELF_API_LOG"),
        type_of(&spec, "SHELF_WORKER_LOG")
    );
}

#[test]
fn a_named_enumeration_that_no_variable_uses_is_allowed() {
    let text = format!(
        "{LOG_LEVEL}{}",
        enumerated(
            "SHELF_STAGE",
            "{ enumeration = [\"dev\", \"prod\"] }",
            "\"required\""
        )
    );

    assert_eq!(spec(&text).enumerations().count(), 1);
}

#[test]
fn a_variable_that_names_an_undeclared_enumeration_is_refused() {
    let text = format!(
        "{LOG_LEVEL}{}",
        enumerated(
            "SHELF_LOG_LEVEL",
            "{ enumeration = \"log-levels\" }",
            "\"required\""
        )
    );

    let error = Spec::parse(&text).unwrap_err();
    let problem = &error.problems()[0];
    assert_eq!(error.problems().len(), 1);
    assert_eq!(problem.kind(), SpecProblemKind::EnumerationUndeclared);
    assert_eq!(
        problem.to_string(),
        "`variables.SHELF_LOG_LEVEL.type.enumeration` is \"log-levels\", which the file does not \
         declare as an enumeration"
    );
}

#[test]
fn a_variable_that_names_a_broken_enumeration_reports_only_the_enumeration() {
    let text = format!(
        "[enumerations.log-level]\ndebug = \"\"\n\n{}",
        enumerated(
            "SHELF_LOG_LEVEL",
            "{ enumeration = \"log-level\" }",
            "\"required\""
        )
    );

    assert_eq!(kinds(&text), [SpecProblemKind::BadValue]);
}

#[test]
fn an_enumeration_type_that_is_neither_a_name_nor_a_list_is_a_bad_value() {
    let text = enumerated("SHELF_LOG_LEVEL", "{ enumeration = 3 }", "\"required\"");

    assert_eq!(kinds(&text), [SpecProblemKind::BadValue]);
}

#[test]
fn an_enumeration_with_a_bad_name_is_refused() {
    let text = "[enumerations.Log_Level]\ndebug = \"Logs everything.\"\n";

    let error = Spec::parse(text).unwrap_err();
    assert_eq!(kinds(text), [SpecProblemKind::BadValue]);
    assert!(
        error
            .to_string()
            .starts_with("`enumerations.Log_Level` is not an enumeration name"),
        "{error}"
    );
}

#[test]
fn an_empty_named_enumeration_is_refused() {
    assert_eq!(
        kinds("[enumerations.log-level]\n"),
        [SpecProblemKind::EmptyEnumeration]
    );
}

#[test]
fn a_named_enumeration_value_with_whitespace_is_refused() {
    assert_eq!(
        kinds("[enumerations.log-level]\n\"very loud\" = \"Logs everything.\"\n"),
        [SpecProblemKind::BadValue]
    );
}

#[test]
fn an_empty_blank_or_missing_description_is_refused() {
    for description in ["\"\"", "\"  \"", "3", "[\"Logs.\"]"] {
        let text = format!("[enumerations.log-level]\ndebug = {description}\n");

        assert_eq!(kinds(&text), [SpecProblemKind::BadValue], "{description}");
    }
}

#[test]
fn a_description_that_is_the_guess_of_init_is_refused() {
    assert_eq!(
        kinds("[enumerations.log-level]\ndebug = \"?\"\n"),
        [SpecProblemKind::Guess]
    );
}

#[test]
fn enumerations_that_is_not_a_table_of_tables_is_a_bad_value() {
    assert_eq!(kinds("enumerations = 3\n"), [SpecProblemKind::BadValue]);
    assert_eq!(
        kinds("[enumerations]\nlog-level = 3\n"),
        [SpecProblemKind::BadValue]
    );
}

#[test]
fn a_default_outside_a_named_enumeration_is_a_bad_default() {
    let text = format!(
        "{LOG_LEVEL}{}",
        enumerated(
            "SHELF_LOG_LEVEL",
            "{ enumeration = \"log-level\" }",
            "{ default = \"trace\" }"
        )
    );

    assert_eq!(kinds(&text), [SpecProblemKind::BadDefault]);
}

#[test]
fn a_withheld_variable_cannot_name_an_enumeration() {
    let text = format!(
        "{LOG_LEVEL}[variables.SHELF_LOG_LEVEL]\npurpose = \"How much it logs.\"\n\
         type = {{ enumeration = \"log-level\" }}\nunset = \"required\"\n\
         show = \"withheld\"\nsource = \"setting\"\n"
    );

    assert_eq!(kinds(&text), [SpecProblemKind::WithheldType]);
}

#[test]
fn a_mode_variable_with_a_named_enumeration_decides_the_mode() {
    let text = format!(
        "[mode]\nvariable = \"SHELF_STAGE\"\nproduction = [\"prod\"]\n\n\
         [enumerations.stage]\ndev = \"A developer's machine.\"\nprod = \"The live service.\"\n\n{}",
        enumerated(
            "SHELF_STAGE",
            "{ enumeration = \"stage\" }",
            "{ default = \"dev\" }"
        )
    );
    let spec = spec(&text);

    let mode = spec.mode().unwrap();
    assert_eq!(mode.decide(Some(OsStr::new("prod"))), Mode::Production);
    assert_eq!(mode.decide(Some(OsStr::new("dev"))), Mode::Local);
}

#[test]
fn the_canonical_form_writes_enumerations_in_name_order_before_the_variables() {
    let text = format!(
        "{}[enumerations.stage]\nprod = \"Live.\"\ndev = \"Local.\"\n\n{LOG_LEVEL}",
        enumerated(
            "SHELF_LOG_LEVEL",
            "{ enumeration = \"log-level\" }",
            "{ default = \"info\" }"
        )
    );
    let original = spec(&text);

    let canonical = original.to_toml();

    assert_eq!(
        canonical,
        "[enumerations.log-level]\n\
         debug = \"Logs every request and its timing.\"\n\
         info = \"Logs each request once.\"\n\
         warn = \"Logs only failures and slow requests.\"\n\
         \n\
         [enumerations.stage]\n\
         prod = \"Live.\"\n\
         dev = \"Local.\"\n\
         \n\
         [variables.SHELF_LOG_LEVEL]\n\
         purpose = \"How much a process logs.\"\n\
         type = { enumeration = \"log-level\" }\n\
         unset = { default = \"info\" }\n\
         show = \"shown\"\n\
         source = \"setting\"\n"
    );
    assert_eq!(spec(&canonical), original);
}

#[test]
fn a_changed_description_changes_the_digest() {
    let text = format!(
        "{LOG_LEVEL}{}",
        enumerated(
            "SHELF_LOG_LEVEL",
            "{ enumeration = \"log-level\" }",
            "\"required\""
        )
    );
    let changed = text.replace("Logs each request once.", "Logs one line per request.");

    assert_ne!(spec(&text).digest(), spec(&changed).digest());
}

#[test]
fn a_renamed_enumeration_changes_the_digest() {
    let text = format!(
        "{LOG_LEVEL}{}",
        enumerated(
            "SHELF_LOG_LEVEL",
            "{ enumeration = \"log-level\" }",
            "\"required\""
        )
    );
    let renamed = text.replace("log-level", "verbosity");

    assert_ne!(spec(&text).digest(), spec(&renamed).digest());
}

#[test]
fn a_pointer_with_an_enumerations_table_is_refused_as_declaring() {
    let text = "spec = \"config/environment.toml\"\n[enumerations.stage]\ndev = \"Local.\"\n";

    assert_eq!(pointer_kinds(text), [SpecProblemKind::PointerDeclares]);
}
