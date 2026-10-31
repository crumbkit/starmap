//! The canonical form of a spec, which [`Spec::to_toml`] returns and
//! [`Spec::digest`] hashes.

use std::path::PathBuf;

use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value as Toml, value};

use super::{
    FORM_WORDS, SHOW_WORDS, SOURCE_WORDS, Source, SourceKind, Spec, UNSET_WORDS, Unset, UnsetRule,
    Variable, word_of,
};
use crate::{Form, Mode, SecretRef, Value};

/// Writes `spec` as TOML: the `[mode]` table, then one table per variable
/// in the spec's order, each with its fields in the order of the field
/// rules.
pub(super) fn to_toml(spec: &Spec) -> String {
    let mut document = DocumentMut::new();
    if let Some(mode) = spec.mode() {
        let mut table = Table::new();
        table.insert("variable", value(mode.variable().as_str()));
        let production: Array = mode.production().iter().map(|c| c.as_str()).collect();
        table.insert("production", value(production));
        document.insert("mode", Item::Table(table));
    }
    let mut variables = Table::new();
    variables.set_implicit(true);
    for (name, declared) in spec.variables() {
        variables.insert(name.as_str(), Item::Table(variable(declared)));
    }
    if !variables.is_empty() {
        document.insert("variables", Item::Table(variables));
    }
    document.to_string()
}

/// Writes one variable's table.
fn variable(variable: &Variable) -> Table {
    let mut table = Table::new();
    table.insert("purpose", value(variable.purpose()));
    table.insert("type", value(form(variable.form())));
    table.insert("unset", value(unset(variable.unset())));
    table.insert("show", value(word_of(SHOW_WORDS, &variable.show())));
    let (kind, bind) = source(variable.source());
    table.insert("source", value(word_of(SOURCE_WORDS, &kind)));
    if let Some(bind) = bind {
        table.insert("bind", value(bind));
    }
    if let Source::Generated { bytes } = variable.source() {
        table.insert("bytes", value(i64::from(bytes.get())));
    }
    table
}

/// Builds an inline table of `entries`.
fn inline<const N: usize>(entries: [(&str, Toml); N]) -> Toml {
    Toml::InlineTable(entries.into_iter().collect::<InlineTable>())
}

/// Writes a form as its word, or as its table when it has no word.
fn form(form: &Form) -> Toml {
    match form {
        Form::Path { absolute: true } => inline([("path", inline([("absolute", true.into())]))]),
        Form::Integer(range) => inline([(
            "integer",
            inline([("min", range.min().into()), ("max", range.max().into())]),
        )]),
        Form::Enumeration(choices) => {
            let values: Array = choices.iter().map(|choice| choice.as_str()).collect();
            inline([("enumeration", Toml::Array(values))])
        }
        word => word_of(FORM_WORDS, word).into(),
    }
}

/// Writes an unset rule as a word, or as a table when it has a default or
/// differs between the modes.
fn unset(rule: &UnsetRule) -> Toml {
    let local = rule.in_mode(Mode::Local);
    let production = rule.in_mode(Mode::Production);
    // A production rule that differs from the local one is required or
    // optional, which UNSET_WORDS lists.
    let production = (production != local).then(|| word_of(UNSET_WORDS, production).into());
    let (key, local) = match local {
        Unset::Default(default) => ("default", text(default).into()),
        word => ("local", word_of(UNSET_WORDS, word).into()),
    };
    match production {
        None if key == "local" => local,
        None => inline([(key, local)]),
        Some(production) => inline([(key, local), ("production", production)]),
    }
}

/// Returns a source's kind and its bind, if it has one.
fn source(source: &Source) -> (SourceKind, Option<Toml>) {
    match source {
        Source::Setting { bind } => (
            SourceKind::Setting,
            bind.as_ref()
                .map(|template| inline([("path", template.as_str().into())])),
        ),
        Source::Secret { bind } => (
            SourceKind::Secret,
            bind.as_ref().map(|reference| match reference {
                SecretRef::OnePassword(reference) => {
                    inline([("one-password", reference.as_str().into())])
                }
            }),
        ),
        Source::Generated { .. } => (SourceKind::Generated, None),
        Source::Inherited => (SourceKind::Inherited, None),
        Source::Provided(reference) => {
            let mut table = InlineTable::new();
            table.insert("provided", reference.kind().as_str().into());
            for (key, qualifier) in reference.qualifiers() {
                table.insert(key, qualifier.into());
            }
            (SourceKind::Provided, Some(Toml::InlineTable(table)))
        }
    }
}

/// Writes a default's value as the text that its form reads back as an
/// equal value.
fn text(value: &Value) -> String {
    match value {
        // A path is written from its components, as PathBuf compares paths,
        // so `a//b` and `a/b/` both write `a/b`. A default is read from TOML
        // text, which is Unicode, so the lossy conversion never changes it.
        Value::Path(path) => path
            .components()
            .collect::<PathBuf>()
            .to_string_lossy()
            .into_owned(),
        Value::String(string) => string.to_string_lossy().into_owned(),
        Value::Port(port) => port.to_string(),
        Value::Url(url) => url.as_str().to_owned(),
        Value::Boolean(boolean) => boolean.to_string(),
        Value::Integer(number) => number.to_string(),
        Value::Choice(choice) => choice.as_str().to_owned(),
    }
}
