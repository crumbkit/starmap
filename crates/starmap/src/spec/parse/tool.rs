//! The parser of `tool` tables. starmap checks that `tool` is a table of
//! tables and copies each one's keys and values into a [`ToolTable`]
//! without reading them.

use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, TableLike, Value};

use super::{Field, Kind, Parser, entries};
use crate::spec::ToolTable;

impl Parser {
    /// Parses a `tool` field into its tool tables, in name order, or
    /// reports each value that is not a table and each empty tool name.
    pub(super) fn tools(&mut self, field: &Field<'_>) -> Option<Vec<ToolTable>> {
        let table = self.table(field)?;
        let mut tools = Vec::new();
        let mut parsed = true;
        for (key, item) in entries(table) {
            let tool = Field {
                path: format!("{}.{}", field.path, key.get()),
                key,
                item,
            };
            if key.get().is_empty() {
                let message = format!(
                    "`{}` has an empty key, and a tool's name is not empty",
                    field.path
                );
                self.report(tool.key_span(), Kind::BadValue, message);
                parsed = false;
                continue;
            }
            match self.table(&tool) {
                Some(table) => tools.push(ToolTable {
                    name: key.get().to_owned(),
                    toml: DocumentMut::from(plain_table(table)).to_string(),
                }),
                None => parsed = false,
            }
        }
        tools.sort_by(|left, right| left.name.cmp(&right.name));
        parsed.then_some(tools)
    }
}

/// Returns a copy of `table` with its keys and values and none of the
/// file's comments or layout. Each nested table becomes a standard table,
/// so the copy prints as a document of its own.
fn plain_table(table: &dyn TableLike) -> Table {
    let mut plain = Table::new();
    for (key, item) in table.iter() {
        let item = match item {
            Item::Table(table) => Item::Table(plain_table(table)),
            Item::Value(Value::InlineTable(table)) => Item::Table(plain_table(table)),
            Item::Value(value) => Item::Value(plain_value(value)),
            Item::ArrayOfTables(tables) => {
                Item::ArrayOfTables(tables.iter().map(|table| plain_table(table)).collect())
            }
            Item::None => continue,
        };
        plain.insert(key, item);
    }
    plain
}

/// Returns a copy of `value` in toml_edit's default spelling.
fn plain_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::from(text.value().as_str()),
        Value::Integer(number) => Value::from(*number.value()),
        Value::Float(number) => Value::from(*number.value()),
        Value::Boolean(flag) => Value::from(*flag.value()),
        Value::Datetime(datetime) => Value::from(*datetime.value()),
        Value::Array(array) => Value::Array(array.iter().map(plain_value).collect::<Array>()),
        Value::InlineTable(table) => Value::InlineTable(
            table
                .iter()
                .map(|(key, value)| (key, plain_value(value)))
                .collect::<InlineTable>(),
        ),
    }
}
