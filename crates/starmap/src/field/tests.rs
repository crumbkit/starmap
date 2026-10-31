//! Tests for the fields of a derived struct and the types they hold.

use std::ffi::OsString;
use std::path::PathBuf;

use super::{Field, FieldError, FieldType, Presence, Shape};
use crate::{Environment, Mode, Port, Secret, Show, Spec, Verdict, capture, check};

/// A spec with a port, a path, a string, an optional integer and a
/// withheld token.
const SPEC: &str = r#"
[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = { default = "8080" }
show = "shown"
source = "setting"

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

[variables.SHELF_PAGE_SIZE]
purpose = "How many books one page of a listing holds."
type = { integer = { min = 1, max = 1000 } }
unset = "optional"
show = "shown"
source = "setting"

[variables.SHELF_MIRROR_TOKEN]
purpose = "The token shelf sends to the catalogue mirror."
type = "string"
unset = { default = "anonymous" }
show = "withheld"
source = "secret"
"#;

/// Checks `pairs` against [`SPEC`] in local mode.
fn environment(pairs: &[(&str, &str)]) -> Environment {
    let spec = Spec::parse(SPEC).unwrap();
    let vars = pairs
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)));
    match check(&spec, &capture(&spec, vars), Mode::Local) {
        Verdict::Ready(environment) => environment,
        Verdict::Refused(refusals) => panic!("the check refused: {refusals:?}"),
    }
}

/// Reads the variable `name` from `environment` as a field of type `T`.
fn read<T: FieldType>(
    environment: &Environment,
    name: &'static str,
) -> Result<Option<T>, FieldError> {
    Field::new::<T>(name, "T", Presence::Maybe).read(environment)
}

/// Reads the variable `name` from `environment` as a field of type `T`
/// that the variable must have.
fn require<T: FieldType>(environment: &Environment, name: &'static str) -> Result<T, FieldError> {
    Field::new::<T>(name, "T", Presence::Always).require(environment)
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
    assert_eq!(u16::SHAPE, Shape::Integer { min: 0, max: 65535 });
    assert_eq!(
        i64::SHAPE,
        Shape::Integer {
            min: i64::MIN,
            max: i64::MAX
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
fn only_a_secret_holds_a_withheld_value() {
    assert_eq!(<Secret<String>>::SHAPE, Shape::String);
    assert_eq!(<Secret<String>>::SHOW, Show::Withheld);
    assert_eq!(String::SHOW, Show::Shown);
}

#[test]
fn a_field_reads_the_checked_value_of_its_variable() {
    let environment = environment(&[("SHELF_PAGE_SIZE", "250")]);

    assert_eq!(
        require(&environment, "SHELF_PORT"),
        Ok(Port::try_from(8080).unwrap())
    );
    assert_eq!(
        require(&environment, "SHELF_DATABASE"),
        Ok(PathBuf::from("books.db"))
    );
    assert_eq!(require(&environment, "SHELF_TITLE"), Ok("Shelf".to_owned()));
    assert_eq!(read(&environment, "SHELF_PAGE_SIZE"), Ok(Some(250_u8)));
}

#[test]
fn an_os_string_field_reads_a_string_or_a_path_variable() {
    let environment = environment(&[]);

    assert_eq!(
        require(&environment, "SHELF_TITLE"),
        Ok(OsString::from("Shelf"))
    );
    assert_eq!(
        require(&environment, "SHELF_DATABASE"),
        Ok(OsString::from("books.db"))
    );
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test checks the secret that the field reads"
)]
fn a_secret_field_reads_a_copy_of_the_withheld_value() {
    let environment = environment(&[]);

    let secret: Secret<String> = require(&environment, "SHELF_MIRROR_TOKEN").unwrap();

    assert_eq!(secret.expose(), "anonymous");
}

#[test]
fn an_unset_variable_reads_as_none_and_a_required_read_refuses_it() {
    let environment = environment(&[]);
    let refused = Err(FieldError {
        name: "SHELF_PAGE_SIZE",
    });

    assert_eq!(read::<u16>(&environment, "SHELF_PAGE_SIZE"), Ok(None));
    assert_eq!(require::<u16>(&environment, "SHELF_PAGE_SIZE"), refused);
}

#[test]
fn a_field_whose_type_cannot_hold_the_value_refuses_it() {
    let environment = environment(&[("SHELF_PAGE_SIZE", "300")]);
    let refused = |name| FieldError { name };

    assert_eq!(
        read::<u8>(&environment, "SHELF_PAGE_SIZE").unwrap_err(),
        refused("SHELF_PAGE_SIZE")
    );
    assert_eq!(
        read::<PathBuf>(&environment, "SHELF_PORT").unwrap_err(),
        refused("SHELF_PORT")
    );
    assert_eq!(
        read::<String>(&environment, "SHELF_MIRROR_TOKEN").unwrap_err(),
        refused("SHELF_MIRROR_TOKEN")
    );
    assert_eq!(
        read::<u8>(&environment, "not a name").unwrap_err(),
        refused("not a name")
    );
}
