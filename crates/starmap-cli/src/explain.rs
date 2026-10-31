//! `starmap explain`, which describes each declared variable and the value
//! it has in this shell.

use std::ffi::OsString;
use std::fmt;

use crumbkit::schemars::{Schema, SchemaGenerator};
use crumbkit::serde::Serializer;
use crumbkit::{Batch, Call, Document, Failure, Recovery, Row, Shown};
use schemars::JsonSchema;
use serde::Serialize;
use starmap::{
    Bindings, Bound, Form, Mode, Name, SecretRef, Show, Source, Spec, Unset, Value, Variable,
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
    /// The sentence that says what the variable is for.
    purpose: String,
}

impl Row for Explained {
    const COLUMNS: &'static [&'static str] = &["name", "type", "unset", "source", "value"];
    const LONG_COLUMNS: &'static [&'static str] = &["show", "bind", "purpose"];
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
/// of the same spec file. The call passes `--spec` only when `file` is not
/// the default, so the usual recovery stays `starmap explain`.
fn list_again(file: &str) -> Recovery {
    let call = Call::new(&["explain"]);
    Recovery::Run(if file == SpecFile::DEFAULT {
        call
    } else {
        call.value("spec", file)
    })
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
    let spec = spec::load(&args.file.spec)?;
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
        kind: form_text(variable.form()),
        unset: unset_text(variable),
        source: source_text(variable.source()),
        value: cell(variable, bindings.get(&name)),
        show: variable.show().to_string(),
        bind: bind_text(variable.source()),
        purpose: variable.purpose().to_owned(),
        name: name.as_str().to_owned(),
    })
}

/// Returns the cell of `variable`'s value, which `bound` gives when this
/// shell sets it. A value that counts as unset, such as a relative path
/// where the type takes an absolute one, falls back to the default.
fn cell(variable: &Variable, bound: Option<&Bound>) -> Cell {
    let withheld = variable.show() == Show::Withheld;
    match bound {
        Some(Bound::Plain(raw)) => match variable.form().parse(raw) {
            Ok(Some(_)) => return Cell::new(State::Set, Some(raw.to_string_lossy().into_owned())),
            Ok(None) => {}
            Err(_) => {
                return Cell::new(State::Invalid, Some(raw.to_string_lossy().into_owned()));
            }
        },
        Some(Bound::Secret(_)) => return Cell::new(State::Set, None),
        Some(_) => return Cell::new(State::Invalid, None),
        None => {}
    }
    match variable.unset().in_mode(Mode::Local) {
        Unset::Default(value) => Cell::new(State::Default, (!withheld).then(|| value_text(value))),
        Unset::Required | Unset::Optional => Cell::new(State::Unset, None),
    }
}

/// Returns a value's text, as an environment would hold it.
pub(crate) fn value_text(value: &Value) -> String {
    match value {
        Value::Path(path) => path.display().to_string(),
        Value::Port(port) => port.to_string(),
        Value::Url(url) => url.to_string(),
        Value::Boolean(boolean) => boolean.to_string(),
        Value::Integer(number) => number.to_string(),
        Value::Choice(choice) => choice.to_string(),
        Value::String(string) => string.to_string_lossy().into_owned(),
        // A kind of value that a later release of starmap adds.
        _ => "a value".to_owned(),
    }
}

/// Returns the type of a form, such as `integer 1..=100`.
fn form_text(form: &Form) -> String {
    match form {
        Form::Path { absolute: false } => "path".to_owned(),
        Form::Path { absolute: true } => "absolute path".to_owned(),
        Form::Port => "port".to_owned(),
        Form::Url => "url".to_owned(),
        Form::Boolean => "boolean".to_owned(),
        Form::Integer(range) => format!("integer {}..={}", range.min(), range.max()),
        Form::Enumeration(choices) => format!("one of {choices}"),
        Form::String => "string".to_owned(),
        // A form that a later release of starmap adds.
        _ => "another type".to_owned(),
    }
}

/// Returns a variable's unset rule, such as `default dev` or `optional;
/// production required`.
fn unset_text(variable: &Variable) -> String {
    let word = |unset: &Unset| match unset {
        Unset::Required => "required".to_owned(),
        Unset::Optional => "optional".to_owned(),
        Unset::Default(_) if variable.show() == Show::Withheld => {
            format!("default {DOTS}")
        }
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
fn source_text(source: &Source) -> String {
    match source {
        Source::Setting { bind: None } => "setting".to_owned(),
        Source::Setting { bind: Some(_) } => "setting, path bind".to_owned(),
        Source::Secret { bind: None } => "secret".to_owned(),
        Source::Secret {
            bind: Some(SecretRef::OnePassword(_)),
        } => "secret, 1Password".to_owned(),
        Source::Generated { .. } => "generated".to_owned(),
        Source::Inherited => "inherited".to_owned(),
        Source::Provided(reference) => format!("provided {}", reference.kind()),
        // A source or reference that a later release of starmap adds.
        _ => "another source".to_owned(),
    }
}

/// Returns a variable's bind, such as `op://Personal/shelf mirror/token`,
/// or `None` for a variable without one.
fn bind_text(source: &Source) -> Option<String> {
    match source {
        Source::Setting {
            bind: Some(template),
        } => Some(template.as_str().to_owned()),
        Source::Secret {
            bind: Some(SecretRef::OnePassword(reference)),
        } => Some(reference.to_string()),
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
