//! The checked environment that [`check`](crate::check) builds, and the
//! one place in the crate that reads a secret's text.

use std::process::Command;

use crate::{Mode, Name, Secret, SecretBytes, Value};

#[cfg(test)]
mod tests;

/// The values of every variable a spec declares, and of each member of a
/// family that is set, each checked against its type and its unset rule in
/// one mode. Only [`check`](crate::check) builds one, so an `Environment`
/// always holds checked values.
#[derive(Debug)]
pub struct Environment {
    mode: Mode,
    /// Each declared variable in the spec's order, with `None` for one that
    /// stays unset, and then each member of a family that is set.
    entries: Vec<(Name, Option<Held>)>,
    /// Each member of a family that is set, in prefix order and then key
    /// order.
    members: Vec<Member>,
}

/// A member of a family that the environment holds a value for.
#[derive(Debug)]
pub(crate) struct Member {
    /// The family's prefix.
    pub(crate) prefix: Name,
    /// The member's key.
    pub(crate) key: String,
    /// The member's name.
    pub(crate) name: Name,
}

/// A checked value.
#[derive(Debug)]
pub(crate) enum Held {
    /// The value of a shown variable.
    Shown(Value),
    /// The value of a withheld variable.
    Withheld(Secret),
    /// The text of a withheld bytes variable, and the bytes it decodes to.
    WithheldBytes(Secret, SecretBytes),
}

impl Environment {
    /// Builds the environment of `entries`, which `check` has checked in
    /// `mode`, where `members` names each entry that is a member of a
    /// family.
    pub(crate) fn new(
        mode: Mode,
        entries: Vec<(Name, Option<Held>)>,
        members: Vec<Member>,
    ) -> Self {
        let mut members = members;
        members.sort_by(|left, right| (&left.prefix, &left.key).cmp(&(&right.prefix, &right.key)));
        Self {
            mode,
            entries,
            members,
        }
    }

    /// Returns the key and the name of each member of the family whose
    /// prefix is `prefix` that has a value, in key order. Its value is
    /// [`Environment::value`] or [`Environment::secret`] of the name. It
    /// returns nothing when the spec declares no such family.
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
    /// let vars = [("SHELF_DB_URL_US", "https://us.example"), ("SHELF_DB_URL_EU", "https://eu.example")]
    ///     .map(|(name, value)| (name.into(), value.into()));
    /// let bindings = starmap::capture(&spec, vars);
    /// let starmap::Verdict::Ready(environment) = starmap::check(&spec, &bindings, starmap::Mode::Local) else {
    ///     return Err("the members fit".into());
    /// };
    /// let prefix: starmap::Name = "SHELF_DB_URL".parse()?;
    /// let keys: Vec<&str> = environment.members(&prefix).map(|(key, _)| key).collect();
    /// assert_eq!(keys, ["EU", "US"]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn members<'e>(&'e self, prefix: &'e Name) -> impl Iterator<Item = (&'e str, &'e Name)> {
        self.members
            .iter()
            .filter(move |member| member.prefix == *prefix)
            .map(|member| (member.key.as_str(), &member.name))
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
            Held::Withheld(_) | Held::WithheldBytes(..) => None,
        }
    }

    /// Returns the value of the withheld variable `name`, as the text the
    /// environment holds. It returns `None` when the variable stays unset,
    /// is not declared, or is shown.
    #[must_use]
    pub fn secret(&self, name: &Name) -> Option<&Secret> {
        match self.held(name)? {
            Held::Withheld(secret) | Held::WithheldBytes(secret, _) => Some(secret),
            Held::Shown(_) => None,
        }
    }

    /// Returns the bytes that the withheld bytes variable `name` decodes
    /// to. It returns `None` when the variable stays unset, is not
    /// declared, is shown, or does not have a bytes type.
    #[must_use]
    pub fn secret_bytes(&self, name: &Name) -> Option<&SecretBytes> {
        match self.held(name)? {
            Held::WithheldBytes(_, bytes) => Some(bytes),
            Held::Withheld(_) | Held::Shown(_) => None,
        }
    }

    /// Sets each declared variable that has a value, and each member of a
    /// family that has one, in `command`'s environment, and removes each
    /// declared variable that stays unset.
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
                Some(Held::Withheld(secret) | Held::WithheldBytes(secret, _)) => {
                    command.env(name.as_str(), secret.expose())
                }
                None => command.env_remove(name.as_str()),
            };
        }
    }

    /// Whether the declared variable `name` has a value.
    pub(crate) fn is_set(&self, name: &Name) -> bool {
        self.held(name).is_some()
    }

    /// Returns the checked value of `name`, if it has one.
    fn held(&self, name: &Name) -> Option<&Held> {
        self.entries
            .iter()
            .find(|(declared, _)| declared == name)
            .and_then(|(_, held)| held.as_ref())
    }
}
