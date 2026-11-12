//! Capturing an environment into bindings, deciding the mode, and checking
//! bindings against a spec.

use std::ffi::{OsStr, OsString};
use std::fmt::{self, Write as _};

use crate::env::{Held, Member};
use crate::field::Rule;
use crate::{
    Bindings, Bound, Empty, Environment, FamilyDeclaration, Field, GroupName, Mode, Name, Secret,
    Show, Spec, Type, TypeError, Unset, Variable,
};

#[cfg(test)]
mod tests;

/// Binds each variable of `vars` that `spec` declares, and each name that
/// is the prefix of a family, an underscore and a key, and ignores every
/// other variable. A member of a listed family whose key the family does
/// not list is bound too, so [`check`] refuses it. A withheld variable's value is held as a
/// [`Secret`], or as [`Bound::NotUnicode`] when it is not Unicode.
/// A shown variable's value is bound as it is, and [`check`] refuses one
/// that is not Unicode unless the variable's type is a path. A name that
/// is not Unicode is ignored, as an undeclared name is.
///
/// An empty value, such as the value of `FOO=`, counts as unset, so
/// `capture` binds nothing for it. When `vars` names a variable more than
/// once, the last non-empty value is bound, and an empty later value leaves
/// an earlier value in place. A variable whose [`Variable::empty`] is
/// [`Empty::Value`] is the exception, which is a variable declared with
/// `empty = "value"` or one that reads its presence: an empty value is a
/// value for it, so `capture` binds its last value, empty or not.
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
        let Some(declaration) = spec.declared(&name) else {
            continue;
        };
        if raw.is_empty() && declaration.variable().empty() == Empty::Unset {
            continue;
        }
        let bound = declaration.variable().capture(raw);
        bindings.bind(name, bound);
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
/// Unicode, its local default decides. An empty value counts as unset
/// unless the mode variable declares `empty = "value"`, and its type then
/// reads it. Any other value that does not fit
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
        // A mode variable that reads its presence decides by the value it
        // was set to, as the child process reads it.
        Some(Ok(Some(Held::Presence(raw)))) => return rule.decide(raw.as_deref()),
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
/// pass through [`capture`], such as a provider's value. A variable
/// declared with `empty = "value"` is the exception: its type reads an
/// empty value, so a string reads it as empty text and an integer refuses
/// it, and its default applies only when it is unbound. A variable that
/// reads its [presence](Variable::presence) is an exception too: any bound
/// value, an empty one included, sets it and reads as true, and it reads
/// as false when it is unbound and optional. A bound value is
/// read in its variable's type, and a value that the type counts as unset,
/// such as a relative path where `absolute = true`, is treated as unbound. An unbound variable takes its default in `mode`,
/// stays unset when it is optional, and is missing when it is required.
/// When the bindings decide another mode than `mode`, the mode variable has
/// a finding too, so a ready environment fits the mode its child decides.
/// A spec without a `[mode]` table is always checked in local mode, which
/// is the only mode its child can decide. When a group is partly set, each
/// of its members that is unset and has no finding of its own gets
/// [`Problem::GroupIncomplete`]. A member counts as set when the check
/// holds a value for it, and a variable that reads its presence counts as
/// set only when it is bound. An invalid mode value checked in
/// production gives the mode variable two findings: `Invalid`, and
/// `ModeMismatch { decided: Local }`.
///
/// Each member of a family that `bindings` binds is checked in the
/// family's type and show after the declared variables, family by family
/// in prefix order and member by member in name order, and a finding names
/// the member. A member whose key a listed family does not list gets
/// [`Problem::UnlistedKey`]. When a listed family is required in `mode`,
/// each listed key whose member is unset gets [`Problem::Missing`].
///
/// The verdict is [`Verdict::Ready`] only when no variable has a finding,
/// and otherwise [`Verdict::Refused`] with each finding in the spec's order
/// and a mode mismatch last (BASE-44).
///
/// A tool that prepares the whole environment of a child process, such as
/// `starmap run`, calls `check`, because it does not know which variables
/// the child reads. [`Env::from_vars`](crate::Env::from_vars) checks only
/// the variables its struct reads and the mode variable.
#[must_use]
pub fn check(spec: &Spec, bindings: &Bindings, mode: Mode) -> Verdict {
    check_with(spec, bindings, mode, Scope::Whole, &[])
}

/// The variables whose findings a check reports.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Scope<'a> {
    /// Every variable the spec declares and every member of a family, as
    /// [`check`] and a tool that prepares a whole environment report them.
    Whole,
    /// The variables that `fields`, the fields of a struct, read: each
    /// variable a field reads, each member of a family a field reads, and
    /// the mode variable, which decides whether each other variable is
    /// required.
    Fields(&'a [Field]),
}

impl Scope<'_> {
    /// Whether a finding about `name` belongs to the scope.
    fn covers(self, spec: &Spec, name: &Name) -> bool {
        let Self::Fields(fields) = self else {
            return true;
        };
        if spec.mode().is_some_and(|rule| rule.variable() == name) {
            return true;
        }
        fields.iter().any(|field| {
            if !field.family {
                return field.name == name.as_str();
            }
            field
                .name
                .parse::<Name>()
                .ok()
                .and_then(|prefix| spec.family(&prefix))
                .is_some_and(|family| family.key_of(name).is_some())
        })
    }

    /// Whether a finding about `name` goes to a deferred field, which holds
    /// it in place of the check refusing: at least one field reads `name`,
    /// every field that reads it is deferred, and `name` is not the mode
    /// variable, because the mode decides the other fields. When a field
    /// that is not deferred reads `name` too, its finding refuses the read,
    /// as it would without the deferred field.
    fn defers(self, spec: &Spec, name: &Name) -> bool {
        let Self::Fields(fields) = self else {
            return false;
        };
        if spec.mode().is_some_and(|rule| rule.variable() == name) {
            return false;
        }
        let mut readers = fields
            .iter()
            .filter(|field| !field.family && field.name == name.as_str())
            .peekable();
        readers.peek().is_some() && readers.all(|field| field.deferred)
    }
}

/// Checks `bindings` as [`check`] does, and runs each of `rules`, the
/// application's own checks of a struct's fields, on its variable when the
/// variable holds a checked value, its default included. A rule that
/// refuses the value adds a [`Problem::Rejected`] finding, which takes its
/// variable's place in the spec's order. A variable that stays unset, or
/// whose value its type refuses, is not passed to its rule.
///
/// Every rule of every variable is checked, a group's rule included, which
/// looks at the values of all of its members. The verdict then keeps only
/// the findings that `scope` covers, so a variable outside the scope never
/// refuses the check. Such a variable that has a finding holds no value in
/// the environment. A finding about the variable of a deferred field in
/// `scope`, other than the mode variable, goes into the environment, where
/// the field reads it, and never refuses the check.
pub(crate) fn check_with(
    spec: &Spec,
    bindings: &Bindings,
    mode: Mode,
    scope: Scope<'_>,
    rules: &[Rule],
) -> Verdict {
    let mode = if spec.mode().is_some() {
        mode
    } else {
        Mode::Local
    };
    let held: Vec<Holding<'_>> = spec
        .variables()
        .map(|(name, variable)| (name, variable, hold(variable, bindings.get(name), mode)))
        .collect();
    let incomplete: Vec<Option<Problem>> = held
        .iter()
        .map(|(name, _, result)| match result {
            Ok(held_value) if !is_set(held_value.as_ref()) => incomplete(spec, &held, name),
            _ => None,
        })
        .collect();
    let mut entries = Vec::new();
    // Each finding with the place of its variable in the spec.
    let mut findings = Vec::new();
    for (place, ((name, variable, result), incomplete)) in
        held.into_iter().zip(incomplete).enumerate()
    {
        match (result, incomplete) {
            (Ok(held), None) => entries.push((name.clone(), held)),
            (Ok(_), Some(problem)) => {
                findings.push((place, Finding::unquoted(name.clone(), problem)))
            }
            (Err(problem), _) => {
                findings.push((place, finding(name, variable, bindings.get(name), problem)));
            }
        }
    }
    let mut members = Vec::new();
    let mut place = spec.variables().count();
    for family in spec.families() {
        let (checked, found) = check_family(family, bindings, mode);
        for (name, value, key) in checked {
            members.push(Member {
                prefix: family.prefix().clone(),
                key,
                name: name.clone(),
            });
            entries.push((name, Some(value)));
        }
        for finding in found {
            findings.push((place, finding));
            place += 1;
        }
    }
    let environment = Environment::new(mode, entries, members);
    for rule in rules {
        let Ok(name) = rule.name.parse::<Name>() else {
            continue;
        };
        let Some((place, variable)) =
            spec.variables()
                .enumerate()
                .find_map(|(place, (declared, variable))| {
                    (*declared == name).then_some((place, variable))
                })
        else {
            continue;
        };
        if let Err(reason) = (rule.run)(&environment, &name) {
            let problem = Problem::Rejected { reason };
            let bound = bindings.get(&name);
            findings.push((place, finding(&name, variable, bound, problem)));
        }
    }
    findings.retain(|(_, finding)| scope.covers(spec, &finding.name));
    findings.sort_by_key(|(place, _)| *place);
    let modes = spec.mode().is_some();
    let (deferred, mut findings): (Vec<Finding>, Vec<Finding>) = findings
        .into_iter()
        .map(|(_, finding)| Finding { modes, ..finding })
        .partition(|finding| scope.defers(spec, &finding.name));
    let decided = self::mode(spec, bindings);
    if let Some(rule) = spec.mode()
        && decided != mode
    {
        findings.push(Finding {
            modes,
            ..Finding::unquoted(rule.variable().clone(), Problem::ModeMismatch { decided })
        });
    }
    if findings.is_empty() {
        Verdict::Ready(environment.with_deferred(deferred))
    } else {
        Verdict::Refused(Refusals { findings })
    }
}

/// Checks each member of `family` that `bindings` binds, in name order, in
/// the family's type and show, and returns each member that holds a value
/// with its key, and each finding in name order. A member whose key a
/// listed family does not list gets [`Problem::UnlistedKey`], and when the
/// family is listed and required in `mode`, each listed key whose member
/// has no value and no finding of its own gets [`Problem::Missing`].
fn check_family(
    family: &FamilyDeclaration,
    bindings: &Bindings,
    mode: Mode,
) -> (Vec<(Name, Held, String)>, Vec<Finding>) {
    let variable = family.variable();
    let mut checked = Vec::new();
    let mut findings = Vec::new();
    for (name, bound) in bindings.iter() {
        let Some(key) = family.key_of(name) else {
            continue;
        };
        if !family.lists(key) {
            findings.push(Finding::unquoted(
                name.clone(),
                Problem::UnlistedKey {
                    family: family.prefix().clone(),
                    keys: family.keys().unwrap_or_default().to_vec(),
                },
            ));
            continue;
        }
        match read(variable, bound) {
            Ok(Some(value)) => checked.push((name.clone(), value, key.to_owned())),
            Ok(None) => {}
            Err(problem) => findings.push(finding(name, variable, Some(bound), problem)),
        }
    }
    if let (Some(keys), Unset::Required) = (family.keys(), variable.unset().in_mode(mode)) {
        for name in keys.iter().filter_map(|key| family.member(key)) {
            let reported = checked.iter().any(|(held, ..)| *held == name)
                || findings.iter().any(|finding| finding.name == name);
            if !reported {
                findings.push(Finding::unquoted(name, Problem::Missing { mode }));
            }
        }
    }
    findings.sort_by(|left, right| left.name.cmp(&right.name));
    (checked, findings)
}

/// One declared variable with the result of holding its value: the value,
/// `None` when it stays unset, or the problem that stops the check.
type Holding<'a> = (&'a Name, &'a Variable, Result<Option<Held>, Problem>);

/// Returns the finding of `name`, which is unset, when its group is partly
/// set: the group, and each member that `held` holds a value for.
fn incomplete(spec: &Spec, held: &[Holding<'_>], name: &Name) -> Option<Problem> {
    let group = spec.group_of(name)?;
    let set: Vec<Name> = group
        .variables()
        .iter()
        .filter(|member| {
            held.iter().any(|(held, _, result)| {
                held == member && matches!(result, Ok(value) if is_set(value.as_ref()))
            })
        })
        .cloned()
        .collect();
    (!set.is_empty()).then(|| Problem::GroupIncomplete {
        group: group.name().clone(),
        set,
    })
}

/// Whether a held value counts as set in its variable's group.
fn is_set(held: Option<&Held>) -> bool {
    held.is_some_and(Held::is_set)
}

/// Holds the checked value of `variable` in `mode`, or `None` when it stays
/// unset. An optional variable that reads its presence and is unbound holds
/// its absence, which reads as false.
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
        Unset::Optional if variable.presence() => Ok(Some(Held::Presence(None))),
        Unset::Optional => Ok(None),
        Unset::Default(value) => Ok(Some(Held::Shown(value.clone()))),
    }
}

/// Returns the finding of `variable`, called `name`, whose bound value is
/// `bound`.
fn finding(name: &Name, variable: &Variable, bound: Option<&Bound>, problem: Problem) -> Finding {
    Finding {
        value: quote(variable, bound, &problem),
        ..Finding::unquoted(name.clone(), problem)
    }
}

/// Returns the quote of the value that `problem` refuses, which only an
/// invalid or rejected plain value of a shown variable has. A withheld variable's
/// value, and any value held as a secret, is never quoted.
fn quote(variable: &Variable, bound: Option<&Bound>, problem: &Problem) -> Option<Quote> {
    match (problem, variable.show(), bound) {
        (Problem::Invalid(_) | Problem::Rejected { .. }, Show::Shown, Some(Bound::Plain(raw))) => {
            Some(Quote::new(raw))
        }
        _ => None,
    }
}

/// Reads `bound` as a value of `variable`. `Ok(None)` means the value
/// counts as unset, which an empty value does unless the variable's
/// [`Variable::empty`] is [`Empty::Value`]. A variable that reads its
/// presence holds any plain value it is set to.
fn read(variable: &Variable, bound: &Bound) -> Result<Option<Held>, Problem> {
    let empty_unset = variable.empty() == Empty::Unset;
    match (variable.show(), bound) {
        (Show::Shown, Bound::Plain(raw)) if variable.presence() => {
            Ok(Some(Held::Presence(Some(raw.clone()))))
        }
        (Show::Shown, Bound::Plain(raw)) if empty_unset && raw.is_empty() => Ok(None),
        (Show::Withheld, Bound::Secret(secret)) if empty_unset && secret.is_empty() => Ok(None),
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
}

impl Refusals {
    /// Returns each finding, in the spec's order of variables, with a mode
    /// mismatch last.
    pub fn iter(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter()
    }
}

impl fmt::Display for Refusals {
    /// Writes each finding as [`Finding`] writes it, separated by `; `,
    /// such as `SHELF_PORT is required and has no value; SHELF_DEBUG: the
    /// value is not one of the words 1, 0, true, false, yes, no, on and
    /// off`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, finding) in self.findings.iter().enumerate() {
            if index > 0 {
                formatter.write_str("; ")?;
            }
            finding.fmt(formatter)?;
        }
        Ok(())
    }
}

/// A variable that stops a check from passing, and why. A field of type
/// [`Deferred`](crate::Deferred) holds the finding of its variable in place
/// of the read refusing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Finding {
    /// The variable.
    pub name: Name,
    /// Why the variable stops the check.
    pub problem: Problem,
    /// The quote of the refused value. Only a [`Problem::Invalid`] or a
    /// [`Problem::Rejected`] of a shown variable whose value is bound as
    /// [`Bound::Plain`] has one; a withheld variable, a value held as a
    /// secret, a default, a missing variable and a mode mismatch never do.
    pub value: Option<Quote>,
    /// Whether the spec has a `[mode]` table, so that a missing variable's
    /// text names the mode only when the spec has modes.
    modes: bool,
}

impl fmt::Display for Finding {
    /// Writes the finding with its variable's name, as [`Refusals`] writes
    /// it, such as `SHELF_PORT is required and has no value`. A missing
    /// variable names its mode only when the spec has a `[mode]` table. A
    /// value that a field's own check refuses is written with the check's
    /// reason, as in `SHELF_PORT: the port must be above 1024`. An invalid
    /// or refused value of a shown variable is quoted after the reason, as
    /// in `SHELF_PORT: the value is not a port from 1 to 65535 (the value
    /// is "80a")`, as [`Quote`] writes it. The text never repeats a
    /// withheld variable's value or a value held as a secret.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = &self.name;
        match &self.problem {
            Problem::Missing { mode } if self.modes => {
                write!(
                    formatter,
                    "{name} is required in {mode} mode and has no value"
                )
            }
            Problem::Missing { .. } => write!(formatter, "{name} is required and has no value"),
            Problem::Invalid(error) => {
                write!(formatter, "{name}: {error}")?;
                self.write_quote(formatter)
            }
            Problem::Rejected { reason } => {
                write!(formatter, "{name}: {reason}")?;
                self.write_quote(formatter)
            }
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
            Problem::UnlistedKey { family, keys } => {
                write!(formatter, "{}", UnlistedKey { name, family, keys })
            }
        }
    }
}

impl Finding {
    /// Returns the finding of `name` for `problem`, without a quote. The
    /// check sets whether the spec has modes before it reports the finding.
    pub(crate) fn unquoted(name: Name, problem: Problem) -> Self {
        Self {
            name,
            problem,
            value: None,
            modes: false,
        }
    }

    /// Writes ` (the value is "…")` after the reason when the finding
    /// quotes its value.
    fn write_quote(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value {
            Some(quote) => write!(formatter, " (the value is {quote})"),
            None => Ok(()),
        }
    }
}

/// A shown value as a refusal quotes it: its first 64 characters, counted
/// as Unicode scalar values, with `…` after them when the value is longer.
/// A value that is not Unicode is read with U+FFFD in place of each
/// invalid sequence. A quote is cut when it is made, so it never holds more
/// of the value than it prints.
///
/// Its `Display` writes the cut text as a JSON string, in double quotes:
/// `"` and `\` are escaped, U+0008, U+000C, a newline, a carriage return
/// and a tab are written as `\b`, `\f`, `\n`, `\r` and `\t`, and every
/// other character below U+0020 as `\u00XX` with lower-case hex digits.
/// A quoted value therefore never breaks a log line or starts another one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote(String);

impl Quote {
    /// How many characters of a value a quote keeps.
    const LIMIT: usize = 64;

    /// Returns the quote of `raw`.
    pub(crate) fn new(raw: &OsStr) -> Self {
        let text = raw.to_string_lossy();
        let mut characters = text.chars();
        let mut cut: String = characters.by_ref().take(Self::LIMIT).collect();
        if characters.next().is_some() {
            cut.push('…');
        }
        Self(cut)
    }

    /// Returns the cut text before it is escaped.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Quote {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_char('"')?;
        for character in self.0.chars() {
            match character {
                '"' => formatter.write_str("\\\""),
                '\\' => formatter.write_str("\\\\"),
                '\u{8}' => formatter.write_str("\\b"),
                '\u{c}' => formatter.write_str("\\f"),
                '\n' => formatter.write_str("\\n"),
                '\r' => formatter.write_str("\\r"),
                '\t' => formatter.write_str("\\t"),
                control if u32::from(control) < 0x20 => {
                    write!(formatter, "\\u{:04x}", u32::from(control))
                }
                other => formatter.write_char(other),
            }?;
        }
        formatter.write_char('"')
    }
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
    /// The application's own check of the field that reads the variable,
    /// which a struct that derives [`Env`](crate::Env) names, refuses the
    /// value. A withheld field's check gives its reason as fixed text, so
    /// the reason never holds the secret.
    Rejected {
        /// Why the check refuses the value, in the application's words.
        reason: String,
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
    /// The variable's name is the prefix of a listed family, an underscore
    /// and a key that the family does not list.
    UnlistedKey {
        /// The family's prefix.
        family: Name,
        /// The keys the family lists, in key order.
        keys: Vec<String>,
    },
}

/// The text of a [`Problem::UnlistedKey`] finding, such as
/// `SHELF_TOKEN_ACMEE is set, and the family SHELF_TOKEN lists only the
/// keys ACME and GLOBEX`.
struct UnlistedKey<'a> {
    /// The member's name.
    name: &'a Name,
    /// The family's prefix.
    family: &'a Name,
    /// The keys the family lists.
    keys: &'a [String],
}

impl fmt::Display for UnlistedKey<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let keys: Vec<&str> = self.keys.iter().map(String::as_str).collect();
        let (noun, joined) = match keys.split_last() {
            Some((last, [])) => ("key", (*last).to_owned()),
            Some((last, rest)) => ("keys", format!("{} and {last}", rest.join(", "))),
            None => ("keys", String::new()),
        };
        write!(
            formatter,
            "{} is set, and the family {} lists only the {noun} {joined}",
            self.name, self.family
        )
    }
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
