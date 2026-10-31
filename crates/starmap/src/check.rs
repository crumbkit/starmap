//! Capturing an environment into bindings, deciding the mode, and checking
//! bindings against a spec.

use std::ffi::OsString;

use crate::env::{self, Held};
use crate::{
    Bindings, Bound, Environment, FormError, Mode, Name, Secret, Show, Spec, Unset, Value, Variable,
};

#[cfg(test)]
mod tests;

/// Binds each variable of `vars` that `spec` declares, and ignores every
/// other variable. A withheld variable's value is held as a
/// [`Secret<String>`], or as [`Bound::NotUnicode`] when it is not Unicode.
/// A shown variable's value is bound as it is.
///
/// The caller passes the environment in, such as `std::env::vars_os()`,
/// because the crate reads no environment variable itself.
#[must_use]
pub fn capture(spec: &Spec, vars: impl IntoIterator<Item = (OsString, OsString)>) -> Bindings {
    let mut bindings = Bindings::new();
    for (key, raw) in vars {
        let Some(name) = key.to_str().and_then(|key| key.parse::<Name>().ok()) else {
            continue;
        };
        let Some(variable) = spec.variable(&name) else {
            continue;
        };
        let bound = match variable.show() {
            Show::Shown => Bound::Plain(raw),
            Show::Withheld => raw
                .into_string()
                .map_or(Bound::NotUnicode, |text| Bound::Secret(Secret::new(text))),
        };
        bindings.bind(name, bound);
    }
    bindings
}

/// Decides the mode from the mode variable's bound value, or from its
/// local default when nothing binds it. A spec without a `[mode]` table
/// always runs in local mode. A bound value decides the mode as it is
/// bound, and [`check`] reports it when it does not fit the variable's
/// form.
#[must_use]
pub fn mode(spec: &Spec, bindings: &Bindings) -> Mode {
    let Some(rule) = spec.mode() else {
        return Mode::Local;
    };
    let name = rule.variable();
    if let Some(bound) = bindings.get(name) {
        return rule.decide(env::text(bound));
    }
    let default =
        spec.variable(name)
            .and_then(|variable| match variable.unset().in_mode(Mode::Local) {
                Unset::Default(value) => Some(value.to_os_string()),
                Unset::Required | Unset::Optional => None,
            });
    rule.decide(default.as_deref())
}

/// Checks `bindings` against every variable `spec` declares, in `mode`.
///
/// A bound value is read in its variable's form, and a value that the form
/// counts as unset, such as a relative path where `absolute = true`, is
/// treated as unbound. An unbound variable takes its default in `mode`,
/// stays unset when it is optional, and is missing when it is required.
/// The verdict is [`Verdict::Ready`] only when no variable is missing or
/// invalid, and otherwise [`Verdict::Refused`] with a finding for each such
/// variable, in the spec's order (BASE-44).
#[must_use]
pub fn check(spec: &Spec, bindings: &Bindings, mode: Mode) -> Verdict {
    let mut entries = Vec::new();
    let mut findings = Vec::new();
    for (name, variable) in spec.variables() {
        let read = bindings
            .get(name)
            .map_or(Ok(None), |bound| read(variable, bound));
        let problem = match read {
            Ok(Some(held)) => {
                entries.push((name.clone(), Some(held)));
                continue;
            }
            Ok(None) => match variable.unset().in_mode(mode) {
                Unset::Required => Problem::Missing { mode },
                Unset::Optional => {
                    entries.push((name.clone(), None));
                    continue;
                }
                Unset::Default(value) => {
                    entries.push((name.clone(), Some(default(variable, value))));
                    continue;
                }
            },
            Err(error) => Problem::Invalid(error),
        };
        findings.push(Finding {
            name: name.clone(),
            problem,
        });
    }
    if findings.is_empty() {
        Verdict::Ready(Environment::new(mode, entries))
    } else {
        Verdict::Refused(Refusals(findings))
    }
}

/// Reads `bound` as a value of `variable`. `Ok(None)` means the value
/// counts as unset.
fn read(variable: &Variable, bound: &Bound) -> Result<Option<Held>, FormError> {
    let text = env::text(bound).ok_or(FormError::NotUnicode)?;
    match variable.show() {
        Show::Shown => Ok(variable.form().parse(text)?.map(Held::Shown)),
        // A withheld variable has the form string, which the spec parser
        // checks, and a string takes any text that a Secret<String> holds.
        Show::Withheld => {
            let text = text.to_str().ok_or(FormError::NotUnicode)?;
            Ok(Some(Held::Withheld(Secret::new(text.to_owned()))))
        }
    }
}

/// Holds `variable`'s default `value`, which is a secret when the variable
/// is withheld.
fn default(variable: &Variable, value: &Value) -> Held {
    match variable.show() {
        Show::Shown => Held::Shown(value.clone()),
        // A default is TOML text, which is Unicode, so the lossy conversion
        // never changes it.
        Show::Withheld => Held::Withheld(Secret::new(
            value.to_os_string().to_string_lossy().into_owned(),
        )),
    }
}

/// The outcome of [`check`].
#[derive(Debug)]
pub enum Verdict {
    /// Every variable fits its form and its unset rule in the mode.
    Ready(Environment),
    /// At least one variable is missing or invalid.
    Refused(Refusals),
}

/// The findings of a refused check. It holds at least one.
#[derive(Debug)]
pub struct Refusals(Vec<Finding>);

impl Refusals {
    /// Returns each finding, in the spec's order of variables.
    pub fn iter(&self) -> impl Iterator<Item = &Finding> {
        self.0.iter()
    }
}

/// A variable that stops a check from passing, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The variable.
    pub name: Name,
    /// Why the variable stops the check.
    pub problem: Problem,
}

/// Why a variable stops a check from passing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Problem {
    /// The variable is required in `mode` and has no value.
    Missing {
        /// The mode the check ran in.
        mode: Mode,
    },
    /// The variable's value does not fit its form.
    Invalid(FormError),
}
