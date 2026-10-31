//! The binds a variable's source takes: the provider of a provided value,
//! the path template of a setting, the reference of a secret, and the size
//! of a generated value.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::str::FromStr;

#[cfg(test)]
mod tests;

/// The keys that name a bind's kind in `starmap.toml`. A provided bind's
/// qualifiers may not use them.
pub(crate) const BIND_KINDS: [&str; 3] = ["provided", "path", "one-password"];

/// Whether `text` is not empty, starts with a lower case ASCII letter and
/// holds only lower case ASCII letters, digits and hyphens.
fn is_lower_word(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The kind of value a provider supplies, written `<host>/<kind>`, such as
/// `multiverse/port`.
///
/// The host and the kind each start with a lower case ASCII letter and hold
/// only lower case ASCII letters, digits and hyphens. [`FromStr`] is the
/// only way to build one.
///
/// ```
/// let kind: starmap::ProviderKind = "multiverse/port".parse()?;
/// assert_eq!(kind.as_str(), "multiverse/port");
/// # Ok::<(), starmap::ProviderKindError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderKind(String);

impl ProviderKind {
    /// Returns the kind as `<host>/<kind>`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProviderKind {
    type Err = ProviderKindError;

    /// Accepts `<host>/<kind>`, where each part fits the rule.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderKindError`] when the text does not have exactly one
    /// `/` or a part does not fit the rule.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text.split_once('/') {
            Some((host, kind)) if is_lower_word(host) && is_lower_word(kind) => {
                Ok(Self(text.to_owned()))
            }
            _ => Err(ProviderKindError(())),
        }
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The reason text is not a [`ProviderKind`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderKindError(());

impl fmt::Display for ProviderKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "a provider kind is written <host>/<kind>, and each part starts with a lower case \
             letter and holds only lower case letters, digits and hyphens",
        )
    }
}

impl Error for ProviderKindError {}

/// A provided bind: the kind of value a provider supplies, and the
/// qualifiers that tell the provider which value, such as
/// `process = "api"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidedRef {
    kind: ProviderKind,
    qualifiers: BTreeMap<String, String>,
}

impl ProvidedRef {
    /// Builds the bind of `kind` with `qualifiers`. A qualifier's name
    /// starts with a lower case ASCII letter, holds only lower case ASCII
    /// letters, digits and hyphens, and is not `provided`, `path` or
    /// `one-password`, which name a bind's kind.
    ///
    /// # Errors
    ///
    /// Returns [`QualifierError`] for the first qualifier whose name breaks
    /// the rule.
    pub fn new(
        kind: ProviderKind,
        qualifiers: BTreeMap<String, String>,
    ) -> Result<Self, QualifierError> {
        if let Some(key) = qualifiers
            .keys()
            .find(|key| BIND_KINDS.contains(&key.as_str()) || !is_lower_word(key))
        {
            return Err(QualifierError { key: key.clone() });
        }
        Ok(Self { kind, qualifiers })
    }

    /// Returns the kind of value the provider supplies.
    #[must_use]
    pub fn kind(&self) -> &ProviderKind {
        &self.kind
    }

    /// Returns the value of the qualifier `key`, if the bind has it.
    #[must_use]
    pub fn qualifier(&self, key: &str) -> Option<&str> {
        self.qualifiers.get(key).map(String::as_str)
    }

    /// Returns each qualifier's name and value, in name order.
    pub fn qualifiers(&self) -> impl Iterator<Item = (&str, &str)> {
        self.qualifiers
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }
}

/// The reason a qualifier's name does not fit a [`ProvidedRef`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifierError {
    key: String,
}

impl fmt::Display for QualifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "a provided bind has the qualifier {:?}, and a qualifier's name starts with a lower \
             case letter, holds only lower case letters, digits and hyphens, and is not \
             provided, path or one-password",
            self.key
        )
    }
}

impl Error for QualifierError {}

/// A place that a path template names, which the host fills in when it
/// resolves the template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Placeholder {
    /// `{workspace}`: the directory of the workspace the app runs in.
    Workspace,
    /// `{checkout}`: the app's checkout.
    Checkout,
    /// `{home}`: the home directory of the person who runs the app.
    Home,
}

impl Placeholder {
    /// Returns the placeholder that `name` names, without its braces.
    fn named(name: &str) -> Option<Self> {
        match name {
            "workspace" => Some(Self::Workspace),
            "checkout" => Some(Self::Checkout),
            "home" => Some(Self::Home),
            _ => None,
        }
    }
}

impl fmt::Display for Placeholder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Workspace => "{workspace}",
            Self::Checkout => "{checkout}",
            Self::Home => "{home}",
        })
    }
}

/// The path template of a setting's path bind, such as
/// `{workspace}/books.db`.
///
/// A template is not empty, and each `{` opens one of the placeholders
/// `{workspace}`, `{checkout}` and `{home}`. A brace outside a placeholder
/// is refused, so a template has no other braces. [`FromStr`] is the only
/// way to build one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathTemplate(String);

impl PathTemplate {
    /// Returns the template as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the placeholders the template names, in order.
    pub fn placeholders(&self) -> impl Iterator<Item = Placeholder> + '_ {
        self.0
            .split('{')
            .skip(1)
            .filter_map(|after| after.split_once('}'))
            .filter_map(|(name, _)| Placeholder::named(name))
    }
}

impl FromStr for PathTemplate {
    type Err = PathTemplateError;

    /// Accepts text that is not empty and whose braces each belong to one
    /// of the three placeholders.
    ///
    /// # Errors
    ///
    /// Returns [`PathTemplateError`] for empty text, an unknown placeholder,
    /// or a brace outside a placeholder.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.is_empty() {
            return Err(PathTemplateError::Empty);
        }
        let mut pieces = text.split('{');
        // The text before the first `{` holds no placeholder.
        if pieces.next().is_some_and(|before| before.contains('}')) {
            return Err(PathTemplateError::Brace);
        }
        for piece in pieces {
            let Some((name, after)) = piece.split_once('}') else {
                return Err(PathTemplateError::Brace);
            };
            if after.contains('}') {
                return Err(PathTemplateError::Brace);
            }
            if Placeholder::named(name).is_none() {
                return Err(PathTemplateError::UnknownPlaceholder {
                    name: name.to_owned(),
                });
            }
        }
        Ok(Self(text.to_owned()))
    }
}

impl fmt::Display for PathTemplate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The reason text is not a [`PathTemplate`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathTemplateError {
    /// The text is empty.
    Empty,
    /// A pair of braces names something other than `workspace`,
    /// `checkout` and `home`.
    UnknownPlaceholder {
        /// The text between the braces.
        name: String,
    },
    /// A `{` has no `}` before the next `{` or the end, or a `}` has no
    /// `{` before it.
    Brace,
}

impl fmt::Display for PathTemplateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("a path template is empty"),
            Self::UnknownPlaceholder { name } => write!(
                formatter,
                "a path template names the placeholder {{{name}}}, and the placeholders are \
                 {{workspace}}, {{checkout}} and {{home}}"
            ),
            Self::Brace => formatter.write_str(
                "a path template has a brace outside {workspace}, {checkout} and {home}",
            ),
        }
    }
}

impl Error for PathTemplateError {}

/// The reference a secret's bind names, which a resolver reads the
/// secret's value from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SecretRef {
    /// A 1Password secret reference.
    OnePassword(OnePasswordRef),
}

/// A 1Password secret reference, written
/// `op://<vault>/<item>[/<section>]/<field>`.
///
/// Each part is not empty and holds no `/` and no control character. A
/// part may hold spaces, as 1Password's names do. [`FromStr`] is the only
/// way to build one.
///
/// ```
/// let reference: starmap::OnePasswordRef = "op://Personal/shelf mirror/token".parse()?;
/// assert_eq!(reference.as_str(), "op://Personal/shelf mirror/token");
/// # Ok::<(), starmap::OnePasswordRefError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OnePasswordRef(String);

impl OnePasswordRef {
    /// Returns the reference as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for OnePasswordRef {
    type Err = OnePasswordRefError;

    /// Accepts `op://` followed by three or four parts separated by `/`,
    /// each not empty and without a control character.
    ///
    /// # Errors
    ///
    /// Returns [`OnePasswordRefError`] for any other text.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let parts = text.strip_prefix("op://").map(|rest| rest.split('/'));
        let fits = parts.is_some_and(|parts| {
            let parts: Vec<&str> = parts.collect();
            (3..=4).contains(&parts.len())
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && !part.chars().any(char::is_control))
        });
        if fits {
            Ok(Self(text.to_owned()))
        } else {
            Err(OnePasswordRefError(()))
        }
    }
}

impl fmt::Display for OnePasswordRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The reason text is not a [`OnePasswordRef`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnePasswordRefError(());

impl fmt::Display for OnePasswordRefError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "a 1password reference is op://vault/item/field or op://vault/item/section/field, \
             and each part is not empty and holds no control character",
        )
    }
}

impl Error for OnePasswordRefError {}

/// The number of random bytes in a generated value, from 16 to 1024. The
/// value is written as base64url whatever its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SecretBytes(u16);

impl SecretBytes {
    /// The fewest bytes a generated value may have.
    pub const MIN: u16 = 16;
    /// The most bytes a generated value may have.
    pub const MAX: u16 = 1024;
    /// The size of a generated value whose variable does not set `bytes`.
    pub const DEFAULT: Self = Self(32);

    /// Builds the size of `bytes` bytes.
    ///
    /// # Errors
    ///
    /// Returns [`BytesError`] when `bytes` lies outside 16 to 1024.
    pub fn new(bytes: u16) -> Result<Self, BytesError> {
        if (Self::MIN..=Self::MAX).contains(&bytes) {
            Ok(Self(bytes))
        } else {
            Err(BytesError(()))
        }
    }

    /// Returns the number of bytes.
    #[must_use]
    pub fn get(self) -> u16 {
        self.0
    }
}

impl Default for SecretBytes {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The reason a number is not [`SecretBytes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytesError(());

impl fmt::Display for BytesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "a generated value takes from {} to {} bytes",
            SecretBytes::MIN,
            SecretBytes::MAX
        )
    }
}

impl Error for BytesError {}
