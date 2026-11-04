//! `starmap run`, which resolves and checks every declared variable and
//! starts a program with exactly the declared environment applied.
//!
//! The values of the caller come from `.env` beside the spec, then the
//! shell, then `--set`, each later one winning, and `.env` and `--set`
//! never give a secret. starmap-resolve then resolves every variable under
//! one deadline, with the 1Password CLI and the Keychain as resolvers and
//! no provider. A required secret without a bind that nothing supplies is
//! asked for at a terminal, held for this run, and saved to the Keychain
//! when the person agrees. The program starts through crumbkit-child,
//! which passes on its exit code.

use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use crumbkit::{Failure, Output, Sink};
use starmap::{
    Bindings, Bound, Environment, Mode, Name, Placeholder, Problem, ProviderKind, Source, Spec,
    Verdict,
};
use starmap_resolve::{
    AppName, Context, Keychain, Lookup, OnePassword, Places, ResolveProblem, Resolver, SecretError,
    WorkspaceName, resolve,
};

use crate::cli::{Assignment, RunArgs};
use crate::codes::{
    BIND_NO_PLACE, BIND_NO_PROVIDER, SECRET_DEADLINE, SECRET_DENIED, SECRET_LOCKED,
    SECRET_NOT_FOUND, SECRET_UNBOUND, SECRET_UNREADABLE, SECRET_UNSUPPORTED, VARIABLE_INVALID,
    VARIABLE_MISSING, VARIABLE_SECRET, VARIABLE_UNKNOWN,
};
use crate::dotenv::{self, DotEnvFailure};
use crate::spec::{self, SpecFailure};
use crate::{files, mode};

mod ask;
mod child;
#[cfg(test)]
mod tests;

pub(crate) use ask::{Ask, Terminal};

/// How long resolution may take, from its start to the last secret store's
/// answer (BASE-34). A 1Password or Keychain prompt that waits for the
/// person counts against it. The question that asks a person for a secret
/// comes after resolution and has no deadline.
const RESOLVE_DEADLINE: Duration = Duration::from_secs(60);

/// How long saving one typed secret to the Keychain may take.
const SAVE_DEADLINE: Duration = Duration::from_secs(30);

/// Why `--json` and `--fields` are refused.
const TEXT_ONLY: &str = "this command hands the terminal to another program";

/// Runs `args.program` with the environment that the spec declares, and
/// returns its exit code. `vars` is the process environment, and `notes`
/// receives each note before the program starts. `no_input` says whether
/// the caller passed `--no-input`.
pub(crate) fn run(
    args: &RunArgs,
    vars: Vec<(OsString, OsString)>,
    mut output: Output,
    no_input: bool,
    notes: &mut dyn Write,
) -> ExitCode {
    let mut sink = Sink::standard();
    if let Some(code) = output.refuse_fields_for_text(&mut sink, TEXT_ONLY) {
        return code;
    }
    let mut command = child::command(&args.program);
    // With --json, a failure prints as JSON for the caller that reads it,
    // no question is asked, and crumbkit-child refuses to start the program
    // once the environment is ready.
    let interactive = !no_input && !output.is_json() && ask::terminals();
    let one_password = OnePassword::new(PathBuf::from("op"));
    let keychain = Keychain::new();
    let resolvers: [&dyn Resolver; 2] = [&one_password, &keychain];
    let prepared = {
        let mut terminal = Terminal::new(&mut output, interactive);
        prepare(args, vars, &resolvers, &mut terminal)
    };
    match prepared {
        Ok(ready) => {
            for note in &ready.notes {
                let _ = writeln!(notes, "{note}");
            }
            ready.environment.apply(&mut command);
            // BASE-37: hand_over ignores SIGINT and SIGQUIT, which the
            // terminal sends to the program too, and forwards no signal.
            // A SIGTERM or SIGHUP sent to starmap alone, as a process
            // manager sends it, ends starmap and leaves the program
            // running. Signal forwarding in crumbkit-child replaces this.
            crumbkit_child::hand_over(output, command)
        }
        Err(refused) => {
            for failure in &refused.failures {
                output.print_failure(&mut sink, failure);
            }
            for note in &refused.notes {
                let _ = writeln!(notes, "{note}");
            }
            let code = refused
                .failures
                .first()
                .map_or(1, |failure| output.failure_exit(failure.code()));
            crumbkit::events::ended(code, None)
        }
    }
}

/// A checked environment that the program starts with, and the notes to
/// print before it starts.
#[derive(Debug)]
pub(crate) struct Ready {
    /// The checked values of every declared variable.
    pub(crate) environment: Environment,
    /// The notes, in the order they print.
    pub(crate) notes: Vec<String>,
}

/// The failures that stop the program from starting, with the notes to
/// print after them.
#[derive(Debug)]
pub(crate) struct Refused {
    /// Each failure, at least one.
    pub(crate) failures: Vec<RunFailure>,
    /// The notes, in the order they print.
    pub(crate) notes: Vec<String>,
}

impl Refused {
    /// Returns the refusal of `failures`, with a closing note that says
    /// nothing started.
    fn new(failures: Vec<RunFailure>, mut notes: Vec<String>) -> Self {
        let count = failures.len();
        let problems = if count == 1 { "problem" } else { "problems" };
        notes.push(format!(
            "starmap started nothing, because of {count} {problems}."
        ));
        Self { failures, notes }
    }
}

/// Reads the spec and `.env`, takes the caller's values, resolves every
/// variable with `resolvers`, asks `ask` for each required secret that
/// nothing supplies, and checks the result.
///
/// # Errors
///
/// Returns every failure that stops the program from starting.
pub(crate) fn prepare(
    args: &RunArgs,
    vars: Vec<(OsString, OsString)>,
    resolvers: &[&dyn Resolver],
    ask: &mut dyn Ask,
) -> Result<Ready, Refused> {
    let mut notes = Vec::new();
    let path = &args.file.spec;
    let spec =
        spec::load(path).map_err(|failure| Refused::new(vec![failure.into()], Vec::new()))?;
    let checkout = checkout(path)?;
    let dotenv_file = path.parent().unwrap_or(Path::new("")).join(".env");
    let dotenv = dotenv::read(&dotenv_file, &spec).map_err(|failures| {
        Refused::new(failures.into_iter().map(Into::into).collect(), Vec::new())
    })?;
    if !dotenv.undeclared.is_empty() {
        notes.push(format!(
            "{} sets {}, which starmap.toml does not declare, so the program does not receive {}.",
            dotenv_file.display(),
            dotenv.undeclared.join(", "),
            if dotenv.undeclared.len() == 1 {
                "it"
            } else {
                "them"
            },
        ));
    }
    let home = vars
        .iter()
        .find(|(key, _)| key == "HOME")
        .map(|(_, value)| PathBuf::from(value));
    let overrides =
        assignments(&spec, &args.set).map_err(|failures| Refused::new(failures, notes.clone()))?;
    let values = dotenv.values.into_iter().chain(vars).chain(overrides);
    let caller = starmap::capture(&spec, values);
    let (mode, mode_note) = mode::decide(&spec, &caller);
    notes.push(mode_note);

    let label = label(&checkout);
    let workspace = args.workspace.clone().unwrap_or_else(|| label.workspace());
    let app = label.app();
    let mut places =
        Places::new(checkout.clone()).workspace(workspace.clone(), app.clone(), checkout);
    if let Some(home) = home {
        places = places.home(home);
    }
    let deadline = Instant::now() + RESOLVE_DEADLINE;
    let mut context = Context::new(places, &caller, deadline);
    for resolver in resolvers {
        context = context.resolver(*resolver);
    }
    let resolution = resolve(&spec, mode, &context);
    for name in &resolution.ignored {
        notes.push(ignored_note(&spec, name));
    }
    let mut bindings = resolution.bindings;
    let mut failures = Vec::new();
    let held = Held {
        workspace: &workspace,
        app: &app,
    };
    for failure in resolution.failures {
        if matches!(failure.problem, ResolveProblem::Unbound) {
            match typed(&failure.name, held, resolvers, ask, &mut notes) {
                Some(bound) => bindings.bind(failure.name, bound),
                None => failures.push(RunFailure::Unbound { name: failure.name }),
            }
        } else {
            failures.push(RunFailure::resolve(failure.name, failure.problem));
        }
    }
    if !failures.is_empty() {
        return Err(Refused::new(failures, notes));
    }
    checked(&spec, &bindings, mode, notes)
}

/// Checks `bindings` in `mode`, and returns the environment or each
/// variable that stops the check.
fn checked(
    spec: &Spec,
    bindings: &Bindings,
    mode: Mode,
    notes: Vec<String>,
) -> Result<Ready, Refused> {
    match starmap::check(spec, bindings, mode) {
        Verdict::Ready(environment) => Ok(Ready { environment, notes }),
        Verdict::Refused(refusals) => {
            let failures = refusals
                .iter()
                .map(|finding| RunFailure::check(finding.name.clone(), &finding.problem))
                .collect();
            Err(Refused::new(failures, notes))
        }
    }
}

/// The workspace and app that a typed secret is saved under.
#[derive(Debug, Clone, Copy)]
struct Held<'a> {
    workspace: &'a WorkspaceName,
    app: &'a AppName,
}

/// Asks `ask` for the secret `name`, and offers to save the answer with the
/// first of `resolvers` that keeps saved values. It returns `None` when no
/// person can answer or the person typed nothing.
fn typed(
    name: &Name,
    held: Held<'_>,
    resolvers: &[&dyn Resolver],
    ask: &mut dyn Ask,
    notes: &mut Vec<String>,
) -> Option<Bound> {
    if !ask.can_ask() {
        return None;
    }
    let secret = ask.secret(name)?;
    let question = format!(
        "Save {name} to the Keychain for workspace {} and app {}, so later runs find it?",
        held.workspace.as_str(),
        held.app.as_str()
    );
    if ask.save(&question) {
        let lookup = Lookup::Saved {
            workspace: held.workspace,
            app: held.app,
            name,
        };
        let deadline = Instant::now() + SAVE_DEADLINE;
        let mut result = Err(SecretError::Unsupported);
        for resolver in resolvers {
            result = resolver.save(lookup, &secret, deadline);
            if !matches!(result, Err(SecretError::Unsupported)) {
                break;
            }
        }
        notes.push(match result {
            Ok(()) => format!(
                "starmap saved {name} to the Keychain as service starmap.{} and account {}/{name}.",
                held.workspace.as_str(),
                held.app.as_str()
            ),
            Err(error) => format!(
                "starmap could not save {name} to the Keychain, because {error}, so only this run has it."
            ),
        });
    }
    Some(Bound::Secret(secret))
}

/// Returns the note of a caller value that resolution ignored for the
/// variable `name`.
fn ignored_note(spec: &Spec, name: &Name) -> String {
    let used = match spec.variable(name).map(starmap::Variable::source) {
        Some(Source::Generated { .. }) => "its generated value in the Keychain",
        _ => "its bind",
    };
    format!("{name} is set in this shell, and starmap ignored it and used {used}.")
}

/// Returns the caller values of `set`, or a failure for each name that the
/// spec does not declare or declares as a secret.
fn assignments(
    spec: &Spec,
    set: &[Assignment],
) -> Result<Vec<(OsString, OsString)>, Vec<RunFailure>> {
    let mut values = Vec::new();
    let mut failures = Vec::new();
    for assignment in set {
        let name = &assignment.name;
        match spec.variable(name).map(starmap::Variable::source) {
            None => failures.push(RunFailure::Unknown { name: name.clone() }),
            Some(Source::Secret { .. } | Source::Generated { .. }) => {
                failures.push(RunFailure::SetSecret { name: name.clone() });
            }
            Some(_) => values.push((
                OsString::from(name.as_str()),
                OsString::from(&assignment.value),
            )),
        }
    }
    if failures.is_empty() {
        Ok(values)
    } else {
        Err(failures)
    }
}

/// Returns the absolute directory of the spec at `path`, the checkout.
fn checkout(path: &Path) -> Result<PathBuf, Refused> {
    let file = path.display().to_string();
    files::canonical(path)
        .map(|path| path.parent().map(Path::to_path_buf).unwrap_or(path))
        .map_err(|error| {
            Refused::new(
                vec![SpecFailure::Unreadable { file, error }.into()],
                Vec::new(),
            )
        })
}

/// The name of a checkout's directory as a workspace and app name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Label(String);

impl Label {
    /// Returns the label as the workspace name.
    fn workspace(&self) -> WorkspaceName {
        self.0
            .parse()
            .unwrap_or_else(|_| Self::fallback().workspace())
    }

    /// Returns the label as the app name.
    fn app(&self) -> AppName {
        self.0.parse().unwrap_or_else(|_| Self::fallback().app())
    }

    /// The label of a directory without a name, such as `/`.
    fn fallback() -> Self {
        Self("app".to_owned())
    }
}

/// Returns the label of the directory `dir`: its name in lower case, with
/// each character other than a letter, a digit or a hyphen written as a
/// hyphen, and `app-` before a name that does not start with a letter. So
/// `My_App` gives `my-app` and `.tmp9` gives `app--tmp9`.
pub(crate) fn label(dir: &Path) -> Label {
    let Some(name) = dir.file_name() else {
        return Label::fallback();
    };
    let text: String = name
        .to_string_lossy()
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    match text.chars().next() {
        Some(first) if first.is_ascii_lowercase() => Label(text),
        Some(_) => Label(format!("app-{text}")),
        None => Label::fallback(),
    }
}

/// A failure that stops `starmap run` from starting the program.
#[derive(Debug, Failure)]
pub(crate) enum RunFailure {
    /// The spec cannot be read or parsed.
    #[failure(forward, diagnostic)]
    Spec(SpecFailure),
    /// `.env` cannot be read or parsed, or sets a secret.
    #[failure(forward)]
    DotEnv(DotEnvFailure),
    /// `--set` names a variable the spec does not declare.
    #[failure(
        code = VARIABLE_UNKNOWN,
        hint = "declare {name} in starmap.toml, or remove --set {name}",
        field(key = name.as_str()),
    )]
    Unknown {
        /// The variable.
        name: Name,
    },
    /// `--set` gives a secret or generated variable a value.
    #[failure(
        code = VARIABLE_SECRET,
        hint = "remove --set {name}, and give {name} a bind in starmap.toml, or let starmap run ask for it at a terminal",
        field(key = name.as_str()),
    )]
    SetSecret {
        /// The variable.
        name: Name,
    },
    /// A required variable has no value.
    #[failure(
        code = VARIABLE_MISSING,
        hint = "set {name} in this shell, in .env or with --set {name}=VALUE",
        field(key = name.as_str()),
    )]
    Missing {
        /// The variable.
        name: Name,
        /// The mode the check ran in.
        mode: Mode,
    },
    /// A variable's value does not fit its type.
    #[failure(
        code = VARIABLE_INVALID,
        hint = "give {name} a value that its type accepts, as starmap explain {name} describes",
        field(key = name.as_str()),
    )]
    Invalid {
        /// The variable.
        name: Name,
        /// What is wrong with the value.
        problem: String,
    },
    /// No provider of the variable's kind is present.
    #[failure(
        code = BIND_NO_PROVIDER,
        hint = "set {name} in this shell, in .env or with --set {name}=VALUE, or start the app under a host that provides {kind}",
        field(key = name.as_str()),
    )]
    NoProvider {
        /// The variable.
        name: Name,
        /// The kind that no provider supplies.
        kind: ProviderKind,
    },
    /// A path bind names a place that run has no directory for.
    #[failure(
        code = BIND_NO_PLACE,
        hint = "set HOME in this shell, or set {name} with --set {name}=VALUE",
        field(key = name.as_str()),
    )]
    NoPlace {
        /// The variable.
        name: Name,
        /// The placeholder without a place.
        placeholder: Placeholder,
    },
    /// A required secret without a bind has no value, and no person typed
    /// one.
    #[failure(
        code = SECRET_UNBOUND,
        hint = "run starmap run at a terminal without --no-input, type {name} and save it to the Keychain, or give {name} a bind in starmap.toml, such as {{ one-password = \"op://vault/item/field\" }}",
        field(key = name.as_str()),
    )]
    Unbound {
        /// The variable.
        name: Name,
    },
    /// The secret store is locked.
    #[failure(
        code = SECRET_LOCKED,
        hint = "unlock 1Password or the Keychain, then run the command again",
        field(key = name.as_str()),
    )]
    Locked {
        /// The variable.
        name: Name,
    },
    /// The secret's reference names no item or field.
    #[failure(
        code = SECRET_NOT_FOUND,
        hint = "correct the bind of {name} in starmap.toml",
        field(key = name.as_str()),
    )]
    NotFound {
        /// The variable.
        name: Name,
    },
    /// The person refused the secret store's prompt.
    #[failure(
        code = SECRET_DENIED,
        hint = "run the command again and allow access",
        field(key = name.as_str()),
    )]
    Denied {
        /// The variable.
        name: Name,
    },
    /// No resolver of this build or platform handles the secret.
    #[failure(
        code = SECRET_UNSUPPORTED,
        hint = "run starmap run on macOS, where the Keychain keeps the value, or start the app under a host that supplies {name}",
        field(key = name.as_str()),
    )]
    Unsupported {
        /// The variable.
        name: Name,
    },
    /// The program that reads the secret store failed.
    #[failure(
        code = SECRET_UNREADABLE,
        hint = "check that the 1Password CLI op is installed and signed in, then run the command again",
        field(key = name.as_str()),
    )]
    Unreadable {
        /// The variable.
        name: Name,
        /// What failed.
        problem: String,
    },
    /// Resolution passed its deadline.
    #[failure(
        code = SECRET_DEADLINE,
        hint = "run the command again; {name} was the variable that waited",
        field(key = name.as_str()),
    )]
    Deadline {
        /// The variable.
        name: Name,
    },
}

impl RunFailure {
    /// Returns the failure of the variable `name` that resolution could
    /// not resolve because of `problem`.
    fn resolve(name: Name, problem: ResolveProblem) -> Self {
        match problem {
            ResolveProblem::NoProvider(kind) => Self::NoProvider { name, kind },
            ResolveProblem::Placeholder(placeholder) => Self::NoPlace { name, placeholder },
            ResolveProblem::Unbound => Self::Unbound { name },
            ResolveProblem::Deadline | ResolveProblem::Secret(SecretError::Deadline) => {
                Self::Deadline { name }
            }
            ResolveProblem::Secret(SecretError::Locked) => Self::Locked { name },
            ResolveProblem::Secret(SecretError::NotFound) => Self::NotFound { name },
            ResolveProblem::Secret(SecretError::Denied) => Self::Denied { name },
            ResolveProblem::NoResolver | ResolveProblem::Secret(SecretError::Unsupported) => {
                Self::Unsupported { name }
            }
            ResolveProblem::Secret(error) => Self::Unreadable {
                name,
                problem: error.to_string(),
            },
            ResolveProblem::Provider(error) => Self::Unreadable {
                name,
                problem: error.to_string(),
            },
            // A problem that a later starmap-resolve adds reads as a store
            // that failed, until this command learns it.
            _ => Self::Unreadable {
                name,
                problem: "the value could not be resolved".to_owned(),
            },
        }
    }

    /// Returns the failure of the variable `name` that the check refused
    /// because of `problem`.
    fn check(name: Name, problem: &Problem) -> Self {
        match problem {
            Problem::Missing { mode } => Self::Missing { name, mode: *mode },
            Problem::Invalid(error) => Self::Invalid {
                name,
                problem: error.to_string(),
            },
            Problem::ShowMismatch { .. } => Self::Invalid {
                name,
                problem: "the value is held as a secret where starmap.toml shows it, or the other way round"
                    .to_owned(),
            },
            Problem::ModeMismatch { decided } => Self::Invalid {
                name,
                problem: format!("the resolved value decides {decided} mode"),
            },
            // A problem that a later starmap adds reads as an invalid
            // value, until this command learns it.
            _ => Self::Invalid {
                name,
                problem: "the value does not fit the variable".to_owned(),
            },
        }
    }
}

impl fmt::Display for RunFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spec(failure) => failure.fmt(formatter),
            Self::DotEnv(failure) => failure.fmt(formatter),
            Self::Unknown { name } => {
                write!(
                    formatter,
                    "--set names {name}, which starmap.toml does not declare"
                )
            }
            Self::SetSecret { name } => write!(
                formatter,
                "--set gives {name} a value, and starmap.toml declares it as a secret"
            ),
            Self::Missing { name, mode } => {
                write!(
                    formatter,
                    "{name} is required in {mode} mode and has no value"
                )
            }
            Self::Invalid { name, problem } => write!(formatter, "{name}: {problem}"),
            Self::NoProvider { name, kind } => write!(
                formatter,
                "{name} needs a value from the provider {kind}, and starmap run has no provider of that kind"
            ),
            Self::NoPlace { name, placeholder } => write!(
                formatter,
                "the path bind of {name} uses {placeholder}, and starmap run has no directory for it"
            ),
            Self::Unbound { name } => write!(
                formatter,
                "{name} is a secret with no bind, no value in this shell and no value saved in the Keychain"
            ),
            Self::Locked { name } => write!(
                formatter,
                "{name} could not be read, because its secret store is locked"
            ),
            Self::NotFound { name } => write!(
                formatter,
                "{name} could not be read, because its bind names no item or field"
            ),
            Self::Denied { name } => write!(
                formatter,
                "{name} could not be read, because access to its secret store was refused"
            ),
            Self::Unsupported { name } => write!(
                formatter,
                "{name} needs a secret store that this build of starmap or this platform lacks"
            ),
            Self::Unreadable { name, problem } => {
                write!(formatter, "{name} could not be read: {problem}")
            }
            Self::Deadline { name } => write!(
                formatter,
                "{name} could not be read in time, because resolution passed its deadline"
            ),
        }
    }
}

impl std::error::Error for RunFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spec(failure) => failure.source(),
            Self::DotEnv(failure) => failure.source(),
            _ => None,
        }
    }
}

impl From<SpecFailure> for RunFailure {
    fn from(failure: SpecFailure) -> Self {
        Self::Spec(failure)
    }
}

impl From<DotEnvFailure> for RunFailure {
    fn from(failure: DotEnvFailure) -> Self {
        Self::DotEnv(failure)
    }
}
