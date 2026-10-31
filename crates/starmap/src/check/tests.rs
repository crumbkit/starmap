//! Tests for capturing an environment, deciding the mode and checking
//! bindings against a spec.

use std::ffi::OsString;

use proptest::prelude::*;

use super::{Finding, Problem, Verdict, capture, check, mode};
use crate::{Bindings, Bound, Environment, FormError, Mode, Name, Secret, Show, Spec, Value};

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
        Problem::Invalid(FormError::NotChoice(_))
    ));
}

#[test]
fn a_mode_value_the_form_counts_as_unset_falls_back_to_the_default() {
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
fn every_variable_that_fits_its_form_makes_the_check_ready() {
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
fn a_value_that_does_not_fit_its_form_is_invalid() {
    assert_eq!(
        findings(verdict(&[("SHELF_PORT", "0")])),
        [finding("SHELF_PORT", Problem::Invalid(FormError::NotPort))]
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
fn a_withheld_value_that_is_not_unicode_is_invalid() {
    let spec = spec();
    let mut bindings = capture(&spec, vars(&[("SHELF_PORT", "8080")]));
    bindings.bind(name("SHELF_MIRROR_TOKEN"), Bound::NotUnicode);

    assert_eq!(
        findings(check(&spec, &bindings, Mode::Local)),
        [finding(
            "SHELF_MIRROR_TOKEN",
            Problem::Invalid(FormError::NotUnicode)
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
fn a_withheld_variable_with_a_default_holds_it_as_a_secret() {
    let spec = Spec::parse(
        r#"
[variables.SHELF_SALT]
purpose = "Salts the cache keys."
type = "string"
unset = { default = "pepper" }
show = "withheld"
source = "setting"
"#,
    )
    .unwrap();
    let environment = ready(check(&spec, &Bindings::new(), Mode::Local));

    assert_eq!(environment.value(&name("SHELF_SALT")), None);
    assert!(environment.secret(&name("SHELF_SALT")).is_some());
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
            finding("SHELF_DEBUG", Problem::Invalid(FormError::NotBoolean)),
            finding(
                "SHELF_MIRROR_TOKEN",
                Problem::Missing {
                    mode: Mode::Production
                }
            ),
        ]
    );
}

// The property of the build plan

/// How a generated variable is bound.
#[derive(Debug, Clone, Copy)]
enum Binding {
    /// Nothing binds it.
    Unbound,
    /// It is bound to a value its form accepts.
    Valid,
    /// It is bound to a value its form refuses.
    Invalid,
}

/// One generated variable: the TOML of its type, its unset rule, a valid
/// value, an invalid value, and how it is bound.
#[derive(Debug, Clone)]
struct Generated {
    form: &'static str,
    unset: &'static str,
    valid: &'static str,
    invalid: &'static str,
    binding: Binding,
}

/// A variable of a port, boolean or integer form with any unset rule, and
/// any binding.
fn generated() -> impl Strategy<Value = Generated> {
    let form = prop::sample::select(vec![
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
    (form, unset, binding).prop_map(|((form, valid, invalid), unset, binding)| Generated {
        form,
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
            variable.form, variable.unset
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
