//! The parser of the `[groups]` table, whose tables each declare a group of
//! variables that are set together or not at all.

use std::ops::Range;

use super::{Field, Kind, Parser, entries, span};
use crate::spec::{Group, GroupName, SourceKind, Unset, Variable};
use crate::{Mode, Name};

#[cfg(test)]
mod tests;

/// The keys of a group's table.
const GROUP_FIELDS: &[&str] = &["variables"];

/// The variables that the groups parsed so far list, each with the name of
/// its group, so a variable in a second group is refused.
type Members = Vec<(Name, GroupName)>;

impl Parser {
    /// Parses the `[groups]` table into its groups, in name order, and
    /// checks each member against the `declared` variables, their sources
    /// and the parsed `variables`.
    pub(super) fn groups(
        &mut self,
        field: &Field<'_>,
        declared: &[(Name, Option<SourceKind>)],
        variables: &[(Name, Variable)],
    ) -> Vec<Group> {
        let Some(table) = self.table(field) else {
            return Vec::new();
        };
        let mut groups = Vec::new();
        let mut members = Members::new();
        for (key, item) in entries(table) {
            let group = Field {
                path: format!("groups.{}", key.get()),
                key,
                item,
            };
            if let Some(group) = self.group(&group, declared, variables, &mut members) {
                groups.push(group);
            }
        }
        groups.sort_by(|left: &Group, right| left.name.cmp(&right.name));
        groups
    }

    /// Parses one `[groups.<name>]` table, or reports each rule it breaks.
    fn group(
        &mut self,
        field: &Field<'_>,
        declared: &[(Name, Option<SourceKind>)],
        variables: &[(Name, Variable)],
        members: &mut Members,
    ) -> Option<Group> {
        let name = match field.key.get().parse::<GroupName>() {
            Ok(name) => Some(name),
            Err(error) => {
                let message = format!("`{}` is not a group name, because {error}", field.path);
                self.report(field.key_span(), Kind::BadValue, message);
                None
            }
        };
        let table = self.table(field)?;
        self.known(table, &field.path, GROUP_FIELDS);
        let list = self.required(field, table, "variables")?;
        let listed = self.members(&list)?;
        let mut whole = name.is_some();
        if listed.len() < 2 {
            let message = format!(
                "`{}` lists {} variable{}, and a group lists at least two",
                list.path,
                listed.len(),
                if listed.len() == 1 { "" } else { "s" }
            );
            self.report(list.span(), Kind::BadGroup, message);
            whole = false;
        }
        let mut kept: Vec<Name> = Vec::new();
        for (member, at) in listed {
            let problem = member_problem(&member, &kept, declared, variables, members);
            if let Some((kind, rule)) = problem {
                let message = format!("`{}` lists {:?}{rule}", list.path, member.as_str());
                self.report(at, kind, message);
                whole = false;
            }
            if !kept.contains(&member) {
                kept.push(member);
            }
        }
        let name = name?;
        for member in &kept {
            if !members.iter().any(|(listed, _)| listed == member) {
                members.push((member.clone(), name.clone()));
            }
        }
        whole.then_some(Group {
            name,
            variables: kept,
        })
    }

    /// Parses the group's `variables` array into names with their spans, or
    /// reports a value that is not an array, an element that is not text,
    /// and text that is not a variable name.
    fn members(&mut self, field: &Field<'_>) -> Option<Vec<(Name, Range<usize>)>> {
        let Some(array) = field.item.as_array() else {
            self.wrong_type(field, "an array of variable names");
            return None;
        };
        let mut parsed = Vec::new();
        for (index, value) in array.iter().enumerate() {
            let path = format!("{}[{index}]", field.path);
            let at = span(value.span());
            let Some(text) = value.as_str() else {
                let message = format!(
                    "`{path}` is a toml {}, and it takes a variable name",
                    value.type_name()
                );
                self.report(at, Kind::BadValue, message);
                continue;
            };
            let name = text.parse::<Name>();
            if let Some(name) = self.checked(at.clone(), &path, text, Kind::BadName, name) {
                parsed.push((name, at));
            }
        }
        (parsed.len() == array.len()).then_some(parsed)
    }
}

/// Returns the kind and the rest of the message of the first rule that
/// `member` breaks, after its group has listed `kept`.
fn member_problem(
    member: &Name,
    kept: &[Name],
    declared: &[(Name, Option<SourceKind>)],
    variables: &[(Name, Variable)],
    members: &Members,
) -> Option<(Kind, String)> {
    if kept.contains(member) {
        return Some((
            Kind::BadGroup,
            " twice, and a group lists each variable once".to_owned(),
        ));
    }
    let Some((_, kind)) = declared.iter().find(|(declared, _)| declared == member) else {
        return Some((
            Kind::GroupUndeclared,
            ", which the file does not declare".to_owned(),
        ));
    };
    if *kind == Some(SourceKind::Generated) {
        return Some((
            Kind::BadGroup,
            ", which is generated, and a member of a group is not, because a generated value \
             always counts as set"
                .to_owned(),
        ));
    }
    let defaulted = variables.iter().any(|(declared, variable)| {
        declared == member && matches!(variable.unset().in_mode(Mode::Local), Unset::Default(_))
    });
    if defaulted {
        return Some((
            Kind::BadGroup,
            ", which has a default, and a member of a group has none, because a default \
             always counts as set"
                .to_owned(),
        ));
    }
    let (_, other) = members.iter().find(|(listed, _)| listed == member)?;
    Some((
        Kind::BadGroup,
        format!(
            ", which the group {other} lists too, and a variable belongs to at most one group"
        ),
    ))
}
