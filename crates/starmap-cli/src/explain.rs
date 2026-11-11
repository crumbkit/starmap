//! `starmap explain`, which describes each declared variable and the value
//! it has in this shell.

use std::ffi::OsString;
use std::fmt;

use crumbkit::schemars::{Schema, SchemaGenerator};
use crumbkit::serde::Serializer;
use crumbkit::{Batch, Document, Failure, Recovery, Row, Shown};
use schemars::JsonSchema;
use serde::Serialize;
use starmap::{
    Bindings, Bound, Cache, Choices, Mode, Name, SecretRef, Source, Spec, Type, Unset, Value,
    Variable,
};

use crate::cli::{ExplainArgs, SpecFile};
use crate::codes::VARIABLE_UNKNOWN;
use crate::mode;
use crate::spec::{self, SpecFailure};

#[cfg(test)]
mod tests;

/// The document that `starmap explain` prints.
pub(crate) const EXPLAIN: Document<Batch<Explained, UnknownVariable>> = Document::new(
    "starmap",
    "explain",
    1,
    "Each variable that a spec declares, with its rules and the value it has in this shell.",
);

/// What a withheld value prints as, whatever its length, so the output
/// does not reveal how long the secret is.
const DOTS: &str = "••••••••";

/// One declared variable, with its rules and the value it has in this
/// shell.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Explained {
    /// The variable's name.
    name: String,
    /// The type of the variable's value, such as port or integer 1..=100.
    #[serde(rename = "type")]
    kind: String,
    /// What happens when the variable has no value, in local mode and, when it differs, in production.
    unset: String,
    /// Where the variable's value comes from, with the kind of its bind.
    source: String,
    /// The variable's value in this shell, or its default; a withheld value prints as eight dots and has no text in JSON.
    value: Cell,
    /// Whether the value may be shown or is withheld.
    show: String,
    /// The variable's bind, such as a 1Password reference, when it has one.
    bind: Option<String>,
    /// `never` when the bind keeps the value out of any cache of resolved values; empty, and left out of `--json`, when a cache may keep it.
    #[serde(skip_serializing_if = "Option::is_none")]
    cache: Option<String>,
    /// The sentence that says what the variable is for.
    purpose: String,
    /// The sentence that says what the application does when the variable is unset; empty, and left out of `--json`, when the spec states none.
    #[serde(skip_serializing_if = "Option::is_none")]
    effect: Option<String>,
    /// The sentence that states the check the application's own code makes of the value, which starmap never runs; empty, and left out of `--json`, when the spec states none.
    #[serde(skip_serializing_if = "Option::is_none")]
    check: Option<String>,
    /// The name of the named enumeration that the variable's type is; left out of `--json` for any other type.
    #[serde(skip_serializing_if = "Option::is_none")]
    enumeration: Option<String>,
    /// Each value of a named enumeration with its description; empty, and left out of `--json`, for any other type.
    #[serde(skip_serializing_if = "Option::is_none")]
    values: Option<Values>,
}

impl Row for Explained {
    const COLUMNS: &'static [&'static str] = &["name", "type", "unset", "source", "value"];
    const LONG_COLUMNS: &'static [&'static str] = &[
        "show", "bind", "cache", "purpose", "effect", "check", "values",
    ];
    const UNCUT_COLUMNS: &'static [&'static str] = &["name"];
}

/// The value of a variable as explain shows it. Its text is the value, the
/// default with `(default)`, `unset`, or eight dots for a withheld value.
/// Its JSON is an object with the state and, for a shown value, the text.
#[derive(Debug)]
pub(crate) struct Cell(CellJson);

/// The JSON form of a [`Cell`].
#[derive(Debug, Serialize, JsonSchema)]
struct CellJson {
    /// Whether the value comes from this shell, is the default, is unset,
    /// or comes from this shell and does not fit the variable's type.
    state: State,
    /// The value's text. A withheld value never has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

/// Where a variable's value comes from in this shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum State {
    /// This shell sets the variable.
    Set,
    /// This shell leaves the variable unset, and its default applies.
    Default,
    /// The variable has no value.
    Unset,
    /// This shell sets the variable to a value that does not fit its type.
    Invalid,
}

impl Cell {
    /// Returns the cell of a value in `state`, whose text is `text`, or
    /// none when the variable is withheld.
    fn new(state: State, text: Option<String>) -> Self {
        Self(CellJson { state, text })
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = self.0.text.as_deref().unwrap_or(DOTS);
        match self.0.state {
            State::Set => formatter.write_str(text),
            State::Default => write!(formatter, "{text} (default)"),
            State::Unset => formatter.write_str("unset"),
            State::Invalid => write!(formatter, "{text} (invalid)"),
        }
    }
}

impl Shown for Cell {
    fn json<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }

    fn schema(generator: &mut SchemaGenerator) -> Schema {
        CellJson::json_schema(generator)
    }
}

crumbkit::shown!(Cell);

/// The values of a named enumeration, each with its description. Its text
/// is `value: description` for each value, separated by `; `, because a
/// table cell holds one line. Its JSON is a list of objects.
#[derive(Debug)]
pub(crate) struct Values(Vec<Described>);

/// One value of a named enumeration with its description.
#[derive(Debug, Serialize, JsonSchema)]
struct Described {
    /// The value.
    value: String,
    /// The sentence that says what choosing the value means.
    description: String,
}

impl Values {
    /// Returns the values of a named enumeration type, or `None` for any
    /// other type.
    pub(crate) fn of(ty: &Type) -> Option<Self> {
        let Type::Enumeration(choices) = ty else {
            return None;
        };
        choices.name()?;
        Some(Self(described(choices)))
    }
}

/// Returns each choice of `choices` with its description.
fn described(choices: &Choices) -> Vec<Described> {
    choices
        .iter()
        .map(|choice| Described {
            value: choice.to_string(),
            description: choices.description(choice).unwrap_or_default().to_owned(),
        })
        .collect()
}

impl fmt::Display for Values {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, described) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str("; ")?;
            }
            write!(formatter, "{}: {}", described.value, described.description)?;
        }
        Ok(())
    }
}

impl Shown for Values {
    fn json<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }

    fn schema(generator: &mut SchemaGenerator) -> Schema {
        Vec::<Described>::json_schema(generator)
    }
}

crumbkit::shown!(Values);

/// A name that `starmap explain` was given and the spec does not declare.
#[derive(Debug, Failure)]
#[failure(
    code = VARIABLE_UNKNOWN,
    recovery = list_again(file),
    field(name = name.as_str()),
    field(file = file.as_str()),
)]
pub(crate) struct UnknownVariable {
    /// The name.
    name: Name,
    /// The spec file, as the caller gave it.
    file: String,
}

impl fmt::Display for UnknownVariable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} does not declare {}", self.file, self.name)
    }
}

/// Returns the recovery of an unknown name, which explains every variable
/// of the same spec file.
fn list_again(file: &str) -> Recovery {
    Recovery::Run(SpecFile::call(&["explain"], file))
}

impl std::error::Error for UnknownVariable {}

/// Reads and parses the spec that `--spec` names, and explains each
/// variable that `names` lists, or every declared variable without names,
/// with its value in `vars`, the process environment. The note names the
/// mode and says that explain resolves no bind.
///
/// # Errors
///
/// Returns [`SpecFailure`] when the spec cannot be read or parsed. A name
/// that the spec does not declare is a failure of that name alone.
pub(crate) fn explain(
    args: &ExplainArgs,
    vars: Vec<(OsString, OsString)>,
) -> Result<Batch<Explained, UnknownVariable>, SpecFailure> {
    let spec = spec::open(&args.file.spec)?.spec;
    let file = args.file.spec.display().to_string();
    let bindings = starmap::capture(&spec, vars);
    let names: Vec<Name> = if args.names.is_empty() {
        spec.variables().map(|(name, _)| name.clone()).collect()
    } else {
        args.names.clone()
    };
    let (_, note) = mode::decide(&spec, &bindings);
    let mut batch = Batch::new(&["explain"], "names", &names).note(format!(
        "{note} Values come from this shell, and explain resolves no bind."
    ));
    for name in names {
        batch.push(row(&spec, &file, &bindings, name));
    }
    Ok(batch)
}

/// Returns the row of the variable `name`, or the failure of a name that
/// `spec`, read from `file`, does not declare.
fn row(
    spec: &Spec,
    file: &str,
    bindings: &Bindings,
    name: Name,
) -> Result<Explained, UnknownVariable> {
    let Some(variable) = spec.variable(&name) else {
        return Err(UnknownVariable {
            name,
            file: file.to_owned(),
        });
    };
    Ok(Explained {
        kind: type_text(variable.value_type()),
        unset: unset_text(variable),
        source: source_text(variable.source()),
        value: cell(variable, bindings.get(&name)),
        show: variable.show().to_string(),
        bind: bind_text(variable.source()),
        cache: cache_text(variable.source()).map(str::to_owned),
        purpose: variable.purpose().to_owned(),
        effect: variable.effect().map(str::to_owned),
        check: variable.check().map(str::to_owned),
        enumeration: enumeration_name(variable.value_type()),
        values: Values::of(variable.value_type()),
        name: name.as_str().to_owned(),
    })
}

/// Returns the cell of `variable`'s value, which `bound` gives when this
/// shell sets it. A value that counts as unset, such as a relative path
/// where the type takes an absolute one, falls back to the default.
fn cell(variable: &Variable, bound: Option<&Bound>) -> Cell {
    match bound {
        Some(Bound::Plain(raw)) => match variable.value_type().parse(raw) {
            Ok(Some(_)) => return Cell::new(State::Set, Some(raw.to_string_lossy().into_owned())),
            Ok(None) => {}
            Err(_) => {
                return Cell::new(State::Invalid, Some(raw.to_string_lossy().into_owned()));
            }
        },
        // The cell never holds a withheld value's text, whichever state it
        // shows.
        Some(Bound::Secret(secret)) => {
            let state = match variable.value_type().check_secret(secret) {
                Ok(()) => State::Set,
                Err(_) => State::Invalid,
            };
            return Cell::new(state, None);
        }
        Some(_) => return Cell::new(State::Invalid, None),
        None => {}
    }
    match variable.unset().in_mode(Mode::Local) {
        Unset::Default(value) => Cell::new(State::Default, Some(value_text(value))),
        Unset::Required | Unset::Optional => Cell::new(State::Unset, None),
    }
}

/// Returns a value's text, as an environment would hold it.
pub(crate) fn value_text(value: &Value) -> String {
    match value {
        Value::Path(path) => path.display().to_string(),
        Value::Port(port) => port.to_string(),
        Value::Url(url) => url.to_string(),
        Value::SocketAddress(address) => address.to_string(),
        Value::Boolean(boolean) => boolean.to_string(),
        Value::Integer(number) => number.to_string(),
        Value::Choice(choice) => choice.to_string(),
        Value::String(string) => string.to_string_lossy().into_owned(),
        Value::Bytes { .. } => value.to_string(),
        // A kind of value that a later release of starmap adds.
        _ => "a value".to_owned(),
    }
}

/// Returns the text of a type, such as `integer 1..=100`.
pub(crate) fn type_text(ty: &Type) -> String {
    match ty {
        Type::Path { absolute: false } => "path".to_owned(),
        Type::Path { absolute: true } => "absolute path".to_owned(),
        Type::Port => "port".to_owned(),
        Type::Url { schemes: None } => "url".to_owned(),
        Type::Url {
            schemes: Some(schemes),
        } => format!("url ({schemes})"),
        Type::SocketAddress => "ip socket address".to_owned(),
        Type::Boolean => "boolean".to_owned(),
        Type::Integer(range) => format!("integer {}..={}", range.min(), range.max()),
        Type::Enumeration(choices) => match choices.name() {
            Some(name) => format!("{name}: one of {choices}"),
            None => format!("one of {choices}"),
        },
        Type::String { pattern: None } => "string".to_owned(),
        Type::String {
            pattern: Some(pattern),
        } => format!("string matching {pattern}"),
        Type::Bytes(rule) => rule.to_string(),
        // A type that a later release of starmap adds.
        _ => "another type".to_owned(),
    }
}

/// Returns the name of a named enumeration type, or `None` for any other
/// type.
fn enumeration_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Enumeration(choices) => choices.name().map(ToString::to_string),
        _ => None,
    }
}

/// Returns a variable's unset rule, such as `default dev` or `optional;
/// production required`.
pub(crate) fn unset_text(variable: &Variable) -> String {
    let word = |unset: &Unset| match unset {
        Unset::Required => "required".to_owned(),
        Unset::Optional => "optional".to_owned(),
        Unset::Default(value) => format!("default {}", value_text(value)),
    };
    let local = variable.unset().in_mode(Mode::Local);
    let production = variable.unset().in_mode(Mode::Production);
    if local == production {
        word(local)
    } else {
        format!("{}; production {}", word(local), word(production))
    }
}

/// Returns a variable's source with the kind of its bind, such as `secret,
/// 1Password` or `provided multiverse/port`.
pub(crate) fn source_text(source: &Source) -> String {
    match source {
        Source::Setting { bind: None } => "setting".to_owned(),
        Source::Setting { bind: Some(_) } => "setting, path bind".to_owned(),
        Source::Secret { bind: None } => "secret".to_owned(),
        Source::Secret { bind: Some(bind) } => match bind.reference() {
            SecretRef::OnePassword(_) => "secret, 1Password".to_owned(),
            // A reference that a later release of starmap adds.
            _ => "secret".to_owned(),
        },
        Source::Generated { .. } => "generated".to_owned(),
        Source::Inherited => "inherited".to_owned(),
        Source::Provided(reference) => format!("provided {}", reference.kind()),
        // A source or reference that a later release of starmap adds.
        _ => "another source".to_owned(),
    }
}

/// Returns `never` for a secret whose bind keeps the value out of any
/// cache of resolved values, and `None` for every other variable.
pub(crate) fn cache_text(source: &Source) -> Option<&'static str> {
    match source {
        Source::Secret { bind: Some(bind) } if bind.cache() == Cache::Never => Some("never"),
        _ => None,
    }
}

/// Returns a variable's bind, such as `op://Personal/shelf mirror/token`,
/// or `None` for a variable without one.
pub(crate) fn bind_text(source: &Source) -> Option<String> {
    match source {
        Source::Setting {
            bind: Some(template),
        } => Some(template.as_str().to_owned()),
        Source::Secret { bind: Some(bind) } => match bind.reference() {
            SecretRef::OnePassword(reference) => Some(reference.to_string()),
            // A reference that a later release of starmap adds.
            _ => None,
        },
        Source::Generated { bytes } => Some(format!("{} random bytes", bytes.get())),
        Source::Provided(reference) => {
            let mut text = reference.kind().to_string();
            for (key, value) in reference.qualifiers() {
                text.push_str(&format!(" {key}={value}"));
            }
            Some(text)
        }
        _ => None,
    }
}
