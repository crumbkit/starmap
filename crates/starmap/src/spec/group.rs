//! A group of variables that are set together or not at all, which a
//! `starmap.toml` declares as `[groups.<name>]`, and the name of a group.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use crate::Name;

/// The name of a group of variables, which a `starmap.toml` declares as
/// `[groups.<name>]`.
///
/// A name starts with a lower case ASCII letter and holds only lower case
/// ASCII letters, digits and hyphens. [`FromStr`] is the only way to build
/// one.
///
/// ```
/// let name: starmap::GroupName = "mirror".parse()?;
/// assert_eq!(name.as_str(), "mirror");
/// # Ok::<(), starmap::GroupNameError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupName(String);

impl GroupName {
    /// Returns the name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for GroupName {
    type Err = GroupNameError;

    /// Accepts text that starts with a lower case ASCII letter and holds
    /// only lower case ASCII letters, digits and hyphens.
    ///
    /// # Errors
    ///
    /// Returns [`GroupNameError`] for empty text, or text with a character
    /// that the rule does not allow at its place.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.is_empty() {
            return Err(GroupNameError::Empty);
        }
        for (at, character) in text.char_indices() {
            let allowed = character.is_ascii_lowercase()
                || (at > 0 && (character.is_ascii_digit() || character == '-'));
            if !allowed {
                return Err(GroupNameError::Character { character, at });
            }
        }
        Ok(Self(text.to_owned()))
    }
}

impl fmt::Display for GroupName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The reason text is not a [`GroupName`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GroupNameError {
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

impl fmt::Display for GroupNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("a group name is empty"),
            Self::Character { character, at } => write!(
                formatter,
                "a group name holds {character:?} at byte {at}, and a name starts with a lower \
                 case letter and holds only lower case letters, digits and hyphens"
            ),
        }
    }
}

impl Error for GroupNameError {}

/// A group of variables that must be set together or not at all, which a
/// `starmap.toml` declares as a `[groups.<name>]` table with a `variables`
/// list.
///
/// [`Spec::parse`](crate::Spec::parse) is the only way to build one. A
/// group lists at least two variables that the file declares, repeats
/// none, and shares none with another group. No member has a default or a
/// generated value, because either would always count as set.
///
/// ```
/// let spec = starmap::Spec::parse(
///     r#"
/// [groups.mirror]
/// variables = ["SHELF_MIRROR_ACCOUNT", "SHELF_MIRROR_KEY"]
///
/// [variables.SHELF_MIRROR_ACCOUNT]
/// purpose = "The account shelf uses at the catalogue mirror."
/// type = "string"
/// unset = "optional"
/// show = "shown"
/// source = "setting"
///
/// [variables.SHELF_MIRROR_KEY]
/// purpose = "The key of the account at the catalogue mirror."
/// type = "string"
/// unset = "optional"
/// show = "shown"
/// source = "setting"
/// "#,
/// )?;
/// let key: starmap::Name = "SHELF_MIRROR_KEY".parse()?;
/// let group = spec.group_of(&key).ok_or("SHELF_MIRROR_KEY has a group")?;
/// assert_eq!(group.name().as_str(), "mirror");
/// assert_eq!(group.variables().len(), 2);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub(super) name: GroupName,
    /// The members, in the order the file lists them.
    pub(super) variables: Vec<Name>,
}

impl Group {
    /// Returns the group's name.
    #[must_use]
    pub fn name(&self) -> &GroupName {
        &self.name
    }

    /// Returns the group's variables, in the order the file lists them.
    #[must_use]
    pub fn variables(&self) -> &[Name] {
        &self.variables
    }
}
