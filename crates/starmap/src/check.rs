//! Capturing an environment into bindings, deciding the mode, and checking
//! bindings against a spec.

use std::ffi::OsString;
use std::fmt;

use crate::env::Held;
use crate::{
    Bindings, Bound, Environment, Mode, Name, Secret, Show, Spec, TypeError, Unset, Variable,
};

#[cfg(test)]
mod tests;

/// Binds each variable of `vars` that `spec` declares, and ignores every
/// other variable. A withheld variable's value is held as a
/// [`Secret`], or as [`Bound::NotUnicode`] when it is not Unicode.
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
        bindings.bind(name, variable.capture(raw));
    }
    bindings
}

impl Variable {
    /// Binds the raw value `raw` by the variable's show, as [`capture`]
    /// binds each value it keeps: a withheld value as a [`Secret`],
    /// or as [`Bound::NotUnicode`] when it is not Unicode, and a shown value
    /// as it is.
    #[must_use]
    pub fn capture(&self, raw: OsString) -> Bound {
        match self.show() {
            Show::Shown => Bound::Plain(raw),
            Show::Withheld => match raw.into_string() {
                Ok(text) => Bound::Secret(Secret::new(text)),
                Err(raw) => {
                    discard(raw);
                    Bound::NotUnicode
                }
            },
        }
    }
}

/// Drops the raw value of a withheld variable that is not Unicode, zeroing
/// its bytes first on unix.
fn discard(raw: OsString) {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        use zeroize::Zeroize as _;
        raw.into_vec().zeroize();
    }
    // BASE-37: on other platforms `OsString` gives no access to its own
    // bytes, so this drops the value without zeroing it. A conversion that
    // reaches the bytes on that platform replaces this when starmap
    // supports one.
    #[cfg(not(unix))]
    drop(raw);
}

/// Decides the mode from the mode variable, read as [`check`] reads it. A
/// spec without a `[mode]` table always runs in local mode. When nothing
/// binds the mode variable, or its value counts as unset or is not
/// Unicode, its local default decides. Any other value that does not fit
/// its type decides local mode, and `check` refuses it.
#[must_use]
pub fn mode(spec: &Spec, bindings: &Bindings) -> Mode {
    let Some(rule) = spec.mode() else {
        return Mode::Local;
    };
    let name = rule.variable();
    let Some(variable) = spec.variable(name) else {
        return Mode::Local;
    };
    let value = match bindings.get(name).map(|bound| read(variable, bound)) {
        Some(Ok(Some(Held::Shown(value)))) => Some(value),
        None | Some(Ok(None) | Err(Problem::Invalid(TypeError::NotUnicode))) => {
            match variable.unset().in_mode(Mode::Local) {
                Unset::Default(value) => Some(value.clone()),
                Unset::Required | Unset::Optional => None,
            }
        }
        // The parser refuses a withheld mode variable, so a withheld value
        // never reaches this arm; an invalid value decides local mode.
        Some(Ok(Some(Held::Withheld(_))) | Err(_)) => return Mode::Local,
    };
    rule.decide(value.map(|value| value.to_os_string()).as_deref())
}

/// Checks `bindings` against every variable `spec` declares, in `mode`.
///
/// A bound value is read in its variable's type, and a value that the type
/// counts as unset, such as a relative path where `absolute = true`, is
/// treated as unbound. An unbound variable takes its default in `mode`,
/// stays unset when it is optional, and is missing when it is required.
/// When the bindings decide another mode than `mode`, the mode variable has
/// a finding too, so a ready environment fits the mode its child decides.
/// A spec without a `[mode]` table is always checked in local mode, which
/// is the only mode its child can decide. An invalid mode value checked in
/// production gives the mode variable two findings: `Invalid`, and
/// `ModeMismatch { decided: Local }`.
///
/// The verdict is [`Verdict::Ready`] only when no variable has a finding,
/// and otherwise [`Verdict::Refused`] with each finding in the spec's order
/// and a mode mismatch last (BASE-44).
#[must_use]
pub fn check(spec: &Spec, bindings: &Bindings, mode: Mode) -> Verdict {
    let mode = if spec.mode().is_some() {
        mode
    } else {
        Mode::Local
    };
    let mut entries = Vec::new();
    let mut findings = Vec::new();
    for (name, variable) in spec.variables() {
        match hold(variable, bindings.get(name), mode) {
            Ok(held) => entries.push((name.clone(), held)),
            Err(problem) => findings.push(Finding {
                name: name.clone(),
                problem,
            }),
        }
    }
    let decided = self::mode(spec, bindings);
    if let Some(rule) = spec.mode()
        && decided != mode
    {
        findings.push(Finding {
            name: rule.variable().clone(),
            problem: Problem::ModeMismatch { decided },
        });
    }
    if findings.is_empty() {
        Verdict::Ready(Environment::new(mode, entries))
    } else {
        Verdict::Refused(Refusals {
            findings,
            modes: spec.mode().is_some(),
        })
    }
}

/// Holds the checked value of `variable` in `mode`, or `None` when it stays
/// unset.
fn hold(variable: &Variable, bound: Option<&Bound>, mode: Mode) -> Result<Option<Held>, Problem> {
    if let Some(held) = bound
        .map(|bound| read(variable, bound))
        .transpose()?
        .flatten()
    {
        return Ok(Some(held));
    }
    match variable.unset().in_mode(mode) {
        Unset::Required => Err(Problem::Missing { mode }),
        Unset::Optional => Ok(None),
        Unset::Default(value) => Ok(Some(Held::Shown(value.clone()))),
    }
}

/// Reads `bound` as a value of `variable`. `Ok(None)` means the value
/// counts as unset.
fn read(variable: &Variable, bound: &Bound) -> Result<Option<Held>, Problem> {
    match (variable.show(), bound) {
        (Show::Shown, Bound::Plain(raw)) => variable
            .value_type()
            .parse(raw)
            .map(|value| value.map(Held::Shown))
            .map_err(Problem::Invalid),
        // A withheld variable has the type string, which the spec parser
        // checks, and a string takes any text that a Secret holds.
        (Show::Withheld, Bound::Secret(secret)) => Ok(Some(Held::Withheld(secret.duplicate()))),
        (Show::Withheld, Bound::NotUnicode) => Err(Problem::Invalid(TypeError::NotUnicode)),
        (show, _) => Err(Problem::ShowMismatch { show }),
    }
}

/// The outcome of [`check`].
#[derive(Debug)]
pub enum Verdict {
    /// Every variable fits its type and its unset rule in the mode.
    Ready(Environment),
    /// At least one variable has a finding.
    Refused(Refusals),
}

/// The findings of a refused check. It holds at least one.
#[derive(Debug)]
pub struct Refusals {
    findings: Vec<Finding>,
    /// Whether the spec has a `[mode]` table, so that a missing variable's
    /// text names the mode only when the spec has modes.
    modes: bool,
}

impl Refusals {
    /// Returns each finding, in the spec's order of variables, with a mode
    /// mismatch last.
    pub fn iter(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter()
    }
}

impl fmt::Display for Refusals {
    /// Writes each finding with its variable's name, separated by `; `,
    /// such as `SHELF_PORT is required and has no value; SHELF_DEBUG: the
    /// value is not one of the words 1, 0, true, false, yes, no, on and
    /// off`. A missing variable names its mode only when the spec has a
    /// `[mode]` table. The text never repeats a value.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, finding) in self.findings.iter().enumerate() {
            if index > 0 {
                formatter.write_str("; ")?;
            }
            let name = &finding.name;
            match &finding.problem {
                Problem::Missing { mode } if self.modes => {
                    write!(
                        formatter,
                        "{name} is required in {mode} mode and has no value"
                    )
                }
                Problem::Missing { .. } => write!(formatter, "{name} is required and has no value"),
                Problem::Invalid(error) => write!(formatter, "{name}: {error}"),
                Problem::ShowMismatch { show } => write!(
                    formatter,
                    "{name} has a value of the wrong kind for a {show} variable"
                ),
                Problem::ModeMismatch { decided } => {
                    write!(formatter, "{name} decides {decided} mode")
                }
            }?;
        }
        Ok(())
    }
}

/// A variable that stops a check from passing, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
    /// The variable's value does not fit its type.
    Invalid(TypeError),
    /// The kind of bound value does not match the variable's `show`: a
    /// plain value is bound to a withheld variable, or a secret or a
    /// [`Bound::NotUnicode`] to a shown one.
    ShowMismatch {
        /// The variable's show.
        show: Show,
    },
    /// The variable is the mode variable, and the bindings decide another
    /// mode than the check ran in.
    ModeMismatch {
        /// The mode the bindings decide.
        decided: Mode,
    },
}
