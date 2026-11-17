//! The row that describes one declared variable and the value it has in
//! this shell, which `starmap explain` and `self environment list` share.

use std::fmt;

use crumbkit::schemars::{Schema, SchemaGenerator};
use crumbkit::serde::Serializer;
use crumbkit::{Row, Shown};
use schemars::JsonSchema;
use serde::Serialize;
use starmap::{
    Bindings, Bound, Cache, Choices, Empty, Mode, Name, SecretRef, Source, Spec, Type, Unset,
    Value, Variable,
};

#[cfg(test)]
mod tests;

/// What a withheld value prints as, whatever its length, so the output
/// does not reveal how long the secret is.
const DOTS: &str = "••••••••";

/// Writes `Explained`, the row of `starmap explain`, and `Listed`, the row of
/// `self environment list`, from one list of fields. `Listed` has the same
/// fields and `owner` after them, so the two documents describe a variable
/// alike and `starmap explain` never has a field that it does not send.
macro_rules! row_types {
    ($($(#[$meta:meta])* $field:ident: $ty:ty,)*) => {
        /// One declared variable, with its rules and the value it has in this
        /// shell.
        #[derive(Debug, Serialize, JsonSchema, Row)]
        pub struct Explained {
            $($(#[$meta])* $field: $ty,)*
        }

        /// One variable of `self environment list`: the fields of
        /// [`Explained`] and the library that owns the variable.
        #[derive(Debug, Serialize, JsonSchema, Row)]
        pub struct Listed {
            $($(#[$meta])* $field: $ty,)*
            /// The library that reads the variable in every tool, such as `crumbkit`; empty, and left out of `--json`, for a variable that the tool's own declaration holds.
            #[serde(skip_serializing_if = "Option::is_none")]
            #[row(column)]
            owner: Option<String>,
        }

        impl Explained {
            /// Returns the row of `self environment list` for this variable,
            /// owned by the library `owner` names, or by the tool itself
            /// without one.
            #[must_use]
            pub fn listed(self, owner: Option<&str>) -> Listed {
                let Self { $($field,)* } = self;
                Listed {
                    $($field,)*
                    owner: owner.map(str::to_owned),
                }
            }
        }
    };
}

row_types! {
    /// The variable's name.
    #[row(column, uncut)]
    name: String,
    /// The type of the variable's value, such as port or integer 1..=100.
    #[serde(rename = "type")]
    #[row(column)]
    kind: String,
    /// The one word that names the type, such as `port`, `enumeration` or `url`; `boolean` for a variable that reads its presence, and `other` for a type a later starmap adds. The `type` text holds the range, pattern, choices and empty-value note.
    type_name: TypeName,
    /// What happens when the variable has no value, in local mode and, when it differs, in production.
    #[row(column)]
    unset: String,
    /// The unset rule as an object: the rule in local mode and, when it differs, the rule in production.
    unset_rule: UnsetRule,
    /// Where the variable's value comes from, with the kind of its bind.
    #[row(column)]
    source: String,
    /// The source as an object: its kind and, when it has a bind, the bind's kind.
    source_kind: SourceKind,
    /// The variable's value in this shell, or its default; a withheld value prints as eight dots and has no text in JSON.
    #[row(column)]
    value: Cell,
    /// Whether the value may be shown or is withheld.
    #[row(long)]
    show: String,
    /// The variable's bind, such as a 1Password reference, when it has one.
    #[row(long)]
    bind: Option<String>,
    /// `never` when the bind keeps the value out of any cache of resolved values; empty, and left out of `--json`, when a cache may keep it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[row(long)]
    cache: Option<String>,
    /// The sentence that says what the variable is for.
    #[row(long)]
    purpose: String,
    /// The sentence that says what the application does when the variable is unset; empty, and left out of `--json`, when the spec states none.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[row(long)]
    effect: Option<String>,
    /// The name of the named enumeration that the variable's type is; left out of `--json` for any other type.
    #[serde(skip_serializing_if = "Option::is_none")]
    enumeration: Option<String>,
    /// Each value of a named enumeration with its description; empty, and left out of `--json`, for any other type.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[row(long)]
    values: Option<Values>,
    /// The sentence that states the check the application's own code makes of the value, which starmap never runs; empty, and left out of `--json`, when the spec states none.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[row(long)]
    check: Option<String>,
    /// The name of the group of variables that are set together or not at all, which the variable belongs to; empty, and left out of `--json`, when it belongs to none.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[row(long)]
    group: Option<String>,
    /// Where the value comes from: `set` when this shell sets the variable, `default` when it leaves the variable unset and the default applies, `unset` when the variable has no value, and `invalid` when this shell sets a value that does not fit the type. The same state as `value.state`, at the top level.
    #[row(long)]
    state: State,
}

/// The value of a variable as explain shows it. Its text is the value, the
/// default with `(default)`, `unset`, or eight dots for a withheld value.
/// Its JSON is an object with the state and, for a shown value, the text.
#[derive(Debug)]
pub struct Cell(CellJson);

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
pub enum State {
    /// This shell sets the variable.
    Set,
    /// This shell leaves the variable unset, and its default applies.
    Default,
    /// The variable has no value.
    Unset,
    /// This shell sets the variable to a value that does not fit its type.
    Invalid,
}

impl fmt::Display for State {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Set => "set",
            Self::Default => "default",
            Self::Unset => "unset",
            Self::Invalid => "invalid",
        })
    }
}

impl Cell {
    /// Returns the cell of a value in `state`, whose text is `text`, or
    /// none when the variable is withheld.
    fn new(state: State, text: Option<String>) -> Self {
        Self(CellJson { state, text })
    }

    /// Returns where the value comes from.
    #[must_use]
    pub const fn state(&self) -> State {
        self.0.state
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
pub struct Values(Vec<Described>);

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
    #[must_use]
    pub fn of(ty: &Type) -> Option<Self> {
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

/// Returns the row of the variable `name` of `spec`, with the value that
/// `bindings` give it, or `None` when `spec` does not declare `name`.
///
/// This is the one place that builds the row of `starmap explain` and of
/// `self environment list`, so the two documents describe a variable alike.
/// It resolves nothing: a bind is named and never read.
#[must_use]
pub fn row(spec: &Spec, bindings: &Bindings, name: Name) -> Option<Explained> {
    let variable = spec.variable(&name)?;
    let value = cell(variable, bindings.get(&name));
    Some(Explained {
        state: value.state(),
        kind: explained_kind(variable),
        type_name: TypeName::of(variable),
        unset: unset_text(variable),
        unset_rule: UnsetRule::of(variable),
        source: source_text(variable.source()),
        source_kind: SourceKind::of(variable.source()),
        value,
        show: variable.show().to_string(),
        bind: bind_text(variable.source()),
        cache: cache_text(variable.source()).map(str::to_owned),
        purpose: variable.purpose().to_owned(),
        effect: variable.effect().map(str::to_owned),
        check: variable.check().map(str::to_owned),
        enumeration: enumeration_name(variable.value_type()),
        values: Values::of(variable.value_type()),
        group: spec.group_of(&name).map(|group| group.name().to_string()),
        name: name.as_str().to_owned(),
    })
}

impl Explained {
    /// Returns the variable's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns where the variable's value comes from in this shell.
    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }
}

/// Returns the cell of `variable`'s value, which `bound` gives when this
/// shell sets it. A value that counts as unset, such as a relative path
/// where the type takes an absolute one, falls back to the default.
/// A shell value that is not Unicode is shown with each invalid sequence
/// replaced by U+FFFD. Any value sets a variable that reads its presence,
/// an empty one included.
fn cell(variable: &Variable, bound: Option<&Bound>) -> Cell {
    match bound {
        Some(Bound::Plain(raw)) if variable.presence() => {
            return Cell::new(State::Set, Some(raw.to_string_lossy().into_owned()));
        }
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
#[must_use]
pub fn value_text(value: &Value) -> String {
    match value {
        Value::Path(path) => path.display().to_string(),
        Value::Port(port) => port.to_string(),
        Value::Url(url) => url.to_string(),
        Value::SocketAddress(address) => address.to_string(),
        Value::Boolean(boolean) => boolean.to_string(),
        Value::Integer(number) => number.to_string(),
        Value::Choice(choice) => choice.to_string(),
        Value::String(string) => string.clone(),
        Value::Bytes { .. } | Value::Spelled { .. } => value.to_string(),
        // A kind of value that a later release of starmap adds.
        _ => "a value".to_owned(),
    }
}

/// Returns the text of a variable's type as explain prints it: the
/// [`kind_text`], followed by `; an empty value is a value` when the
/// variable declares `empty = "value"`.
fn explained_kind(variable: &Variable) -> String {
    let kind = kind_text(variable);
    match empty_text(variable) {
        Some(empty) => format!("{kind}; {empty}"),
        None => kind,
    }
}

/// Returns `an empty value is a value` when `variable` declares
/// `empty = "value"`, and `None` otherwise. A variable that reads its
/// presence counts an empty value as a value too, and its type text says
/// so already, so it has no empty text.
#[must_use]
pub fn empty_text(variable: &Variable) -> Option<&'static str> {
    (variable.empty() == Empty::Value && !variable.presence())
        .then_some("an empty value is a value")
}

/// Returns the text of a variable's type, which says so when the variable
/// reads its presence.
#[must_use]
pub fn kind_text(variable: &Variable) -> String {
    if variable.presence() {
        "boolean (true when set to any value)".to_owned()
    } else {
        type_text(variable.value_type())
    }
}

/// Returns the text of a type, such as `integer 1..=100`.
fn type_text(ty: &Type) -> String {
    match ty {
        Type::Path { absolute: false } => "path".to_owned(),
        Type::Path { absolute: true } => "absolute path".to_owned(),
        Type::Port => "port".to_owned(),
        Type::Url { schemes: None } => "url".to_owned(),
        Type::Url {
            schemes: Some(schemes),
        } => format!("url ({schemes})"),
        Type::SocketAddress => "ip socket address".to_owned(),
        Type::Boolean { spellings: None } => "boolean".to_owned(),
        Type::Boolean {
            spellings: Some(spellings),
        } => format!("boolean ({spellings})"),
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

/// The one word that names a variable's type, from a closed list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TypeName {
    /// A path, relative or absolute.
    Path,
    /// A TCP or UDP port.
    Port,
    /// A URL, with or without a list of schemes.
    Url,
    /// An IP socket address.
    SocketAddress,
    /// A boolean, with or without its own spellings, or a variable that
    /// reads its presence.
    Boolean,
    /// An integer in a range.
    Integer,
    /// One of a list of choices, named or inline.
    Enumeration,
    /// A string, with or without a pattern.
    String,
    /// Bytes in an encoding.
    Bytes,
    /// A type that a later release of starmap adds.
    Other,
}

impl TypeName {
    /// Returns the type name of `variable`, which is `boolean` when the
    /// variable reads its presence, as [`kind_text`] says.
    #[must_use]
    pub(crate) fn of(variable: &Variable) -> Self {
        if variable.presence() {
            return Self::Boolean;
        }
        match variable.value_type() {
            Type::Path { .. } => Self::Path,
            Type::Port => Self::Port,
            Type::Url { .. } => Self::Url,
            Type::SocketAddress => Self::SocketAddress,
            Type::Boolean { .. } => Self::Boolean,
            Type::Integer(_) => Self::Integer,
            Type::Enumeration(_) => Self::Enumeration,
            Type::String { .. } => Self::String,
            Type::Bytes(_) => Self::Bytes,
            // A type that a later release of starmap adds, whose text is
            // `another type`.
            _ => Self::Other,
        }
    }
}

/// A variable's unset rule as an object. Its JSON holds the local rule and,
/// only when it differs, the production rule, as the `unset` text does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub(crate) struct UnsetRule {
    /// The rule in local mode.
    local: Rule,
    /// The rule in production mode; left out when it equals the local rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    production: Option<Rule>,
}

/// What happens when a variable has no value in one mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "rule", rename_all = "lowercase")]
pub(crate) enum Rule {
    /// The application refuses to start.
    Required,
    /// The application runs without the value.
    Optional,
    /// The default applies.
    Default {
        /// The default's text, as an environment would hold it.
        default: String,
    },
}

impl Rule {
    /// Returns the rule of `unset`.
    fn of(unset: &Unset) -> Self {
        match unset {
            Unset::Required => Self::Required,
            Unset::Optional => Self::Optional,
            Unset::Default(value) => Self::Default {
                default: value_text(value),
            },
        }
    }
}

impl UnsetRule {
    /// Returns the unset rule of `variable`.
    #[must_use]
    pub(crate) fn of(variable: &Variable) -> Self {
        let local = variable.unset().in_mode(Mode::Local);
        let production = variable.unset().in_mode(Mode::Production);
        Self {
            local: Rule::of(local),
            production: (local != production).then(|| Rule::of(production)),
        }
    }
}

/// A variable's source as an object: its kind and the kind of its bind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub(crate) struct SourceKind {
    /// Where the value comes from.
    kind: SourceWord,
    /// The kind of the variable's bind, such as `path`, `1Password` or
    /// `multiverse/port`; left out for a variable without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    bind: Option<String>,
}

/// The one word that names where a variable's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SourceWord {
    /// A setting that the person or the deployment chooses.
    Setting,
    /// A secret.
    Secret,
    /// Random bytes that a tool generates.
    Generated,
    /// A value that the operating system or the shell sets.
    Inherited,
    /// A value that a provider supplies.
    Provided,
    /// A source that a later release of starmap adds.
    Other,
}

impl SourceKind {
    /// Returns the source kind of `source`, which agrees with
    /// [`source_text`].
    #[must_use]
    pub(crate) fn of(source: &Source) -> Self {
        let (kind, bind) = match source {
            Source::Setting { bind } => (
                SourceWord::Setting,
                bind.as_ref().map(|_| "path".to_owned()),
            ),
            Source::Secret { bind } => (
                SourceWord::Secret,
                bind.as_ref().and_then(|bind| match bind.reference() {
                    SecretRef::OnePassword(_) => Some("1Password".to_owned()),
                    // A reference that a later release of starmap adds.
                    _ => None,
                }),
            ),
            Source::Generated { .. } => (SourceWord::Generated, None),
            Source::Inherited => (SourceWord::Inherited, None),
            Source::Provided(reference) => {
                (SourceWord::Provided, Some(reference.kind().to_string()))
            }
            // A source that a later release of starmap adds.
            _ => (SourceWord::Other, None),
        };
        Self { kind, bind }
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
#[must_use]
pub fn unset_text(variable: &Variable) -> String {
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
#[must_use]
pub fn source_text(source: &Source) -> String {
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
#[must_use]
pub fn cache_text(source: &Source) -> Option<&'static str> {
    match source {
        Source::Secret { bind: Some(bind) } if bind.cache() == Cache::Never => Some("never"),
        _ => None,
    }
}

/// Returns a variable's bind, such as `op://Personal/shelf mirror/token`,
/// or `None` for a variable without one.
#[must_use]
pub fn bind_text(source: &Source) -> Option<String> {
    match source {
        Source::Setting {
            bind: Some(template),
        } => Some(template.as_str().to_owned()),
        Source::Secret { bind: Some(bind) } => match bind.reference() {
            SecretRef::OnePassword(reference) => Some(reference.to_string()),
            // A reference that a later release of starmap adds.
            _ => None,
        },
        Source::Generated { bytes, encoding } => {
            Some(format!("{} random bytes as {encoding}", bytes.get()))
        }
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
