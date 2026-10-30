//! The mode an application runs in, and the rule that decides it.

use std::ffi::OsStr;
use std::fmt;

use crate::{Choice, Name};

#[cfg(test)]
mod tests;

/// The mode an application runs in. A variable's unset rule may differ
/// between the two modes, such as a token that is optional locally and
/// required in production.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// The mode of a developer's machine, and the mode when a spec has no
    /// `[mode]` table.
    Local,
    /// The mode that the `[mode]` table's production values select.
    Production,
}

impl fmt::Display for Mode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Local => "local",
            Self::Production => "production",
        })
    }
}

/// The `[mode]` table of a `starmap.toml`: the variable that selects the
/// mode, and the values of it that select production.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeRule {
    variable: Name,
    production: Vec<Choice>,
}

impl ModeRule {
    /// Builds the rule that selects production when `variable` holds one of
    /// the `production` values. The spec parser checks that the variable is
    /// declared and that it is a setting or inherited variable.
    #[must_use]
    pub fn new(variable: Name, production: impl IntoIterator<Item = Choice>) -> Self {
        Self {
            variable,
            production: production.into_iter().collect(),
        }
    }

    /// Returns the variable that selects the mode.
    #[must_use]
    pub fn variable(&self) -> &Name {
        &self.variable
    }

    /// Returns the values that select production, in the order the spec
    /// lists them.
    pub fn production(&self) -> impl Iterator<Item = &Choice> {
        self.production.iter()
    }

    /// Decides the mode from the mode variable's value after defaults apply.
    /// The mode is production exactly when the value equals one of the
    /// production values, byte for byte, and local otherwise, including
    /// when the variable is unset.
    #[must_use]
    pub fn decide(&self, value: Option<&OsStr>) -> Mode {
        let production = value.is_some_and(|value| {
            self.production
                .iter()
                .any(|choice| OsStr::new(choice.as_str()) == value)
        });
        if production {
            Mode::Production
        } else {
            Mode::Local
        }
    }
}
