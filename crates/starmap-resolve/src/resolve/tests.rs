//! Tests for resolving a spec's variables, with fake providers and
//! resolvers.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use starmap::{
    Bindings, Bound, Mode, Name, Placeholder, ProvidedRef, ProviderKind, Secret, Spec, Verdict,
};

use super::{Context, Resolution, ResolveFailure, ResolveProblem, resolve};
use crate::{Lookup, Places, Provider, ProviderError, Resolver, SecretError};

/// A spec with every source kind: a mode setting, a required and an
/// optional provided variable, a setting with a path bind, a secret with a
/// 1Password bind that production requires, a required generated value, an
/// inherited path and a withheld setting.
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
source = "provided"
bind = { provided = "multiverse/port", process = "api" }

[variables.SHELF_CATALOGUE_URL]
purpose = "The base URL of the shelf app whose catalogue this checkout reads."
type = "url"
unset = "optional"
show = "shown"
source = "provided"
bind = { provided = "multiverse/url", app = "shelf" }

[variables.SHELF_DATABASE]
purpose = "The SQLite file that holds the catalogue."
type = "path"
unset = { default = "books.db" }
show = "shown"
source = "setting"
bind = { path = "{workspace}/books.db" }

[variables.SHELF_MIRROR_TOKEN]
purpose = "The token shelf sends to the catalogue mirror."
type = "string"
unset = { local = "optional", production = "required" }
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

[variables.XDG_STATE_HOME]
purpose = "The directory that holds shelf's state."
type = { path = { absolute = true } }
unset = "optional"
show = "shown"
source = "inherited"

[variables.SHELF_API_KEY]
purpose = "The key shelf sends to the catalogue API."
type = "string"
unset = "optional"
show = "withheld"
source = "setting"
"#;

/// Returns the parsed [`SPEC`].
fn spec() -> Spec {
    Spec::parse(SPEC).unwrap()
}

/// Returns the name `text`.
fn name(text: &str) -> Name {
    text.parse().unwrap()
}

/// Returns a deadline that no test reaches.
fn later() -> Instant {
    Instant::now() + Duration::from_secs(3600)
}

/// Returns the caller bindings of `pairs`, with each withheld variable's
/// value held as a secret, as `starmap::capture` holds them.
fn caller(pairs: &[(&str, &str)]) -> Bindings {
    let vars = pairs
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)));
    starmap::capture(&spec(), vars)
}

/// Returns places with a checkout, a workspace and a home.
fn places() -> Places {
    Places::new(PathBuf::from("/checkout"))
        .workspace(
            "shelf-dev".parse().unwrap(),
            "shelf".parse().unwrap(),
            PathBuf::from("/workspace"),
        )
        .home(PathBuf::from("/home/luke"))
}

/// Returns the value bound to `variable` as text: `plain:<value>`,
/// `secret:<value>` or `not unicode`.
#[expect(
    clippy::disallowed_methods,
    reason = "the tests check the text that a resolved secret holds"
)]
fn bound(resolution: &Resolution, variable: &str) -> Option<String> {
    match resolution.bindings.get(&name(variable))? {
        Bound::Plain(raw) => Some(format!("plain:{}", raw.to_string_lossy())),
        Bound::Secret(secret) => Some(format!("secret:{}", secret.expose())),
        _ => Some("not unicode".to_owned()),
    }
}

/// Returns the name of each variable that failed, in order.
fn failed(resolution: &Resolution) -> Vec<&str> {
    resolution
        .failures
        .iter()
        .map(|failure| failure.name.as_str())
        .collect()
}

/// Returns the problem of the variable `variable`, if it failed.
fn problem<'r>(resolution: &'r Resolution, variable: &str) -> Option<&'r ResolveProblem> {
    resolution
        .failures
        .iter()
        .find(|failure| failure.name.as_str() == variable)
        .map(|failure| &failure.problem)
}

/// A provider that answers every request with one value or one error, and
/// records each request with its qualifiers and deadline.
struct FakeProvider {
    kind: ProviderKind,
    answer: Result<&'static str, ProviderError>,
    waits_out_deadline: bool,
    asked: RefCell<Vec<Request>>,
}

/// A request a fake provider received.
struct Request {
    name: String,
    qualifiers: Vec<(String, String)>,
    deadline: Instant,
}

impl FakeProvider {
    /// Returns a provider of `kind` that answers `value`.
    fn answering(kind: &str, value: &'static str) -> Self {
        Self::new(kind, Ok(value))
    }

    /// Returns a provider of `kind` that answers `value` only once the
    /// deadline it receives has passed.
    fn waiting(kind: &str, value: &'static str) -> Self {
        Self {
            waits_out_deadline: true,
            ..Self::answering(kind, value)
        }
    }

    /// Returns a provider of `kind` with `answer`.
    fn new(kind: &str, answer: Result<&'static str, ProviderError>) -> Self {
        Self {
            kind: kind.parse().unwrap(),
            answer,
            waits_out_deadline: false,
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl Provider for FakeProvider {
    fn kind(&self) -> &ProviderKind {
        &self.kind
    }

    fn provide(
        &self,
        name: &Name,
        reference: &ProvidedRef,
        deadline: Instant,
    ) -> Result<OsString, ProviderError> {
        let qualifiers = reference
            .qualifiers()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        self.asked.borrow_mut().push(Request {
            name: name.as_str().to_owned(),
            qualifiers,
            deadline,
        });
        if self.waits_out_deadline {
            wait_out(deadline);
        }
        self.answer.clone().map(OsString::from)
    }
}

/// Returns once `deadline` has passed.
#[expect(
    clippy::disallowed_methods,
    reason = "BASE-11: the deadline passing is the event this fake waits for, and the plan's \
              Instant deadline has no clock owner to sleep in"
)]
fn wait_out(deadline: Instant) {
    while Instant::now() < deadline {
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
    }
}

/// A lookup a fake resolver received, with owned text.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asked {
    Reference(String),
    Generated {
        workspace: String,
        app: String,
        name: String,
        bytes: u16,
    },
}

/// A resolver that answers each lookup with `secret:<n>` for the lookup's
/// position, or with an error, and records each lookup and deadline.
struct FakeResolver {
    fails: Option<fn() -> SecretError>,
    asked: RefCell<Vec<(Asked, Instant)>>,
}

impl FakeResolver {
    /// Returns a resolver that answers each lookup.
    fn answering() -> Self {
        Self {
            fails: None,
            asked: RefCell::new(Vec::new()),
        }
    }

    /// Returns a resolver that fails each lookup with `error`.
    fn failing(error: fn() -> SecretError) -> Self {
        Self {
            fails: Some(error),
            asked: RefCell::new(Vec::new()),
        }
    }

    /// Returns each lookup the resolver received.
    fn lookups(&self) -> Vec<Asked> {
        self.asked
            .borrow()
            .iter()
            .map(|(asked, _)| asked.clone())
            .collect()
    }
}

impl Resolver for FakeResolver {
    fn resolve(
        &self,
        lookup: Lookup<'_>,
        deadline: Instant,
    ) -> Result<Secret<String>, SecretError> {
        let asked = match lookup {
            Lookup::Reference(starmap::SecretRef::OnePassword(reference)) => {
                Asked::Reference(reference.as_str().to_owned())
            }
            Lookup::Generated {
                workspace,
                app,
                name,
                bytes,
            } => Asked::Generated {
                workspace: workspace.as_str().to_owned(),
                app: app.as_str().to_owned(),
                name: name.as_str().to_owned(),
                bytes: bytes.get(),
            },
            _ => Asked::Reference("another reference".to_owned()),
        };
        let mut asked_so_far = self.asked.borrow_mut();
        asked_so_far.push((asked, deadline));
        match self.fails {
            Some(error) => Err(error()),
            None => Ok(Secret::new(format!("resolved-{}", asked_so_far.len()))),
        }
    }
}

#[test]
fn a_caller_value_wins_for_setting_and_inherited_variables() {
    let caller = caller(&[
        ("SHELF_DATABASE", "/elsewhere/books.db"),
        ("XDG_STATE_HOME", "/state"),
        ("SHELF_API_KEY", "key-from-shell"),
    ]);
    let resolution = resolve(
        &spec(),
        Mode::Local,
        &Context::new(places(), &caller, later()),
    );

    assert_eq!(
        bound(&resolution, "SHELF_DATABASE").as_deref(),
        Some("plain:/elsewhere/books.db")
    );
    assert_eq!(
        bound(&resolution, "XDG_STATE_HOME").as_deref(),
        Some("plain:/state")
    );
    assert_eq!(
        bound(&resolution, "SHELF_API_KEY").as_deref(),
        Some("secret:key-from-shell")
    );
    assert!(resolution.ignored.is_empty());
}

#[test]
fn a_setting_without_a_caller_value_expands_its_path_bind() {
    let caller = Bindings::new();
    let resolution = resolve(
        &spec(),
        Mode::Local,
        &Context::new(places(), &caller, later()),
    );
    assert_eq!(
        bound(&resolution, "SHELF_DATABASE").as_deref(),
        Some("plain:/workspace/books.db")
    );
    assert_eq!(bound(&resolution, "XDG_STATE_HOME"), None);
}

#[test]
fn a_path_bind_without_its_place_leaves_an_optional_setting_to_its_default() {
    let caller = caller(&[("SHELF_PORT", "8080")]);
    let places = Places::new(PathBuf::from("/checkout"));
    let resolution = resolve(
        &spec(),
        Mode::Local,
        &Context::new(places, &caller, later()),
    );
    assert_eq!(bound(&resolution, "SHELF_DATABASE"), None);
    assert!(problem(&resolution, "SHELF_DATABASE").is_none());
}

#[test]
fn a_provided_variable_takes_the_caller_value_when_no_provider_of_its_kind_is_registered() {
    let caller = caller(&[("SHELF_PORT", "8080")]);
    let urls = FakeProvider::answering("multiverse/url", "http://shelf.test/");
    let context = Context::new(places(), &caller, later()).provider(&urls);
    let resolution = resolve(&spec(), Mode::Local, &context);

    assert_eq!(
        bound(&resolution, "SHELF_PORT").as_deref(),
        Some("plain:8080")
    );
    assert_eq!(
        bound(&resolution, "SHELF_CATALOGUE_URL").as_deref(),
        Some("plain:http://shelf.test/")
    );
}

#[test]
fn a_registered_provider_wins_over_the_caller_value() {
    let caller = caller(&[("SHELF_PORT", "8080")]);
    let ports = FakeProvider::answering("multiverse/port", "9090");
    let context = Context::new(places(), &caller, later()).provider(&ports);
    let resolution = resolve(&spec(), Mode::Local, &context);

    assert_eq!(
        bound(&resolution, "SHELF_PORT").as_deref(),
        Some("plain:9090")
    );
    let asked = ports.asked.borrow();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].name, "SHELF_PORT");
    assert_eq!(
        asked[0].qualifiers,
        vec![("process".to_owned(), "api".to_owned())]
    );
}

#[test]
fn a_required_provided_variable_without_a_provider_or_caller_value_fails() {
    let caller = Bindings::new();
    let resolution = resolve(
        &spec(),
        Mode::Local,
        &Context::new(places(), &caller, later()),
    );

    assert!(matches!(
        problem(&resolution, "SHELF_PORT"),
        Some(ResolveProblem::NoProvider(kind)) if kind.as_str() == "multiverse/port"
    ));
    // The optional provided variable stays unbound, and check leaves it
    // unset.
    assert!(problem(&resolution, "SHELF_CATALOGUE_URL").is_none());
    assert_eq!(bound(&resolution, "SHELF_CATALOGUE_URL"), None);
}

#[test]
fn a_caller_value_for_a_bound_secret_or_a_generated_variable_is_ignored_with_a_note() {
    let caller = caller(&[
        ("SHELF_MIRROR_TOKEN", "token-from-shell"),
        ("SHELF_SESSION_KEY", "key-from-shell"),
    ]);
    let resolver = FakeResolver::answering();
    let context = Context::new(places(), &caller, later()).resolver(&resolver);
    let resolution = resolve(&spec(), Mode::Local, &context);

    assert_eq!(
        resolution.ignored,
        vec![name("SHELF_MIRROR_TOKEN"), name("SHELF_SESSION_KEY")]
    );
    assert_eq!(
        bound(&resolution, "SHELF_MIRROR_TOKEN").as_deref(),
        Some("secret:resolved-1")
    );
    assert_eq!(
        bound(&resolution, "SHELF_SESSION_KEY").as_deref(),
        Some("secret:resolved-2")
    );
}

#[test]
fn a_secret_asks_the_resolver_by_reference_and_a_generated_value_by_generated_lookup() {
    let caller = Bindings::new();
    let resolver = FakeResolver::answering();
    let context = Context::new(places(), &caller, later()).resolver(&resolver);
    let _resolution = resolve(&spec(), Mode::Local, &context);

    assert_eq!(
        resolver.lookups(),
        vec![
            Asked::Reference("op://Personal/shelf mirror/token".to_owned()),
            Asked::Generated {
                workspace: "shelf-dev".to_owned(),
                app: "shelf".to_owned(),
                name: "SHELF_SESSION_KEY".to_owned(),
                bytes: 48,
            },
        ]
    );
}

#[test]
fn resolvers_are_asked_in_order_until_one_supports_the_lookup() {
    let caller = Bindings::new();
    let unsupported = FakeResolver::failing(|| SecretError::Unsupported);
    let resolver = FakeResolver::answering();
    let context = Context::new(places(), &caller, later())
        .resolver(&unsupported)
        .resolver(&resolver);
    let resolution = resolve(&spec(), Mode::Local, &context);

    assert_eq!(unsupported.lookups().len(), 2);
    assert_eq!(resolver.lookups().len(), 2);
    assert_eq!(
        bound(&resolution, "SHELF_MIRROR_TOKEN").as_deref(),
        Some("secret:resolved-1")
    );
}

#[test]
fn a_lookup_that_no_resolver_supports_fails_a_required_variable_as_unsupported() {
    let caller = Bindings::new();
    let unsupported = FakeResolver::failing(|| SecretError::Unsupported);
    let context = Context::new(places(), &caller, later()).resolver(&unsupported);
    let resolution = resolve(&spec(), Mode::Production, &context);

    for variable in ["SHELF_MIRROR_TOKEN", "SHELF_SESSION_KEY"] {
        assert!(
            matches!(
                problem(&resolution, variable),
                Some(ResolveProblem::Secret(SecretError::Unsupported))
            ),
            "{variable}"
        );
    }
}

#[test]
fn a_lookup_that_no_resolver_supports_leaves_an_optional_variable_unbound() {
    let caller = Bindings::new();
    let unsupported = FakeResolver::failing(|| SecretError::Unsupported);
    let context = Context::new(places(), &caller, later()).resolver(&unsupported);
    let resolution = resolve(&spec(), Mode::Local, &context);

    assert_eq!(unsupported.lookups().len(), 2);
    assert!(problem(&resolution, "SHELF_MIRROR_TOKEN").is_none());
    assert_eq!(bound(&resolution, "SHELF_MIRROR_TOKEN"), None);
}

#[test]
fn a_secret_without_a_resolver_fails_only_where_the_mode_requires_it() {
    let caller = caller(&[("SHELF_PORT", "8080"), ("SHELF_ENV", "prod")]);
    let local = resolve(
        &spec(),
        Mode::Local,
        &Context::new(places(), &caller, later()),
    );
    let production = resolve(
        &spec(),
        Mode::Production,
        &Context::new(places(), &caller, later()),
    );

    assert!(problem(&local, "SHELF_MIRROR_TOKEN").is_none());
    assert!(matches!(
        problem(&production, "SHELF_MIRROR_TOKEN"),
        Some(ResolveProblem::NoResolver)
    ));
}

#[test]
fn a_generated_value_without_a_workspace_fails_on_the_workspace_placeholder() {
    let caller = Bindings::new();
    let resolver = FakeResolver::answering();
    let places = Places::new(PathBuf::from("/checkout"));
    let context = Context::new(places, &caller, later()).resolver(&resolver);
    let resolution = resolve(&spec(), Mode::Local, &context);

    assert!(matches!(
        problem(&resolution, "SHELF_SESSION_KEY"),
        Some(ResolveProblem::Placeholder(Placeholder::Workspace))
    ));
}

#[test]
fn every_provider_and_resolver_receives_the_one_deadline_of_the_context() {
    let caller = Bindings::new();
    let deadline = later();
    let ports = FakeProvider::answering("multiverse/port", "9090");
    let urls = FakeProvider::answering("multiverse/url", "http://shelf.test/");
    let resolver = FakeResolver::answering();
    let context = Context::new(places(), &caller, deadline)
        .provider(&ports)
        .provider(&urls)
        .resolver(&resolver);
    let _resolution = resolve(&spec(), Mode::Local, &context);

    let mut deadlines: Vec<Instant> = ports
        .asked
        .borrow()
        .iter()
        .map(|asked| asked.deadline)
        .collect();
    deadlines.extend(urls.asked.borrow().iter().map(|asked| asked.deadline));
    deadlines.extend(resolver.asked.borrow().iter().map(|asked| asked.1));
    assert_eq!(deadlines, vec![deadline; 4]);
}

#[test]
fn a_passed_deadline_fails_each_variable_that_needs_a_provider_or_resolver() {
    let caller = caller(&[("XDG_STATE_HOME", "/state")]);
    let ports = FakeProvider::answering("multiverse/port", "9090");
    let resolver = FakeResolver::answering();
    // The deadline is the instant before resolution starts, so it has
    // passed at every step.
    let deadline = Instant::now();
    let context = Context::new(places(), &caller, deadline)
        .provider(&ports)
        .resolver(&resolver);
    let resolution = resolve(&spec(), Mode::Local, &context);

    assert_eq!(
        failed(&resolution),
        ["SHELF_PORT", "SHELF_MIRROR_TOKEN", "SHELF_SESSION_KEY"]
    );
    assert!(
        resolution
            .failures
            .iter()
            .all(|failure| matches!(failure.problem, ResolveProblem::Deadline))
    );
    assert!(ports.asked.borrow().is_empty());
    assert!(resolver.asked.borrow().is_empty());
    // Steps that ask nobody still resolve.
    assert_eq!(
        bound(&resolution, "XDG_STATE_HOME").as_deref(),
        Some("plain:/state")
    );
    assert_eq!(
        bound(&resolution, "SHELF_DATABASE").as_deref(),
        Some("plain:/workspace/books.db")
    );
}

#[test]
fn one_call_reports_every_failure_against_its_own_variable() {
    let caller = Bindings::new();
    let ports = FakeProvider::new(
        "multiverse/port",
        Err(ProviderError::new("the port pool is empty")),
    );
    let urls = FakeProvider::answering("multiverse/url", "http://shelf.test/");
    let locked = FakeResolver::failing(|| SecretError::Locked);
    let places = Places::new(PathBuf::from("/checkout"));
    let context = Context::new(places, &caller, later())
        .provider(&ports)
        .provider(&urls)
        .resolver(&locked);
    let resolution = resolve(&spec(), Mode::Local, &context);

    assert_eq!(
        failed(&resolution),
        ["SHELF_PORT", "SHELF_MIRROR_TOKEN", "SHELF_SESSION_KEY"]
    );
    assert!(matches!(
        problem(&resolution, "SHELF_PORT"),
        Some(ResolveProblem::Provider(error))
            if *error == ProviderError::new("the port pool is empty")
    ));
    assert!(matches!(
        problem(&resolution, "SHELF_MIRROR_TOKEN"),
        Some(ResolveProblem::Secret(SecretError::Locked))
    ));
    assert!(matches!(
        problem(&resolution, "SHELF_SESSION_KEY"),
        Some(ResolveProblem::Placeholder(Placeholder::Workspace))
    ));
    assert_eq!(
        bound(&resolution, "SHELF_CATALOGUE_URL").as_deref(),
        Some("plain:http://shelf.test/")
    );
}

#[test]
fn a_provided_value_for_a_withheld_variable_is_bound_as_a_secret() {
    const WITHHELD: &str = r#"
[variables.SHELF_UPSTREAM_TOKEN]
purpose = "The token the host issues for the upstream catalogue."
type = "string"
unset = "required"
show = "withheld"
source = "provided"
bind = { provided = "multiverse/token" }
"#;
    let spec = Spec::parse(WITHHELD).unwrap();
    let caller = Bindings::new();
    let tokens = FakeProvider::answering("multiverse/token", "issued");
    let context = Context::new(places(), &caller, later()).provider(&tokens);
    let resolution = resolve(&spec, Mode::Local, &context);
    assert_eq!(
        bound(&resolution, "SHELF_UPSTREAM_TOKEN").as_deref(),
        Some("secret:issued")
    );
}

#[test]
fn a_resolution_without_failures_passes_check() {
    let caller = caller(&[("SHELF_ENV", "prod"), ("XDG_STATE_HOME", "/state")]);
    let ports = FakeProvider::answering("multiverse/port", "9090");
    let resolver = FakeResolver::answering();
    let context = Context::new(places(), &caller, later())
        .provider(&ports)
        .resolver(&resolver);
    let resolution = resolve(&spec(), Mode::Production, &context);

    assert!(resolution.failures.is_empty(), "{:?}", resolution.failures);
    assert!(matches!(
        starmap::check(&spec(), &resolution.bindings, Mode::Production),
        Verdict::Ready(_)
    ));
}

#[test]
fn a_deadline_that_passes_partway_fails_each_later_variable_without_asking_its_provider_or_resolver()
 {
    let spec = spec();
    let caller = Bindings::new();
    let ports = FakeProvider::waiting("multiverse/port", "9090");
    let urls = FakeProvider::answering("multiverse/url", "http://shelf.test/");
    let resolver = FakeResolver::answering();
    // The deadline starts once the fixture is ready (BASE-35). Its margin
    // lets the port's provider be asked in time on a busy machine, and the
    // provider then waits the rest of it out.
    let deadline = Instant::now() + Duration::from_millis(200);
    let context = Context::new(places(), &caller, deadline)
        .provider(&ports)
        .provider(&urls)
        .resolver(&resolver);
    let resolution = resolve(&spec, Mode::Local, &context);

    // The port's provider was asked in time and answered after the
    // deadline, so its value stands and every later step fails.
    assert_eq!(
        bound(&resolution, "SHELF_PORT").as_deref(),
        Some("plain:9090")
    );
    assert_eq!(
        failed(&resolution),
        [
            "SHELF_CATALOGUE_URL",
            "SHELF_MIRROR_TOKEN",
            "SHELF_SESSION_KEY"
        ]
    );
    assert!(
        resolution
            .failures
            .iter()
            .all(|failure| matches!(failure.problem, ResolveProblem::Deadline))
    );
    assert!(urls.asked.borrow().is_empty());
    assert!(resolver.asked.borrow().is_empty());
}

/// A spec whose secret has no bind, so only the caller can supply it.
const UNBOUND_SECRET: &str = r#"
[variables.SHELF_UPSTREAM_TOKEN]
purpose = "The token shelf sends to the upstream catalogue."
type = "string"
unset = "required"
show = "withheld"
source = "secret"
"#;

#[test]
fn a_caller_value_supplies_a_secret_without_a_bind_and_passes_check() {
    let spec = Spec::parse(UNBOUND_SECRET).unwrap();
    let caller = starmap::capture(
        &spec,
        [(
            OsString::from("SHELF_UPSTREAM_TOKEN"),
            OsString::from("token-from-shell"),
        )],
    );
    let resolver = FakeResolver::answering();
    let context = Context::new(places(), &caller, later()).resolver(&resolver);
    let resolution = resolve(&spec, Mode::Production, &context);

    assert_eq!(
        bound(&resolution, "SHELF_UPSTREAM_TOKEN").as_deref(),
        Some("secret:token-from-shell")
    );
    assert!(resolution.ignored.is_empty());
    assert!(resolution.failures.is_empty());
    assert!(resolver.lookups().is_empty());
    assert!(matches!(
        starmap::check(&spec, &resolution.bindings, Mode::Production),
        Verdict::Ready(_)
    ));
}

#[test]
fn a_host_builds_a_resolution_and_a_failure_for_its_own_tests() {
    let failure = ResolveFailure::new(name("SHELF_PORT"), ResolveProblem::Deadline);
    let resolution = Resolution::new(Bindings::new(), vec![failure], vec![name("SHELF_ENV")]);

    assert_eq!(failed(&resolution), ["SHELF_PORT"]);
    assert!(matches!(
        problem(&resolution, "SHELF_PORT"),
        Some(ResolveProblem::Deadline)
    ));
    assert_eq!(resolution.ignored, [name("SHELF_ENV")]);
}
