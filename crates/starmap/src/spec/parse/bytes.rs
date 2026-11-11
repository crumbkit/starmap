//! The parser of byte counts: a bytes type's length, a generated
//! variable's `bytes` field, and the count of random bytes that a
//! generated variable with a bytes type takes from its type.

use super::{ENCODING_WORDS, Field, Kind, Parser, SourceKind};
use crate::{ByteCount, ByteLength, ByteRule, BytesError, Type};

impl Parser {
    /// Parses `{ bytes = { encoding = .., length = .. } }`, which takes
    /// exactly one of `length` and `min-length`.
    pub(super) fn bytes_type(&mut self, field: &Field<'_>) -> Option<Type> {
        let table = self.table(field)?;
        self.known(table, &field.path, &["encoding", "length", "min-length"]);
        let encoding = self
            .required(field, table, "encoding")
            .and_then(|encoding| self.word(&encoding, ENCODING_WORDS));
        let length = match (
            field.child(table, "length"),
            field.child(table, "min-length"),
        ) {
            (Some(length), None) => self.count(&length, ByteCount::new).map(ByteLength::Exactly),
            (None, Some(length)) => self.count(&length, ByteCount::new).map(ByteLength::AtLeast),
            (length, _) => {
                let (kind, count) = match length {
                    Some(_) => (Kind::BadValue, "both"),
                    None => (Kind::MissingField, "neither"),
                };
                let message = format!(
                    "`{}` sets {count} length and min-length, and it takes one of them",
                    field.path
                );
                self.report(field.span(), kind, message);
                None
            }
        };
        Some(Type::Bytes(ByteRule::new(encoding?, length?)))
    }

    /// Parses `bytes`, which only a generated variable with the type string
    /// takes; `typed` says the variable has a bytes type. It returns
    /// `Some(None)` when the field is absent and `None` after a problem.
    pub(super) fn bytes(
        &mut self,
        variable: &Field<'_>,
        kind: SourceKind,
        field: Option<Field<'_>>,
        typed: bool,
    ) -> Option<Option<ByteCount>> {
        let Some(field) = field else {
            return Some(None);
        };
        if kind != SourceKind::Generated {
            let message = format!(
                "`{}` sets bytes, and only a generated variable takes bytes",
                variable.path
            );
            self.report(field.key_span(), Kind::BadBytes, message);
            return None;
        }
        if typed {
            let message = format!(
                "`{}` is set, and a generated variable with a bytes type takes its length from \
                 the type",
                field.path
            );
            self.report(field.key_span(), Kind::BadBytes, message);
            return None;
        }
        self.count(&field, ByteCount::generated).map(Some)
    }

    /// Returns the number of random bytes a generated variable of the bytes
    /// type `rule` has: its length, or its smallest length. It reports a
    /// count outside the range a generated value takes.
    pub(super) fn generated_length(
        &mut self,
        rule: ByteRule,
        field: &Field<'_>,
    ) -> Option<ByteCount> {
        let (ByteLength::Exactly(count) | ByteLength::AtLeast(count)) = rule.length();
        match ByteCount::generated(count.get()) {
            Ok(count) => Some(count),
            Err(error) => {
                let message = format!("`{}` asks for {count}, and {error}", field.path);
                self.report(field.span(), Kind::BadBytes, message);
                None
            }
        }
    }

    /// Parses a whole number of bytes through `build`, which refuses a
    /// number outside its range.
    fn count(
        &mut self,
        field: &Field<'_>,
        build: fn(u16) -> Result<ByteCount, BytesError>,
    ) -> Option<ByteCount> {
        let Some(number) = field.item.as_integer() else {
            self.wrong_type(field, "a whole number");
            return None;
        };
        // A number beyond u16 is above ByteCount::MAX too, so u16::MAX
        // stands in for it and `build` refuses it. The clamp only serves
        // that refusal: it never admits a bad value, and the report still
        // names the BadBytes problem with the number as written.
        let count = build(u16::try_from(number).unwrap_or(u16::MAX));
        let text = number.to_string();
        self.checked(field.span(), &field.path, &text, Kind::BadBytes, count)
    }
}
