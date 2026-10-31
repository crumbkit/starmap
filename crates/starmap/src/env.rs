//! The checked environment that [`check`](crate::check) builds, and the
//! one place in the crate that reads a secret's text.

use std::process::Command;

use crate::{Mode, Name, Secret, Value};

#[cfg(test)]
mod tests;

/// The values of every variable a spec declares, each checked against its
/// form and its unset rule in one mode. Only [`check`](crate::check)
/// builds one, so an `Environment` always holds checked values.
#[derive(Debug)]
pub struct Environment {
    mode: Mode,
    /// Each declared variable in the spec's order, with `None` for one that
    /// stays unset.
    entries: Vec<(Name, Option<Held>)>,
}

/// A checked value.
#[derive(Debug)]
pub(crate) enum Held {
    /// The value of a shown variable.
    Shown(Value),
    /// The value of a withheld variable.
    Withheld(Secret<String>),
}

impl Environment {
    /// Builds the environment of `entries`, which `check` has checked in
    /// `mode`.
    pub(crate) fn new(mode: Mode, entries: Vec<(Name, Option<Held>)>) -> Self {
        Self { mode, entries }
    }

    /// Returns the mode the values were checked in.
    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Returns the value of the shown variable `name`. It returns `None`
    /// when the variable stays unset, is not declared, or is withheld,
    /// whose value only [`Environment::secret`] returns.
    #[must_use]
    pub fn value(&self, name: &Name) -> Option<&Value> {
        match self.held(name)? {
            Held::Shown(value) => Some(value),
            Held::Withheld(_) => None,
        }
    }

    /// Returns the value of the withheld variable `name`. It returns `None`
    /// when the variable stays unset, is not declared, or is shown.
    #[must_use]
    pub fn secret(&self, name: &Name) -> Option<&Secret<String>> {
        match self.held(name)? {
            Held::Withheld(secret) => Some(secret),
            Held::Shown(_) => None,
        }
    }

    /// Sets each declared variable that has a value in `command`'s
    /// environment, and removes each declared variable that stays unset.
    /// It leaves every other variable of `command` as it was, so the caller
    /// decides whether the child inherits the rest of the environment.
    ///
    /// The command keeps its own copy of each withheld value, and that copy
    /// is not zeroed when the command is dropped, because `Command` holds
    /// its environment in plain strings.
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-24: a withheld value leaves the program here, in the child's environment"
    )]
    pub fn apply(&self, command: &mut Command) {
        for (name, held) in &self.entries {
            match held {
                Some(Held::Shown(value)) => command.env(name.as_str(), value.to_os_string()),
                Some(Held::Withheld(secret)) => command.env(name.as_str(), secret.expose()),
                None => command.env_remove(name.as_str()),
            };
        }
    }

    /// Returns the checked value of `name`, if it has one.
    fn held(&self, name: &Name) -> Option<&Held> {
        self.entries
            .iter()
            .find(|(declared, _)| declared == name)
            .and_then(|(_, held)| held.as_ref())
    }
}
