//! The parser of the `[families]` table, whose tables each declare a family
//! of variables named at run time, and the rule that a declared variable is
//! no family's member.

use std::fmt;
use std::ops::Range;
use std::str::FromStr;

use super::{Field, Kind, Parser, entries};
use crate::spec::family::is_key;
use crate::spec::{FamilyDeclaration, Source, Unset, Variable};
use crate::{Mode, Name};

#[cfg(test)]
mod tests;

/// The keys of a family's table: a variable's keys and `keys`. A family
/// refuses `bind` and the sources that need one after its table parses, so
/// the message names the family rule.
const FAMILY_FIELDS: &[&str] = &[
    "purpose", "effect", "check", "type", "empty", "unset", "show", "source", "bind", "bytes",
    "tool", "keys",
];

/// The rule that a key follows, as a refusal of a key states it.
const KEY_RULE: &str = "a key starts with an upper case letter or a digit and holds only upper \
                        case letters, digits and underscores";

/// One listed key of a family, which matches `[A-Z0-9][A-Z0-9_]*`.
struct Key(String);

/// The reason text is not a [`Key`].
struct KeyError;

impl fmt::Display for KeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(KEY_RULE)
    }
}

impl FromStr for Key {
    type Err = KeyError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if is_key(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(KeyError)
        }
    }
}

impl Parser {
    /// Parses the `[families]` table into its families, in prefix order,
    /// and reports each pair of prefixes that would share a member.
    pub(super) fn families(&mut self, field: &Field<'_>) -> Vec<FamilyDeclaration> {
        let Some(table) = self.table(field) else {
            return Vec::new();
        };
        let mut families = Vec::new();
        let mut prefixes: Vec<(Name, Range<usize>)> = Vec::new();
        for (key, item) in entries(table) {
            let family = Field {
                path: format!("families.{}", key.get()),
                key,
                item,
            };
            let prefix = self.name(&family);
            if let Some(prefix) = &prefix {
                prefixes.push((prefix.clone(), family.key_span()));
            }
            if let Some(family) = self.family(&family, prefix) {
                families.push(family);
            }
        }
        self.overlaps(&prefixes);
        families.sort_by(|left: &FamilyDeclaration, right| left.prefix.cmp(&right.prefix));
        families
    }

    /// Reports each prefix of `prefixes` that is itself a member name of a
    /// shorter prefix, because the two families would then share members,
    /// such as `SHELF_DB` and `SHELF_DB_URL`, which both have the member
    /// `SHELF_DB_URL_EU`. The test reads only the prefixes, so it holds
    /// however a family's keys change.
    fn overlaps(&mut self, prefixes: &[(Name, Range<usize>)]) {
        for (longer, at) in prefixes {
            let shorter = prefixes.iter().find(|(shorter, _)| {
                crate::spec::family::key_after(shorter.as_str(), longer.as_str()).is_some()
            });
            if let Some((shorter, _)) = shorter {
                let message = format!(
                    "`families.{longer}` is a member name of the family {shorter}, so the two \
                     families would share members, and each name has one declaration"
                );
                self.report(at.clone(), Kind::BadFamily, message);
            }
        }
    }

    /// Parses one `[families.<PREFIX>]` table, whose prefix is `prefix`
    /// when its key is a variable name, or reports each rule it breaks.
    fn family(&mut self, field: &Field<'_>, prefix: Option<Name>) -> Option<FamilyDeclaration> {
        let (_, variable) = self.variable(field, FAMILY_FIELDS);
        // `variable` reported a value that is not a table.
        let table = field.item.as_table_like()?;
        let keys = match field.child(table, "keys") {
            None => Some(None),
            Some(keys) => self.keys(&keys, prefix.as_ref()).map(Some),
        };
        let (variable, prefix, keys) = (variable?, prefix?, keys?);
        let whole = self.family_rules(field, table, &variable, keys.is_some());
        whole.then_some(FamilyDeclaration {
            prefix,
            variable,
            keys,
        })
    }

    /// Parses a family's `keys` into its keys in key order, or reports a
    /// key that breaks the key rule, an empty list, a repeated key and a
    /// key that makes the member name of `prefix` too long.
    fn keys(&mut self, field: &Field<'_>, prefix: Option<&Name>) -> Option<Vec<String>> {
        let mut keys: Vec<String> = self
            .texts::<Key>(field)?
            .into_iter()
            .map(|key| key.0)
            .collect();
        if keys.is_empty() {
            let message = format!(
                "`{}` lists no keys, and a family that lists its keys lists at least one; remove \
                 `keys` to leave the family's keys open",
                field.path
            );
            self.report(field.span(), Kind::BadFamily, message);
            return None;
        }
        keys.sort();
        let mut whole = true;
        let repeat = keys
            .iter()
            .zip(keys.iter().skip(1))
            .find_map(|(first, second)| (first == second).then_some(first));
        if let Some(repeat) = repeat {
            let message = format!(
                "`{}` lists {repeat:?} twice, and a family lists each key once",
                field.path
            );
            self.report(field.span(), Kind::BadFamily, message);
            whole = false;
        }
        if let Some(prefix) = prefix {
            for key in &keys {
                let bytes = prefix.as_str().len() + 1 + key.len();
                if bytes > Name::MAX_BYTES {
                    let message = format!(
                        "`{}` lists {key:?}, which makes the member name {bytes} bytes long, and \
                         a name has at most {} bytes",
                        field.path,
                        Name::MAX_BYTES
                    );
                    self.report(field.span(), Kind::BadFamily, message);
                    whole = false;
                }
            }
        }
        whole.then_some(keys)
    }

    /// Reports each rule that a family whose table parsed into `variable`
    /// breaks: a default, a required open family, and a source other than a
    /// setting without a bind or inherited. `listed` says whether the family
    /// lists its keys. It returns whether the family keeps every rule.
    fn family_rules(
        &mut self,
        field: &Field<'_>,
        table: &dyn toml_edit::TableLike,
        variable: &Variable,
        listed: bool,
    ) -> bool {
        let mut whole = true;
        let unset = field.child(table, "unset");
        let unset_at = unset.as_ref().map_or_else(|| field.key_span(), Field::span);
        if matches!(variable.unset().in_mode(Mode::Local), Unset::Default(_)) {
            let message = format!(
                "`{}.unset` sets a default, and a family has none, because a default would apply \
                 to keys that nobody names",
                field.path
            );
            self.report(unset_at.clone(), Kind::BadFamily, message);
            whole = false;
        }
        let required = [Mode::Local, Mode::Production]
            .into_iter()
            .find(|mode| matches!(variable.unset().in_mode(*mode), Unset::Required));
        if let (Some(mode), false) = (required, listed) {
            let message = format!(
                "`{}.unset` requires a value in {mode} mode, and only a family with `keys` may be \
                 required, because starmap cannot know which keys an open family must have",
                field.path
            );
            self.report(unset_at, Kind::BadFamily, message);
            whole = false;
        }
        let refused = match variable.source() {
            Source::Setting { bind: None } | Source::Inherited => None,
            Source::Setting { bind: Some(_) } => Some(("bind", "a path bind")),
            Source::Secret { .. } => Some(("source", "\"secret\"")),
            Source::Generated { .. } => Some(("source", "\"generated\"")),
            Source::Provided(_) => Some(("source", "\"provided\"")),
        };
        if let Some((key, what)) = refused {
            let at = field
                .child(table, key)
                .map_or_else(|| field.key_span(), |child| child.span());
            let message = format!(
                "`{}.{key}` is {what}, and a family is a setting without a bind or inherited, \
                 because a secret, generated, provided or bound variable needs one bind per \
                 member",
                field.path
            );
            self.report(at, Kind::BadFamily, message);
            whole = false;
        }
        whole
    }

    /// Reports the declared variable `name`, whose table is `field`, when
    /// it is a member of one of `families`, because each name has one
    /// declaration. A table whose key is not a variable name has no `name`,
    /// and its own problem was reported.
    pub(super) fn not_a_member(
        &mut self,
        field: &Field<'_>,
        name: Option<&Name>,
        families: &[FamilyDeclaration],
    ) {
        let Some(name) = name else {
            return;
        };
        if let Some(family) = families.iter().find(|family| family.key_of(name).is_some()) {
            let message = format!(
                "`{}` is a member of the family {}, and each name has one declaration, so a \
                 declared variable is no family's member",
                field.path,
                family.prefix()
            );
            self.report(field.key_span(), Kind::BadFamily, message);
        }
    }
}
