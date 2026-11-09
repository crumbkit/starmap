//! The type a variable's value has, the types that check each type, and
//! the value a type reads.

use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::net::SocketAddr;
use std::num::{IntErrorKind, NonZeroU16};
use std::path::PathBuf;
use std::str::FromStr;

use url::Url;

#[cfg(test)]
mod tests;

/// The words the boolean type reads as `true`.
const TRUE_WORDS: [&str; 4] = ["1", "true", "yes", "on"];
/// The words the boolean type reads as `false`.
const FALSE_WORDS: [&str; 4] = ["0", "false", "no", "off"];

/// The type of a variable's value, which a variable's `type` field in
/// `starmap.toml` states.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Type {
    /// A file system path. With `absolute`, a relative path counts as unset,
    /// as the XDG Base Directory rule says of its variables.
    Path {
        /// Whether a relative path counts as unset.
        absolute: bool,
    },
    /// A TCP or UDP port from 1 to 65535.
    Port,
    /// An absolute URL.
    Url,
    /// An IP socket address, such as `0.0.0.0:8080` or `[::1]:8080`, as
    /// [`SocketAddr`]'s `FromStr` reads it. It accepts port 0 and a numeric
    /// IPv6 zone id, such as `[fe80::1%1]:80`. A host name such as
    /// `localhost:8080` and a named zone id such as `[fe80::1%eth0]:80` are
    /// refused.
    SocketAddress,
    /// One of the words 1, 0, true, false, yes, no, on and off, in any mix
    /// of ASCII case.
    Boolean,
    /// A whole number within a range.
    Integer(IntegerRange),
    /// One of a fixed set of values.
    Enumeration(Choices),
    /// Any text, which need not be Unicode.
    String,
}

impl Type {
    /// Reads `raw` in this type.
    ///
    /// `Ok(None)` means the value counts as unset, which happens only for a
    /// relative path, including an empty one, where `absolute` is true. The
    /// path and string types read a value that is not Unicode; every other
    /// type needs Unicode text. A value is never trimmed, so a space around
    /// a word makes it a different word.
    ///
    /// # Errors
    ///
    /// Returns [`TypeError`] when the value does not fit the type.
    pub fn parse(&self, raw: &OsStr) -> Result<Option<Value>, TypeError> {
        let value = match self {
            Self::Path { absolute } => {
                let path = PathBuf::from(raw);
                if *absolute && !path.is_absolute() {
                    return Ok(None);
                }
                Value::Path(path)
            }
            Self::String => Value::String(raw.to_os_string()),
            Self::Port => Value::Port(text(raw)?.parse().map_err(|_| TypeError::NotPort)?),
            Self::Url => Value::Url(Url::parse(text(raw)?).map_err(TypeError::NotUrl)?),
            Self::SocketAddress => Value::SocketAddress(
                text(raw)?
                    .parse()
                    .map_err(|_| TypeError::NotSocketAddress)?,
            ),
            Self::Boolean => Value::Boolean(boolean(text(raw)?)?),
            Self::Integer(range) => Value::Integer(range.parse(text(raw)?)?),
            Self::Enumeration(choices) => {
                let text = text(raw)?;
                if text.is_empty() {
                    return Err(TypeError::EmptyChoice(choices.clone()));
                }
                let choice = choices
                    .get(text)
                    .ok_or_else(|| TypeError::NotChoice(choices.clone()))?;
                Value::Choice(choice.clone())
            }
        };
        Ok(Some(value))
    }
}

impl fmt::Display for Type {
    /// Writes what a value of the type is, such as `a port` or `a whole
    /// number from 1 to 100`, as a program's `--help` describes a variable.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path { absolute: false } => formatter.write_str("a path"),
            Self::Path { absolute: true } => formatter.write_str("an absolute path"),
            Self::Port => formatter.write_str("a port"),
            Self::Url => formatter.write_str("a url"),
            Self::SocketAddress => formatter.write_str("an ip socket address"),
            Self::Boolean => formatter.write_str("a boolean"),
            Self::Integer(range) => write!(
                formatter,
                "a whole number from {} to {}",
                range.min, range.max
            ),
            Self::Enumeration(choices) => write!(formatter, "one of {choices}"),
            Self::String => formatter.write_str("a string"),
        }
    }
}

/// Returns `raw` as Unicode text.
fn text(raw: &OsStr) -> Result<&str, TypeError> {
    raw.to_str().ok_or(TypeError::NotUnicode)
}

/// Reads one of the eight boolean words, in any mix of ASCII case.
fn boolean(text: &str) -> Result<bool, TypeError> {
    let is = |words: [&str; 4]| words.iter().any(|word| word.eq_ignore_ascii_case(text));
    if is(TRUE_WORDS) {
        Ok(true)
    } else if is(FALSE_WORDS) {
        Ok(false)
    } else {
        Err(TypeError::NotBoolean)
    }
}

/// A value that a [`Type`] has read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Value {
    /// The value of a path type.
    Path(PathBuf),
    /// The value of the port type.
    Port(Port),
    /// The value of the URL type.
    Url(Url),
    /// The value of the socket-address type.
    SocketAddress(SocketAddr),
    /// The value of the boolean type.
    Boolean(bool),
    /// The value of an integer type, which lies in the type's range.
    Integer(i64),
    /// The value of an enumeration type, which is one of the type's choices.
    Choice(Choice),
    /// The value of the string type.
    String(OsString),
}

impl Value {
    /// Returns the value as the text an environment holds, which its type
    /// reads back as an equal value.
    pub(crate) fn to_os_string(&self) -> OsString {
        match self {
            Self::Path(path) => path.clone().into_os_string(),
            Self::String(string) => string.clone(),
            Self::Port(port) => port.to_string().into(),
            Self::Url(url) => url.as_str().into(),
            Self::SocketAddress(address) => address.to_string().into(),
            Self::Boolean(boolean) => boolean.to_string().into(),
            Self::Integer(number) => number.to_string().into(),
            Self::Choice(choice) => choice.as_str().into(),
        }
    }
}

impl fmt::Display for Value {
    /// Writes the text an environment holds, with each part that is not
    /// Unicode replaced by U+FFFD.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_os_string().to_string_lossy())
    }
}

/// The reason a value does not fit its [`Type`]. The text names the type's
/// rule and never repeats the value, which may be a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TypeError {
    /// The type needs Unicode text, and the value is not Unicode.
    NotUnicode,
    /// The value is not a port from 1 to 65535.
    NotPort,
    /// The value is not a URL, for the reason the parse error gives.
    NotUrl(url::ParseError),
    /// The value is not an IP socket address.
    NotSocketAddress,
    /// The value is not one of the eight boolean words.
    NotBoolean,
    /// The value is not a whole number.
    NotInteger,
    /// The value is a whole number outside the type's range.
    OutOfRange {
        /// The smallest number the type accepts.
        min: i64,
        /// The largest number the type accepts.
        max: i64,
    },
    /// The value is not one of the type's choices.
    NotChoice(Choices),
    /// The value is empty, and the type takes one of these choices.
    EmptyChoice(Choices),
}

impl fmt::Display for TypeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUnicode => formatter.write_str("the value is not unicode text"),
            Self::NotPort => formatter.write_str("the value is not a port from 1 to 65535"),
            Self::NotUrl(_) => formatter.write_str("the value is not a url"),
            Self::NotSocketAddress => formatter.write_str(
                "the value is not an ip socket address, such as 127.0.0.1:8080 or [::1]:8080",
            ),
            Self::NotBoolean => formatter.write_str(
                "the value is not one of the words 1, 0, true, false, yes, no, on and off",
            ),
            Self::NotInteger => formatter.write_str("the value is not a whole number"),
            Self::OutOfRange { min, max } => write!(
                formatter,
                "the value is not a whole number from {min} to {max}"
            ),
            Self::NotChoice(choices) => write!(formatter, "the value is not one of {choices}"),
            Self::EmptyChoice(choices) => {
                write!(
                    formatter,
                    "the value is empty, and it must be one of {choices}"
                )
            }
        }
    }
}

impl Error for TypeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::NotUrl(error) => Some(error),
            _ => None,
        }
    }
}

/// A TCP or UDP port from 1 to 65535.
///
/// ```
/// let port: starmap::Port = "8080".parse()?;
/// assert_eq!(port.get(), 8080);
/// # Ok::<(), starmap::PortError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Port(NonZeroU16);

impl Port {
    /// Returns the port's number.
    #[must_use]
    pub fn get(self) -> u16 {
        self.0.get()
    }
}

impl TryFrom<u16> for Port {
    type Error = PortError;

    /// Accepts every number except 0.
    ///
    /// # Errors
    ///
    /// Returns [`PortError`] for 0.
    fn try_from(number: u16) -> Result<Self, Self::Error> {
        NonZeroU16::new(number).map(Self).ok_or(PortError(()))
    }
}

impl FromStr for Port {
    type Err = PortError;

    /// Accepts one or more ASCII digits whose number lies from 1 to 65535.
    /// A sign or a space is refused.
    ///
    /// # Errors
    ///
    /// Returns [`PortError`] for any other text.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(PortError(()));
        }
        let number: u16 = text.parse().map_err(|_| PortError(()))?;
        Self::try_from(number)
    }
}

impl From<Port> for u16 {
    fn from(port: Port) -> Self {
        port.get()
    }
}

impl fmt::Display for Port {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The reason a number or text is not a [`Port`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortError(());

impl fmt::Display for PortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a port is a whole number from 1 to 65535")
    }
}

impl Error for PortError {}

/// The range of an integer type, from its minimum to its maximum, both
/// included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntegerRange {
    min: i64,
    max: i64,
}

impl IntegerRange {
    /// Builds the range from `min` to `max`, both included.
    ///
    /// # Errors
    ///
    /// Returns [`RangeError`] when `min` is above `max`.
    pub fn new(min: i64, max: i64) -> Result<Self, RangeError> {
        if min > max {
            return Err(RangeError { min, max });
        }
        Ok(Self { min, max })
    }

    /// Returns the smallest number in the range.
    #[must_use]
    pub fn min(self) -> i64 {
        self.min
    }

    /// Returns the largest number in the range.
    #[must_use]
    pub fn max(self) -> i64 {
        self.max
    }

    /// Returns whether `number` lies in the range.
    pub(crate) fn contains(self, number: i64) -> bool {
        (self.min..=self.max).contains(&number)
    }

    /// Reads a whole number in the range: an optional `+` or `-` followed
    /// by ASCII digits. A number beyond the 64 bits of `i64` lies outside
    /// every range.
    fn parse(self, text: &str) -> Result<i64, TypeError> {
        let out_of_range = TypeError::OutOfRange {
            min: self.min,
            max: self.max,
        };
        let number = i64::from_str(text).map_err(|error| match error.kind() {
            IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => out_of_range.clone(),
            _ => TypeError::NotInteger,
        })?;
        if self.contains(number) {
            Ok(number)
        } else {
            Err(out_of_range)
        }
    }
}

/// The reason two bounds do not make an [`IntegerRange`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeError {
    min: i64,
    max: i64,
}

impl RangeError {
    /// Returns the minimum that was given.
    #[must_use]
    pub fn min(&self) -> i64 {
        self.min
    }

    /// Returns the maximum that was given, which is below the minimum.
    #[must_use]
    pub fn max(&self) -> i64 {
        self.max
    }
}

impl fmt::Display for RangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "an integer range's minimum {} is above its maximum {}",
            self.min, self.max
        )
    }
}

impl Error for RangeError {}

/// One value of an enumeration type: text that is not empty and holds no
/// whitespace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Choice(String);

impl Choice {
    /// Returns the choice as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Choice {
    type Err = ChoiceError;

    /// Accepts text that is not empty and holds no whitespace, as
    /// [`char::is_whitespace`] defines it.
    ///
    /// # Errors
    ///
    /// Returns [`ChoiceError`] for empty text or text with whitespace.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.is_empty() {
            return Err(ChoiceError::Empty);
        }
        if let Some((at, _)) = text.char_indices().find(|(_, c)| c.is_whitespace()) {
            return Err(ChoiceError::Whitespace { at });
        }
        Ok(Self(text.to_owned()))
    }
}

impl fmt::Display for Choice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The reason text is not a [`Choice`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChoiceError {
    /// The text is empty.
    Empty,
    /// The text holds whitespace.
    Whitespace {
        /// The byte offset of the first whitespace character.
        at: usize,
    },
}

impl fmt::Display for ChoiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("an enumeration value is empty"),
            Self::Whitespace { at } => write!(
                formatter,
                "an enumeration value holds whitespace at byte {at}"
            ),
        }
    }
}

impl Error for ChoiceError {}

/// The values of an enumeration type: at least one [`Choice`], none of them
/// repeated, in the order the spec lists them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Choices(Vec<Choice>);

impl Choices {
    /// Builds the set from `choices`, keeping their order.
    ///
    /// # Errors
    ///
    /// Returns [`ChoicesError`] when there is no choice or one repeats.
    pub fn new(choices: impl IntoIterator<Item = Choice>) -> Result<Self, ChoicesError> {
        let mut kept: Vec<Choice> = Vec::new();
        for choice in choices {
            if kept.contains(&choice) {
                return Err(ChoicesError::Repeated(choice));
            }
            kept.push(choice);
        }
        if kept.is_empty() {
            return Err(ChoicesError::Empty);
        }
        Ok(Self(kept))
    }

    /// Returns the choices in order.
    pub fn iter(&self) -> impl Iterator<Item = &Choice> {
        self.0.iter()
    }

    /// Returns the choice that equals `text` exactly, if there is one.
    pub(crate) fn get(&self, text: &str) -> Option<&Choice> {
        self.0.iter().find(|choice| choice.as_str() == text)
    }
}

impl fmt::Display for Choices {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, choice) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            formatter.write_str(choice.as_str())?;
        }
        Ok(())
    }
}

/// The reason a list of values is not [`Choices`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChoicesError {
    /// The list is empty.
    Empty,
    /// The list holds this choice more than once.
    Repeated(Choice),
}

impl fmt::Display for ChoicesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("an enumeration has no values"),
            Self::Repeated(choice) => {
                write!(formatter, "an enumeration lists the value {choice} twice")
            }
        }
    }
}

impl Error for ChoicesError {}
