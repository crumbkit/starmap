//! Tests for preparing the environment of `starmap run`, with a fake
//! resolver and a fake person, so no test needs 1Password, a Keychain or a
//! terminal.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Instant;

use crumbkit::Code;
use starmap::{Name, Secret};
use starmap_resolve::{Lookup, ResolveProblem, Resolver, SecretError};

use super::{Ask, Ready, Refused, RunFailure, gather, run_with, settle};
use crate::cli::{Assignment, RunArgs, SpecFile, Verb};
use crate::codes;
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
