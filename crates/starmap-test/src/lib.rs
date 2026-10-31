//! starmap-test checks that an application's `starmap.toml` declares exactly
//! the environment variables its code reads. An application calls the check
//! from an ordinary `#[test]`, so the declaration serves as its catalog of
//! environment variables (BASE-28).
//!
//! ```rust,ignore
//! #[test]
//! fn the_env_struct_matches_starmap_toml() {
//!     starmap_test::check::<Env>(include_str!("../starmap.toml"));
//! }
//! ```

use std::collections::BTreeSet;
use std::fmt::Write as _;

use starmap::{Env, Field, Form, Mode, Presence, Shape, Show, Spec, Unset, Variable};

#[cfg(test)]
mod tests;

/// A variable that `starmap.toml` declares and the struct need not read,
/// such as one that only a dependency reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allow {
    /// The name of the variable.
    pub name: &'static str,
    /// Why the struct does not read the variable.
    pub reason: &'static str,
}

/// Checks that the struct `E` and the text of its `starmap.toml` agree, as
/// [`check_allowing`] does with no [`Allow`] entries.
///
/// # Panics
///
/// Panics as [`check_allowing`] does.
pub fn check<E: Env>(spec: &str) {
    check_allowing::<E>(spec, &[]);
}

/// Checks that the struct `E` and the text of its `starmap.toml` agree,
/// where each of `allow` names a declared variable the struct need not
/// read.
///
/// The check compares the names the struct reads with the names the spec
/// declares, each field's shape with its variable's type, each field's
/// presence with the variable's unset rule in both modes, and each field's
/// show with the variable's show. An [`Allow`] entry that excuses nothing,
/// because the struct reads the variable or the spec does not declare it,
/// is a disagreement too.
///
/// # Panics
///
/// Panics when `spec` differs from the text `E` embeds, which happens when
/// the test reads another file than the derive names. Otherwise it panics
/// with every disagreement at once when the spec does not parse or the
/// struct and the spec disagree.
#[expect(
    clippy::panic,
    reason = "a failed check fails the test that calls it, as an assertion does"
)]
pub fn check_allowing<E: Env>(spec: &str, allow: &[Allow]) {
    let type_name = std::any::type_name::<E>();
    if spec != E::SPEC {
        panic!(
            "the spec text the test passes differs from the starmap.toml that {type_name} embeds, so the test reads another file than the derive names"
        );
    }
    if let Err(report) = compare(type_name, E::FIELDS, spec, allow) {
        panic!("{report}");
    }
}

/// Returns the report of every disagreement between the struct `type_name`
/// with `fields` and the spec `text`, or `Ok` when they agree.
fn compare(type_name: &str, fields: &[Field], text: &str, allow: &[Allow]) -> Result<(), String> {
    let spec =
        Spec::parse(text).map_err(|error| format!("starmap.toml does not parse: {error}"))?;
    let mut found: Vec<(&str, String)> = Vec::new();
    for field in fields {
        let variable = field
            .name
            .parse()
            .ok()
            .and_then(|name| spec.variable(&name));
        match variable {
            Some(variable) => {
                let messages = disagreements(field, variable, spec.mode().is_some());
                found.extend(messages.into_iter().map(|message| (field.name, message)));
            }
            None => found.push((
                field.name,
                "the struct reads it, and starmap.toml does not declare it".to_owned(),
            )),
        }
    }
    let read: BTreeSet<&str> = fields.iter().map(|field| field.name).collect();
    let allowed: BTreeSet<&str> = allow.iter().map(|entry| entry.name).collect();
    for (name, _) in spec.variables() {
        let name = name.as_str();
        if !read.contains(name) && !allowed.contains(name) {
            found.push((
                name,
                "starmap.toml declares it, and the struct neither reads it nor has an Allow entry for it".to_owned(),
            ));
        }
    }
    for entry in allow {
        let declared = entry
            .name
            .parse()
            .is_ok_and(|name| spec.variable(&name).is_some());
        let why = if read.contains(entry.name) {
            "the struct reads it"
        } else if !declared {
            "starmap.toml does not declare it"
        } else {
            continue;
        };
        found.push((
            entry.name,
            format!("its Allow entry excuses nothing, because {why}"),
        ));
    }
    if found.is_empty() {
        return Ok(());
    }
    found.sort_by(|left, right| left.0.cmp(right.0));
    let width = found.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    let places = if found.len() == 1 { "place" } else { "places" };
    let mut report = format!(
        "starmap.toml and {type_name} disagree in {} {places}:",
        found.len()
    );
    for (name, message) in found {
        // Writing to a String cannot fail.
        let _ = write!(report, "\n  {name:width$}  {message}");
    }
    Err(report)
}

/// Returns each way `field` disagrees with its `variable`. `has_mode` says
/// whether the spec has a `[mode]` table, without which the variable is
/// only ever read in local mode.
fn disagreements(field: &Field, variable: &Variable, has_mode: bool) -> Vec<String> {
    let rust_type = field.rust_type;
    let mut found = Vec::new();
    match (variable.show(), field.show) {
        (Show::Withheld, Show::Shown) => found.push(format!(
            "starmap.toml withholds it, and the field type {rust_type} is not starmap::Secret<String>"
        )),
        (Show::Shown, Show::Withheld) => found.push(format!(
            "starmap.toml shows it, and the field type {rust_type} holds a withheld value"
        )),
        _ => {}
    }
    found.extend(shape(field, variable.form()));
    let modes: &[Mode] = if has_mode {
        &[Mode::Local, Mode::Production]
    } else {
        &[Mode::Local]
    };
    let unset_in: Vec<String> = modes
        .iter()
        .filter(|mode| matches!(variable.unset().in_mode(**mode), Unset::Optional))
        .map(|mode| format!("{mode} mode"))
        .collect();
    match (field.presence, unset_in.is_empty()) {
        (Presence::Always, false) => found.push(format!(
            "starmap.toml lets it stay unset in {}, and the field type {rust_type} is not inside an Option",
            unset_in.join(" and ")
        )),
        (Presence::Maybe, true) => found.push(format!(
            "starmap.toml always gives it a value, and the field type Option<{rust_type}> holds an unset value"
        )),
        _ => {}
    }
    found
}

/// Returns each way the shape of `field` cannot hold a value of `form`.
fn shape(field: &Field, form: &Form) -> Vec<String> {
    let rust_type = field.rust_type;
    match (field.shape, form) {
        (Shape::Path, Form::Path { .. })
        | (Shape::Port, Form::Port)
        | (Shape::Url, Form::Url)
        | (Shape::Boolean, Form::Boolean)
        | (Shape::String | Shape::OsString, Form::String)
        | (Shape::OsString, Form::Path { .. }) => Vec::new(),
        (Shape::Integer { min, max }, Form::Integer(range)) => {
            let mut found = Vec::new();
            if range.max() > max {
                found.push(format!(
                    "starmap.toml allows up to {}, and the field type {rust_type} holds at most {max}",
                    range.max()
                ));
            }
            if range.min() < min {
                found.push(format!(
                    "starmap.toml allows down to {}, and the field type {rust_type} holds at least {min}",
                    range.min()
                ));
            }
            found
        }
        (Shape::Enumeration(values), Form::Enumeration(choices)) => choices
            .iter()
            .filter(|choice| !values.contains(&choice.as_str()))
            .map(|choice| {
                format!("starmap.toml allows {choice}, and the field type {rust_type} has no value {choice}")
            })
            .collect(),
        (shape, form) => vec![format!(
            "starmap.toml types it as {}, and the field type {rust_type} holds {}",
            form_words(form),
            shape_words(shape)
        )],
    }
}

/// Returns the words for a value of `form`, such as `a port`.
fn form_words(form: &Form) -> String {
    match form {
        Form::Path { absolute: true } => "an absolute path".to_owned(),
        Form::Path { absolute: false } => "a path".to_owned(),
        Form::Port => "a port".to_owned(),
        Form::Url => "a url".to_owned(),
        Form::Boolean => "a boolean".to_owned(),
        Form::Integer(range) => format!("a whole number from {} to {}", range.min(), range.max()),
        Form::Enumeration(choices) => format!("one of {choices}"),
        Form::String => "a string".to_owned(),
        _ => "a type this check does not know".to_owned(),
    }
}

/// Returns the words for what a field of `shape` holds, such as `a path`.
fn shape_words(shape: Shape) -> &'static str {
    match shape {
        Shape::Path => "a path",
        Shape::Port => "a port",
        Shape::Url => "a url",
        Shape::Boolean => "a boolean",
        Shape::Integer { .. } => "a whole number",
        Shape::Enumeration(_) => "an enumeration",
        Shape::String => "a string",
        Shape::OsString => "a string or a path",
        _ => "a shape this check does not know",
    }
}
