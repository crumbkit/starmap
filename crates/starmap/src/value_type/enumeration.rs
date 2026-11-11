//! The values of an enumeration type ([`Choice`] and [`Choices`]), and the
//! name of an enumeration that a `starmap.toml` declares in its
//! `[enumerations]` table.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use super::{distinct, write_list};

/// The name of an enumeration that a `starmap.toml` declares as
/// `[enumerations.<name>]` and a variable names with
/// `type = { enumeration = "<name>" }`.
///
/// A name starts with a lower case ASCII letter and holds only lower case
/// ASCII letters, digits and hyphens. [`FromStr`] is the only way to build
/// one.
///
/// ```
/// let name: starmap::EnumerationName = "log-level".parse()?;
/// assert_eq!(name.as_str(), "log-level");
/// # Ok::<(), starmap::EnumerationNameError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnumerationName(String);

impl EnumerationName {
    /// Returns the name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for EnumerationName {
    type Err = EnumerationNameError;

    /// Accepts text that starts with a lower case ASCII letter and holds
    /// only lower case ASCII letters, digits and hyphens.
    ///
    /// # Errors
    ///
    /// Returns [`EnumerationNameError`] for empty text, or text with a
    /// character that the rule does not allow at its place.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.is_empty() {
            return Err(EnumerationNameError::Empty);
        }
        for (at, character) in text.char_indices() {
            let allowed = character.is_ascii_lowercase()
                || (at > 0 && (character.is_ascii_digit() || character == '-'));
            if !allowed {
                return Err(EnumerationNameError::Character { character, at });
            }
        }
        Ok(Self(text.to_owned()))
    }
}

impl fmt::Display for EnumerationName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The reason text is not an [`EnumerationName`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EnumerationNameError {
    /// The text is empty.
    Empty,
    /// The text holds a character that the rule does not allow at that
    /// place, such as an upper case letter, or a digit at the start.
    Character {
        /// The first character that does not fit.
        character: char,
        /// The character's byte offset in the text.
        at: usize,
    },
}

impl fmt::Display for EnumerationNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("an enumeration name is empty"),
            Self::Character { character, at } => write!(
                formatter,
                "an enumeration name holds {character:?} at byte {at}, and a name starts with a \
                 lower case letter and holds only lower case letters, digits and hyphens"
            ),
        }
    }
}

impl Error for EnumerationNameError {}

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
///
/// The values of a named enumeration, which a spec declares as
/// `[enumerations.<name>]`, also carry the enumeration's name and a
/// description of each value. An inline list has neither.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Choices {
    choices: Vec<Choice>,
    /// The name and the descriptions of a named enumeration, with one
    /// description for each choice, in the same order.
    named: Option<(EnumerationName, Vec<String>)>,
}

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
        Ok(Self {
            choices: kept,
            named: None,
        })
    }

    /// Builds the named enumeration `name` from `values`, each a choice
    /// with its description, keeping their order. The spec parser checks
    /// each description before it calls this.
    ///
    /// # Errors
    ///
    /// Returns [`ChoicesError`] when there is no value or one repeats.
    pub(crate) fn named(
        name: EnumerationName,
        values: impl IntoIterator<Item = (Choice, String)>,
    ) -> Result<Self, ChoicesError> {
        let (choices, descriptions): (Vec<Choice>, Vec<String>) = values.into_iter().unzip();
        let mut built = Self::new(choices)?;
        built.named = Some((name, descriptions));
        Ok(built)
    }

    /// Returns the choices in order.
    pub fn iter(&self) -> impl Iterator<Item = &Choice> {
        self.choices.iter()
    }

    /// Returns the name of the enumeration, or `None` for an inline list.
    #[must_use]
    pub fn name(&self) -> Option<&EnumerationName> {
        self.named.as_ref().map(|(name, _)| name)
    }

    /// Returns the description of `choice` in a named enumeration, or
    /// `None` for an inline list or a choice that is not one of these.
    #[must_use]
    pub fn description(&self, choice: &Choice) -> Option<&str> {
        let (_, descriptions) = self.named.as_ref()?;
        let at = self.choices.iter().position(|listed| listed == choice)?;
        descriptions.get(at).map(String::as_str)
    }

    /// Returns the choice that equals `text` exactly, if there is one.
    pub(crate) fn get(&self, text: &str) -> Option<&Choice> {
        self.choices.iter().find(|choice| choice.as_str() == text)
    }
}

impl fmt::Display for Choices {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_list(formatter, &self.choices)
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
