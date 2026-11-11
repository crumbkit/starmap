//! A family of variables named at run time, which a `starmap.toml`
//! declares as `[families.<PREFIX>]`, and the declaration a name has.

use crate::Name;

use super::Variable;

/// A family of variables whose names are chosen at run time, which a
/// `starmap.toml` declares as a `[families.<PREFIX>]` table. A member is
/// any variable whose name is the prefix, an underscore and a key, where a
/// key matches `[A-Z0-9][A-Z0-9_]*` and the whole name has at most
/// [`Name::MAX_BYTES`] bytes.
///
/// A family's keys are open unless the file lists them with `keys = [...]`.
/// The members of a listed family are exactly the listed keys, and a check
/// refuses a value whose key is not listed. Every member shares the
/// family's fields, which [`FamilyDeclaration::variable`] returns.
///
/// [`Spec::parse`](crate::Spec::parse) is the only way to build one. A
/// family has no default, an open family is optional, and its source is a
/// setting without a bind or inherited.
///
/// ```
/// let spec = starmap::Spec::parse(
///     r#"
/// [families.SHELF_DB_URL]
/// purpose = "The database of one region, keyed by the region's code."
/// type = "url"
/// unset = "optional"
/// show = "shown"
/// source = "setting"
/// "#,
/// )?;
/// let prefix: starmap::Name = "SHELF_DB_URL".parse()?;
/// let family = spec.family(&prefix).ok_or("SHELF_DB_URL is a family")?;
/// let member: starmap::Name = "SHELF_DB_URL_EU".parse()?;
/// assert_eq!(family.key_of(&member), Some("EU"));
/// assert_eq!(family.keys(), None);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyDeclaration {
    pub(super) prefix: Name,
    /// The fields every member shares.
    pub(super) variable: Variable,
    /// The listed keys in key order, or `None` for an open family.
    pub(super) keys: Option<Vec<String>>,
}

impl FamilyDeclaration {
    /// Returns the prefix of every member's name.
    #[must_use]
    pub fn prefix(&self) -> &Name {
        &self.prefix
    }

    /// Returns the fields every member shares: its purpose, type, unset
    /// rule, show and source.
    #[must_use]
    pub fn variable(&self) -> &Variable {
        &self.variable
    }

    /// Returns the listed keys in key order, or `None` for an open family,
    /// whose members may have any key.
    #[must_use]
    pub fn keys(&self) -> Option<&[String]> {
        self.keys.as_deref()
    }

    /// Returns the key of `name` when `name` is the prefix, an underscore
    /// and a key. It returns the key of a listed family's name whose key
    /// the family does not list too, so a check can refuse that name
    /// instead of ignoring it.
    #[must_use]
    pub fn key_of<'n>(&self, name: &'n Name) -> Option<&'n str> {
        key_after(self.prefix.as_str(), name.as_str())
    }

    /// Returns whether the family takes the key `key`: any key for an open
    /// family, and only a listed key for a listed one.
    #[must_use]
    pub fn lists(&self, key: &str) -> bool {
        self.keys
            .as_ref()
            .is_none_or(|keys| keys.iter().any(|listed| listed == key))
    }

    /// Returns the name of the member whose key is `key`, if the name is
    /// a valid [`Name`].
    pub(crate) fn member(&self, key: &str) -> Option<Name> {
        format!("{}_{key}", self.prefix).parse().ok()
    }
}

/// The declaration of a name in a spec, which
/// [`Spec::declared`](crate::Spec::declared) returns: the variable the
/// file declares with that name, or the family the name is a member of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Declaration<'s> {
    /// The name is a declared variable.
    Variable(&'s Variable),
    /// The name is a member of a family.
    Member {
        /// The family.
        family: &'s FamilyDeclaration,
        /// The member's key, the part of its name after the prefix and the
        /// underscore.
        key: &'s str,
    },
}

impl<'s> Declaration<'s> {
    /// Returns the fields of the declaration: the variable's own, or the
    /// fields every member of the family shares.
    #[must_use]
    pub fn variable(&self) -> &'s Variable {
        match self {
            Self::Variable(variable) => variable,
            Self::Member { family, .. } => family.variable(),
        }
    }
}

/// Whether `text` is a key of a family: an upper case ASCII letter or a
/// digit, then upper case ASCII letters, digits and underscores.
pub(super) fn is_key(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_uppercase() || first.is_ascii_digit())
        && characters.all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

/// Returns the key of `name` after `prefix` and an underscore, when the
/// rest of `name` is a key.
pub(super) fn key_after<'n>(prefix: &str, name: &'n str) -> Option<&'n str> {
    let key = name.strip_prefix(prefix)?.strip_prefix('_')?;
    is_key(key).then_some(key)
}
