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
/// A string's pattern is over 1024 bytes or does not compile.
pub const SPEC_BAD_PATTERN: Code = Code::new("starmap", "spec", "bad_pattern");
/// An enumeration or a url type's scheme list has no values, or repeats one.
pub const SPEC_EMPTY_ENUMERATION: Code = Code::new("starmap", "spec", "empty_enumeration");
/// A secret or generated variable is shown.
pub const SPEC_SHOWN_SECRET: Code = Code::new("starmap", "spec", "shown_secret");
/// A withheld variable has a type other than string, with or without a
/// pattern.
pub const SPEC_WITHHELD_TYPE: Code = Code::new("starmap", "spec", "withheld_type");
/// A withheld variable has a default.
pub const SPEC_WITHHELD_DEFAULT: Code = Code::new("starmap", "spec", "withheld_default");
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
/// A file points to another file with `spec` and declares variables too.
pub const SPEC_POINTER_DECLARES: Code = Code::new("starmap", "spec", "pointer_declares");
/// A pointer names a file that is a pointer too.
pub const SPEC_POINTER_CHAIN: Code = Code::new("starmap", "spec", "pointer_chain");
/// `explain` names a variable that the spec does not declare.
pub const VARIABLE_UNKNOWN: Code = Code::new("starmap", "variable", "unknown");
/// `check` finds source code that reads a variable the spec does not
/// declare.
pub const VARIABLE_UNDECLARED: Code = Code::new("starmap", "variable", "undeclared");
/// A file or directory under `src` cannot be read as Rust source.
pub const SOURCE_UNREADABLE: Code = Code::new("starmap", "source", "unreadable");
/// `diff` runs in a directory that is not in a git repository.
pub const REVISION_NO_REPOSITORY: Code = Code::new("starmap", "revision", "no_repository");
/// `diff` names a revision that names no commit.
pub const REVISION_NOT_FOUND: Code = Code::new("starmap", "revision", "not_found");
/// git cannot start, or cannot read the revision that `diff` names.
pub const REVISION_UNREADABLE: Code = Code::new("starmap", "revision", "unreadable");

/// `.env` sets a secret or generated variable, which only a bind supplies.
pub const DOTENV_SECRET: Code = Code::new("starmap", "dotenv", "secret");
/// `.env` holds a line that the `.env` format does not allow.
pub const DOTENV_SYNTAX: Code = Code::new("starmap", "dotenv", "syntax");
/// `.env` exists and cannot be read, or is over 256 KiB.
pub const DOTENV_UNREADABLE: Code = Code::new("starmap", "dotenv", "unreadable");
/// `--set` gives a value to a secret or generated variable.
pub const VARIABLE_SECRET: Code = Code::new("starmap", "variable", "secret");
/// A required variable has no value in the run's mode.
pub const VARIABLE_MISSING: Code = Code::new("starmap", "variable", "missing");
/// A variable's value does not fit its type.
pub const VARIABLE_INVALID: Code = Code::new("starmap", "variable", "invalid");
/// No provider of a provided variable's kind is present, and nothing else
/// gives it a value.
pub const BIND_NO_PROVIDER: Code = Code::new("starmap", "bind", "no_provider");
/// A path bind names a place that `run` has no directory for.
pub const BIND_NO_PLACE: Code = Code::new("starmap", "bind", "no_place");
/// A required secret has no bind, no value in the shell and no saved
/// value, and no person can type it.
pub const SECRET_UNBOUND: Code = Code::new("starmap", "secret", "unbound");
/// A secret store could not return a secret: it is locked, has no such
/// item, was refused, is missing on this platform, failed, or did not
/// answer in time. The message names which.
pub const SECRET_UNREADABLE: Code = Code::new("starmap", "secret", "unreadable");

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
    &SPEC_BAD_PATTERN,
    &SPEC_EMPTY_ENUMERATION,
    &SPEC_SHOWN_SECRET,
    &SPEC_WITHHELD_TYPE,
    &SPEC_WITHHELD_DEFAULT,
    &SPEC_BIND_MISMATCH,
    &SPEC_BAD_BYTES,
    &SPEC_UNKNOWN_PLACEHOLDER,
    &SPEC_MODE_UNDECLARED,
    &SPEC_MODE_SOURCE,
    &SPEC_POINTER_DECLARES,
    &SPEC_POINTER_CHAIN,
    &VARIABLE_UNKNOWN,
    &VARIABLE_UNDECLARED,
    &SOURCE_UNREADABLE,
    &REVISION_NO_REPOSITORY,
    &REVISION_NOT_FOUND,
    &REVISION_UNREADABLE,
    &DOTENV_SECRET,
    &DOTENV_SYNTAX,
    &DOTENV_UNREADABLE,
    &VARIABLE_SECRET,
    &VARIABLE_MISSING,
    &VARIABLE_INVALID,
    &BIND_NO_PROVIDER,
    &BIND_NO_PLACE,
    &SECRET_UNBOUND,
    &SECRET_UNREADABLE,
];
