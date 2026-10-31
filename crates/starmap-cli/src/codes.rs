//! The failure codes of the `starmap` command. Each code has the form
//! `starmap::<area>::<name>`, where the area is a noun from starmap's
//! domain (CLI-3).

use crumbkit::Code;

/// No file is at the spec path.
pub const SPEC_NOT_FOUND: Code = Code::new("starmap", "spec", "not_found");
/// The spec file exists and cannot be read, such as a directory.
pub const SPEC_UNREADABLE: Code = Code::new("starmap", "spec", "unreadable");
/// `init` finds a file at the spec path already.
pub const SPEC_EXISTS: Code = Code::new("starmap", "spec", "exists");
/// `init` cannot create or write the spec file.
pub const SPEC_UNWRITABLE: Code = Code::new("starmap", "spec", "unwritable");
/// The spec file is over 256 KiB.
pub const SPEC_TOO_LARGE: Code = Code::new("starmap", "spec", "too_large");
/// The spec file is not valid TOML, or not UTF-8 text.
pub const SPEC_SYNTAX: Code = Code::new("starmap", "spec", "syntax");
/// A table holds a key that starmap does not know.
pub const SPEC_UNKNOWN_FIELD: Code = Code::new("starmap", "spec", "unknown_field");
/// A table lacks a key it needs.
pub const SPEC_MISSING_FIELD: Code = Code::new("starmap", "spec", "missing_field");
/// A value has the wrong TOML type or is not one its key takes.
pub const SPEC_BAD_VALUE: Code = Code::new("starmap", "spec", "bad_value");
/// A variable name is not upper case letters, digits and underscores.
pub const SPEC_BAD_NAME: Code = Code::new("starmap", "spec", "bad_name");
/// A value still ends in the `?` that `starmap init` wrote.
pub const SPEC_GUESS: Code = Code::new("starmap", "spec", "guess");
/// A default does not fit the variable's type.
pub const SPEC_BAD_DEFAULT: Code = Code::new("starmap", "spec", "bad_default");
/// An integer's minimum is above its maximum.
pub const SPEC_BAD_RANGE: Code = Code::new("starmap", "spec", "bad_range");
/// An enumeration has no values, or repeats one.
pub const SPEC_EMPTY_ENUMERATION: Code = Code::new("starmap", "spec", "empty_enumeration");
/// A secret or generated variable is shown.
pub const SPEC_SHOWN_SECRET: Code = Code::new("starmap", "spec", "shown_secret");
/// A withheld variable has a type other than string.
pub const SPEC_SECRET_FORM: Code = Code::new("starmap", "spec", "secret_form");
/// A bind does not fit the variable's source.
pub const SPEC_BIND_MISMATCH: Code = Code::new("starmap", "spec", "bind_mismatch");
/// `bytes` lies outside 16 to 1024, or is on a variable that is not
/// generated.
pub const SPEC_BAD_BYTES: Code = Code::new("starmap", "spec", "bad_bytes");
/// A path template names an unknown placeholder.
pub const SPEC_UNKNOWN_PLACEHOLDER: Code = Code::new("starmap", "spec", "unknown_placeholder");
/// `[mode]` names a variable the file does not declare.
pub const SPEC_MODE_UNDECLARED: Code = Code::new("starmap", "spec", "mode_undeclared");
/// The mode variable is not a setting or inherited variable.
pub const SPEC_MODE_SOURCE: Code = Code::new("starmap", "spec", "mode_source");
/// `explain` names a variable that the spec does not declare.
pub const VARIABLE_UNKNOWN: Code = Code::new("starmap", "variable", "unknown");
/// `check` finds source code that reads a variable the spec does not
/// declare.
pub const VARIABLE_UNDECLARED: Code = Code::new("starmap", "variable", "undeclared");
/// A file or directory under `src` cannot be read as Rust source.
pub const SOURCE_UNREADABLE: Code = Code::new("starmap", "source", "unreadable");

/// Every code above.
pub const ALL: &[&Code] = &[
    &SPEC_NOT_FOUND,
    &SPEC_UNREADABLE,
    &SPEC_EXISTS,
    &SPEC_UNWRITABLE,
    &SPEC_TOO_LARGE,
    &SPEC_SYNTAX,
    &SPEC_UNKNOWN_FIELD,
    &SPEC_MISSING_FIELD,
    &SPEC_BAD_VALUE,
    &SPEC_BAD_NAME,
    &SPEC_GUESS,
    &SPEC_BAD_DEFAULT,
    &SPEC_BAD_RANGE,
    &SPEC_EMPTY_ENUMERATION,
    &SPEC_SHOWN_SECRET,
    &SPEC_SECRET_FORM,
    &SPEC_BIND_MISMATCH,
    &SPEC_BAD_BYTES,
    &SPEC_UNKNOWN_PLACEHOLDER,
    &SPEC_MODE_UNDECLARED,
    &SPEC_MODE_SOURCE,
    &VARIABLE_UNKNOWN,
    &VARIABLE_UNDECLARED,
    &SOURCE_UNREADABLE,
];
