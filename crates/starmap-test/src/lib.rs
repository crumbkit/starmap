//! starmap-test checks that an application's `starmap.toml` declares exactly
//! the environment variables its code reads. An application calls the check
//! from an ordinary `#[test]`, so the declaration serves as its catalog of
//! environment variables (BASE-28).
//!
//! ```rust,ignore
//! #[test]
//! fn the_env_struct_matches_starmap_toml() {
//!     starmap_test::check::<Env>();
//! }
//! ```
//!
//! The check reads the spec that the struct embeds, [`Env::SPEC`], which
//! is the file the struct reads at run time, so a test cannot check the
//! struct against another file. Each message names that file by
//! [`Env::SPEC_FILE`], which is the file a pointer names when the struct's
//! `starmap.toml` is a pointer.
//!
//! An application whose commands each read their own struct from one
//! `starmap.toml` checks the structs together with [`check_together`], so
//! a variable counts as read when any of the structs reads it.
//!
//! ```rust,ignore
//! #[test]
//! fn the_structs_together_read_every_declared_variable() {
//!     use starmap_test::Reader;
//!     starmap_test::check_together(&[Reader::of::<Serve>(), Reader::of::<Api>()], &[]);
//! }
//! ```

use std::collections::BTreeSet;
use std::fmt::Write as _;

use starmap::{Env, Field, Mode, Presence, Shape, Show, Spec, Type, Unset, Variable};

#[cfg(test)]
mod tests;

/// A variable that `starmap.toml` declares and the struct need not read,
/// such as one that only a dependency reads.
///
/// ```
/// const ALLOW: &[starmap_test::Allow] =
///     &[starmap_test::Allow::new("XDG_CONFIG_HOME", "only a dependency reads it")];
/// assert_eq!(ALLOW[0].name, "XDG_CONFIG_HOME");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Allow {
    /// The name of the variable.
    pub name: &'static str,
    /// Why the struct does not read the variable.
    pub reason: &'static str,
}

impl Allow {
    /// Returns the entry that lets the struct leave the variable `name`
    /// unread, for `reason`.
    #[must_use]
    pub const fn new(name: &'static str, reason: &'static str) -> Self {
        Self { name, reason }
    }
}

/// Checks that the struct `E` and the `starmap.toml` it embeds agree, as
/// [`check_allowing`] does with no [`Allow`] entries.
///
/// # Panics
///
/// Panics as [`check_allowing`] does.
pub fn check<E: Env>() {
    check_allowing::<E>(&[]);
}

/// Checks that the struct `E` and the `starmap.toml` it embeds agree,
/// where each of `allow` names a declared variable the struct need not
/// read.
///
/// The check compares the names the struct reads with the names the spec
/// declares, each field's shape with its variable's type, each field's
/// presence with the variable's unset rule in both modes, and each field's
/// show with the variable's show. A field that reads a family into a
/// `starmap::Family<T>` is compared with the family's declaration: its
/// prefix must name a family, and its value type must fit the family's
/// type and show. A declared family that no field reads is a disagreement,
/// as a declared variable is, and an [`Allow`] entry may name its prefix.
/// A `starmap::Deferred<T>` field is compared by the `T` inside it, and
/// its variable must always have a value, as a `T` field's must; a
/// `Deferred<Option<T>>` field is compared as an `Option<T>` field is. A
/// deferred field that reads the mode variable is a disagreement, because
/// the read refuses an invalid mode value even when its field is deferred.
/// An [`Allow`] entry that excuses nothing,
/// because the struct reads the variable or the spec does not declare it,
/// is a disagreement too.
///
/// # Panics
///
/// Panics with every disagreement at once when the spec does not parse or
/// the struct and the spec disagree.
#[expect(
    clippy::panic,
    reason = "a failed check fails the test that calls it, as an assertion does"
)]
pub fn check_allowing<E: Env>(allow: &[Allow]) {
    let type_name = std::any::type_name::<E>();
    if let Err(report) = compare(type_name, E::SPEC_FILE, E::FIELDS, E::SPEC, allow) {
        panic!("{report}");
    }
}

/// One struct that [`check_together`] checks: its name, its fields and the
/// `starmap.toml` it embeds.
#[derive(Debug, Clone, Copy)]
pub struct Reader {
    /// The struct's type name.
    type_name: &'static str,
    /// The path of the file the struct embeds, as [`Env::SPEC_FILE`] gives
    /// it.
    file: &'static str,
    /// The text of the file the struct embeds.
    spec: &'static str,
    /// The struct's fields.
    fields: &'static [Field],
}

impl Reader {
    /// Returns the reader of the struct `E`.
    #[must_use]
    pub fn of<E: Env>() -> Self {
        Self {
            type_name: std::any::type_name::<E>(),
            file: E::SPEC_FILE,
            spec: E::SPEC,
            fields: E::FIELDS,
        }
    }
}

/// Checks that the structs of `readers`, which each embed the same
/// `starmap.toml`, agree with it together, where each of `allow` names a
/// declared variable that none of them need read.
///
/// Each struct's fields are compared with their declarations as
/// [`check_allowing`] compares them. A declared variable or family counts
/// as read when any of the structs reads it, so an application whose
/// commands each read their own struct keeps one declaration for all of
/// them. Each struct's read enforces only the variables that struct reads,
/// and this check is what keeps every declared variable read by some
/// struct. An [`Allow`] entry that names a variable one of the structs
/// reads excuses nothing, and is a disagreement.
///
/// # Panics
///
/// Panics when `readers` is empty, when the structs embed different files,
/// and otherwise with every disagreement at once when the spec does not
/// parse or the structs and the spec disagree.
#[expect(
    clippy::panic,
    reason = "a failed check fails the test that calls it, as an assertion does"
)]
pub fn check_together(readers: &[Reader], allow: &[Allow]) {
    let Some(first) = readers.first() else {
        panic!("check_together needs at least one struct");
    };
    if let Some(other) = readers.iter().find(|reader| reader.spec != first.spec) {
        panic!(
            "check_together checks structs that embed one file, and {} embeds {}, which is not the file that {} embeds",
            other.type_name, other.file, first.type_name
        );
    }
    let structs: Vec<(&str, &[Field])> = readers
        .iter()
        .map(|reader| (reader.type_name, reader.fields))
        .collect();
    if let Err(report) = compare_together(&structs, first.file, first.spec, allow) {
        panic!("{report}");
    }
}

/// Returns the report of every disagreement between the struct `type_name`
/// with `fields` and the spec `text`, read from `file`, or `Ok` when they
/// agree. Each message names `file`.
fn compare(
    type_name: &str,
    file: &str,
    fields: &[Field],
    text: &str,
    allow: &[Allow],
) -> Result<(), String> {
    compare_together(&[(type_name, fields)], file, text, allow)
}

/// Returns the report of every disagreement between `structs`, each a type
/// name with its fields, and the spec `text`, read from `file`, or `Ok`
/// when they agree. A declared variable counts as read when any of the
/// structs reads it. When there is more than one struct, each message about
/// a field names the struct whose field it is.
fn compare_together(
    structs: &[(&str, &[Field])],
    file: &str,
    text: &str,
    allow: &[Allow],
) -> Result<(), String> {
    let spec = Spec::parse(text).map_err(|error| format!("{file} does not parse: {error}"))?;
    let several = structs.len() > 1;
    let mut found: Vec<(&str, String)> = Vec::new();
    for (type_name, field) in structs
        .iter()
        .flat_map(|(type_name, fields)| fields.iter().map(move |field| (*type_name, field)))
    {
        let name = field.name.parse().ok();
        let messages = if field.family {
            match name.as_ref().and_then(|prefix| spec.family(prefix)) {
                Some(family) => kinds(file, field, family.variable()),
                None => vec![
                    "the struct reads it as a family, and starmap.toml declares no family with that prefix"
                        .to_owned(),
                ],
            }
        } else {
            match name.as_ref().and_then(|name| spec.variable(name)) {
                Some(variable) => {
                    let mut found = disagreements(file, field, variable, spec.mode().is_some());
                    let mode_variable = spec
                        .mode()
                        .is_some_and(|rule| Some(rule.variable()) == name.as_ref());
                    if field.deferred && mode_variable {
                        found.push(format!(
                            "{file} makes it the mode variable, which decides every other variable, and the field type {} defers it",
                            written(field, field.presence)
                        ));
                    }
                    found
                }
                None => {
                    vec!["the struct reads it, and starmap.toml does not declare it".to_owned()]
                }
            }
        };
        found.extend(messages.into_iter().map(|message| {
            let message = if several {
                format!("in {type_name}, {message}")
            } else {
                message
            };
            (field.name, message)
        }));
    }
    let read_by = |family: bool| -> BTreeSet<&str> {
        structs
            .iter()
            .flat_map(|(_, fields)| fields.iter())
            .filter(|field| field.family == family)
            .map(|field| field.name)
            .collect()
    };
    // The struct an Allow message names as reading `name`.
    let reader_of = |name: &str| {
        if several {
            structs
                .iter()
                .find(|(_, fields)| fields.iter().any(|field| field.name == name))
                .map_or("the struct", |(type_name, _)| *type_name)
        } else {
            "the struct"
        }
    };
    let (read, read_families) = (read_by(false), read_by(true));
    let allowed: BTreeSet<&str> = allow.iter().map(|entry| entry.name).collect();
    let unread_variables = spec
        .variables()
        .map(|(name, _)| (name.as_str(), "it", &read))
        .filter(|(name, _, read)| !read.contains(name));
    let unread_families = spec
        .families()
        .map(|family| (family.prefix().as_str(), "the family", &read_families))
        .filter(|(prefix, _, read)| !read.contains(prefix));
    let unread = if several {
        ", none of the structs reads it, and no Allow entry names it"
    } else {
        ", and the struct neither reads it nor has an Allow entry for it"
    };
    for (name, what, _) in unread_variables.chain(unread_families) {
        if !allowed.contains(name) {
            found.push((name, format!("{file} declares {what}{unread}")));
        }
    }
    for entry in allow {
        let declared = entry
            .name
            .parse()
            .is_ok_and(|name| spec.variable(&name).is_some() || spec.family(&name).is_some());
        let why = if read.contains(entry.name) || read_families.contains(entry.name) {
            format!("{} reads it", reader_of(entry.name))
        } else if !declared {
            format!("{file} does not declare it")
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
    let names: Vec<&str> = structs.iter().map(|(type_name, _)| *type_name).collect();
    let who = match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => {
            format!("the structs {} and {last}", rest.join(", "))
        }
        _ => names.concat(),
    };
    let mut report = format!("{file} and {who} disagree in {} {places}:", found.len());
    for (name, message) in found {
        // Writing to a String cannot fail.
        let _ = write!(report, "\n  {name:width$}  {message}");
    }
    Err(report)
}

/// Returns each way `field` disagrees with its `variable`, which `file`
/// declares. `has_mode` says whether the spec has a `[mode]` table, without
/// which the variable is only ever read in local mode.
fn disagreements(file: &str, field: &Field, variable: &Variable, has_mode: bool) -> Vec<String> {
    let mut found = kinds(file, field, variable);
    let modes: &[Mode] = if has_mode {
        &[Mode::Local, Mode::Production]
    } else {
        &[Mode::Local]
    };
    // A variable that reads its presence always has a value, true or false.
    let unset_in: Vec<String> = modes
        .iter()
        .filter(|mode| matches!(variable.unset().in_mode(**mode), Unset::Optional))
        .filter(|_| !variable.presence())
        .map(|mode| format!("{mode} mode"))
        .collect();
    match (field.presence, unset_in.is_empty()) {
        (Presence::Always, false) => {
            let fix = if field.deferred {
                format!("is not {}", written(field, Presence::Maybe))
            } else {
                "is not inside an Option".to_owned()
            };
            found.push(format!(
                "{file} lets it stay unset in {}, and the field type {} {fix}",
                unset_in.join(" and "),
                written(field, Presence::Always)
            ));
        }
        (Presence::Maybe, true) => found.push(format!(
            "{file} always gives it a value, and the field type {} holds an unset value",
            written(field, Presence::Maybe)
        )),
        _ => {}
    }
    found
}

/// Returns the type of `field` as the struct writes it with `presence`,
/// such as `Option<url::Url>`, or `starmap::Deferred<Option<url::Url>>`
/// for a deferred field.
fn written(field: &Field, presence: Presence) -> String {
    let rust_type = field.rust_type;
    match (field.deferred, presence) {
        (false, Presence::Always) => rust_type.to_owned(),
        (false, Presence::Maybe) => format!("Option<{rust_type}>"),
        (true, Presence::Always) => format!("starmap::Deferred<{rust_type}>"),
        (true, Presence::Maybe) => format!("starmap::Deferred<Option<{rust_type}>>"),
    }
}

/// Returns each way the show and the shape of `field` disagree with
/// `variable`, which `file` declares, or with the fields every member of a
/// family shares. A family field's type holds each member's value, so
/// these are the only disagreements a family field can have.
fn kinds(file: &str, field: &Field, variable: &Variable) -> Vec<String> {
    let rust_type = field.rust_type;
    let mut found = Vec::new();
    match (variable.show(), field.show) {
        (Show::Withheld, Show::Shown) => found.push(format!(
            "{file} withholds it, and the field type {rust_type} is not starmap::Secret or starmap::SecretBytes"
        )),
        (Show::Shown, Show::Withheld) => found.push(format!(
            "{file} shows it, and the field type {rust_type} holds a withheld value"
        )),
        _ => {}
    }
    found.extend(shape(file, field, variable.value_type()));
    found
}

/// Returns each way the shape of `field` cannot hold a value of `ty`, which
/// `file` declares.
fn shape(file: &str, field: &Field, ty: &Type) -> Vec<String> {
    let rust_type = field.rust_type;
    match (field.shape, ty) {
        (Shape::Path, Type::Path { .. })
        | (Shape::Port, Type::Port)
        | (Shape::Url, Type::Url { .. })
        | (Shape::SocketAddress, Type::SocketAddress)
        | (Shape::Boolean, Type::Boolean { .. })
        | (Shape::String | Shape::OsString, Type::String { .. })
        | (Shape::Bytes, Type::Bytes(_))
        | (Shape::OsString, Type::Path { .. }) => Vec::new(),
        (Shape::Integer { min, max }, Type::Integer(range)) => {
            let mut found = Vec::new();
            if range.max() > max {
                found.push(format!(
                    "{file} allows up to {}, and the field type {rust_type} holds at most {max}",
                    range.max()
                ));
            }
            if range.min() < min {
                found.push(format!(
                    "{file} allows down to {}, and the field type {rust_type} holds at least {min}",
                    range.min()
                ));
            }
            found
        }
        (Shape::Enumeration(values), Type::Enumeration(choices)) => choices
            .iter()
            .filter(|choice| !values.contains(&choice.as_str()))
            .map(|choice| {
                format!(
                    "{file} allows {choice}, and the field type {rust_type} has no value {choice}"
                )
            })
            .collect(),
        (shape, ty) => vec![format!(
            "{file} types it as {}, and the field type {rust_type} holds {}",
            ty,
            shape_words(shape)
        )],
    }
}

/// Returns the words for what a field of `shape` holds, such as `a path`.
fn shape_words(shape: Shape) -> &'static str {
    match shape {
        Shape::Path => "a path",
        Shape::Port => "a port",
        Shape::Url => "a url",
        Shape::SocketAddress => "an ip socket address",
        Shape::Boolean => "a boolean",
        Shape::Integer { .. } => "a whole number",
        Shape::Enumeration(_) => "an enumeration",
        Shape::String => "a string",
        Shape::OsString => "a string or a path",
        Shape::Bytes => "bytes",
        _ => "a shape this check does not know",
    }
}
