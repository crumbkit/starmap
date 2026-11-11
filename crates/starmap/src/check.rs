//! Capturing an environment into bindings, deciding the mode, and checking
//! bindings against a spec.

use std::ffi::OsString;
use std::fmt;

use crate::env::Held;
use crate::{
    Bindings, Bound, Environment, GroupName, Mode, Name, Secret, Show, Spec, Type, TypeError,
    Unset, Variable,
};

#[cfg(test)]
mod tests;

/// Binds each variable of `vars` that `spec` declares, and ignores every
/// other variable. A withheld variable's value is held as a
/// [`Secret`], or as [`Bound::NotUnicode`] when it is not Unicode.
/// A shown variable's value is bound as it is.
///
/// An empty value, such as the value of `FOO=`, counts as unset, so
/// `capture` binds nothing for it. When `vars` names a variable more than
/// once, the last non-empty value is bound, and an empty later value leaves
/// an earlier value in place.
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
        if raw.is_empty() {
            continue;
        }
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
/// binds the mode variable, or its value is empty, counts as unset or is not
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
        Some(Ok(Some(Held::Withheld(_) | Held::WithheldBytes(..))) | Err(_)) => return Mode::Local,
    };
    rule.decide(value.map(|value| value.to_os_string()).as_deref())
}

/// Checks `bindings` against every variable `spec` declares, in `mode`.
///
/// An empty bound value, shown or withheld, counts as unset before any type
/// reads it, so it is treated as unbound. This covers values that do not
/// pass through [`capture`], such as a provider's value. A bound value is
/// read in its variable's type, and a value that the type counts as unset,
/// such as a relative path where `absolute = true`, is treated as unbound. An unbound variable takes its default in `mode`,
/// stays unset when it is optional, and is missing when it is required.
/// When the bindings decide another mode than `mode`, the mode variable has
/// a finding too, so a ready environment fits the mode its child decides.
/// A spec without a `[mode]` table is always checked in local mode, which
/// is the only mode its child can decide. When a group is partly set, each
/// of its members that is unset and has no finding of its own gets
/// [`Problem::GroupIncomplete`]. A member counts as set when the check
/// holds a value for it. An invalid mode value checked in
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
    let held: Vec<(&Name, Result<Option<Held>, Problem>)> = spec
        .variables()
        .map(|(name, variable)| (name, hold(variable, bindings.get(name), mode)))
        .collect();
    let incomplete: Vec<Option<Problem>> = held
        .iter()
        .map(|(name, result)| match result {
            Ok(None) => incomplete(spec, &held, name),
            _ => None,
        })
        .collect();
    let mut entries = Vec::new();
    let mut findings = Vec::new();
    for ((name, result), incomplete) in held.into_iter().zip(incomplete) {
        match (result, incomplete) {
            (Ok(held), None) => entries.push((name.clone(), held)),
            (Err(problem), _) | (Ok(_), Some(problem)) => findings.push(Finding {
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

/// Returns the finding of `name`, which is unset, when its group is partly
/// set: the group, and each member that `held` holds a value for.
fn incomplete(
    spec: &Spec,
    held: &[(&Name, Result<Option<Held>, Problem>)],
    name: &Name,
) -> Option<Problem> {
    let group = spec.group_of(name)?;
    let set: Vec<Name> = group
        .variables()
        .iter()
        .filter(|member| {
            held.iter()
                .any(|(held, result)| held == member && matches!(result, Ok(Some(_))))
        })
        .cloned()
        .collect();
    (!set.is_empty()).then(|| Problem::GroupIncomplete {
        group: group.name().clone(),
        set,
    })
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
/// counts as unset, which an empty value always does.
fn read(variable: &Variable, bound: &Bound) -> Result<Option<Held>, Problem> {
    match (variable.show(), bound) {
        (Show::Shown, Bound::Plain(raw)) if raw.is_empty() => Ok(None),
        (Show::Withheld, Bound::Secret(secret)) if secret.is_empty() => Ok(None),
        (Show::Shown, Bound::Plain(raw)) => variable
            .value_type()
            .parse(raw)
            .map(|value| value.map(Held::Shown))
            .map_err(Problem::Invalid),
        // A withheld variable has the type string or bytes, which the spec
        // parser checks. A bytes value is decoded inside the secret and its
        // bytes are held beside the text. A string with a pattern is matched
        // inside the secret. Each refusal names only the rule.
        (Show::Withheld, Bound::Secret(secret)) => match variable.value_type() {
            Type::Bytes(rule) => {
                let bytes = secret.decode(*rule).map_err(Problem::Invalid)?;
                Ok(Some(Held::WithheldBytes(secret.duplicate(), bytes)))
            }
            ty => {
                ty.check_secret(secret).map_err(Problem::Invalid)?;
                Ok(Some(Held::Withheld(secret.duplicate())))
            }
        },
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
                Problem::GroupIncomplete { group, set } => {
                    let verb = if set.len() == 1 { "is" } else { "are" };
                    write!(
                        formatter,
                        "{name} is unset, and the group {group} needs it because {} {verb} set",
                        Names(set)
                    )
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
    /// The variable is unset, and other members of its group are set, so
    /// the group is partly set.
    GroupIncomplete {
        /// The variable's group.
        group: GroupName,
        /// The members of the group that are set, in the group's order.
        set: Vec<Name>,
    },
}

/// Names written as `A`, `A and B`, or `A, B and C`.
struct Names<'a>(&'a [Name]);

impl fmt::Display for Names<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, name) in self.0.iter().enumerate() {
            if index > 0 {
                let joiner = if index + 1 == self.0.len() {
                    " and "
                } else {
                    ", "
                };
                formatter.write_str(joiner)?;
            }
            write!(formatter, "{name}")?;
        }
        Ok(())
    }
}
