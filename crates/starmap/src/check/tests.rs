//! Tests for capturing an environment, deciding the mode and checking
//! bindings against a spec.

use std::ffi::OsString;

use proptest::prelude::*;

use super::{Finding, Problem, Quote, Verdict, capture, check, mode};
use crate::{Bindings, Bound, Environment, Mode, Name, Secret, Show, Spec, TypeError, Value};

/// A spec with a mode variable, a required port, a defaulted boolean, an
/// optional absolute path, and a withheld token that only production
/// requires.
const SPEC: &str = r#"
[mode]
variable = "SHELF_ENV"
production = ["prod"]

[variables.SHELF_ENV]
purpose = "Selects local or production behaviour."
type = { enumeration = ["dev", "prod"] }
unset = { default = "dev" }
show = "shown"
source = "setting"

[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = "required"
show = "shown"
source = "setting"

[variables.SHELF_DEBUG]
purpose = "Logs each catalogue query."
type = "boolean"
unset = { default = "false" }
show = "shown"
source = "setting"

[variables.XDG_STATE_HOME]
purpose = "The directory that holds shelf's state."
type = { path = { absolute = true } }
unset = "optional"
show = "shown"
source = "inherited"

[variables.SHELF_MIRROR_TOKEN]
purpose = "The token shelf sends to the catalogue mirror."
type = "string"
unset = { local = "optional", production = "required" }
show = "withheld"
source = "secret"
"#;

/// Returns the parsed [`SPEC`].
fn spec() -> Spec {
    Spec::parse(SPEC).unwrap()
}

/// Returns the name `text`.
fn name(text: &str) -> Name {
    text.parse().unwrap()
}

/// Returns `pairs` as an environment.
fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect()
}

/// Captures `pairs` with [`SPEC`] and checks them in the mode they decide.
fn verdict(pairs: &[(&str, &str)]) -> Verdict {
    let spec = spec();
    let bindings = capture(&spec, vars(pairs));
    check(&spec, &bindings, mode(&spec, &bindings))
}

/// Returns the environment of a verdict that is ready.
fn ready(verdict: Verdict) -> Environment {
    match verdict {
        Verdict::Ready(environment) => environment,
        Verdict::Refused(refusals) => panic!("the check refused: {refusals:?}"),
    }
}

/// Returns the findings of a verdict, which are none when it is ready.
fn findings(verdict: Verdict) -> Vec<Finding> {
    match verdict {
        Verdict::Refused(refusals) => refusals.iter().cloned().collect(),
        Verdict::Ready(_) => Vec::new(),
    }
}

/// Returns a finding of `problem` for the variable `text`.
fn finding(text: &str, problem: Problem) -> Finding {
    Finding {
        name: name(text),
        problem,
        value: None,
    }
}

// capture

#[test]
fn capture_binds_only_the_variables_the_spec_declares() {
    let bindings = capture(
        &spec(),
        vars(&[
            ("SHELF_PORT", "8080"),
            ("SHELF_THEME", "dark"),
            ("shelf_debug", "on"),
        ]),
    );

    assert!(matches!(
        bindings.get(&name("SHELF_PORT")),
        Some(Bound::Plain(value)) if value == "8080"
    ));
    assert!(bindings.get(&name("SHELF_THEME")).is_none());
    assert!(bindings.get(&name("SHELF_DEBUG")).is_none());
}

#[test]
fn capture_holds_a_withheld_value_as_a_secret() {
    let bindings = capture(&spec(), vars(&[("SHELF_MIRROR_TOKEN", "hunter2")]));

    assert!(matches!(
        bindings.get(&name("SHELF_MIRROR_TOKEN")),
        Some(Bound::Secret(_))
    ));
}

#[test]
#[cfg(unix)]
fn capture_binds_a_withheld_value_that_is_not_unicode_as_not_unicode() {
    use std::os::unix::ffi::OsStringExt;

    let raw = OsString::from_vec(vec![0xff, 0xfe]);
    let bindings = capture(&spec(), [("SHELF_MIRROR_TOKEN".into(), raw)]);

    assert!(matches!(
        bindings.get(&name("SHELF_MIRROR_TOKEN")),
        Some(Bound::NotUnicode)
    ));
}

#[test]
#[cfg(unix)]
fn a_variable_binds_one_raw_value_by_its_show() {
    use std::os::unix::ffi::OsStringExt;

    let spec = spec();
    let port = spec.variable(&name("SHELF_PORT")).unwrap();
    let token = spec.variable(&name("SHELF_MIRROR_TOKEN")).unwrap();

    assert!(matches!(
        port.capture("8080".into()),
        Bound::Plain(value) if value == "8080"
    ));
    assert!(matches!(token.capture("hunter2".into()), Bound::Secret(_)));
    assert!(matches!(
        token.capture(OsString::from_vec(vec![0xff, 0xfe])),
        Bound::NotUnicode
    ));
}

#[test]
fn capture_binds_nothing_for_an_empty_value() {
    let bindings = capture(
        &spec(),
        vars(&[("SHELF_PORT", ""), ("SHELF_MIRROR_TOKEN", "")]),
    );

    assert!(bindings.get(&name("SHELF_PORT")).is_none());
    assert!(bindings.get(&name("SHELF_MIRROR_TOKEN")).is_none());
}

#[test]
fn an_empty_later_value_leaves_an_earlier_value_in_place() {
    let bindings = capture(&spec(), vars(&[("SHELF_PORT", "8080"), ("SHELF_PORT", "")]));

    assert!(matches!(
        bindings.get(&name("SHELF_PORT")),
        Some(Bound::Plain(value)) if value == "8080"
    ));
}

// mode

#[test]
fn a_spec_without_a_mode_table_runs_in_local_mode() {
    let spec = Spec::parse("").unwrap();

    assert_eq!(mode(&spec, &Bindings::new()), Mode::Local);
}

#[test]
fn an_unbound_mode_variable_decides_the_mode_by_its_default() {
    assert_eq!(mode(&spec(), &Bindings::new()), Mode::Local);
}

#[test]
fn a_production_value_of_the_mode_variable_selects_production() {
    let spec = spec();
    let bindings = capture(&spec, vars(&[("SHELF_ENV", "prod")]));

    assert_eq!(mode(&spec, &bindings), Mode::Production);
}

#[test]
fn an_invalid_mode_value_decides_local_mode_and_check_refuses_it() {
    let spec = spec();
    let bindings = capture(
        &spec,
        vars(&[("SHELF_ENV", "live"), ("SHELF_PORT", "8080")]),
    );

    assert_eq!(mode(&spec, &bindings), Mode::Local);
    let findings = findings(check(&spec, &bindings, Mode::Local));
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].name, name("SHELF_ENV"));
    assert!(matches!(
        findings[0].problem,
        Problem::Invalid(TypeError::NotChoice(_))
    ));
}

#[test]
fn a_mode_value_the_type_counts_as_unset_falls_back_to_the_default() {
    let spec = Spec::parse(
        r#"
[mode]
variable = "STAGE_DIR"
production = ["/prod"]

[variables.STAGE_DIR]
purpose = "Selects the stage by its directory."
type = { path = { absolute = true } }
unset = { default = "/prod" }
show = "shown"
source = "setting"
"#,
    )
    .unwrap();
    let bindings = capture(&spec, vars(&[("STAGE_DIR", "relative")]));

    assert_eq!(mode(&spec, &bindings), Mode::Production);
}

// check

#[test]
fn every_variable_that_fits_its_type_makes_the_check_ready() {
    let environment = ready(verdict(&[
        ("SHELF_PORT", "8080"),
        ("SHELF_DEBUG", "yes"),
        ("XDG_STATE_HOME", "/state"),
        ("SHELF_MIRROR_TOKEN", "hunter2"),
    ]));

    assert_eq!(environment.mode(), Mode::Local);
    assert_eq!(
        environment.value(&name("SHELF_PORT")),
        Some(&Value::Port(8080.try_into().unwrap()))
    );
    assert_eq!(
        environment.value(&name("SHELF_DEBUG")),
        Some(&Value::Boolean(true))
    );
    assert_eq!(
        environment.value(&name("XDG_STATE_HOME")),
        Some(&Value::Path("/state".into()))
    );
    assert!(environment.secret(&name("SHELF_MIRROR_TOKEN")).is_some());
}

#[test]
fn an_unbound_variable_takes_its_default() {
    let environment = ready(verdict(&[("SHELF_PORT", "8080")]));

    assert_eq!(
        environment.value(&name("SHELF_DEBUG")),
        Some(&Value::Boolean(false))
    );
    assert_eq!(
        environment.value(&name("SHELF_ENV")),
        Some(&Value::Choice("dev".parse().unwrap()))
    );
}

#[test]
fn an_unbound_optional_variable_stays_unset() {
    let environment = ready(verdict(&[("SHELF_PORT", "8080")]));

    assert_eq!(environment.value(&name("XDG_STATE_HOME")), None);
    assert!(environment.secret(&name("SHELF_MIRROR_TOKEN")).is_none());
}

#[test]
fn an_unbound_required_variable_is_missing_in_the_mode() {
    assert_eq!(
        findings(verdict(&[])),
        [finding(
            "SHELF_PORT",
            Problem::Missing { mode: Mode::Local }
        )]
    );
}

#[test]
fn a_variable_that_production_requires_is_missing_only_in_production() {
    assert_eq!(
        findings(verdict(&[("SHELF_PORT", "8080"), ("SHELF_ENV", "prod")])),
        [finding(
            "SHELF_MIRROR_TOKEN",
            Problem::Missing {
                mode: Mode::Production
            }
        )]
    );
}

#[test]
fn a_value_that_does_not_fit_its_type_is_invalid() {
    assert_eq!(
        findings(verdict(&[("SHELF_PORT", "0")])),
        [Finding {
            value: Some(Quote::new(&OsString::from("0"))),
            ..finding("SHELF_PORT", Problem::Invalid(TypeError::NotPort))
        }]
    );
}

#[test]
fn a_relative_value_of_an_absolute_path_counts_as_unset() {
    let environment = ready(verdict(&[
        ("SHELF_PORT", "8080"),
        ("XDG_STATE_HOME", "state"),
    ]));

    assert_eq!(environment.value(&name("XDG_STATE_HOME")), None);
}

#[test]
fn an_empty_value_in_the_environment_takes_the_default_of_its_variable() {
    let environment = ready(verdict(&[
        ("SHELF_PORT", "8080"),
        ("SHELF_DEBUG", ""),
        ("SHELF_ENV", ""),
    ]));

    assert_eq!(
        environment.value(&name("SHELF_DEBUG")),
        Some(&Value::Boolean(false))
    );
    assert_eq!(environment.mode(), Mode::Local);
}

#[test]
fn an_empty_value_in_the_environment_leaves_an_optional_variable_unset() {
    let environment = ready(verdict(&[
        ("SHELF_PORT", "8080"),
        ("XDG_STATE_HOME", ""),
        ("SHELF_MIRROR_TOKEN", ""),
    ]));

    assert_eq!(environment.value(&name("XDG_STATE_HOME")), None);
    assert!(environment.secret(&name("SHELF_MIRROR_TOKEN")).is_none());
}

#[test]
fn an_empty_value_in_the_environment_leaves_a_required_variable_missing() {
    assert_eq!(
        findings(verdict(&[("SHELF_PORT", "")])),
        [finding(
            "SHELF_PORT",
            Problem::Missing { mode: Mode::Local }
        )]
    );
}

#[test]
fn an_empty_value_leaves_a_required_string_missing() {
    let spec = Spec::parse(
        r#"
[variables.SHELF_WORKERS]
purpose = "How many catalogue workers shelf starts."
type = "string"
unset = "required"
show = "shown"
source = "setting"
"#,
    )
    .unwrap();
    let bindings = capture(&spec, vars(&[("SHELF_WORKERS", "")]));

    assert_eq!(
        findings(check(&spec, &bindings, Mode::Local)),
        [finding(
            "SHELF_WORKERS",
            Problem::Missing { mode: Mode::Local }
        )]
    );
}

#[test]
fn an_empty_bound_value_counts_as_unset() {
    let spec = spec();
    let mut bindings = Bindings::new();
    bindings.bind(name("SHELF_PORT"), Bound::Plain(OsString::new()));
    bindings.bind(name("SHELF_ENV"), Bound::Plain(OsString::new()));
    bindings.bind(
        name("SHELF_MIRROR_TOKEN"),
        Bound::Secret(Secret::new(String::new())),
    );

    assert_eq!(mode(&spec, &bindings), Mode::Local);
    assert_eq!(
        findings(check(&spec, &bindings, Mode::Local)),
        [finding(
            "SHELF_PORT",
            Problem::Missing { mode: Mode::Local }
        )]
    );
}

#[test]
fn a_withheld_value_that_is_not_unicode_is_invalid() {
    let spec = spec();
    let mut bindings = capture(&spec, vars(&[("SHELF_PORT", "8080")]));
    bindings.bind(name("SHELF_MIRROR_TOKEN"), Bound::NotUnicode);

    assert_eq!(
        findings(check(&spec, &bindings, Mode::Local)),
        [finding(
            "SHELF_MIRROR_TOKEN",
            Problem::Invalid(TypeError::NotUnicode)
        )]
    );
}

#[test]
fn a_plain_value_bound_to_a_withheld_variable_is_a_show_mismatch() {
    let spec = spec();
    let mut bindings = capture(&spec, vars(&[("SHELF_PORT", "8080")]));
    bindings.bind(name("SHELF_MIRROR_TOKEN"), Bound::Plain("hunter2".into()));

    assert_eq!(
        findings(check(&spec, &bindings, Mode::Local)),
        [finding(
            "SHELF_MIRROR_TOKEN",
            Problem::ShowMismatch {
                show: Show::Withheld
            }
        )]
    );
}

#[test]
fn a_secret_bound_to_a_shown_variable_is_a_show_mismatch() {
    let spec = spec();
    let mut bindings = Bindings::new();
    bindings.bind(
        name("SHELF_PORT"),
        Bound::Secret(Secret::new("8080".to_owned())),
    );

    assert_eq!(
        findings(check(&spec, &bindings, Mode::Local)),
        [finding(
            "SHELF_PORT",
            Problem::ShowMismatch { show: Show::Shown }
        )]
    );
}

#[test]
fn a_check_in_another_mode_than_the_bindings_decide_is_refused_on_the_mode_variable() {
    let spec = spec();
    let bindings = capture(
        &spec,
        vars(&[
            ("SHELF_PORT", "8080"),
            ("SHELF_ENV", "prod"),
            ("SHELF_MIRROR_TOKEN", "t"),
        ]),
    );

    assert_eq!(
        findings(check(&spec, &bindings, Mode::Local)),
        [finding(
            "SHELF_ENV",
            Problem::ModeMismatch {
                decided: Mode::Production
            }
        )]
    );
}

#[test]
fn a_spec_without_a_mode_table_is_checked_in_local_mode() {
    let spec = Spec::parse(
        r#"
[variables.SHELF_TOKEN]
purpose = "A token that production requires."
type = "string"
unset = { local = "optional", production = "required" }
show = "withheld"
source = "secret"
"#,
    )
    .unwrap();
    let environment = ready(check(&spec, &Bindings::new(), Mode::Production));

    assert_eq!(environment.mode(), Mode::Local);
}

#[test]
fn a_refusal_lists_every_finding_in_the_spec_order() {
    assert_eq!(
        findings(verdict(&[("SHELF_ENV", "prod"), ("SHELF_DEBUG", "maybe")])),
        [
            finding(
                "SHELF_PORT",
                Problem::Missing {
                    mode: Mode::Production
                }
            ),
            Finding {
                value: Some(Quote::new(&OsString::from("maybe"))),
                ..finding("SHELF_DEBUG", Problem::Invalid(TypeError::NotBoolean))
            },
            finding(
                "SHELF_MIRROR_TOKEN",
                Problem::Missing {
                    mode: Mode::Production
                }
            ),
        ]
    );
}

// The property that binding a missing variable removes only its finding

/// How a generated variable is bound.
#[derive(Debug, Clone, Copy)]
enum Binding {
    /// Nothing binds it.
    Unbound,
    /// It is bound to a value its type accepts.
    Valid,
    /// It is bound to a value its type refuses.
    Invalid,
}

/// One generated variable: the TOML of its type, its unset rule, a valid
/// value, an invalid value, and how it is bound.
#[derive(Debug, Clone)]
struct Generated {
    ty: &'static str,
    unset: &'static str,
    valid: &'static str,
    invalid: &'static str,
    binding: Binding,
}

/// A variable of a port, boolean or integer type with any unset rule, and
/// any binding.
fn generated() -> impl Strategy<Value = Generated> {
    let ty = prop::sample::select(vec![
        ("\"port\"", "8080", "0"),
        ("\"boolean\"", "on", "maybe"),
        ("{ integer = { min = 1, max = 9 } }", "5", "10"),
    ]);
    let unset = prop::sample::select(vec![
        "\"required\"",
        "\"optional\"",
        "{ local = \"optional\", production = \"required\" }",
        "{ local = \"required\", production = \"optional\" }",
    ]);
    let binding = prop::sample::select(vec![Binding::Unbound, Binding::Valid, Binding::Invalid]);
    (ty, unset, binding).prop_map(|((ty, valid, invalid), unset, binding)| Generated {
        ty,
        unset,
        valid,
        invalid,
        binding,
    })
}

/// Returns the spec of `variables`, named `V0`, `V1` and so on, with an
/// optional mode variable `MODE` whose value `prod` selects production.
fn generated_spec(variables: &[Generated]) -> Spec {
    let mut text = String::from(
        "[mode]\nvariable = \"MODE\"\nproduction = [\"prod\"]\n\n[variables.MODE]\n\
         purpose = \"Selects the mode.\"\ntype = { enumeration = [\"dev\", \"prod\"] }\n\
         unset = \"optional\"\nshow = \"shown\"\nsource = \"setting\"\n",
    );
    for (index, variable) in variables.iter().enumerate() {
        text.push_str(&format!(
            "[variables.V{index}]\npurpose = \"A variable.\"\ntype = {}\nunset = {}\n\
             show = \"shown\"\nsource = \"setting\"\n",
            variable.ty, variable.unset
        ));
    }
    Spec::parse(&text).unwrap()
}

/// Returns the bindings of `variables`.
fn generated_bindings(variables: &[Generated]) -> Bindings {
    let mut bindings = Bindings::new();
    for (index, variable) in variables.iter().enumerate() {
        let value = match variable.binding {
            Binding::Unbound => continue,
            Binding::Valid => variable.valid,
            Binding::Invalid => variable.invalid,
        };
        bindings.bind(name(&format!("V{index}")), Bound::Plain(value.into()));
    }
    bindings
}

proptest! {
    #[test]
    fn binding_a_valid_value_to_a_missing_variable_removes_exactly_its_finding(
        mut variables in prop::collection::vec(generated(), 0..8),
        missing in generated(),
        production in any::<bool>(),
        pick in any::<prop::sample::Index>(),
    ) {
        // One variable that is required and unbound gives every case a
        // missing variable, so no case is rejected.
        variables.push(Generated {
            unset: "\"required\"",
            binding: Binding::Unbound,
            ..missing
        });
        let mode = if production { Mode::Production } else { Mode::Local };
        let spec = generated_spec(&variables);
        let mut bindings = generated_bindings(&variables);
        if production {
            bindings.bind(name("MODE"), Bound::Plain("prod".into()));
        }
        let before = findings(check(&spec, &bindings, mode));
        let missing: Vec<&Finding> = before
            .iter()
            .filter(|finding| matches!(finding.problem, Problem::Missing { .. }))
            .collect();
        let fixed = missing[pick.index(missing.len())].name.clone();
        let index: usize = fixed.as_str()[1..].parse().unwrap();

        bindings.bind(fixed.clone(), Bound::Plain(variables[index].valid.into()));
        let after = findings(check(&spec, &bindings, mode));

        let expected: Vec<Finding> =
            before.into_iter().filter(|finding| finding.name != fixed).collect();
        prop_assert_eq!(after, expected);
    }
}

// Refusals as text

#[test]
fn refusals_name_each_variable_and_the_mode_a_missing_one_is_required_in() {
    let Verdict::Refused(refusals) = verdict(&[("SHELF_DEBUG", "maybe")]) else {
        panic!("the port is missing and the debug flag is invalid");
    };

    assert_eq!(
        refusals.to_string(),
        "SHELF_PORT is required in local mode and has no value; \
         SHELF_DEBUG: the value is not one of the words 1, 0, true, false, yes, no, on and off \
         (the value is \"maybe\")"
    );
}

#[test]
fn without_a_mode_table_a_missing_variable_is_required_without_a_mode() {
    let spec = Spec::parse(
        r#"
[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = "required"
show = "shown"
source = "setting"
"#,
    )
    .unwrap();
    let bindings = capture(&spec, vars(&[]));
    let Verdict::Refused(refusals) = check(&spec, &bindings, Mode::Local) else {
        panic!("the port is missing");
    };

    assert_eq!(
        refusals.to_string(),
        "SHELF_PORT is required and has no value"
    );
}

// patterns

/// A spec with a shown revision and a withheld key, each of whose string
/// types has a pattern.
const PATTERNS: &str = r#"
[variables.SHELF_REVISION]
purpose = "Which commit of the catalogue shelf serves."
type = { string = { pattern = "[0-9a-f]{40}" } }
unset = "required"
show = "shown"
source = "setting"

[variables.SHELF_MIRROR_KEY]
purpose = "The key shelf calls the catalogue mirror with."
type = { string = { pattern = "sk-[a-z0-9]+" } }
unset = "required"
show = "withheld"
source = "secret"
"#;

/// Captures `pairs` with [`PATTERNS`] and checks them in local mode.
fn pattern_verdict(pairs: &[(&str, &str)]) -> Verdict {
    let spec = Spec::parse(PATTERNS).unwrap();
    let bindings = capture(&spec, vars(pairs));
    check(&spec, &bindings, Mode::Local)
}

#[test]
fn values_that_match_their_patterns_pass_the_check() {
    let revision = "0123456789abcdef0123456789abcdef01234567";

    let environment = ready(pattern_verdict(&[
        ("SHELF_REVISION", revision),
        ("SHELF_MIRROR_KEY", "sk-abc123"),
    ]));

    assert_eq!(
        environment.value(&name("SHELF_REVISION")),
        Some(&Value::String(revision.into()))
    );
}

#[test]
fn a_shown_value_that_does_not_match_names_the_variable_and_the_pattern() {
    let Verdict::Refused(refusals) = pattern_verdict(&[
        ("SHELF_REVISION", "main"),
        ("SHELF_MIRROR_KEY", "sk-abc123"),
    ]) else {
        panic!("the check passed");
    };

    assert_eq!(
        refusals.to_string(),
        "SHELF_REVISION: the value does not match the pattern [0-9a-f]{40} (the value is \"main\")"
    );
}

#[test]
fn a_withheld_value_that_does_not_match_is_refused_without_its_text() {
    let revision = "0123456789abcdef0123456789abcdef01234567";

    let Verdict::Refused(refusals) = pattern_verdict(&[
        ("SHELF_REVISION", revision),
        ("SHELF_MIRROR_KEY", "pk-live-hidden"),
    ]) else {
        panic!("the check passed");
    };

    let text = refusals.to_string();
    assert_eq!(
        text,
        "SHELF_MIRROR_KEY: the value does not match the pattern sk-[a-z0-9]+"
    );
    assert!(!format!("{refusals:?}").contains("pk-live-hidden"));
}

// Quotes of refused values

/// Returns the refusals of a verdict that is refused.
fn refused(verdict: Verdict) -> super::Refusals {
    match verdict {
        Verdict::Refused(refusals) => refusals,
        Verdict::Ready(_) => panic!("the check passed"),
    }
}

/// Returns the quoted text of the first finding of `refusals`.
fn first_quote(refusals: &super::Refusals) -> Option<String> {
    let finding = refusals.iter().next()?;
    finding
        .value
        .as_ref()
        .map(|quote| quote.as_str().to_owned())
}

/// A spec with two shown variables: a required port and an optional
/// integer from 1 to 9.
const QUOTES: &str = r#"
[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = "required"
show = "shown"
source = "setting"

[variables.SHELF_WORKERS]
purpose = "How many catalogue workers shelf starts."
type = { integer = { min = 1, max = 9 } }
unset = "optional"
show = "shown"
source = "setting"
"#;

/// Checks `bindings` against [`QUOTES`] in local mode.
fn quote_verdict(bindings: &Bindings) -> Verdict {
    check(&Spec::parse(QUOTES).unwrap(), bindings, Mode::Local)
}

/// Returns bindings that bind each of `pairs` as a plain value.
fn plain(pairs: &[(&str, OsString)]) -> Bindings {
    let mut bindings = Bindings::new();
    for (key, value) in pairs {
        bindings.bind(name(key), Bound::Plain(value.clone()));
    }
    bindings
}

#[test]
fn a_shown_variables_invalid_value_is_quoted_in_its_finding_and_the_refusals_text() {
    let refusals = refused(verdict(&[("SHELF_PORT", "80a")]));

    let port = refusals
        .iter()
        .find(|finding| finding.name == name("SHELF_PORT"))
        .unwrap();
    assert_eq!(port.value.as_ref().map(Quote::as_str), Some("80a"));
    assert_eq!(
        refusals.to_string(),
        "SHELF_PORT: the value is not a port from 1 to 65535 (the value is \"80a\")"
    );
}

#[test]
fn a_withheld_value_that_does_not_match_its_pattern_is_not_quoted_anywhere() {
    let refusals = refused(pattern_verdict(&[
        ("SHELF_REVISION", "0123456789abcdef0123456789abcdef01234567"),
        ("SHELF_MIRROR_KEY", "pk-live-hidden"),
    ]));

    let finding = refusals.iter().next().unwrap();
    assert_eq!(finding.value, None);
    for text in [
        refusals.to_string(),
        format!("{refusals:?}"),
        format!("{refusals:#?}"),
        format!("{finding:?}"),
    ] {
        assert!(!text.contains("pk-live-hidden"), "{text}");
        assert!(!text.contains("the value is \""), "{text}");
    }
}

#[cfg(unix)]
#[test]
fn a_withheld_value_that_is_not_unicode_is_not_quoted() {
    use std::os::unix::ffi::OsStringExt as _;

    let spec = spec();
    let bindings = capture(
        &spec,
        [
            (OsString::from("SHELF_PORT"), OsString::from("8080")),
            (
                OsString::from("SHELF_MIRROR_TOKEN"),
                OsString::from_vec(vec![b'k', 0xff, 0xfe]),
            ),
        ],
    );
    let refusals = refused(check(&spec, &bindings, Mode::Local));

    assert!(refusals.iter().all(|finding| finding.value.is_none()));
    assert_eq!(
        refusals.to_string(),
        "SHELF_MIRROR_TOKEN: the value is not unicode text"
    );
}

#[test]
fn a_missing_variable_and_a_mode_mismatch_have_no_quote() {
    let refusals = refused(verdict(&[("SHELF_ENV", "prod")]));
    let spec = spec();
    let mut bindings = capture(&spec, vars(&[("SHELF_PORT", "8080")]));
    bindings.bind(name("SHELF_ENV"), Bound::Plain("prod".into()));
    let mismatch = refused(check(&spec, &bindings, Mode::Local));

    assert!(
        refusals
            .iter()
            .chain(mismatch.iter())
            .all(|finding| finding.value.is_none())
    );
    assert!(!refusals.to_string().contains("(the value is"));
    assert!(!mismatch.to_string().contains("(the value is"));
}

#[test]
fn a_secret_bound_to_a_shown_variable_is_not_quoted() {
    let bindings = {
        let mut bindings = Bindings::new();
        bindings.bind(
            name("SHELF_PORT"),
            Bound::Secret(Secret::new("hidden-80a".into())),
        );
        bindings
    };
    let refusals = refused(quote_verdict(&bindings));

    assert_eq!(refusals.iter().next().unwrap().value, None);
    assert!(!refusals.to_string().contains("hidden-80a"));
    assert!(!format!("{refusals:?}").contains("hidden-80a"));
}

#[test]
fn a_quote_longer_than_64_characters_is_cut_and_ends_with_an_ellipsis() {
    let long = "9".repeat(65);
    let refusals = refused(quote_verdict(&plain(&[(
        "SHELF_PORT",
        long.clone().into(),
    )])));

    let cut = format!("{}…", "9".repeat(64));
    assert_eq!(first_quote(&refusals), Some(cut.clone()));
    assert!(
        refusals
            .to_string()
            .ends_with(&format!("(the value is \"{cut}\")"))
    );
}

#[test]
fn a_quote_of_exactly_64_characters_is_whole() {
    let whole = "9".repeat(64);
    let refusals = refused(quote_verdict(&plain(&[(
        "SHELF_PORT",
        whole.clone().into(),
    )])));

    assert_eq!(first_quote(&refusals), Some(whole));
}

#[test]
fn a_cut_quote_never_splits_a_character() {
    let long = "é".repeat(40) + &"🦀".repeat(40);
    let refusals = refused(quote_verdict(&plain(&[("SHELF_PORT", long.into())])));

    let cut = format!("{}{}…", "é".repeat(40), "🦀".repeat(24));
    assert_eq!(first_quote(&refusals), Some(cut));
}

#[test]
fn quotes_backslashes_and_control_characters_are_escaped_as_in_a_json_string() {
    let raw = "a\"b\\c\nSHELF_PORT is fine\td\r\u{1b}[31m\u{7}";
    let refusals = refused(quote_verdict(&plain(&[("SHELF_PORT", raw.into())])));

    assert_eq!(first_quote(&refusals).as_deref(), Some(raw));
    assert_eq!(
        refusals.to_string(),
        "SHELF_PORT: the value is not a port from 1 to 65535 \
         (the value is \"a\\\"b\\\\c\\nSHELF_PORT is fine\\td\\r\\u001b[31m\\u0007\")"
    );
    assert!(!refusals.to_string().contains('\n'));
}

#[cfg(unix)]
#[test]
fn a_shown_value_that_is_not_unicode_is_quoted_with_a_replacement_character() {
    use std::os::unix::ffi::OsStringExt as _;

    let raw = OsString::from_vec(vec![b'8', 0xff, b'0']);
    let refusals = refused(quote_verdict(&plain(&[("SHELF_PORT", raw)])));

    assert_eq!(
        refusals.to_string(),
        "SHELF_PORT: the value is not unicode text (the value is \"8\u{fffd}0\")"
    );
}

#[test]
fn each_invalid_shown_value_is_quoted_after_its_own_reason() {
    let refusals = refused(quote_verdict(&plain(&[
        ("SHELF_PORT", "0".into()),
        ("SHELF_WORKERS", "10".into()),
    ])));

    assert_eq!(
        refusals.to_string(),
        "SHELF_PORT: the value is not a port from 1 to 65535 (the value is \"0\"); \
         SHELF_WORKERS: the value is not a whole number from 1 to 9 (the value is \"10\")"
    );
}

proptest! {
    #[test]
    fn a_quote_is_the_json_string_of_the_cut_value(text in "\\PC*|[\\x00-\\x1f\"\\\\a]{0,80}") {
        let quoted = Quote::new(&OsString::from(&text));
        let cut: String = if text.chars().count() > 64 {
            text.chars().take(64).chain(['…']).collect()
        } else {
            text.clone()
        };

        prop_assert_eq!(quoted.as_str(), cut.as_str());
        prop_assert_eq!(quoted.to_string(), serde_json::to_string(&cut).unwrap());
    }
}
