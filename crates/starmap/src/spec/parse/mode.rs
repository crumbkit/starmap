//! The parser of the `[mode]` table, which names the variable that decides
//! the mode.

use super::{Field, Kind, Parser};
use crate::spec::{SOURCE_WORDS, Show, SourceKind, Variable, word_of};
use crate::{ModeRule, Name};

/// The keys of the `[mode]` table.
const MODE_FIELDS: &[&str] = &["variable", "production"];

impl Parser {
    /// Parses the `[mode]` table and checks its variable against the
    /// declared ones, their sources and their shows.
    pub(super) fn mode(
        &mut self,
        field: &Field<'_>,
        declared: &[(Name, Option<SourceKind>)],
        variables: &[(Name, Variable)],
    ) -> Option<ModeRule> {
        let table = self.table(field)?;
        self.known(table, "mode", MODE_FIELDS);
        let variable = self.required(field, table, "variable");
        let name = variable.as_ref().and_then(|variable| {
            let text = self.string(variable)?;
            let name = text.parse::<Name>();
            self.checked(variable.span(), &variable.path, text, Kind::BadName, name)
        });
        let production = self
            .required(field, table, "production")
            .and_then(|production| self.choices(&production));
        let (name, variable) = (name?, variable?);
        let (kind, message) = match declared.iter().find(|(declared, _)| *declared == name) {
            None => (
                Kind::ModeUndeclared,
                format!(
                    "`mode.variable` is {:?}, which the file does not declare",
                    name.as_str()
                ),
            ),
            Some((_, Some(kind)))
                if !matches!(kind, SourceKind::Setting | SourceKind::Inherited) =>
            {
                (
                    Kind::ModeSource,
                    format!(
                        "`mode.variable` is {:?}, which is a {} variable, and the mode variable \
                         is a setting or inherited variable",
                        name.as_str(),
                        word_of(SOURCE_WORDS, kind)
                    ),
                )
            }
            // The mode is never decided from a secret, so the mode variable
            // is shown.
            Some(_)
                if variables.iter().any(|(declared, variable)| {
                    *declared == name && variable.show() == Show::Withheld
                }) =>
            {
                (
                    Kind::ModeSource,
                    format!(
                        "`mode.variable` is {:?}, which is withheld, and the mode variable is \
                         shown",
                        name.as_str()
                    ),
                )
            }
            Some(_) => return Some(ModeRule::new(name, production?)),
        };
        self.report(variable.span(), kind, message);
        None
    }
}
