//! The type a variable's value has, the checking types that back it
//! ([`Port`], [`IntegerRange`], [`Choices`], [`Choice`], [`Pattern`],
//! [`Schemes`], [`Scheme`] and [`ByteRule`]), and the value a type reads.

use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::net::SocketAddr;
use std::num::{IntErrorKind, NonZeroU16};
use std::path::PathBuf;
use std::str::FromStr;

use base64::Engine as _;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, STANDARD};
use regex::Regex;
use url::Url;
use zeroize::Zeroize as _;

use crate::ByteCount;

use crate::Secret;

mod scheme;
#[cfg(test)]
mod tests;

pub use scheme::{Scheme, SchemeError, Schemes, SchemesError};

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
    /// An absolute URL. With `schemes`, its scheme must be one of them.
    Url {
        /// The schemes the URL may have, or `None` for any scheme.
        schemes: Option<Schemes>,
    },
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
    /// Any text, which need not be Unicode. With a pattern, Unicode text
    /// that the pattern matches as a whole.
    String {
        /// The pattern the whole value must match, if the type has one.
        pattern: Option<Pattern>,
    },
    /// Bytes written as text in an encoding, whose decoded length fits a
    /// rule.
    Bytes(ByteRule),
}

impl Type {
    /// Reads `raw` in this type.
    ///
    /// `Ok(None)` means the value counts as unset, which happens only for a
    /// relative path, including an empty one, where `absolute` is true. The
    /// path type and a string type without a pattern read a value that is
    /// not Unicode; every other type needs Unicode text. A value is never
    /// trimmed, so a space around a word makes it a different word.
    ///
    /// [`check`](crate::check) treats an empty value as unset before any
    /// type reads it, so this method sees an empty value only when a caller
    /// passes one directly. It then reads the empty value in the type as
    /// it reads any other value.
    ///
    /// # Errors
    ///
    /// Returns [`TypeError`] when the value does not fit the type. A bytes
    /// value that does not decode fails with [`TypeError::NotEncoded`], and
    /// one of the wrong size with [`TypeError::WrongLength`].
    pub fn parse(&self, raw: &OsStr) -> Result<Option<Value>, TypeError> {
        let value = match self {
            Self::Path { absolute } => {
                let path = PathBuf::from(raw);
                if *absolute && !path.is_absolute() {
                    return Ok(None);
                }
                Value::Path(path)
            }
            Self::String { pattern } => {
                if let Some(pattern) = pattern {
                    pattern.check(text(raw)?)?;
                }
                Value::String(raw.to_os_string())
            }
            Self::Port => Value::Port(text(raw)?.parse().map_err(|_| TypeError::NotPort)?),
            Self::Url { schemes } => {
                let url = Url::parse(text(raw)?).map_err(TypeError::NotUrl)?;
                // The url crate writes every scheme it parses in lower case.
                if let Some(schemes) = schemes
                    && !schemes.contains(url.scheme())
                {
                    return Err(TypeError::SchemeNotAllowed(schemes.clone()));
                }
                Value::Url(url)
            }
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
            Self::Bytes(rule) => Value::Bytes {
                bytes: rule.decode(text(raw)?)?,
                encoding: rule.encoding(),
            },
        };
        Ok(Some(value))
    }
}

impl Type {
    /// Checks a withheld value against this type without its text leaving
    /// the secret. A string type with a pattern refuses a value the pattern
    /// does not match as a whole, and the refusal names only the pattern. A
    /// bytes type refuses a value that does not decode or has the wrong
    /// size, and the refusal names only the encoding or the length; the
    /// decoded bytes are zeroed when the check ends. The spec parser gives a
    /// withheld variable only the string and bytes types, so every other
    /// type accepts the secret.
    ///
    /// # Errors
    ///
    /// Returns [`TypeError::NoMatch`] when the pattern does not match,
    /// [`TypeError::NotEncoded`] when a bytes value does not decode, and
    /// [`TypeError::WrongLength`] when it decodes to the wrong size.
    pub fn check_secret(&self, secret: &Secret) -> Result<(), TypeError> {
        match self {
            Self::String {
                pattern: Some(pattern),
            } => secret.check(pattern),
            Self::Bytes(rule) => secret.decode(*rule).map(drop),
            Self::String { pattern: None }
            | Self::Path { .. }
            | Self::Port
            | Self::Url { .. }
            | Self::SocketAddress
            | Self::Boolean
            | Self::Integer(_)
            | Self::Enumeration(_) => Ok(()),
        }
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
            Self::Url { schemes: None } => formatter.write_str("a url"),
            Self::Url {
                schemes: Some(schemes),
            } => write!(formatter, "a url whose scheme is one of {schemes}"),
            Self::SocketAddress => formatter.write_str("an ip socket address"),
            Self::Boolean => formatter.write_str("a boolean"),
            Self::Integer(range) => write!(
                formatter,
                "a whole number from {} to {}",
                range.min, range.max
            ),
            Self::Enumeration(choices) => write!(formatter, "one of {choices}"),
            Self::String { pattern: None } => formatter.write_str("a string"),
            Self::String {
                pattern: Some(pattern),
            } => write!(formatter, "a string matching {pattern}"),
            Self::Bytes(rule) => rule.fmt(formatter),
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
    /// The value of a bytes type of a shown variable, decoded from its
    /// encoding.
    Bytes {
        /// The decoded bytes.
        bytes: Vec<u8>,
        /// The encoding the value is written in.
        encoding: Encoding,
    },
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
            Self::Bytes { bytes, encoding } => encoding.encode(bytes).into(),
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
    /// The value is a URL whose scheme is not one of these.
    SchemeNotAllowed(Schemes),
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
    /// The value is not text in this encoding.
    NotEncoded(Encoding),
    /// The value decodes to a number of bytes that this length refuses.
    WrongLength(ByteLength),
    /// The value does not match the string type's pattern as a whole.
    NoMatch(Pattern),
}

impl fmt::Display for TypeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUnicode => formatter.write_str("the value is not unicode text"),
            Self::NotPort => formatter.write_str("the value is not a port from 1 to 65535"),
            Self::NotUrl(_) => formatter.write_str("the value is not a url"),
            Self::SchemeNotAllowed(schemes) => write!(
                formatter,
                "the value is a url, and its scheme is not one of {schemes}"
            ),
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
            Self::NotEncoded(encoding) => write!(formatter, "the value is not {encoding}"),
            Self::WrongLength(length) => {
                write!(formatter, "the value does not decode to {length}")
            }
            Self::NoMatch(pattern) => {
                write!(formatter, "the value does not match the pattern {pattern}")
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
        let kept = distinct(choices).map_err(ChoicesError::Repeated)?;
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
        write_list(formatter, &self.0)
    }
}

/// Returns `items` in their order, or the first item that repeats an
/// earlier one.
fn distinct<T: PartialEq>(items: impl IntoIterator<Item = T>) -> Result<Vec<T>, T> {
    let mut kept: Vec<T> = Vec::new();
    for item in items {
        if kept.contains(&item) {
            return Err(item);
        }
        kept.push(item);
    }
    Ok(kept)
}

/// Writes `items` separated by a comma and a space.
fn write_list<T: fmt::Display>(formatter: &mut fmt::Formatter<'_>, items: &[T]) -> fmt::Result {
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            formatter.write_str(", ")?;
        }
        item.fmt(formatter)?;
    }
    Ok(())
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

/// The encoding of a bytes type's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Encoding {
    /// Two hexadecimal digits per byte, in upper or lower case. It is
    /// written in lower case.
    Hex,
    /// Base64 as RFC 4648 section 4 defines it, with its padding required.
    Base64,
    /// Base64 with the URL and file name alphabet, as RFC 4648 section 5
    /// defines it, written either with all its padding or with none.
    /// Partly padded text is refused. It is written without padding.
    Base64Url,
}

/// The base64url engine for unpadded text, which also writes text without
/// padding.
const BASE64_URL: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::URL_SAFE,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::RequireNone),
);

/// The base64url engine for text that ends in `=`, which requires the full
/// padding, so partly padded text is refused.
const BASE64_URL_PADDED: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::RequireCanonical),
);

impl Encoding {
    /// Returns the bytes that `text` decodes to, or `None` when it is not
    /// text in the encoding. Whitespace is refused in each encoding. The
    /// buffer is zeroed before it is dropped on a refusal, because the
    /// text may be a secret.
    fn decode(self, text: &str) -> Option<Vec<u8>> {
        let engine = match self {
            Self::Hex => return hex(text),
            Self::Base64 => &STANDARD,
            Self::Base64Url if text.ends_with('=') => &BASE64_URL_PADDED,
            Self::Base64Url => &BASE64_URL,
        };
        // The buffer is allocated once at its largest size, so decoding
        // into it never moves the bytes and leaves an unzeroed copy.
        let mut bytes = vec![0; base64::decoded_len_estimate(text.len())];
        match engine.decode_slice(text, &mut bytes) {
            Ok(length) => {
                // The bytes past the decoded length are zeroed before the
                // Vec forgets them.
                bytes.get_mut(length..).unwrap_or_default().fill(0);
                bytes.truncate(length);
                Some(bytes)
            }
            Err(_) => {
                bytes.zeroize();
                None
            }
        }
    }

    /// Writes `bytes` in the encoding's one spelling: lower case hex, padded
    /// base64 or unpadded base64url.
    pub(crate) fn encode(self, bytes: &[u8]) -> String {
        match self {
            Self::Hex => bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
            Self::Base64 => STANDARD.encode(bytes),
            Self::Base64Url => BASE64_URL.encode(bytes),
        }
    }
}

/// Reads hexadecimal digits in pairs, in either case.
fn hex(text: &str) -> Option<Vec<u8>> {
    let (pairs, []) = text.as_bytes().as_chunks::<2>() else {
        return None;
    };
    let digit = |byte: u8| char::from(byte).to_digit(16);
    // The capacity is the final length, so pushing never moves the bytes.
    let mut bytes = Vec::with_capacity(text.len() / 2);
    for &[high, low] in pairs {
        let (Some(high), Some(low)) = (digit(high), digit(low)) else {
            bytes.zeroize();
            return None;
        };
        // Two hex digits make a number below 256.
        bytes.push(u8::try_from(high * 16 + low).unwrap_or_default());
    }
    Some(bytes)
}

impl fmt::Display for Encoding {
    /// Writes the word a `starmap.toml` uses: `hex`, `base64` or
    /// `base64url`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Hex => "hex",
            Self::Base64 => "base64",
            Self::Base64Url => "base64url",
        })
    }
}

/// The length a bytes type allows after decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ByteLength {
    /// Exactly this many bytes.
    Exactly(ByteCount),
    /// This many bytes or more.
    AtLeast(ByteCount),
}

impl ByteLength {
    /// Returns whether `length` bytes fit.
    fn fits(self, length: usize) -> bool {
        match self {
            Self::Exactly(count) => length == usize::from(count.get()),
            Self::AtLeast(count) => length >= usize::from(count.get()),
        }
    }
}

impl fmt::Display for ByteLength {
    /// Writes the length, such as `32 bytes` or `at least 16 bytes`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exactly(count) => count.fmt(formatter),
            Self::AtLeast(count) => write!(formatter, "at least {count}"),
        }
    }
}

/// The rule of a bytes type: the encoding its text is written in, and the
/// length its bytes have after decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ByteRule {
    encoding: Encoding,
    length: ByteLength,
}

impl ByteRule {
    /// Builds the rule of `length` bytes written in `encoding`.
    #[must_use]
    pub fn new(encoding: Encoding, length: ByteLength) -> Self {
        Self { encoding, length }
    }

    /// Returns the encoding.
    #[must_use]
    pub fn encoding(self) -> Encoding {
        self.encoding
    }

    /// Returns the length after decoding.
    #[must_use]
    pub fn length(self) -> ByteLength {
        self.length
    }

    /// Returns the bytes that `text` decodes to. On a refusal the decoded
    /// bytes are zeroed before they are dropped, because the text may be a
    /// secret.
    pub(crate) fn decode(self, text: &str) -> Result<Vec<u8>, TypeError> {
        let mut bytes = self
            .encoding
            .decode(text)
            .ok_or(TypeError::NotEncoded(self.encoding))?;
        if self.length.fits(bytes.len()) {
            Ok(bytes)
        } else {
            bytes.zeroize();
            Err(TypeError::WrongLength(self.length))
        }
    }
}

impl fmt::Display for ByteRule {
    /// Writes the rule, such as `32 bytes as base64`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} as {}", self.length, self.encoding)
    }
}
/// The pattern of a string type, in the syntax of the Rust `regex` crate. The
/// whole value must match it, so `^` and `$` are allowed and unnecessary.
///
/// The syntax is close to RE2's. It has no look-around and no
/// backreferences, and matching takes time linear in the value's length.
/// Two patterns are equal when their texts are equal.
///
/// ```
/// let pattern: starmap::Pattern = "[0-9a-f]{40}".parse()?;
/// assert_eq!(pattern.as_str(), "[0-9a-f]{40}");
/// # Ok::<(), starmap::PatternError>(())
/// ```
#[derive(Clone)]
pub struct Pattern {
    /// The text the spec wrote.
    text: String,
    /// The text anchored at both ends of the value.
    whole: Regex,
}

impl Pattern {
    /// The most bytes a pattern's text holds.
    pub const MAX_BYTES: usize = 1024;

    /// Returns the pattern's text as the spec wrote it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Refuses `text` unless the pattern matches all of it.
    pub(crate) fn check(&self, text: &str) -> Result<(), TypeError> {
        if self.whole.is_match(text) {
            Ok(())
        } else {
            Err(TypeError::NoMatch(self.clone()))
        }
    }
}

impl FromStr for Pattern {
    type Err = PatternError;

    /// Accepts a pattern of at most [`Pattern::MAX_BYTES`] bytes that the
    /// `regex` crate compiles within its default size limit, after it is
    /// anchored as `\A(?:pattern)\z`.
    ///
    /// # Errors
    ///
    /// Returns [`PatternError`] for a longer pattern, or one that does not
    /// compile.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() > Self::MAX_BYTES {
            return Err(PatternError::TooLong { bytes: text.len() });
        }
        let invalid = |error: regex::Error| PatternError::Invalid(reason(&error));
        // The text compiles alone first, so a pattern such as `a)|(b` cannot
        // close the group that anchors it and match only part of a value.
        Regex::new(text).map_err(invalid)?;
        // The text compiled alone, so the anchored form fails only when the
        // text ends inside a comment of the `x` flag, which then runs over
        // the `)` and `\z` that starmap adds.
        let whole = Regex::new(&format!("\\A(?:{text})\\z"))
            .map_err(|_| PatternError::Invalid(ENDS_IN_COMMENT.to_owned()))?;
        Ok(Self {
            text: text.to_owned(),
            whole,
        })
    }
}

/// The reason a pattern that ends inside a comment is refused.
const ENDS_IN_COMMENT: &str = "it ends inside a comment, which would hide the end of the value, so \
                               end the comment with a newline";

/// Returns the reason the `regex` crate gives for refusing a pattern, on one
/// line.
fn reason(error: &regex::Error) -> String {
    match error {
        // The syntax error's text draws the pattern with a caret under the
        // fault over several lines, and ends with a line `error: <reason>`.
        // Only that line is kept, so a spec problem stays on one line. A
        // text without such a line is kept whole, with its lines joined.
        regex::Error::Syntax(text) => text
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("error: "))
            .map_or_else(|| text.lines().collect::<Vec<_>>().join(" "), str::to_owned),
        regex::Error::CompiledTooBig(limit) => {
            format!("it compiles to more than the size limit of {limit} bytes")
        }
        other => other.to_string(),
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Eq for Pattern {}

impl fmt::Debug for Pattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("Pattern").field(&self.text).finish()
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text)
    }
}

/// The reason text is not a [`Pattern`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatternError {
    /// The text is over [`Pattern::MAX_BYTES`].
    TooLong {
        /// The number of bytes the text holds.
        bytes: usize,
    },
    /// The `regex` crate does not compile the text, for this reason.
    Invalid(String),
}

impl fmt::Display for PatternError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { bytes } => write!(
                formatter,
                "the pattern is {bytes} bytes, and a pattern holds at most {}",
                Pattern::MAX_BYTES
            ),
            Self::Invalid(reason) => write!(formatter, "the pattern does not compile: {reason}"),
        }
    }
}

impl Error for PatternError {}
