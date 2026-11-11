//! `starmap run`, which resolves and checks every declared variable and
//! starts a program with exactly the declared environment applied.
//!
//! The values of the caller come from `.env` beside the spec, or the file
//! that a pointer names, the shell and `--set`, and `.env` and `--set` never give a secret. The doc of
//! `Verb::Run` in cli.rs, which `starmap run --help` prints, states the
//! order in which a value is taken from them, a setting's path bind and
//! the default. starmap-resolve then resolves every variable under
//! one deadline, with the 1Password CLI and the Keychain as resolvers and
//! no provider. A required secret without a bind that nothing supplies is
//! asked for at a terminal, held for this run, and saved to the Keychain
//! when the person agrees. The program starts through crumbkit-child,
//! which passes on its exit code. With `--only-declared`, the program
//! inherits nothing, and receives the declared values and the built-in
//! variables of [`starmap::BuiltIns`].

use std::ffi::OsString;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use crumbkit::{Failure, Recovery, Run, Sink};
use starmap::{
    Bindings, Bound, Environment, Finding, GroupName, Mode, Name, NoPlace, Placeholder, Problem,
    ProviderKind, Quote, Source, Spec, Verdict,
};
use starmap_resolve::{
    AppName, Context, Keychain, Lookup, OnePassword, Places, ResolveProblem, Resolver, SecretError,
    Slot, WorkspaceName, resolve,
};

use crate::cli::{Assignment, RunArgs, SpecFile};
use crate::codes::{
    BIND_NO_PLACE, BIND_NO_PROVIDER, FAMILY_UNLISTED_KEY, GROUP_INCOMPLETE, SECRET_UNBOUND,
    SECRET_UNREADABLE, VARIABLE_INVALID, VARIABLE_MISSING, VARIABLE_SECRET, VARIABLE_UNKNOWN,
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

/// Why `--fields` is refused.
///
/// BASE-37: crumbkit-child 0.2.0 keeps its own copy of this text private,
/// so this one repeats it word for word, and the two can drift apart. A
/// crumbkit-child release that exports the text replaces this copy.
const TEXT_ONLY: &str = "this command hands the terminal to another program";

/// Runs `args.program` with the environment that the spec declares, and
/// returns its exit code. `vars` is the process environment, and `notes`
/// receives each note before the program starts. `output` says whether the
/// caller passed `--no-input`. 1Password and the Keychain resolve the
/// secrets.
pub(crate) fn run(
    args: &RunArgs,
    vars: Vec<(OsString, OsString)>,
    output: Run,
    notes: &mut dyn Write,
) -> ExitCode {
    let one_password = OnePassword::new(PathBuf::from("op"));
    let keychain = Keychain::new();
    run_with(args, vars, output, notes, &[&one_password, &keychain])
}

/// Runs `args.program` as [`run`] does, with `resolvers` resolving the
/// secrets.
fn run_with(
    args: &RunArgs,
    vars: Vec<(OsString, OsString)>,
    mut output: Run,
    notes: &mut dyn Write,
    resolvers: &[&dyn Resolver],
) -> ExitCode {
    let mut sink = Sink::standard();
    if let Some(code) = output.refuse_fields_for_text(&mut sink, TEXT_ONLY) {
        return code;
    }
    let mut command = child::command(&args.program);
    let inherited = args.only_declared.then(|| vars.clone());
    let gathered = match gather(args, vars) {
        Ok(gathered) => gathered,
        Err(refused) => return refuse(&mut sink, &mut output, &refused, notes),
    };
    if output.is_json() {
        print_notes(notes, &gathered.notes);
        // crumbkit-child refuses --json before it starts anything, because
        // the program takes the terminal. Refusing here, before
        // resolution, means no secret store is asked and no Keychain item
        // is created for a run that cannot start.
        return crumbkit_child::hand_over(output, command);
    }
    let interactive = !output.no_input() && ask::terminals();
    let settled = {
        let mut terminal = Terminal::new(&mut output, interactive);
        settle(gathered, resolvers, &mut terminal)
    };
    match settled {
        Ok(ready) => {
            print_notes(notes, &ready.notes);
            child::apply(&ready.environment, inherited, &mut command);
            // BASE-37: hand_over ignores SIGINT and SIGQUIT, which the
            // terminal sends to the program too, and forwards no signal.
            // A SIGTERM or SIGHUP sent to starmap alone, as a process
            // manager sends it, ends starmap and leaves the program
            // running. Signal forwarding in crumbkit-child replaces this.
            crumbkit_child::hand_over(output, command)
        }
        Err(refused) => refuse(&mut sink, &mut output, &refused, notes),
    }
}

/// Prints each failure of `refused` and then its notes, and returns the
/// exit code of its first failure.
fn refuse<O: Write, E: Write>(
    sink: &mut Sink<O, E>,
    output: &mut Run,
    refused: &Refused,
    notes: &mut dyn Write,
) -> ExitCode {
    for failure in &refused.failures {
        output.print_failure(sink, failure);
    }
    print_notes(notes, &refused.notes);
    let code = refused
        .failures
        .first()
        .map_or(1, |failure| output.failure_exit(failure.code()));
    crumbkit::events::ended(code, None)
}

/// Writes each of `lines` to `notes`. A note that cannot be written is
/// dropped, because the run's outcome does not depend on it.
fn print_notes(notes: &mut dyn Write, lines: &[String]) {
    for line in lines {
        let _ = writeln!(notes, "{line}");
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

/// Everything a run knows before it asks a secret store: the spec, the
/// caller's values, the mode and the places.
#[derive(Debug)]
pub(crate) struct Gathered {
    spec: Spec,
    /// The spec file, as the caller gave it, which a recovery passes to
    /// `starmap explain`.
    file: String,
    caller: Bindings,
    mode: Mode,
    places: Places,
    /// The workspace that a typed secret is saved under.
    workspace: WorkspaceName,
    /// The app that a typed secret is saved under.
    app: AppName,
    notes: Vec<String>,
}

/// Reads the spec and `.env`, takes the caller's values and decides the
/// mode. It asks no secret store.
///
/// # Errors
///
/// Returns every failure of the spec, `.env` or `--set`.
pub(crate) fn gather(args: &RunArgs, vars: Vec<(OsString, OsString)>) -> Result<Gathered, Refused> {
    let mut notes = Vec::new();
    let path = &args.file.spec;
    let spec::Opened { spec, located } =
        spec::open(path).map_err(|failure| Refused::new(vec![failure.into()], Vec::new()))?;
    let checkout = checkout(path)?;
    let dotenv_file = located.dotenv;
    let dotenv = dotenv::read(&dotenv_file, &spec).map_err(|failures| {
        Refused::new(failures.into_iter().map(Into::into).collect(), Vec::new())
    })?;
    if !dotenv.undeclared.is_empty() {
        notes.push(format!(
            "{} sets {}, which {} does not declare, so the program does not receive {}.",
            dotenv_file.display(),
            dotenv.undeclared.join(", "),
            located.file.display(),
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

    let app = AppName::of_dir(&checkout);
    let workspace = args
        .workspace
        .clone()
        .unwrap_or_else(|| WorkspaceName::from(&app));
    let mut places =
        Places::new(checkout.clone()).workspace(workspace.clone(), app.clone(), checkout);
    if let Some(home) = home {
        places = places.home(home);
    }
    Ok(Gathered {
        spec,
        file: path.display().to_string(),
        caller,
        mode,
        places,
        workspace,
        app,
        notes,
    })
}

/// Resolves every variable of `gathered` with `resolvers`, asks `ask` for
/// each required secret that nothing supplies when nothing else failed,
/// and checks the result.
///
/// # Errors
///
/// Returns every failure that stops the program from starting.
pub(crate) fn settle(
    gathered: Gathered,
    resolvers: &[&dyn Resolver],
    ask: &mut dyn Ask,
) -> Result<Ready, Refused> {
    let Gathered {
        spec,
        file,
        caller,
        mode,
        places,
        workspace,
        app,
        mut notes,
    } = gathered;
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
    let (unbound, other): (Vec<_>, Vec<_>) = resolution
        .failures
        .into_iter()
        .partition(|failure| matches!(failure.problem, ResolveProblem::Unbound));
    let mut failures: Vec<RunFailure> = other
        .into_iter()
        .map(|failure| RunFailure::resolve(failure.name, failure.problem))
        .collect();
    // A person is asked only when the typed values would let the program
    // start, so nobody types a secret for a run that fails anyway. The
    // check runs first on every value but the unbound secrets, which it
    // refuses as missing until someone types them.
    if failures.is_empty()
        && !unbound.is_empty()
        && let Verdict::Refused(refusals) = starmap::check(&spec, &bindings, mode)
    {
        failures.extend(
            refusals
                .iter()
                .filter(|finding| !unbound.iter().any(|failure| failure.name == finding.name))
                .map(|finding| RunFailure::check(finding, &file)),
        );
    }
    let can_start = failures.is_empty();
    for failure in unbound {
        let typed = if can_start {
            typed(
                Slot::new(&workspace, &app, &failure.name),
                resolvers,
                ask,
                &mut notes,
            )
        } else {
            None
        };
        match typed {
            Some(bound) => bindings.bind(failure.name, bound),
            None => failures.push(RunFailure::Unbound { name: failure.name }),
        }
    }
    if !failures.is_empty() {
        return Err(Refused::new(failures, notes));
    }
    checked(&spec, &file, &bindings, mode, notes)
}

/// Checks `bindings` in `mode`, and returns the environment or each
/// variable that stops the check.
fn checked(
    spec: &Spec,
    file: &str,
    bindings: &Bindings,
    mode: Mode,
    notes: Vec<String>,
) -> Result<Ready, Refused> {
    match starmap::check(spec, bindings, mode) {
        Verdict::Ready(environment) => Ok(Ready { environment, notes }),
        Verdict::Refused(refusals) => {
            let failures = refusals
                .iter()
                .map(|finding| RunFailure::check(finding, file))
                .collect();
            Err(Refused::new(failures, notes))
        }
    }
}

/// Asks `ask` for the secret of `slot`, and offers to save the answer with
/// the first of `resolvers` that keeps saved values. It returns `None` when
/// no person can answer or the person typed nothing.
fn typed(
    slot: Slot<'_>,
    resolvers: &[&dyn Resolver],
    ask: &mut dyn Ask,
    notes: &mut Vec<String>,
) -> Option<Bound> {
    if !ask.can_ask() {
        return None;
    }
    let name = slot.name;
    let secret = ask.secret(name)?;
    let lookup = Lookup::Saved { slot };
    let (workspace, app) = (slot.workspace.as_str(), slot.app.as_str());
    let question = format!(
        "Save {name} to the Keychain for workspace {workspace} and app {app}, so later runs find it?"
    );
    if ask.save(&question) {
        let deadline = Instant::now() + SAVE_DEADLINE;
        let mut result = Err(SecretError::Unsupported);
        for resolver in resolvers {
            result = resolver.save(lookup, &secret, deadline);
            if !matches!(result, Err(SecretError::Unsupported)) {
                break;
            }
        }
        // The Keychain keeps a value it already holds for the same item,
        // so the note says what the Keychain holds, not that it took the
        // typed value.
        notes.push(match result {
            Ok(()) => format!(
                "The Keychain holds {name} as service starmap.{workspace} and account {app}/{name}, and later runs read it from there."
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
        let declared = spec.declared(name);
        match declared
            .as_ref()
            .map(|declared| declared.variable().source())
        {
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

/// Returns `names` as `A`, `A and B`, or `A, B and C`.
fn names(names: &[Name]) -> String {
    let texts: Vec<&str> = names.iter().map(Name::as_str).collect();
    self::texts(&texts)
}

/// Returns `texts` as `A`, `A and B`, or `A, B and C`.
fn texts(texts: &[&str]) -> String {
    match texts.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// Returns the absolute directory of the file at `path`, the checkout. For a
/// pointer, the checkout is the pointer's directory, wherever the file it
/// names lies.
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
        hint = "declare {name} in starmap.toml",
        alternative = Recovery::Hint(format!("remove --set {name}")),
        field(key = name.as_str()),
    )]
    Unknown {
        /// The variable.
        name: Name,
    },
    /// `--set` gives a secret or generated variable a value.
    #[failure(
        code = VARIABLE_SECRET,
        hint = "remove --set {name}, and give {name} a bind in starmap.toml",
        alternative = Recovery::Hint(format!(
            "remove --set {name}, and let starmap run ask for it at a terminal"
        )),
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
        recovery = Recovery::Run(SpecFile::call(&["explain"], file).value("names", name.as_str())),
        field(key = name.as_str()),
        field(value = value.as_ref().map(Quote::as_str)),
    )]
    Invalid {
        /// The variable.
        name: Name,
        /// What is wrong with the value.
        problem: String,
        /// The quote of the refused value, which only a shown variable's
        /// value has.
        value: Option<Quote>,
        /// The spec file, as the caller gave it.
        file: String,
    },
    /// A variable's name is the prefix of a listed family and a key that
    /// the family does not list.
    #[failure(
        code = FAMILY_UNLISTED_KEY,
        hint = "unset {name} in this shell, in .env and in --set",
        alternative = Recovery::Hint(format!(
            "add {key} to keys in [families.{family}] in starmap.toml"
        )),
        field(key = name.as_str()),
    )]
    UnlistedKey {
        /// The variable.
        name: Name,
        /// The family's prefix.
        family: Name,
        /// The variable's key, the part of its name after the prefix.
        key: String,
        /// The keys the family lists.
        keys: Vec<String>,
    },
    /// A variable is unset, and other members of its group are set.
    #[failure(
        code = GROUP_INCOMPLETE,
        hint = "set {name} in this shell, in .env or with --set {name}=VALUE",
        alternative = Recovery::Hint(format!("unset {}", names(set))),
        field(key = name.as_str()),
    )]
    GroupIncomplete {
        /// The variable.
        name: Name,
        /// The variable's group.
        group: GroupName,
        /// The members of the group that are set.
        set: Vec<Name>,
    },
    /// No provider of the variable's kind is present.
    #[failure(
        code = BIND_NO_PROVIDER,
        hint = "set {name} in this shell, in .env or with --set {name}=VALUE",
        alternative = Recovery::Hint(format!("start the app under a host that provides {kind}")),
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
        hint = "set HOME in this shell",
        alternative = Recovery::Hint(format!("set {name} with --set {name}=VALUE")),
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
        hint = "run starmap run at a terminal without --no-input, type {name} and save it to the Keychain",
        alternative = Recovery::Hint(format!(
            "give {name} a bind in starmap.toml, such as {{ one-password = \"op://vault/item/field\" }}"
        )),
        field(key = name.as_str()),
    )]
    Unbound {
        /// The variable.
        name: Name,
    },
    /// No secret store that run registers handles the variable's lookup,
    /// such as a generated value where the session has no login keychain.
    #[failure(
        code = SECRET_UNREADABLE,
        hint = "starmap run keeps generated values only in the macOS login keychain, so run it in a macOS session that has one, or make {name} a secret with a one-password bind in starmap.toml",
        field(key = name.as_str()),
    )]
    NoStore {
        /// The variable.
        name: Name,
    },
    /// A secret store could not return the secret.
    #[failure(
        code = SECRET_UNREADABLE,
        hint = "check the bind of {name} in starmap.toml, unlock 1Password or the Keychain, and check that the 1Password CLI op is installed and signed in, then run the command again",
        field(key = name.as_str()),
    )]
    Secret {
        /// The variable.
        name: Name,
        /// What the secret store returned.
        error: SecretError,
    },
}

impl RunFailure {
    /// Returns the failure of the variable `name` that resolution could
    /// not resolve because of `problem`.
    fn resolve(name: Name, problem: ResolveProblem) -> Self {
        match problem {
            ResolveProblem::NoProvider(kind) => Self::NoProvider { name, kind },
            ResolveProblem::NoPlace(NoPlace(placeholder)) => Self::NoPlace { name, placeholder },
            ResolveProblem::Unbound => Self::Unbound { name },
            ResolveProblem::Secret(error) => Self::Secret { name, error },
            ResolveProblem::Deadline => Self::Secret {
                name,
                error: SecretError::Deadline,
            },
            ResolveProblem::NoResolver => Self::NoStore { name },
            // run registers no provider, so a provider error reads as a
            // store that failed. So does a problem that a later
            // starmap-resolve adds, until this command learns it.
            ResolveProblem::Provider(error) => Self::Secret {
                name,
                error: SecretError::Program(io::Error::other(error.to_string())),
            },
            _ => Self::Secret {
                name,
                error: SecretError::Program(io::Error::other("the value could not be resolved")),
            },
        }
    }

    /// Returns the failure of `finding`, which the check of the spec `file`
    /// refused.
    fn check(finding: &Finding, file: &str) -> Self {
        let name = finding.name.clone();
        let file = file.to_owned();
        let invalid = |problem: String| Self::Invalid {
            name: finding.name.clone(),
            problem,
            value: finding.value.clone(),
            file: file.clone(),
        };
        match &finding.problem {
            Problem::Missing { mode } => Self::Missing { name, mode: *mode },
            Problem::Invalid(error) => invalid(error.to_string()),
            Problem::ShowMismatch { .. } => invalid(
                "the value is held as a secret where starmap.toml shows it, or the other way round"
                    .to_owned(),
            ),
            Problem::ModeMismatch { decided } => {
                invalid(format!("the resolved value decides {decided} mode"))
            }
            Problem::GroupIncomplete { group, set } => Self::GroupIncomplete {
                name,
                group: group.clone(),
                set: set.clone(),
            },
            Problem::UnlistedKey { family, keys } => Self::UnlistedKey {
                key: name
                    .as_str()
                    .get(family.as_str().len() + 1..)
                    .unwrap_or_default()
                    .to_owned(),
                name,
                family: family.clone(),
                keys: keys.clone(),
            },
            // A problem that a later starmap adds reads as an invalid
            // value, until this command learns it.
            _ => invalid("the value does not fit the variable".to_owned()),
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
            Self::Invalid {
                name,
                problem,
                value,
                ..
            } => {
                write!(formatter, "{name}: {problem}")?;
                match value {
                    Some(quote) => write!(formatter, " (the value is {quote})"),
                    None => Ok(()),
                }
            }
            Self::GroupIncomplete { name, group, set } => {
                let verb = if set.len() == 1 { "is" } else { "are" };
                write!(
                    formatter,
                    "{name} is unset, and the group {group} needs it because {} {verb} set",
                    names(set)
                )
            }
            Self::UnlistedKey {
                name, family, keys, ..
            } => {
                let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
                let noun = if keys.len() == 1 { "key" } else { "keys" };
                write!(
                    formatter,
                    "{name} is set, and the family {family} lists only the {noun} {}",
                    texts(&keys)
                )
            }
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
            Self::NoStore { name } => write!(
                formatter,
                "no secret store that starmap run uses on this machine handles {name}"
            ),
            Self::Secret { name, error } => {
                write!(formatter, "{name} could not be read, because {error}")
            }
        }
    }
}

impl std::error::Error for RunFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spec(failure) => failure.source(),
            Self::DotEnv(failure) => failure.source(),
            Self::Secret { error, .. } => Some(error),
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
