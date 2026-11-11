//! The canonical form of a spec, which [`Spec::to_toml`] returns and
//! [`Spec::digest`] hashes.

use std::path::PathBuf;

use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value as Toml, value};

use super::{
    ENCODING_WORDS, SHOW_WORDS, SOURCE_WORDS, Source, SourceKind, Spec, TYPE_WORDS, UNSET_WORDS,
    Unset, UnsetRule, Variable, word_of,
};
use crate::{ByteLength, Cache, Mode, Name, SecretBind, SecretRef, Type, Value};

/// Writes `spec` as TOML: the `[mode]` table, then one table per group in
/// name order, then one table per variable in the spec's order, each with its fields in the order of the field
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
    let mut groups = Table::new();
    groups.set_implicit(true);
    for group in spec.groups() {
        let mut table = Table::new();
        let members: Array = group.variables().iter().map(Name::as_str).collect();
        table.insert("variables", value(members));
        groups.insert(group.name().as_str(), Item::Table(table));
    }
    if !groups.is_empty() {
        document.insert("groups", Item::Table(groups));
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
    if let Some(effect) = variable.effect() {
        table.insert("effect", value(effect));
    }
    table.insert("type", value(value_type(variable.value_type())));
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

/// Writes a type as its word, or as its table when it has no word.
fn value_type(value_type: &Type) -> Toml {
    match value_type {
        Type::Path { absolute: true } => inline([("path", inline([("absolute", true.into())]))]),
        Type::Integer(range) => inline([(
            "integer",
            inline([("min", range.min().into()), ("max", range.max().into())]),
        )]),
        Type::Enumeration(choices) => {
            let values: Array = choices.iter().map(|choice| choice.as_str()).collect();
            inline([("enumeration", Toml::Array(values))])
        }
        Type::Bytes(rule) => {
            let (key, count) = match rule.length() {
                ByteLength::Exactly(count) => ("length", count),
                ByteLength::AtLeast(count) => ("min-length", count),
            };
            inline([(
                "bytes",
                inline([
                    ("encoding", word_of(ENCODING_WORDS, &rule.encoding()).into()),
                    (key, i64::from(count.get()).into()),
                ]),
            )])
        }
        Type::Url {
            schemes: Some(schemes),
        } => {
            let values: Array = schemes.iter().map(|scheme| scheme.as_str()).collect();
            inline([("url", inline([("schemes", Toml::Array(values))]))])
        }
        Type::String {
            pattern: Some(pattern),
        } => inline([("string", inline([("pattern", pattern.as_str().into())]))]),
        word => word_of(TYPE_WORDS, word).into(),
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
        Source::Secret { bind } => (SourceKind::Secret, bind.as_ref().map(secret_bind)),
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

/// Returns a secret's bind as an inline table. `cache = false` is written
/// only for [`Cache::Never`], so a bind that writes `cache = true` has the
/// same canonical form as one without `cache`.
fn secret_bind(bind: &SecretBind) -> Toml {
    let mut table = InlineTable::new();
    match bind.reference() {
        SecretRef::OnePassword(reference) => {
            table.insert("one-password", reference.as_str().into());
        }
    }
    if bind.cache() == Cache::Never {
        table.insert("cache", false.into());
    }
    Toml::InlineTable(table)
}

/// Writes a default's value as the text that its type reads back as an
/// equal value. A default is read from TOML text, which is Unicode, so the
/// lossy conversions never change it.
fn text(value: &Value) -> String {
    match value {
        // A path is written from its components, as PathBuf compares paths,
        // so `a//b` and `a/b/` both write `a/b`.
        Value::Path(path) => path
            .components()
            .collect::<PathBuf>()
            .to_string_lossy()
            .into_owned(),
        _ => value.to_os_string().to_string_lossy().into_owned(),
    }
}
