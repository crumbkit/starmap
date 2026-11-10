//! The parser of the `[enumerations]` table, whose tables each declare a
//! named enumeration with a description of each value, and of a type that
//! names one.

use std::collections::BTreeMap;

use super::{Field, Kind, Parser, entries};
use crate::{Choice, Choices, EnumerationName, Type};

/// The enumerations a file declares, by the text of each table's key. An
/// enumeration that did not parse maps to `None`, so a variable that names
/// it reports nothing more.
pub(super) type Declared = BTreeMap<String, Option<Choices>>;

impl Parser {
    /// Parses the `[enumerations]` table into its enumerations, or reports
    /// each name, value and description that breaks a rule.
    pub(super) fn enumerations(&mut self, field: &Field<'_>) -> Declared {
        let mut declared = Declared::new();
        let Some(table) = self.table(field) else {
            return declared;
        };
        for (key, item) in entries(table) {
            let enumeration = Field {
                path: format!("enumerations.{}", key.get()),
                key,
                item,
            };
            let parsed = self.enumeration(&enumeration);
            declared.insert(key.get().to_owned(), parsed);
        }
        declared
    }

    /// Parses one `[enumerations.<name>]` table, whose keys are the values
    /// and whose values are their descriptions.
    fn enumeration(&mut self, field: &Field<'_>) -> Option<Choices> {
        let name = match field.key.get().parse::<EnumerationName>() {
            Ok(name) => Some(name),
            Err(error) => {
                let message = format!(
                    "`{}` is not an enumeration name, because {error}",
                    field.path
                );
                self.report(field.key_span(), Kind::BadValue, message);
                None
            }
        };
        let table = self.table(field)?;
        if table.is_empty() {
            let message = format!("`{}` has no values, and it lists at least one", field.path);
            self.report(field.span(), Kind::EmptyEnumeration, message);
            return None;
        }
        let mut values = Vec::new();
        let mut whole = true;
        for (key, _) in entries(table) {
            let Some(value) = field.child(table, key.get()) else {
                continue;
            };
            let choice = key.get().parse::<Choice>();
            let choice = self.checked(
                value.key_span(),
                &value.path,
                key.get(),
                Kind::BadValue,
                choice,
            );
            let description =
                self.sentence(&value, "a description", "what choosing the value means");
            match (choice, description) {
                (Some(choice), Some(description)) => values.push((choice, description)),
                _ => whole = false,
            }
        }
        let (name, true) = (name?, whole) else {
            return None;
        };
        // TOML refuses a repeated key, so the values never repeat, and the
        // table is not empty, so they are never empty either.
        Choices::named(name, values).ok()
    }

    /// Parses `{ enumeration = .. }`: the name of a declared enumeration, or
    /// an inline array of values.
    pub(super) fn enumeration_type(&mut self, field: &Field<'_>) -> Option<Type> {
        if field.item.is_array() {
            return self.choices(field).map(Type::Enumeration);
        }
        let Some(text) = field.item.as_str() else {
            self.wrong_type(field, "the name of an enumeration or an array of text");
            return None;
        };
        match self.enumerations.get(text).cloned() {
            Some(choices) => choices.map(Type::Enumeration),
            None => {
                let message = format!(
                    "`{}` is {text:?}, which the file does not declare as an enumeration",
                    field.path
                );
                self.report(field.span(), Kind::EnumerationUndeclared, message);
                None
            }
        }
    }
}
