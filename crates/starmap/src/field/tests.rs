//! Tests for the fields of a derived struct and the types they hold. The
//! integration test tests/derive.rs reads the example struct through them.

use std::ffi::OsString;

use super::{Field, FieldError, FieldProblem, FieldType, Presence, ReadField as _, Shape};
use crate::{Environment, Mode, Secret, Spec, Verdict, capture, check};

/// A spec with a path, a string and a withheld token.
const SPEC: &str = r#"
[variables.SHELF_DATABASE]
purpose = "The SQLite file that holds the catalogue."
type = "path"
unset = { default = "books.db" }
show = "shown"
source = "setting"

[variables.SHELF_TITLE]
purpose = "The title of the web view."
type = "string"
unset = { default = "Shelf" }
show = "shown"
source = "setting"

[variables.SHELF_MIRROR_TOKEN]
purpose = "The token shelf sends to the catalogue mirror."
type = "string"
unset = { default = "anonymous" }
show = "withheld"
source = "secret"
"#;

/// Checks `vars` against [`SPEC`] in local mode.
fn environment(vars: Vec<(OsString, OsString)>) -> Environment {
    let spec = Spec::parse(SPEC).unwrap();
    match check(&spec, &capture(&spec, vars), Mode::Local) {
        Verdict::Ready(environment) => environment,
        Verdict::Refused(refusals) => panic!("the check refused: {refusals:?}"),
    }
}

/// Reads the variable `name` from `environment` as a field of type `T`
/// that the variable must have.
fn required<T: FieldType>(environment: &Environment, name: &'static str) -> Result<T, FieldError> {
    Field::new::<T>(name, "T", Presence::Always).require(environment)
}

/// The text `SHELF_TITLE` holds in the tests of a value that is not
/// Unicode.
#[cfg(unix)]
fn not_unicode() -> Vec<(OsString, OsString)> {
    use std::os::unix::ffi::OsStringExt as _;
    vec![("SHELF_TITLE".into(), OsString::from_vec(vec![b'S', 0xff]))]
}

#[test]
fn an_integer_type_gives_its_bounds_clamped_to_those_of_i64() {
    assert_eq!(
        i8::SHAPE,
        Shape::Integer {
            min: -128,
            max: 127
        }
    );
    assert_eq!(
        u64::SHAPE,
        Shape::Integer {
            min: 0,
            max: i64::MAX
        }
    );
}

#[test]
fn an_os_string_field_reads_a_path_variable() {
    let environment = environment(Vec::new());

    assert_eq!(
        required(&environment, "SHELF_DATABASE"),
        Ok(OsString::from("books.db"))
    );
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test checks the secret that the field reads"
)]
fn a_secret_field_reads_a_copy_of_the_withheld_default() {
    let environment = environment(Vec::new());

    let secret: Secret<String> = required(&environment, "SHELF_MIRROR_TOKEN").unwrap();

    assert_eq!(secret.expose(), "anonymous");
}

#[test]
#[cfg(unix)]
fn an_os_string_field_reads_a_value_that_is_not_unicode() {
    use std::os::unix::ffi::OsStrExt as _;
    let environment = environment(not_unicode());

    let field = Field::new::<OsString>("SHELF_TITLE", "OsString", Presence::Maybe);
    let title: OsString = field.read(&environment).unwrap().unwrap();

    assert_eq!(title.as_bytes(), [b'S', 0xff]);
}

#[test]
#[cfg(unix)]
fn a_string_field_refuses_a_value_that_is_not_unicode_without_claiming_a_disagreement() {
    let environment = environment(not_unicode());

    let error = required::<String>(&environment, "SHELF_TITLE").unwrap_err();

    assert_eq!(error.problem(), FieldProblem::NotUnicode);
    assert_eq!(
        error.to_string(),
        "the field that reads SHELF_TITLE holds unicode text, and the value is not unicode"
    );
}
