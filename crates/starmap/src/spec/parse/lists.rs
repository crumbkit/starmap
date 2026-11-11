//! The parser of the arrays a type lists: an enumeration's choices and a
//! url type's schemes. Each array has at least one value and repeats none.

use std::fmt::Display;
use std::str::FromStr;

use super::{Field, Kind, Parser, span};
use crate::{Choice, Choices, ChoicesError, Scheme, Schemes, SchemesError, Type};

impl Parser {
    /// Parses `{ url = { schemes = [..] } }`, whose schemes are optional.
    pub(super) fn url_type(&mut self, field: &Field<'_>) -> Option<Type> {
        let table = self.table(field)?;
        self.known(table, &field.path, &["schemes"]);
        let Some(list) = field.child(table, "schemes") else {
            return Some(Type::Url { schemes: None });
        };
        let schemes = match Schemes::new(self.texts::<Scheme>(&list)?) {
            Ok(schemes) => schemes,
            Err(SchemesError::Repeated(scheme)) => {
                return self.not_distinct(&list, Some(scheme.as_str()));
            }
            Err(_) => return self.not_distinct(&list, None),
        };
        Some(Type::Url {
            schemes: Some(schemes),
        })
    }

    /// Parses an array of choices, which has at least one and repeats none.
    pub(super) fn choices(&mut self, field: &Field<'_>) -> Option<Choices> {
        match Choices::new(self.texts::<Choice>(field)?) {
            Ok(choices) => Some(choices),
            Err(ChoicesError::Repeated(choice)) => self.not_distinct(field, Some(choice.as_str())),
            Err(_) => self.not_distinct(field, None),
        }
    }

    /// Parses each element of the field's array as a `T`, or reports each
    /// element that is not text or that `T` refuses.
    pub(super) fn texts<T: FromStr>(&mut self, field: &Field<'_>) -> Option<Vec<T>>
    where
        T::Err: Display,
    {
        let Some(array) = field.item.as_array() else {
            self.wrong_type(field, "an array of text");
            return None;
        };
        let mut parsed = Vec::new();
        for (index, value) in array.iter().enumerate() {
            let path = format!("{}[{index}]", field.path);
            let at = span(value.span());
            let Some(text) = value.as_str() else {
                let message = format!(
                    "`{path}` is a toml {}, and it takes text",
                    value.type_name()
                );
                self.report(at, Kind::BadValue, message);
                continue;
            };
            let item = text.parse::<T>();
            if let Some(item) = self.checked(at, &path, text, Kind::BadValue, item) {
                parsed.push(item);
            }
        }
        (parsed.len() == array.len()).then_some(parsed)
    }

    /// Reports that the field's array lists `repeat` twice, or has no
    /// values when `repeat` is `None`.
    fn not_distinct<T>(&mut self, field: &Field<'_>, repeat: Option<&str>) -> Option<T> {
        let (at, message) = match repeat {
            Some(repeat) => {
                // The second element that holds the value is the repeat.
                let second = field
                    .item
                    .as_array()
                    .and_then(|array| {
                        array
                            .iter()
                            .filter(|value| value.as_str() == Some(repeat))
                            .nth(1)
                    })
                    .and_then(toml_edit::Value::span);
                let message = format!(
                    "`{}` lists {repeat:?} twice, and it lists each value once",
                    field.path
                );
                (span(second.or_else(|| field.item.span())), message)
            }
            None => {
                let message = format!("`{}` has no values, and it lists at least one", field.path);
                (field.span(), message)
            }
        };
        self.report(at, Kind::EmptyEnumeration, message);
        None
    }
}

/// Joins `words`, each wrapped in `quote`, as "a, b and c".
pub(super) fn list(words: &[&str], quote: &str) -> String {
    let quoted: Vec<String> = words
        .iter()
        .map(|word| format!("{quote}{word}{quote}"))
        .collect();
    match quoted.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}
