//! Tests for the `[groups]` table: its parsing, its refusals, its
//! canonical form and its digest.

use crate::{GroupName, Name, Pointer, Spec, SpecProblemKind};

/// A spec with the group `mirror` of three optional settings, and a port
/// outside any group.
const MIRROR: &str = r#"
[groups.mirror]
variables = ["SHELF_MIRROR_ACCOUNT", "SHELF_MIRROR_KEY", "SHELF_MIRROR_SECRET"]

[variables.SHELF_PORT]
purpose = "The port the api process listens on."
type = "port"
unset = { default = "8080" }
show = "shown"
source = "setting"

[variables.SHELF_MIRROR_ACCOUNT]
purpose = "The account shelf uses at the catalogue mirror."
type = "string"
unset = "optional"
show = "shown"
source = "setting"

[variables.SHELF_MIRROR_KEY]
purpose = "The key of the account at the catalogue mirror."
type = "string"
unset = "optional"
show = "shown"
source = "setting"

[variables.SHELF_MIRROR_SECRET]
purpose = "The secret of the key at the catalogue mirror."
type = "string"
unset = "optional"
show = "withheld"
source = "secret"
"#;

/// Returns the name `text`.
fn name(text: &str) -> Name {
    text.parse().unwrap()
}

/// Returns [`MIRROR`] with its `[groups]` table replaced by `groups`.
fn with_groups(groups: &str) -> String {
    let (_, variables) = MIRROR.split_once("\n\n").unwrap();
    format!("{groups}\n\n{variables}")
}

/// Parses `text`, which is not a valid spec, and returns the kind and the
/// message of each problem.
fn problems(text: &str) -> Vec<(SpecProblemKind, String)> {
    Spec::parse(text)
        .unwrap_err()
        .problems()
        .iter()
        .map(|problem| (problem.kind(), problem.to_string()))
        .collect()
}

#[test]
fn a_group_parses_with_its_variables_in_the_file_order() {
    let spec = Spec::parse(MIRROR).unwrap();

    let mirror: GroupName = "mirror".parse().unwrap();
    let group = spec.group(&mirror).unwrap();
    let members: Vec<&str> = group.variables().iter().map(Name::as_str).collect();

    assert_eq!(
        members,
        [
            "SHELF_MIRROR_ACCOUNT",
            "SHELF_MIRROR_KEY",
            "SHELF_MIRROR_SECRET"
        ]
    );
    assert_eq!(
        spec.group_of(&name("SHELF_MIRROR_KEY"))
            .map(|group| group.name()),
        Some(&mirror)
    );
    assert_eq!(spec.group_of(&name("SHELF_PORT")), None);
}

#[test]
fn groups_are_listed_in_name_order() {
    let text = with_groups(
        "[groups.mirror]\nvariables = [\"SHELF_MIRROR_ACCOUNT\", \"SHELF_MIRROR_KEY\"]\n\n\
         [groups.archive]\nvariables = [\"SHELF_PORT\", \"SHELF_MIRROR_SECRET\"]",
    )
    .replace("unset = { default = \"8080\" }", "unset = \"optional\"");

    let spec = Spec::parse(&text).unwrap();
    let names: Vec<&str> = spec.groups().map(|group| group.name().as_str()).collect();

    assert_eq!(names, ["archive", "mirror"]);
}

#[test]
fn a_bad_group_name_is_refused_as_a_bad_value() {
    let text = MIRROR.replace("[groups.mirror]", "[groups.Mirror]");

    let found = problems(&text);

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, SpecProblemKind::BadValue);
    assert!(
        found[0].1.contains("`groups.Mirror` is not a group name"),
        "{found:?}"
    );
}

#[test]
fn an_undeclared_member_is_refused_and_named_with_its_group() {
    let text = MIRROR.replace("\"SHELF_MIRROR_SECRET\"]", "\"SHELF_MIRROR_TOKEN\"]");

    assert_eq!(
        problems(&text),
        [(
            SpecProblemKind::GroupUndeclared,
            "`groups.mirror.variables` lists \"SHELF_MIRROR_TOKEN\", which the file does not \
             declare"
                .to_owned()
        )]
    );
}

#[test]
fn a_group_of_one_is_refused() {
    let text = with_groups("[groups.mirror]\nvariables = [\"SHELF_MIRROR_ACCOUNT\"]");

    let kinds: Vec<SpecProblemKind> = problems(&text).into_iter().map(|(kind, _)| kind).collect();

    assert_eq!(kinds, [SpecProblemKind::BadGroup]);
}

#[test]
fn a_repeated_member_is_refused() {
    let text = with_groups(
        "[groups.mirror]\nvariables = [\"SHELF_MIRROR_ACCOUNT\", \"SHELF_MIRROR_KEY\", \
         \"SHELF_MIRROR_ACCOUNT\"]",
    );

    assert_eq!(
        problems(&text),
        [(
            SpecProblemKind::BadGroup,
            "`groups.mirror.variables` lists \"SHELF_MIRROR_ACCOUNT\" twice, and a group lists \
             each variable once"
                .to_owned()
        )]
    );
}

#[test]
fn a_variable_in_two_groups_is_refused() {
    let text = with_groups(
        "[groups.mirror]\nvariables = [\"SHELF_MIRROR_ACCOUNT\", \"SHELF_MIRROR_KEY\"]\n\n\
         [groups.secrets]\nvariables = [\"SHELF_MIRROR_KEY\", \"SHELF_MIRROR_SECRET\"]",
    );

    assert_eq!(
        problems(&text),
        [(
            SpecProblemKind::BadGroup,
            "`groups.secrets.variables` lists \"SHELF_MIRROR_KEY\", which the group mirror lists \
             too, and a variable belongs to at most one group"
                .to_owned()
        )]
    );
}

#[test]
fn a_member_with_a_default_is_refused() {
    let text =
        with_groups("[groups.mirror]\nvariables = [\"SHELF_MIRROR_ACCOUNT\", \"SHELF_PORT\"]");

    assert_eq!(
        problems(&text),
        [(
            SpecProblemKind::BadGroup,
            "`groups.mirror.variables` lists \"SHELF_PORT\", which has a default, and a member of \
             a group has none, because a default always counts as set"
                .to_owned()
        )]
    );
}

#[test]
fn a_generated_member_is_refused() {
    let text = MIRROR.replace(
        "show = \"withheld\"\nsource = \"secret\"",
        "show = \"withheld\"\nsource = \"generated\"",
    );

    assert_eq!(
        problems(&text),
        [(
            SpecProblemKind::BadGroup,
            "`groups.mirror.variables` lists \"SHELF_MIRROR_SECRET\", which is generated, and a \
             member of a group is not, because a generated value always counts as set"
                .to_owned()
        )]
    );
}

#[test]
fn a_member_that_is_not_a_variable_name_is_refused_as_a_bad_name() {
    let text = MIRROR.replace("\"SHELF_MIRROR_SECRET\"]", "\"shelf-secret\"]");

    let kinds: Vec<SpecProblemKind> = problems(&text).into_iter().map(|(kind, _)| kind).collect();

    assert_eq!(kinds, [SpecProblemKind::BadName]);
}

#[test]
fn a_group_without_variables_or_with_another_field_is_refused() {
    let missing = with_groups("[groups.mirror]\npurpose = \"The mirror.\"");

    let kinds: Vec<SpecProblemKind> = problems(&missing)
        .into_iter()
        .map(|(kind, _)| kind)
        .collect();

    assert_eq!(
        kinds,
        [SpecProblemKind::UnknownField, SpecProblemKind::MissingField]
    );
}

#[test]
fn the_canonical_form_writes_groups_in_name_order_before_the_variables_and_round_trips() {
    let text = with_groups(
        "[groups.mirror]\nvariables = [\"SHELF_MIRROR_KEY\", \"SHELF_MIRROR_ACCOUNT\"]\n\n\
         [groups.archive]\nvariables = [\"SHELF_PORT\", \"SHELF_MIRROR_SECRET\"]",
    )
    .replace("unset = { default = \"8080\" }", "unset = \"optional\"");
    let spec = Spec::parse(&text).unwrap();

    let canonical = spec.to_toml();

    assert!(
        canonical.starts_with(
            "[groups.archive]\nvariables = [\"SHELF_PORT\", \"SHELF_MIRROR_SECRET\"]\n\n\
             [groups.mirror]\nvariables = [\"SHELF_MIRROR_KEY\", \"SHELF_MIRROR_ACCOUNT\"]\n\n\
             [variables.SHELF_PORT]\n"
        ),
        "{canonical}"
    );
    assert_eq!(Spec::parse(&canonical).unwrap(), spec);
}

#[test]
fn adding_a_group_changes_the_digest_and_a_file_without_groups_keeps_its_own() {
    let (_, variables) = MIRROR.split_once("\n\n").unwrap();
    let without = Spec::parse(variables).unwrap();
    let with = Spec::parse(MIRROR).unwrap();

    assert_ne!(with.digest(), without.digest());
    assert!(
        !without.to_toml().contains("groups"),
        "{}",
        without.to_toml()
    );
}

#[test]
fn a_pointer_with_groups_is_refused_as_declaring() {
    let text =
        "spec = \"config/environment.toml\"\n\n[groups.mirror]\nvariables = [\"A\", \"B\"]\n";

    let kinds: Vec<SpecProblemKind> = Pointer::parse(text)
        .unwrap_err()
        .problems()
        .iter()
        .map(crate::SpecProblem::kind)
        .collect();

    assert_eq!(kinds, [SpecProblemKind::PointerDeclares]);
}

#[test]
fn the_canonical_form_writes_groups_after_the_named_enumerations() {
    let text = format!("[enumerations.region]\neu = \"The European mirror.\"\n{MIRROR}");

    let canonical = Spec::parse(&text).unwrap().to_toml();

    assert!(
        canonical.starts_with(
            "[enumerations.region]\neu = \"The European mirror.\"\n\n[groups.mirror]\n"
        ),
        "{canonical}"
    );
}
