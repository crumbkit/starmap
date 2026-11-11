//! Tests for preparing the environment of `starmap run`, with a fake
//! resolver and a fake person, so no test needs 1Password, a Keychain or a
//! terminal.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Instant;

use crumbkit::{Code, Failure, Field, FieldValue, Recovery};
use starmap::{Name, Secret};
use starmap_resolve::{Lookup, ResolveProblem, Resolver, SecretError};

use super::{Ask, Ready, Refused, RunFailure, gather, run_with, settle};
use crate::cli::{Assignment, RunArgs, SpecFile, Verb};
use crate::codes;
use crate::dotenv::DotEnvFailure;
use crate::{Cli, TOOL};

/// A spec with a setting that has a default, a bound secret, a secret
/// without a bind, a provided variable and an inherited one.
const SPEC: &str = r#"
[variables.SHELF_PAGE_SIZE]
purpose = "How many books one page of a listing holds."
type = { integer = { min = 1, max = 100 } }
unset = { default = "20" }
show = "shown"
source = "setting"

[variables.SHELF_MIRROR_TOKEN]
purpose = "The token shelf sends to the catalogue mirror."
type = "string"
unset = "optional"
show = "withheld"
source = "secret"
bind = { one-password = "op://Personal/shelf mirror/token" }

[variables.SHELF_UPSTREAM_TOKEN]
purpose = "The token shelf sends to the upstream catalogue."
type = "string"
unset = "required"
show = "withheld"
source = "secret"

[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = "optional"
show = "shown"
source = "provided"
bind = { provided = "multiverse/port", process = "api" }

[variables.XDG_STATE_HOME]
purpose = "The directory that holds shelf's state."
type = "path"
unset = "optional"
show = "shown"
source = "inherited"
"#;

/// A checkout in a temporary directory, named `shelf`, that holds the spec
/// and, optionally, a `.env` file.
struct Checkout {
    _root: tempfile::TempDir,
    dir: PathBuf,
}

impl Checkout {
    fn new(dotenv: Option<&str>) -> Self {
        Self::with_spec(SPEC, dotenv)
    }

    fn with_spec(spec: &str, dotenv: Option<&str>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("shelf");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("starmap.toml"), spec).unwrap();
        if let Some(text) = dotenv {
            std::fs::write(dir.join(".env"), text).unwrap();
        }
        Self { _root: root, dir }
    }

    fn args(&self, set: &[(&str, &str)]) -> RunArgs {
        RunArgs {
            file: SpecFile {
                spec: self.dir.join("starmap.toml"),
            },
            workspace: None,
            set: set
                .iter()
                .map(|(name, value)| Assignment {
                    name: name.parse().unwrap(),
                    value: (*value).to_owned(),
                })
                .collect(),
            program: vec![OsString::from("true")],
        }
    }
}

/// Returns the shell values of `pairs`, with the upstream token set so the
/// required secret has a value unless a test removes it.
fn shell(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect()
}

fn name(text: &str) -> Name {
    text.parse().unwrap()
}

/// A resolver that reads every 1Password reference as `from-op`, and keeps
/// saved values in memory.
#[derive(Default)]
struct FakeResolver {
    saved: RefCell<BTreeMap<String, String>>,
}

impl FakeResolver {
    /// Returns a resolver that already holds `value` for `key`, a
    /// `<workspace>/<app>/<NAME>` key.
    fn holding(key: &str, value: &str) -> Self {
        let resolver = Self::default();
        resolver
            .saved
            .borrow_mut()
            .insert(key.to_owned(), value.to_owned());
        resolver
    }

    fn keys(&self) -> Vec<String> {
        self.saved.borrow().keys().cloned().collect()
    }
}

fn key(lookup: Lookup<'_>) -> Option<String> {
    match lookup {
        Lookup::Saved { slot } => Some(format!(
            "{}/{}/{}",
            slot.workspace.as_str(),
            slot.app.as_str(),
            slot.name
        )),
        _ => None,
    }
}

impl Resolver for FakeResolver {
    fn resolve(&self, lookup: Lookup<'_>, _deadline: Instant) -> Result<Secret, SecretError> {
        if let Lookup::Reference { .. } = lookup {
            return Ok(Secret::new("from-op".to_owned()));
        }
        let key = key(lookup).ok_or(SecretError::Unsupported)?;
        self.saved
            .borrow()
            .get(&key)
            .map(|value| Secret::new(value.clone()))
            .ok_or(SecretError::NotFound)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the fake keeps the text of a saved secret so a later lookup returns it"
    )]
    fn save(
        &self,
        lookup: Lookup<'_>,
        value: &Secret,
        _deadline: Instant,
    ) -> Result<(), SecretError> {
        let key = key(lookup).ok_or(SecretError::Unsupported)?;
        self.saved
            .borrow_mut()
            .insert(key, value.expose().to_owned());
        Ok(())
    }
}

/// A person who answers from a script, and records each question.
struct Person {
    present: bool,
    typed: Option<&'static str>,
    saves: bool,
    asked: Vec<String>,
}

impl Person {
    fn absent() -> Self {
        Self {
            present: false,
            typed: None,
            saves: false,
            asked: Vec::new(),
        }
    }

    fn typing(typed: &'static str, saves: bool) -> Self {
        Self {
            present: true,
            typed: Some(typed),
            saves,
            asked: Vec::new(),
        }
    }
}

impl Ask for Person {
    fn can_ask(&self) -> bool {
        self.present
    }

    fn secret(&mut self, name: &Name) -> Option<Secret> {
        self.asked.push(format!("secret {name}"));
        self.typed
            .filter(|typed| !typed.is_empty())
            .map(|typed| Secret::new(typed.to_owned()))
    }

    fn save(&mut self, question: &str) -> bool {
        self.asked.push(question.to_owned());
        self.saves
    }
}

/// Prepares the run of `args` with `vars`, `resolver` and `person`.
fn prepared(
    args: &RunArgs,
    vars: &[(&str, &str)],
    resolver: &FakeResolver,
    person: &mut Person,
) -> Result<Ready, Refused> {
    gather(args, shell(vars)).and_then(|gathered| settle(gathered, &[resolver], person))
}

/// Returns the shown value of `variable`, as text.
fn value(ready: &Ready, variable: &str) -> Option<String> {
    ready
        .environment
        .value(&name(variable))
        .map(crate::explain::value_text)
}

/// Returns the withheld value of `variable`.
#[expect(
    clippy::disallowed_methods,
    reason = "the tests check the text of a resolved secret"
)]
fn secret(ready: &Ready, variable: &str) -> Option<String> {
    ready
        .environment
        .secret(&name(variable))
        .map(|secret| secret.expose().to_owned())
}

/// Returns the code of each failure.
fn codes(refused: &Refused) -> Vec<&'static Code> {
    refused
        .failures
        .iter()
        .map(crumbkit::Failure::code)
        .collect()
}

const TOKEN: (&str, &str) = ("SHELF_UPSTREAM_TOKEN", "token-from-shell");

#[test]
fn set_beats_the_shell_which_beats_dotenv_which_beats_the_default() {
    let checkout = Checkout::new(Some("SHELF_PAGE_SIZE=30\nXDG_STATE_HOME=/from/dotenv\n"));
    let resolver = FakeResolver::default();
    let mut person = Person::absent();

    let default = Checkout::new(None);
    let ready = prepared(&default.args(&[]), &[TOKEN], &resolver, &mut person).unwrap();
    assert_eq!(value(&ready, "SHELF_PAGE_SIZE").as_deref(), Some("20"));

    let ready = prepared(&checkout.args(&[]), &[TOKEN], &resolver, &mut person).unwrap();
    assert_eq!(value(&ready, "SHELF_PAGE_SIZE").as_deref(), Some("30"));
    assert_eq!(
        value(&ready, "XDG_STATE_HOME").as_deref(),
        Some("/from/dotenv")
    );

    let vars = [
        TOKEN,
        ("SHELF_PAGE_SIZE", "40"),
        ("XDG_STATE_HOME", "/from/shell"),
    ];
    let ready = prepared(&checkout.args(&[]), &vars, &resolver, &mut person).unwrap();
    assert_eq!(value(&ready, "SHELF_PAGE_SIZE").as_deref(), Some("40"));
    assert_eq!(
        value(&ready, "XDG_STATE_HOME").as_deref(),
        Some("/from/shell")
    );

    let set = checkout.args(&[("SHELF_PAGE_SIZE", "50")]);
    let ready = prepared(&set, &vars, &resolver, &mut person).unwrap();
    assert_eq!(value(&ready, "SHELF_PAGE_SIZE").as_deref(), Some("50"));
}

#[test]
fn an_empty_value_in_dotenv_is_unset_for_starmap_run() {
    let checkout = Checkout::new(Some("SHELF_PAGE_SIZE=\nXDG_STATE_HOME=\n"));
    let resolver = FakeResolver::default();
    let mut person = Person::absent();

    let ready = prepared(&checkout.args(&[]), &[TOKEN], &resolver, &mut person).unwrap();
    assert_eq!(value(&ready, "SHELF_PAGE_SIZE").as_deref(), Some("20"));
    assert_eq!(value(&ready, "XDG_STATE_HOME"), None);
}

#[test]
fn an_empty_value_in_dotenv_does_not_hide_a_value_from_the_shell() {
    let checkout = Checkout::new(Some("SHELF_PAGE_SIZE=\n"));
    let resolver = FakeResolver::default();
    let mut person = Person::absent();

    let vars = [TOKEN, ("SHELF_PAGE_SIZE", "40")];
    let ready = prepared(&checkout.args(&[]), &vars, &resolver, &mut person).unwrap();
    assert_eq!(value(&ready, "SHELF_PAGE_SIZE").as_deref(), Some("40"));
}

#[test]
fn an_empty_value_in_the_shell_does_not_hide_a_value_from_dotenv() {
    let checkout = Checkout::new(Some("SHELF_PAGE_SIZE=30\n"));
    let resolver = FakeResolver::default();
    let mut person = Person::absent();

    let vars = [TOKEN, ("SHELF_PAGE_SIZE", "")];
    let ready = prepared(&checkout.args(&[]), &vars, &resolver, &mut person).unwrap();
    assert_eq!(value(&ready, "SHELF_PAGE_SIZE").as_deref(), Some("30"));
}

#[test]
fn an_empty_secret_in_the_shell_is_refused_as_unbound_when_no_person_can_answer() {
    let checkout = Checkout::new(None);
    let refused = prepared(
        &checkout.args(&[]),
        &[("SHELF_UPSTREAM_TOKEN", "")],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();
    assert_eq!(codes(&refused), [&codes::SECRET_UNBOUND]);
}

#[test]
fn a_provided_variable_takes_the_shell_or_dotenv_value_without_a_provider() {
    let checkout = Checkout::new(Some("SHELF_PORT=8081\n"));
    let resolver = FakeResolver::default();
    let mut person = Person::absent();

    let ready = prepared(&checkout.args(&[]), &[TOKEN], &resolver, &mut person).unwrap();
    assert_eq!(value(&ready, "SHELF_PORT").as_deref(), Some("8081"));

    let vars = [TOKEN, ("SHELF_PORT", "8080")];
    let ready = prepared(&checkout.args(&[]), &vars, &resolver, &mut person).unwrap();
    assert_eq!(value(&ready, "SHELF_PORT").as_deref(), Some("8080"));
}

#[test]
fn a_bound_secret_in_the_shell_is_ignored_for_its_bind_with_a_note() {
    let checkout = Checkout::new(None);
    let resolver = FakeResolver::default();
    let vars = [TOKEN, ("SHELF_MIRROR_TOKEN", "token-from-shell")];
    let ready = prepared(&checkout.args(&[]), &vars, &resolver, &mut Person::absent()).unwrap();

    assert_eq!(
        secret(&ready, "SHELF_MIRROR_TOKEN").as_deref(),
        Some("from-op")
    );
    assert!(
        ready.notes.contains(
            &"SHELF_MIRROR_TOKEN is set in this shell, and starmap ignored it and used its bind."
                .to_owned()
        )
    );
}

#[test]
fn a_secret_without_a_bind_takes_the_shell_value() {
    let checkout = Checkout::new(None);
    let resolver = FakeResolver::default();
    let mut person = Person::typing("never-asked", true);
    let ready = prepared(&checkout.args(&[]), &[TOKEN], &resolver, &mut person).unwrap();

    assert_eq!(
        secret(&ready, "SHELF_UPSTREAM_TOKEN").as_deref(),
        Some("token-from-shell")
    );
    assert!(person.asked.is_empty());
}

#[test]
fn set_is_refused_for_a_secret_and_admitted_for_a_setting() {
    let checkout = Checkout::new(None);
    let resolver = FakeResolver::default();
    let mut person = Person::absent();

    let secret_set = checkout.args(&[("SHELF_UPSTREAM_TOKEN", "typed-on-the-command-line")]);
    let refused = prepared(&secret_set, &[TOKEN], &resolver, &mut person).unwrap_err();
    assert_eq!(codes(&refused), [&codes::VARIABLE_SECRET]);

    let setting_set = checkout.args(&[("SHELF_PAGE_SIZE", "7")]);
    assert!(prepared(&setting_set, &[TOKEN], &resolver, &mut person).is_ok());
}

#[test]
fn set_is_refused_for_an_undeclared_variable() {
    let checkout = Checkout::new(None);
    let refused = prepared(
        &checkout.args(&[("SHELF_THEME", "dark")]),
        &[TOKEN],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();
    assert_eq!(codes(&refused), [&codes::VARIABLE_UNKNOWN]);
}

#[test]
fn a_secret_in_dotenv_is_refused_and_names_the_variable() {
    let checkout = Checkout::new(Some("SHELF_UPSTREAM_TOKEN=pasted-test-value\n"));
    let refused = prepared(
        &checkout.args(&[]),
        &[TOKEN],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();

    assert_eq!(codes(&refused), [&codes::DOTENV_SECRET]);
    let message = refused.failures[0].to_string();
    assert!(message.contains("SHELF_UPSTREAM_TOKEN"), "{message}");
    assert!(!message.contains("pasted-test-value"), "{message}");
}

#[test]
fn an_undeclared_key_in_dotenv_is_left_out_with_a_note() {
    let checkout = Checkout::new(Some("SHELF_THEME=dark\n"));
    let ready = prepared(
        &checkout.args(&[]),
        &[TOKEN],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap();
    assert!(ready.notes.iter().any(|note| note.contains("SHELF_THEME")));
}

#[test]
fn an_unbound_secret_is_refused_when_no_person_can_answer() {
    let checkout = Checkout::new(None);
    let refused = prepared(
        &checkout.args(&[]),
        &[],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();
    assert_eq!(codes(&refused), [&codes::SECRET_UNBOUND]);
}

#[test]
fn an_unbound_secret_is_refused_when_the_person_types_nothing() {
    let checkout = Checkout::new(None);
    let mut person = Person::typing("", true);
    let refused = prepared(
        &checkout.args(&[]),
        &[],
        &FakeResolver::default(),
        &mut person,
    )
    .unwrap_err();
    assert_eq!(codes(&refused), [&codes::SECRET_UNBOUND]);
    assert_eq!(person.asked, ["secret SHELF_UPSTREAM_TOKEN"]);
}

#[test]
fn a_typed_secret_is_used_for_this_run_and_not_saved_when_the_person_declines() {
    let checkout = Checkout::new(None);
    let resolver = FakeResolver::default();
    let mut person = Person::typing("typed-test-value", false);
    let ready = prepared(&checkout.args(&[]), &[], &resolver, &mut person).unwrap();

    assert_eq!(
        secret(&ready, "SHELF_UPSTREAM_TOKEN").as_deref(),
        Some("typed-test-value")
    );
    assert_eq!(
        person.asked,
        [
            "secret SHELF_UPSTREAM_TOKEN",
            "Save SHELF_UPSTREAM_TOKEN to the Keychain for workspace shelf and app shelf, so later runs find it?",
        ]
    );
    assert!(resolver.keys().is_empty());
}

#[test]
fn a_typed_secret_that_the_person_saves_is_found_by_the_next_run_without_asking() {
    let checkout = Checkout::new(None);
    let resolver = FakeResolver::default();
    let mut first = Person::typing("typed-test-value", true);
    let ready = prepared(&checkout.args(&[]), &[], &resolver, &mut first).unwrap();

    assert_eq!(resolver.keys(), ["shelf/shelf/SHELF_UPSTREAM_TOKEN"]);
    assert!(ready.notes.contains(
        &"The Keychain holds SHELF_UPSTREAM_TOKEN as service starmap.shelf and account shelf/SHELF_UPSTREAM_TOKEN, and later runs read it from there."
            .to_owned()
    ));

    let mut second = Person::absent();
    let ready = prepared(&checkout.args(&[]), &[], &resolver, &mut second).unwrap();
    assert_eq!(
        secret(&ready, "SHELF_UPSTREAM_TOKEN").as_deref(),
        Some("typed-test-value")
    );
}

#[test]
fn the_workspace_flag_names_the_saved_value() {
    let checkout = Checkout::new(None);
    let resolver =
        FakeResolver::holding("shelf-dev/shelf/SHELF_UPSTREAM_TOKEN", "saved-test-value");
    let mut args = checkout.args(&[]);

    assert!(prepared(&args, &[], &resolver, &mut Person::absent()).is_err());
    args.workspace = Some("shelf-dev".parse().unwrap());
    let ready = prepared(&args, &[], &resolver, &mut Person::absent()).unwrap();
    assert_eq!(
        secret(&ready, "SHELF_UPSTREAM_TOKEN").as_deref(),
        Some("saved-test-value")
    );
}

#[test]
fn a_value_that_does_not_fit_its_type_is_refused_as_invalid() {
    let checkout = Checkout::new(None);
    let refused = prepared(
        &checkout.args(&[("SHELF_PAGE_SIZE", "1000")]),
        &[TOKEN],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();
    assert_eq!(codes(&refused), [&codes::VARIABLE_INVALID]);
    assert!(
        refused
            .notes
            .contains(&"starmap started nothing, because of 1 problem.".to_owned())
    );
}

#[test]
fn a_shown_value_that_does_not_fit_is_quoted_in_the_failure_text_and_its_value_field() {
    let checkout = Checkout::new(None);
    let refused = prepared(
        &checkout.args(&[("SHELF_PAGE_SIZE", "1000")]),
        &[TOKEN],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();
    let failure = &refused.failures[0];

    assert_eq!(
        failure.to_string(),
        "SHELF_PAGE_SIZE: the value is not a whole number from 1 to 100 (the value is \"1000\")"
    );
    assert_eq!(
        failure.fields(),
        [
            Field::new("key", "SHELF_PAGE_SIZE"),
            Field::new("value", "1000"),
        ]
    );
}

/// A spec whose secret without a bind has a pattern.
const PATTERN_SPEC: &str = r#"
[variables.SHELF_UPSTREAM_TOKEN]
purpose = "The token shelf sends to the upstream catalogue."
type = { string = { pattern = "sk-[a-z0-9]+" } }
unset = "required"
show = "withheld"
source = "secret"
"#;

#[test]
fn a_withheld_value_that_does_not_fit_is_not_quoted_in_the_failure() {
    let checkout = Checkout::with_spec(PATTERN_SPEC, None);
    let refused = prepared(
        &checkout.args(&[]),
        &[("SHELF_UPSTREAM_TOKEN", "pk-live-hidden")],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();
    let failure = &refused.failures[0];

    assert_eq!(
        failure.to_string(),
        "SHELF_UPSTREAM_TOKEN: the value does not match the pattern sk-[a-z0-9]+"
    );
    assert_eq!(
        failure.fields(),
        [
            Field::new("key", "SHELF_UPSTREAM_TOKEN"),
            Field::new("value", FieldValue::Null),
        ]
    );
    assert!(!format!("{refused:?}").contains("pk-live-hidden"));
}

/// A spec whose secret without a bind is 4 bytes of base64.
const BYTES_SPEC: &str = r#"
[variables.SHELF_UPSTREAM_TOKEN]
purpose = "The key shelf signs upstream requests with."
type = { bytes = { encoding = "base64", length = 4 } }
unset = "required"
show = "withheld"
source = "secret"
"#;

#[test]
fn a_withheld_bytes_value_that_does_not_fit_is_not_quoted_in_the_failure() {
    let checkout = Checkout::with_spec(BYTES_SPEC, None);
    for raw in ["hunter2!", "c2VjcmV0LWtleS12YWx1ZQ=="] {
        let refused = prepared(
            &checkout.args(&[]),
            &[("SHELF_UPSTREAM_TOKEN", raw)],
            &FakeResolver::default(),
            &mut Person::absent(),
        )
        .unwrap_err();
        let failure = &refused.failures[0];

        assert_eq!(codes(&refused), [&codes::VARIABLE_INVALID]);
        assert_eq!(
            failure.fields(),
            [
                Field::new("key", "SHELF_UPSTREAM_TOKEN"),
                Field::new("value", FieldValue::Null),
            ]
        );
        for text in [failure.to_string(), format!("{refused:?}")] {
            assert!(!text.contains(raw), "{text}");
            assert!(!text.contains("(the value is"), "{text}");
        }
    }
}

/// Returns the recovery and the alternatives of each failure of `refused`.
fn repairs(refused: &Refused) -> Vec<(Recovery, Vec<Recovery>)> {
    refused
        .failures
        .iter()
        .map(|failure| (failure.recovery(), failure.alternatives()))
        .collect()
}

/// Returns a hint with `text`.
fn hint(text: &str) -> Recovery {
    Recovery::hint(text)
}

#[test]
fn a_value_that_does_not_fit_its_type_recovers_with_starmap_explain_of_the_variable() {
    let checkout = Checkout::new(None);
    let args = checkout.args(&[("SHELF_PAGE_SIZE", "1000")]);
    let refused = prepared(
        &args,
        &[TOKEN],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();
    let file = args.file.spec.display().to_string();
    assert_eq!(
        repairs(&refused),
        [(
            Recovery::Run(SpecFile::call(&["explain"], &file).value("names", "SHELF_PAGE_SIZE")),
            Vec::new()
        )]
    );
}

#[test]
fn set_of_an_undeclared_variable_offers_to_declare_it_or_to_remove_the_set() {
    let checkout = Checkout::new(None);
    let refused = prepared(
        &checkout.args(&[("SHELF_THEME", "dark")]),
        &[TOKEN],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();
    assert_eq!(
        repairs(&refused),
        [(
            hint("declare SHELF_THEME in starmap.toml"),
            vec![hint("remove --set SHELF_THEME")]
        )]
    );
}

#[test]
fn set_of_a_secret_offers_a_bind_or_a_typed_value_as_two_repairs() {
    let checkout = Checkout::new(None);
    let refused = prepared(
        &checkout.args(&[("SHELF_UPSTREAM_TOKEN", "typed-on-the-command-line")]),
        &[TOKEN],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();
    assert_eq!(
        repairs(&refused),
        [(
            hint(
                "remove --set SHELF_UPSTREAM_TOKEN, and give SHELF_UPSTREAM_TOKEN a bind in starmap.toml"
            ),
            vec![hint(
                "remove --set SHELF_UPSTREAM_TOKEN, and let starmap run ask for it at a terminal"
            )]
        )]
    );
}

#[test]
fn a_secret_in_dotenv_offers_a_bind_or_a_typed_value_as_two_repairs() {
    let checkout = Checkout::new(Some("SHELF_UPSTREAM_TOKEN=pasted-test-value\n"));
    let refused = prepared(
        &checkout.args(&[]),
        &[TOKEN],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();
    let [RunFailure::DotEnv(DotEnvFailure::Secret { file, .. })] = refused.failures.as_slice()
    else {
        panic!("{:?}", refused.failures);
    };
    assert_eq!(
        repairs(&refused),
        [(
            hint(&format!(
                "remove SHELF_UPSTREAM_TOKEN from {file}, and give it a bind in starmap.toml, such as {{ one-password = \"op://vault/item/field\" }}"
            )),
            vec![hint(&format!(
                "remove SHELF_UPSTREAM_TOKEN from {file}, and let starmap run ask for it and save it to the Keychain"
            ))]
        )]
    );
}

#[test]
fn an_unbound_secret_offers_a_typed_value_or_a_bind_as_two_repairs() {
    let checkout = Checkout::new(None);
    let refused = prepared(
        &checkout.args(&[]),
        &[],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();
    assert_eq!(
        repairs(&refused),
        [(
            hint(
                "run starmap run at a terminal without --no-input, type SHELF_UPSTREAM_TOKEN and save it to the Keychain"
            ),
            vec![hint(
                "give SHELF_UPSTREAM_TOKEN a bind in starmap.toml, such as { one-password = \"op://vault/item/field\" }"
            )]
        )]
    );
}

/// A spec with a required setting, a required provided variable and a
/// required setting whose path bind is under the home directory.
const REQUIRED: &str = r#"
[variables.SHELF_NAME]
purpose = "The name the catalogue shows."
type = "string"
unset = "required"
show = "shown"
source = "setting"

[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = "required"
show = "shown"
source = "provided"
bind = { provided = "multiverse/port", process = "api" }

[variables.SHELF_DATA]
purpose = "The directory that holds the catalogue."
type = "path"
unset = "required"
show = "shown"
source = "setting"
bind = { path = "{home}/shelf" }
"#;

#[test]
fn each_required_variable_without_a_value_is_refused_and_each_is_admitted_with_one() {
    let checkout = Checkout::with_spec(REQUIRED, None);
    let resolver = FakeResolver::default();
    let mut person = Person::absent();

    let refused = prepared(&checkout.args(&[]), &[], &resolver, &mut person).unwrap_err();
    assert_eq!(
        codes(&refused),
        [&codes::BIND_NO_PROVIDER, &codes::BIND_NO_PLACE]
    );
    assert_eq!(
        repairs(&refused),
        [
            (
                hint("set SHELF_PORT in this shell, in .env or with --set SHELF_PORT=VALUE"),
                vec![hint(
                    "start the app under a host that provides multiverse/port"
                )]
            ),
            (
                hint("set HOME in this shell"),
                vec![hint("set SHELF_DATA with --set SHELF_DATA=VALUE")]
            ),
        ]
    );

    let vars = [("SHELF_PORT", "8080"), ("HOME", "/home/test")];
    let refused = prepared(&checkout.args(&[]), &vars, &resolver, &mut person).unwrap_err();
    assert_eq!(codes(&refused), [&codes::VARIABLE_MISSING]);

    let set = checkout.args(&[("SHELF_NAME", "shelf")]);
    let ready = prepared(&set, &vars, &resolver, &mut person).unwrap();
    assert_eq!(value(&ready, "SHELF_PORT").as_deref(), Some("8080"));
    assert_eq!(
        value(&ready, "SHELF_DATA").as_deref(),
        Some("/home/test/shelf")
    );
}

#[test]
fn each_secret_store_problem_is_unreadable_and_says_what_the_store_returned() {
    let cases = [
        (
            ResolveProblem::Secret(SecretError::Locked),
            "the secret store is locked",
        ),
        (
            ResolveProblem::Deadline,
            "the secret store did not answer in time",
        ),
    ];
    for (problem, says) in cases {
        let failure = RunFailure::resolve(name("SHELF_MIRROR_TOKEN"), problem);
        assert_eq!(crumbkit::Failure::code(&failure), &codes::SECRET_UNREADABLE);
        assert_eq!(
            failure.to_string(),
            format!("SHELF_MIRROR_TOKEN could not be read, because {says}")
        );
    }
}

#[test]
fn a_generated_variable_that_no_store_handles_fails_as_unreadable_and_names_the_variable() {
    let checkout = Checkout::with_spec(BOUND_AND_GENERATED, None);
    let refused = prepared(
        &checkout.args(&[]),
        &[],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();

    assert_eq!(codes(&refused), [&codes::SECRET_UNREADABLE]);
    assert_eq!(
        refused.failures.first().map(ToString::to_string).as_deref(),
        Some("no secret store that starmap run uses on this machine handles SHELF_SESSION_KEY")
    );
}

#[test]
fn an_unbound_secret_is_not_asked_for_when_another_variable_fails() {
    let checkout = Checkout::with_spec(&format!("{SPEC}{REQUIRED_PORT}"), None);
    let mut person = Person::typing("typed-test-value", true);
    let refused = prepared(
        &checkout.args(&[]),
        &[],
        &FakeResolver::default(),
        &mut person,
    )
    .unwrap_err();
    assert_eq!(
        codes(&refused),
        [&codes::BIND_NO_PROVIDER, &codes::SECRET_UNBOUND]
    );
    assert!(person.asked.is_empty(), "{:?}", person.asked);

    let mut person = Person::typing("typed-test-value", false);
    prepared(
        &checkout.args(&[]),
        &[("SHELF_API_PORT", "8080")],
        &FakeResolver::default(),
        &mut person,
    )
    .unwrap();
    assert_eq!(
        person.asked.first().map(String::as_str),
        Some("secret SHELF_UPSTREAM_TOKEN")
    );
}

/// A required provided variable, which fails resolution without a value.
const REQUIRED_PORT: &str = r#"
[variables.SHELF_API_PORT]
purpose = "The port the api process listens on, which a host provides."
type = "port"
unset = "required"
show = "shown"
source = "provided"
bind = { provided = "multiverse/port", process = "api" }
"#;

/// A spec with a bound secret and a generated one, which a run resolves
/// through 1Password and the Keychain.
const BOUND_AND_GENERATED: &str = r#"
[variables.SHELF_MIRROR_TOKEN]
purpose = "The token shelf sends to the catalogue mirror."
type = "string"
unset = "required"
show = "withheld"
source = "secret"
bind = { one-password = "op://Personal/shelf mirror/token" }

[variables.SHELF_SESSION_KEY]
purpose = "Signs the cookies of the web view."
type = "string"
unset = "required"
show = "withheld"
source = "generated"
bytes = 48
"#;

/// A resolver that counts its calls and answers every lookup.
#[derive(Default)]
struct Counting {
    calls: Cell<usize>,
}

impl Resolver for Counting {
    fn resolve(&self, _lookup: Lookup<'_>, _deadline: Instant) -> Result<Secret, SecretError> {
        self.calls.set(self.calls.get() + 1);
        Ok(Secret::new("from-the-store".to_owned()))
    }
}

/// Parses `starmap run --json` against the spec in `checkout`, and runs it
/// with `resolver`, without input. The program is `true`, which `--json`
/// never starts.
fn run_json(checkout: &Checkout, resolver: &Counting) {
    let spec = checkout.dir.join("starmap.toml");
    let words = [
        "starmap",
        "run",
        "--no-input",
        "--json",
        "--spec",
        spec.to_str().unwrap(),
        "--",
        "true",
    ];
    let Ok((cli, output)) = crumbkit::parse_from::<Cli>(&TOOL, words) else {
        panic!("the command line does not parse");
    };
    let Verb::Run(args) = cli.verb else {
        panic!("the command line is not run");
    };
    let mut notes = Vec::new();
    let _ = run_with(&args, Vec::new(), output, &mut notes, &[resolver]);
}

#[test]
fn json_is_refused_before_any_secret_store_is_asked() {
    let checkout = Checkout::with_spec(BOUND_AND_GENERATED, None);

    let resolver = Counting::default();
    run_json(&checkout, &resolver);
    assert_eq!(resolver.calls.get(), 0);

    // The same run as text asks both stores. It runs through gather and
    // settle, so no program starts inside the test process.
    let resolver = Counting::default();
    let ready = gather(&checkout.args(&[]), Vec::new())
        .and_then(|gathered| settle(gathered, &[&resolver], &mut Person::absent()));
    assert!(ready.is_ok(), "{ready:?}");
    assert_eq!(resolver.calls.get(), 2);
}

#[test]
fn an_unbound_secret_is_not_asked_for_while_a_setting_fails_the_check() {
    let checkout = Checkout::new(None);
    let mut person = Person::typing("typed-test-value", true);
    let refused = prepared(
        &checkout.args(&[("SHELF_PAGE_SIZE", "1000")]),
        &[],
        &FakeResolver::default(),
        &mut person,
    )
    .unwrap_err();
    assert_eq!(
        codes(&refused),
        [&codes::VARIABLE_INVALID, &codes::SECRET_UNBOUND]
    );
    assert!(person.asked.is_empty(), "{:?}", person.asked);

    let mut person = Person::typing("typed-test-value", false);
    prepared(
        &checkout.args(&[("SHELF_PAGE_SIZE", "30")]),
        &[],
        &FakeResolver::default(),
        &mut person,
    )
    .unwrap();
    assert_eq!(
        person.asked.first().map(String::as_str),
        Some("secret SHELF_UPSTREAM_TOKEN")
    );
}

/// A spec with the group `mirror` of two optional settings.
const MIRROR: &str = r#"
[groups.mirror]
variables = ["SHELF_MIRROR_ACCOUNT", "SHELF_MIRROR_KEY"]

[variables.SHELF_MIRROR_ACCOUNT]
purpose = "The account shelf uses at the catalogue mirror."
type = "string"
unset = "optional"
show = "shown"
source = "setting"

[variables.SHELF_MIRROR_KEY]
purpose = "The key of the account at the catalogue mirror."
type = "string"
unset = "optional"
show = "shown"
source = "setting"
"#;

#[test]
fn a_partly_set_group_is_refused_and_offers_to_set_the_rest_or_unset_the_others() {
    let checkout = Checkout::with_spec(MIRROR, None);
    let refused = prepared(
        &checkout.args(&[("SHELF_MIRROR_ACCOUNT", "shelf")]),
        &[],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();

    assert_eq!(codes(&refused), [&codes::GROUP_INCOMPLETE]);
    assert_eq!(
        refused.failures[0].to_string(),
        "SHELF_MIRROR_KEY is unset, and the group mirror needs it because SHELF_MIRROR_ACCOUNT \
         is set"
    );
    assert_eq!(
        repairs(&refused),
        [(
            hint(
                "set SHELF_MIRROR_KEY in this shell, in .env or with --set SHELF_MIRROR_KEY=VALUE"
            ),
            vec![hint("unset SHELF_MIRROR_ACCOUNT")]
        )]
    );
}

#[test]
fn a_fully_set_group_is_admitted() {
    let checkout = Checkout::with_spec(MIRROR, None);
    let ready = prepared(
        &checkout.args(&[
            ("SHELF_MIRROR_ACCOUNT", "shelf"),
            ("SHELF_MIRROR_KEY", "k-1"),
        ]),
        &[],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap();

    assert_eq!(value(&ready, "SHELF_MIRROR_KEY").as_deref(), Some("k-1"));
}

/// A spec with the open family `SHELF_DB_URL` and the listed family
/// `SHELF_TOKEN`.
const FAMILIES: &str = r#"
[families.SHELF_DB_URL]
purpose = "The database of one region, keyed by the region's code."
type = "url"
unset = "optional"
show = "shown"
source = "setting"

[families.SHELF_TOKEN]
purpose = "The API token of one tenant."
type = "string"
unset = "optional"
show = "withheld"
source = "setting"
keys = ["ACME", "GLOBEX"]
"#;

#[test]
fn dotenv_the_shell_and_set_each_give_a_member_its_value() {
    let checkout = Checkout::with_spec(FAMILIES, Some("SHELF_DB_URL_EU=https://eu.example/\n"));
    let ready = prepared(
        &checkout.args(&[("SHELF_DB_URL_US", "https://us.example/")]),
        &[("SHELF_TOKEN_ACME", "abc")],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap();

    assert_eq!(
        value(&ready, "SHELF_DB_URL_EU").as_deref(),
        Some("https://eu.example/")
    );
    assert_eq!(
        value(&ready, "SHELF_DB_URL_US").as_deref(),
        Some("https://us.example/")
    );
    assert_eq!(secret(&ready, "SHELF_TOKEN_ACME").as_deref(), Some("abc"));
    assert!(
        !ready
            .notes
            .iter()
            .any(|note| note.contains("does not declare")),
        "{:?}",
        ready.notes
    );
}

#[test]
fn an_unlisted_key_is_refused_and_offers_to_unset_it_or_to_list_the_key() {
    let checkout = Checkout::with_spec(FAMILIES, None);
    let refused = prepared(
        &checkout.args(&[]),
        &[("SHELF_TOKEN_INITECH", "abc")],
        &FakeResolver::default(),
        &mut Person::absent(),
    )
    .unwrap_err();

    assert_eq!(codes(&refused), [&codes::FAMILY_UNLISTED_KEY]);
    assert_eq!(
        refused.failures[0].to_string(),
        "SHELF_TOKEN_INITECH is set, and the family SHELF_TOKEN lists only the keys ACME and \
         GLOBEX"
    );
    assert_eq!(
        repairs(&refused),
        [(
            hint("unset SHELF_TOKEN_INITECH in this shell, in .env and in --set"),
            vec![hint(
                "add INITECH to keys in [families.SHELF_TOKEN] in starmap.toml"
            )]
        )]
    );
}
