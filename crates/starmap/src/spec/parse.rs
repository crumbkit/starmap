//! The parser of `starmap.toml`. It reads the text once into the spec's
//! types (BASE-32), builds each value through its checking constructor
//! (BASE-42), and collects every problem it finds before it returns.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt::Display;
use std::ops::Range;

use toml_edit::{Document, Item, Key, TableLike};

use super::{
    FORM_WORDS, SHOW_WORDS, SOURCE_WORDS, Show, Source, SourceKind, Spec, SpecError, SpecProblem,
    SpecProblemKind as Kind, UNSET_WORDS, Unset, UnsetRule, Variable, word_of,
};
use crate::bind::BIND_KINDS;
use crate::{
    Choice, Choices, ChoicesError, Form, IntegerRange, ModeRule, Name, OnePasswordRef,
    PathTemplate, PathTemplateError, ProvidedRef, ProviderKind, SecretBytes, SecretRef, Value,
};

/// The keys of the file's top-level table.
const ROOT_FIELDS: &[&str] = &["mode", "variables"];
/// The keys of the `[mode]` table.
const MODE_FIELDS: &[&str] = &["variable", "production"];
/// The keys of a variable's table.
const VARIABLE_FIELDS: &[&str] = &[
    "purpose", "type", "unset", "show", "source", "bind", "bytes",
];
/// The keys of a type table.
const FORM_TABLES: &[&str] = &["path", "integer", "enumeration"];
/// The keys of an unset table.
const UNSET_FIELDS: &[&str] = &["local", "default", "production"];

/// Parses `text` into a spec, or returns every problem it holds.
pub(super) fn parse(text: &str) -> Result<Spec, SpecError> {
    let mut parser = Parser::default();
    let spec = parser.document(text);
    if parser.problems.is_empty() {
        Ok(spec)
    } else {
        Err(SpecError {
            problems: parser.problems,
        })
    }
}

impl SourceKind {
    /// Returns the rule for the binds this source takes.
    fn bind_rule(self) -> &'static str {
        match self {
            Self::Setting => "a setting takes only a path bind",
            Self::Secret => "a secret takes only a one-password bind",
            Self::Generated => "a generated variable takes no bind",
            Self::Inherited => "an inherited variable takes no bind",
            Self::Provided => "a provided variable takes only a provided bind",
        }
    }
}

/// A parsed bind, before it is merged with the source.
enum Bind {
    Provided(ProvidedRef),
    Path(PathTemplate),
    OnePassword(OnePasswordRef),
}

impl Bind {
    /// Returns the key that names the bind's kind.
    fn kind(&self) -> &'static str {
        match self {
            Self::Provided(_) => "provided",
            Self::Path(_) => "path",
            Self::OnePassword(_) => "one-password",
        }
    }
}

/// Returns the span of `range`, or the empty span at the start of the text
/// when the parser has no position.
fn span(range: Option<Range<usize>>) -> Range<usize> {
    range.unwrap_or(0..0)
}

/// One key of a table and its item, with the dotted path that names it in
/// messages, such as `variables.SHELF_PORT.type`.
struct Field<'a> {
    path: String,
    key: &'a Key,
    item: &'a Item,
}

impl<'a> Field<'a> {
    /// Returns the span of the key.
    fn key_span(&self) -> Range<usize> {
        span(self.key.span())
    }

    /// Returns the span of the value, or of the key when the value has
    /// none, as a table made of dotted keys has none.
    fn span(&self) -> Range<usize> {
        span(self.item.span().or_else(|| self.key.span()))
    }

    /// Returns the field `name` of `table`, whose path is this field's.
    fn child(&self, table: &'a dyn TableLike, name: &str) -> Option<Self> {
        let (key, item) = table.get_key_value(name)?;
        Some(Self {
            path: format!("{}.{name}", self.path),
            key,
            item,
        })
    }
}

/// The parser's state: the problems it has found so far.
#[derive(Default)]
struct Parser {
    problems: Vec<SpecProblem>,
}

impl Parser {
    /// Records a problem.
    fn report(&mut self, span: Range<usize>, kind: Kind, message: String) {
        self.problems.push(SpecProblem {
            span,
            kind,
            message,
        });
    }

    /// Parses the whole file. The spec it returns holds every part that
    /// parsed, and it is the result only when no problem was reported.
    fn document(&mut self, text: &str) -> Spec {
        let mut spec = Spec {
            mode: None,
            variables: Vec::new(),
        };
        if text.len() > Spec::MAX_BYTES {
            let message = format!(
                "the file has {} bytes, and the limit is {} bytes",
                text.len(),
                Spec::MAX_BYTES
            );
            self.report(0..0, Kind::TooLarge, message);
            return spec;
        }
        let document = match Document::parse(text) {
            Ok(document) => document,
            Err(error) => {
                // BASE-37: the message copies toml_edit's text and drops the
                // error itself. A typed detail on SpecProblem replaces it if
                // a caller needs the cause.
                let message = format!("the file is not valid toml: {}", error.message().trim());
                self.report(span(error.span()), Kind::Syntax, message);
                return spec;
            }
        };
        let root = document.as_table();
        self.known(root, "", ROOT_FIELDS);
        let mut declared = Vec::new();
        if let Some(field) = root_field(root, "variables")
            && let Some(table) = self.table(&field)
        {
            for (key, item) in entries(table) {
                let field = Field {
                    path: format!("variables.{}", key.get()),
                    key,
                    item,
                };
                let name = self.name(&field);
                let (kind, variable) = self.variable(&field);
                if let Some(name) = name {
                    declared.push((name.clone(), kind));
                    if let Some(variable) = variable {
                        spec.variables.push((name, variable));
                    }
                }
            }
        }
        if let Some(field) = root_field(root, "mode") {
            spec.mode = self.mode(&field, &declared);
        }
        spec
    }

    /// Reports each key of `table` that is not in `known`. `path` is the
    /// table's dotted path, which is empty for the file's top level.
    fn known(&mut self, table: &dyn TableLike, path: &str, known: &[&str]) {
        let owner = if path.is_empty() {
            "the file".to_owned()
        } else {
            format!("`{path}`")
        };
        for (key, _) in entries(table) {
            if !known.contains(&key.get()) {
                let message = format!(
                    "{owner} has the field `{}`, and it takes {}",
                    key.get(),
                    list(known, "")
                );
                self.report(span(key.span()), Kind::UnknownField, message);
            }
        }
    }

    /// Returns the field `name` of `table`, or reports that it is missing
    /// from the table that `owner` is.
    fn required<'a>(
        &mut self,
        owner: &Field<'a>,
        table: &'a dyn TableLike,
        name: &str,
    ) -> Option<Field<'a>> {
        let field = owner.child(table, name);
        if field.is_none() {
            let message = format!("`{}` has no `{name}` field, and it needs one", owner.path);
            self.report(owner.key_span(), Kind::MissingField, message);
        }
        field
    }

    /// Returns the field's value as a table, or reports what it is.
    fn table<'a>(&mut self, field: &Field<'a>) -> Option<&'a dyn TableLike> {
        let table = field.item.as_table_like();
        if table.is_none() {
            self.wrong_type(field, "a table");
        }
        table
    }

    /// Returns the field's value as text, or reports what it is.
    fn string<'a>(&mut self, field: &Field<'a>) -> Option<&'a str> {
        let text = field.item.as_str();
        if text.is_none() {
            self.wrong_type(field, "text");
        }
        text
    }

    /// Reports that the field's value has the wrong TOML type, and that it
    /// takes `wanted`.
    fn wrong_type(&mut self, field: &Field<'_>, wanted: &str) {
        let message = format!(
            "`{}` is a toml {}, and it takes {wanted}",
            field.path,
            field.item.type_name()
        );
        self.report(field.span(), Kind::BadValue, message);
    }

    /// Returns the field's text, or reports it when it is not text or still
    /// ends in the `?` that `starmap init` wrote.
    fn confirmed<'a>(&mut self, field: &Field<'a>) -> Option<&'a str> {
        let text = self.string(field)?;
        if text.ends_with('?') {
            let message = format!(
                "`{}` is {text:?}, which still ends in the ? that starmap init wrote",
                field.path
            );
            self.report(field.span(), Kind::Guess, message);
            return None;
        }
        Some(text)
    }

    /// Returns the value that the field's word names in `words`, or reports
    /// a guess or a word that is not listed.
    fn word<T: Clone>(&mut self, field: &Field<'_>, words: &[(&str, T)]) -> Option<T> {
        let text = self.confirmed(field)?;
        let found = words.iter().find(|(word, _)| *word == text);
        if found.is_none() {
            let names: Vec<&str> = words.iter().map(|(word, _)| *word).collect();
            let message = format!(
                "`{}` is {text:?}, and it takes one of {}",
                field.path,
                list(&names, "\"")
            );
            self.report(field.span(), Kind::BadValue, message);
        }
        found.map(|(_, value)| value.clone())
    }

    /// Returns the value of a checking constructor's result, or reports its
    /// refusal at `at`, naming `path` and quoting `text` when it is not
    /// empty.
    fn checked<T, E: Display>(
        &mut self,
        at: Range<usize>,
        path: &str,
        text: &str,
        kind: Kind,
        result: Result<T, E>,
    ) -> Option<T> {
        // BASE-37: the message flattens each constructor's error type to its
        // text and drops the error's source(). A typed detail on SpecProblem
        // replaces this if a caller needs the cause.
        let error = match result {
            Ok(value) => return Some(value),
            Err(error) => error,
        };
        let message = if text.is_empty() {
            format!("`{path}` does not fit, because {error}")
        } else {
            format!("`{path}` is {text:?}, and {error}")
        };
        self.report(at, kind, message);
        None
    }

    /// Returns the variable name that the field's key holds, or reports
    /// why it is not one.
    fn name(&mut self, field: &Field<'_>) -> Option<Name> {
        match field.key.get().parse::<Name>() {
            Ok(name) => Some(name),
            Err(error) => {
                let message = format!("`{}` is not a variable name, because {error}", field.path);
                self.report(field.key_span(), Kind::BadName, message);
                None
            }
        }
    }

    /// Parses one variable's table. It returns the variable's source word,
    /// when that parsed, even when the variable as a whole did not, so that
    /// the mode rule can still be checked against it.
    fn variable(&mut self, field: &Field<'_>) -> (Option<SourceKind>, Option<Variable>) {
        let Some(table) = self.table(field) else {
            return (None, None);
        };
        self.known(table, &field.path, VARIABLE_FIELDS);
        let purpose = self
            .required(field, table, "purpose")
            .and_then(|purpose| self.purpose(&purpose));
        let form = self
            .required(field, table, "type")
            .and_then(|form| Some((self.form(&form)?, form)));
        let unset = self
            .required(field, table, "unset")
            .and_then(|unset| self.unset(&unset, form.as_ref().map(|(form, _)| form)));
        let show = self
            .required(field, table, "show")
            .and_then(|show| Some((self.word(&show, SHOW_WORDS)?, show)));
        let kind = self
            .required(field, table, "source")
            .and_then(|source| Some((self.word(&source, SOURCE_WORDS)?, source)));
        let bind = field.child(table, "bind").map(|bind| self.bind(&bind));
        let bytes = field.child(table, "bytes");

        if let (Some((Show::Shown, show)), Some((kind, _))) = (&show, &kind)
            && matches!(kind, SourceKind::Secret | SourceKind::Generated)
        {
            let message = format!(
                "`{}` is \"shown\", and a secret or generated variable is withheld",
                show.path
            );
            self.report(show.span(), Kind::ShownSecret, message);
        }
        if let (Some((Show::Withheld, _)), Some((form, type_field))) = (&show, &form)
            && *form != Form::String
        {
            let message = format!(
                "`{}` is not \"string\", and a withheld variable has the type string",
                type_field.path
            );
            self.report(type_field.span(), Kind::SecretForm, message);
        }
        let source = kind
            .as_ref()
            .and_then(|(kind, source)| self.source(field, source, *kind, bind, bytes));
        let variable = match (purpose, form, unset, show, source) {
            (Some(purpose), Some((form, _)), Some(unset), Some((show, _)), Some(source)) => {
                Some(Variable {
                    purpose,
                    form,
                    unset,
                    show,
                    source,
                })
            }
            _ => None,
        };
        (kind.map(|(kind, _)| kind), variable)
    }

    /// Parses the purpose sentence.
    fn purpose(&mut self, field: &Field<'_>) -> Option<String> {
        let text = self.string(field)?;
        let (kind, rule) = if text == "?" {
            (
                Kind::Guess,
                "the guess that starmap init wrote, and a purpose",
            )
        } else if text.trim().is_empty() {
            (Kind::BadValue, "which is empty, and a purpose")
        } else {
            return Some(text.to_owned());
        };
        let message = format!(
            "`{}` is {text:?}, {rule} is one sentence that says what the variable is for",
            field.path
        );
        self.report(field.span(), kind, message);
        None
    }

    /// Parses the `type` field: a word, or a table with one key.
    fn form(&mut self, field: &Field<'_>) -> Option<Form> {
        if field.item.is_str() {
            return self.word(field, FORM_WORDS);
        }
        let table = self.table(field)?;
        self.known(table, &field.path, FORM_TABLES);
        let named: Vec<&str> = FORM_TABLES
            .iter()
            .copied()
            .filter(|name| table.contains_key(name))
            .collect();
        match named.as_slice() {
            ["path"] => self.path_form(&field.child(table, "path")?),
            ["integer"] => self.integer_form(&field.child(table, "integer")?),
            ["enumeration"] => {
                let enumeration = field.child(table, "enumeration")?;
                self.choices(&enumeration).map(Form::Enumeration)
            }
            // Each unknown key was reported above.
            [] if !table.is_empty() => None,
            [] => {
                let message = format!(
                    "`{}` is an empty table, and it takes one of path, integer and enumeration",
                    field.path
                );
                self.report(field.span(), Kind::MissingField, message);
                None
            }
            _ => {
                let message = format!(
                    "`{}` names {}, and a type names one of them",
                    field.path,
                    list(&named, "")
                );
                self.report(field.span(), Kind::BadValue, message);
                None
            }
        }
    }

    /// Parses `{ path = { absolute = .. } }`.
    fn path_form(&mut self, field: &Field<'_>) -> Option<Form> {
        let table = self.table(field)?;
        self.known(table, &field.path, &["absolute"]);
        let Some(absolute) = field.child(table, "absolute") else {
            return Some(Form::Path { absolute: false });
        };
        let value = absolute.item.as_bool();
        if value.is_none() {
            self.wrong_type(&absolute, "true or false");
        }
        Some(Form::Path { absolute: value? })
    }

    /// Parses `{ integer = { min = .., max = .. } }`.
    fn integer_form(&mut self, field: &Field<'_>) -> Option<Form> {
        let table = self.table(field)?;
        self.known(table, &field.path, &["min", "max"]);
        let mut bound = |name: &str| {
            let bound = self.required(field, table, name)?;
            let number = bound.item.as_integer();
            if number.is_none() {
                self.wrong_type(&bound, "a whole number");
            }
            number
        };
        let (min, max) = (bound("min"), bound("max"));
        let (min, max) = (min?, max?);
        let range = IntegerRange::new(min, max).ok();
        if range.is_none() {
            let message = format!(
                "`{}` has the min {min} above the max {max}, and a range runs from its min up \
                 to its max",
                field.path
            );
            self.report(field.span(), Kind::BadRange, message);
        }
        range.map(Form::Integer)
    }

    /// Parses an array of choices, which has at least one and repeats none.
    fn choices(&mut self, field: &Field<'_>) -> Option<Choices> {
        let Some(array) = field.item.as_array() else {
            self.wrong_type(field, "an array of text");
            return None;
        };
        let mut choices = Vec::new();
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
            let choice = text.parse::<Choice>();
            if let Some(choice) = self.checked(at, &path, text, Kind::BadValue, choice) {
                choices.push(choice);
            }
        }
        if choices.len() < array.len() {
            return None;
        }
        let (at, message) = match Choices::new(choices) {
            Ok(choices) => return Some(choices),
            Err(ChoicesError::Repeated(choice)) => {
                // The second element that holds the choice is the repeat.
                let repeat = array
                    .iter()
                    .filter(|value| value.as_str() == Some(choice.as_str()))
                    .nth(1)
                    .and_then(toml_edit::Value::span);
                let message = format!(
                    "`{}` lists {:?} twice, and it lists each value once",
                    field.path,
                    choice.as_str()
                );
                (span(repeat.or_else(|| field.item.span())), message)
            }
            Err(_) => {
                let message = format!("`{}` has no values, and it lists at least one", field.path);
                (field.span(), message)
            }
        };
        self.report(at, Kind::EmptyEnumeration, message);
        None
    }

    /// Parses the `unset` field: a word, or a table with `local` or
    /// `default` and an optional `production`. A default is checked against
    /// `form`, and a variable whose type did not parse gets no rule.
    fn unset(&mut self, field: &Field<'_>, form: Option<&Form>) -> Option<UnsetRule> {
        if field.item.is_str() {
            let local = self.word(field, UNSET_WORDS)?;
            return Some(UnsetRule {
                production: local.clone(),
                local,
            });
        }
        let table = self.table(field)?;
        self.known(table, &field.path, UNSET_FIELDS);
        let production = match field.child(table, "production") {
            None => Some(None),
            Some(production) => self.word(&production, UNSET_WORDS).map(Some),
        };
        let local = match (field.child(table, "local"), field.child(table, "default")) {
            (Some(local), None) => self.word(&local, UNSET_WORDS),
            (None, Some(default)) => {
                let text = default.item.as_str();
                if text.is_none() {
                    self.wrong_type(&default, "text, such as \"20\"");
                }
                self.default_value(&default, text?, form?)
                    .map(Unset::Default)
            }
            (local, _) => {
                let (kind, count) = match local {
                    Some(_) => (Kind::BadValue, "both"),
                    None => (Kind::MissingField, "neither"),
                };
                let message = format!(
                    "`{}` sets {count} local and default, and it takes one of them",
                    field.path
                );
                self.report(field.span(), kind, message);
                None
            }
        };
        let (local, production) = (local?, production?);
        Some(UnsetRule {
            production: production.unwrap_or_else(|| local.clone()),
            local,
        })
    }

    /// Reads a default in the variable's form, or reports why it does not
    /// fit.
    fn default_value(&mut self, field: &Field<'_>, text: &str, form: &Form) -> Option<Value> {
        let result = match form.parse(OsStr::new(text)) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err("a relative path, and the type takes only an absolute path".to_owned()),
            Err(error) => Err(error.to_string()),
        };
        self.checked(field.span(), &field.path, text, Kind::BadDefault, result)
    }

    /// Parses the `bind` field: a table with exactly one of `provided`,
    /// `path` and `one-password`.
    fn bind(&mut self, field: &Field<'_>) -> Option<(Bind, Range<usize>)> {
        let table = self.table(field)?;
        let named: Vec<&str> = BIND_KINDS
            .iter()
            .copied()
            .filter(|name| table.contains_key(name))
            .collect();
        let bind = match named.as_slice() {
            ["provided"] => self.provided(field, table).map(Bind::Provided),
            [kind @ ("path" | "one-password")] => {
                self.known(table, &field.path, &[kind]);
                let value = field.child(table, kind)?;
                let text = self.confirmed(&value)?;
                let (at, path) = (value.span(), value.path.as_str());
                if *kind == "path" {
                    let template = text.parse::<PathTemplate>();
                    let problem = match template {
                        Err(PathTemplateError::Empty) => Kind::BadValue,
                        _ => Kind::UnknownPlaceholder,
                    };
                    self.checked(at, path, text, problem, template)
                        .map(Bind::Path)
                } else {
                    let reference = text.parse::<OnePasswordRef>();
                    self.checked(at, path, text, Kind::BadValue, reference)
                        .map(Bind::OnePassword)
                }
            }
            _ => {
                let named = if named.is_empty() {
                    "no kind of bind".to_owned()
                } else {
                    list(&named, "")
                };
                let message = format!(
                    "`{}` names {named}, and a bind names exactly one of provided, path and \
                     one-password",
                    field.path
                );
                self.report(field.span(), Kind::BadValue, message);
                None
            }
        };
        Some((bind?, field.span()))
    }

    /// Parses `{ provided = "<host>/<kind>", .. }`, whose other keys are
    /// qualifiers.
    fn provided(&mut self, field: &Field<'_>, table: &dyn TableLike) -> Option<ProvidedRef> {
        let kind_field = field.child(table, "provided")?;
        let kind = self.confirmed(&kind_field).and_then(|text| {
            let kind = text.parse::<ProviderKind>();
            self.checked(
                kind_field.span(),
                &kind_field.path,
                text,
                Kind::BadValue,
                kind,
            )
        });
        let mut qualifiers = BTreeMap::new();
        let mut whole = true;
        for (key, _) in entries(table).filter(|(key, _)| key.get() != "provided") {
            let Some(qualifier) = field.child(table, key.get()) else {
                continue;
            };
            match self.string(&qualifier) {
                Some(value) => {
                    qualifiers.insert(key.get().to_owned(), value.to_owned());
                }
                None => whole = false,
            }
        }
        let kind = kind?;
        if !whole {
            return None;
        }
        let reference = ProvidedRef::new(kind, qualifiers);
        self.checked(field.span(), &field.path, "", Kind::BadValue, reference)
    }

    /// Merges the source word with its bind and its `bytes`, or reports a
    /// combination the rules forbid (BASE-45).
    fn source(
        &mut self,
        variable: &Field<'_>,
        field: &Field<'_>,
        kind: SourceKind,
        bind: Option<Option<(Bind, Range<usize>)>>,
        bytes: Option<Field<'_>>,
    ) -> Option<Source> {
        let bytes = self.bytes(variable, kind, bytes);
        let bind = match bind {
            None => None,
            // The bind's own problem was reported.
            Some(None) => return None,
            Some(Some(bind)) => Some(bind),
        };
        let source = match (kind, bind) {
            (SourceKind::Setting, None) => Source::Setting { bind: None },
            (SourceKind::Setting, Some((Bind::Path(template), _))) => Source::Setting {
                bind: Some(template),
            },
            (SourceKind::Secret, None) => Source::Secret { bind: None },
            (SourceKind::Secret, Some((Bind::OnePassword(reference), _))) => Source::Secret {
                bind: Some(SecretRef::OnePassword(reference)),
            },
            (SourceKind::Generated, None) => Source::Generated {
                bytes: bytes?.unwrap_or_default(),
            },
            (SourceKind::Inherited, None) => Source::Inherited,
            (SourceKind::Provided, Some((Bind::Provided(reference), _))) => {
                Source::Provided(reference)
            }
            (SourceKind::Provided, None) => {
                let message = format!(
                    "`{}` is \"provided\", and a provided variable needs a provided bind, such \
                     as {{ provided = \"host/kind\" }}",
                    field.path
                );
                self.report(field.span(), Kind::BindMismatch, message);
                return None;
            }
            (kind, Some((bind, at))) => {
                let message = format!(
                    "`{}.bind` is a {} bind, and {}",
                    variable.path,
                    bind.kind(),
                    kind.bind_rule()
                );
                self.report(at, Kind::BindMismatch, message);
                return None;
            }
        };
        bytes?;
        Some(source)
    }

    /// Parses `bytes`, which only a generated variable takes. It returns
    /// `Some(None)` when the field is absent and `None` after a problem.
    fn bytes(
        &mut self,
        variable: &Field<'_>,
        kind: SourceKind,
        field: Option<Field<'_>>,
    ) -> Option<Option<SecretBytes>> {
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
        let Some(number) = field.item.as_integer() else {
            self.wrong_type(&field, "a whole number");
            return None;
        };
        // A number beyond u16 is above SecretBytes::MAX too, so u16::MAX
        // stands in for it and SecretBytes::new refuses it.
        let bytes = SecretBytes::new(u16::try_from(number).unwrap_or(u16::MAX));
        let text = number.to_string();
        self.checked(field.span(), &field.path, &text, Kind::BadBytes, bytes)
            .map(Some)
    }

    /// Parses the `[mode]` table and checks its variable against the
    /// declared ones and their sources.
    fn mode(
        &mut self,
        field: &Field<'_>,
        declared: &[(Name, Option<SourceKind>)],
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
            Some(_) => return Some(ModeRule::new(name, production?)),
        };
        self.report(variable.span(), kind, message);
        None
    }
}

/// Returns the top-level field `name`, whose path is its name.
fn root_field<'a>(root: &'a dyn TableLike, name: &str) -> Option<Field<'a>> {
    let (key, item) = root.get_key_value(name)?;
    Some(Field {
        path: name.to_owned(),
        key,
        item,
    })
}

/// Returns each key of `table` with its item, in the file's order.
fn entries(table: &dyn TableLike) -> impl Iterator<Item = (&Key, &Item)> {
    table
        .iter()
        .filter_map(|(name, _)| table.get_key_value(name))
}

/// Joins `words`, each wrapped in `quote`, as "a, b and c".
fn list(words: &[&str], quote: &str) -> String {
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
