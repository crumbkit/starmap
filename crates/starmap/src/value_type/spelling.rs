//! The spellings a boolean type declares for true and for false
//! ([`Spellings`]).

use std::error::Error;
use std::fmt;

use super::write_list;

#[cfg(test)]
mod tests;

/// The spellings a boolean type declares: at least one for true and at
/// least one for false, none of them empty and none repeated in either
/// list. A value matches a spelling exactly, so `yes` does not match `YES`.
///
/// ```
/// let spellings = starmap::Spellings::new(
///     ["1".to_owned(), "yes".to_owned()],
///     ["0".to_owned(), "no".to_owned()],
/// )?;
/// assert_eq!(spellings.for_true(), ["1", "yes"]);
/// assert_eq!(spellings.to_string(), "1, yes for true and 0, no for false");
/// # Ok::<(), starmap::SpellingsError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Spellings {
    /// The spellings of true, in the order the spec lists them.
    for_true: Vec<String>,
    /// The spellings of false, in the order the spec lists them.
    for_false: Vec<String>,
}

impl Spellings {
    /// Builds the spellings from the list for true and the list for false,
    /// keeping the order of each.
    ///
    /// # Errors
    ///
    /// Returns [`SpellingsError`] when a list is empty, a spelling is empty,
    /// or a spelling appears twice, in one list or in both.
    pub fn new(
        for_true: impl IntoIterator<Item = String>,
        for_false: impl IntoIterator<Item = String>,
    ) -> Result<Self, SpellingsError> {
        let spellings = Self {
            for_true: for_true.into_iter().collect(),
            for_false: for_false.into_iter().collect(),
        };
        for (list, value) in [(&spellings.for_true, true), (&spellings.for_false, false)] {
            if list.is_empty() {
                return Err(SpellingsError::Empty(value));
            }
        }
        let mut seen: Vec<&String> = Vec::new();
        for spelling in spellings.for_true.iter().chain(&spellings.for_false) {
            if spelling.is_empty() {
                return Err(SpellingsError::EmptySpelling);
            }
            if seen.contains(&spelling) {
                return Err(SpellingsError::Repeated(spelling.clone()));
            }
            seen.push(spelling);
        }
        Ok(spellings)
    }

    /// Returns the spellings of true in order.
    #[must_use]
    pub fn for_true(&self) -> &[String] {
        &self.for_true
    }

    /// Returns the spellings of false in order.
    #[must_use]
    pub fn for_false(&self) -> &[String] {
        &self.for_false
    }

    /// Returns the value that `text` spells, compared exactly, or `None`
    /// when it is no declared spelling.
    pub(crate) fn read(&self, text: &str) -> Option<bool> {
        let holds = |list: &[String]| list.iter().any(|spelling| spelling == text);
        if holds(&self.for_true) {
            Some(true)
        } else if holds(&self.for_false) {
            Some(false)
        } else {
            None
        }
    }
}

impl fmt::Display for Spellings {
    /// Writes both lists, as `1, yes for true and 0, no for false`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_list(formatter, &self.for_true)?;
        formatter.write_str(" for true and ")?;
        write_list(formatter, &self.for_false)?;
        formatter.write_str(" for false")
    }
}

/// The reason two lists are not [`Spellings`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpellingsError {
    /// The list for this value is empty.
    Empty(bool),
    /// A spelling is empty text.
    EmptySpelling,
    /// This spelling appears twice, in one list or in both.
    Repeated(String),
}

impl fmt::Display for SpellingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty(value) => write!(formatter, "a boolean lists no spellings for {value}"),
            Self::EmptySpelling => formatter.write_str("a boolean lists an empty spelling"),
            Self::Repeated(spelling) => {
                write!(formatter, "a boolean lists the spelling {spelling:?} twice")
            }
        }
    }
}

impl Error for SpellingsError {}
