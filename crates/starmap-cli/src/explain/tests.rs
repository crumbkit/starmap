use std::path::Path;

use crumbkit::{Call, Recovery};
use starmap::Spec;

use super::{list_again, row};
use crate::spec;

/// Returns the example spec in `crates/starmap/tests/example/starmap.toml`.
fn example() -> Spec {
    spec::open(Path::new("../starmap/tests/example/starmap.toml"))
        .unwrap()
        .spec
}

#[test]
fn a_name_the_spec_does_not_declare_is_a_failure_of_that_name() {
    let spec = example();
    let bindings = starmap::capture(&spec, Vec::new());
    let failure = row(
        &spec,
        "starmap.toml",
        &bindings,
        "SHELF_THEME".parse().unwrap(),
    )
    .unwrap_err();
    assert_eq!(
        failure.to_string(),
        "starmap.toml does not declare SHELF_THEME"
    );
}

#[test]
fn a_declared_name_has_the_row_that_the_built_in_listing_builds() {
    let spec = example();
    let bindings = starmap::capture(&spec, Vec::new());
    let name: starmap::Name = "SHELF_PORT".parse().unwrap();

    let explained =
        serde_json::to_value(row(&spec, "starmap.toml", &bindings, name.clone()).unwrap());
    let built_in = serde_json::to_value(crumbkit_starmap::row(&spec, &bindings, name).unwrap());

    assert_eq!(explained.unwrap(), built_in.unwrap());
}

#[test]
fn the_recovery_of_an_unknown_name_keeps_a_spec_file_that_is_not_the_default() {
    assert_eq!(
        list_again("other.toml"),
        Recovery::Run(Call::new(&["explain"]).value("spec", "other.toml"))
    );
    assert_eq!(
        list_again("starmap.toml"),
        Recovery::Run(Call::new(&["explain"]))
    );
}
