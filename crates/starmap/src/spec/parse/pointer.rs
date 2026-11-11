//! The parser of a pointer file, and the refusal of a pointer where a file
//! that declares variables belongs.

use std::path::PathBuf;

use toml_edit::TableLike;

use super::{Field, Kind, Parser, entries, list, root_field, span};
use crate::spec::{Pointer, SpecError};

/// The keys of a pointer file's top-level table. The key `spec` makes a
/// file a pointer.
const POINTER_FIELDS: &[&str] = &["spec", "src", "dotenv", "tool"];
/// The top-level keys of a file that declares variables. A pointer that
/// holds one of them declares too, which starmap refuses. Both kinds of
/// file take `tool`, so it is not among them.
const DECLARING_FIELDS: &[&str] = &["mode", "groups", "variables"];
/// Parses `text` into a pointer, returns `None` when the file has no `spec`
/// key and so declares variables itself, or returns every problem of a
/// pointer.
pub(in crate::spec) fn parse_pointer(text: &str) -> Result<Option<Pointer>, SpecError> {
    let mut parser = Parser::default();
    let pointer = parser.pointer(text);
    if parser.problems.is_empty() {
        Ok(pointer)
    } else {
        Err(SpecError {
            problems: parser.problems,
        })
    }
}

impl Parser {
    /// Reports a pointer that the spec parser reads, where a file that
    /// declares variables belongs: a file that both points and declares,
    /// or a pointer that another pointer names.
    pub(super) fn points(&mut self, root: &dyn TableLike, spec: &Field<'_>) {
        if !self.declares_too(root) {
            let message = "the file has `spec`, so it points to another file, and starmap reads \
                           variables only from a file that declares them"
                .to_owned();
            self.report(spec.key_span(), Kind::PointerChain, message);
        }
    }

    /// Reports each key of a pointer's top-level table that declares
    /// variables, and returns whether there was one.
    fn declares_too(&mut self, root: &dyn TableLike) -> bool {
        let mut found = false;
        for (key, _) in entries(root).filter(|(key, _)| DECLARING_FIELDS.contains(&key.get())) {
            let message = format!(
                "the file has `spec`, so it points to another file, and it has `{}` too, \
                 and a file either points or declares variables",
                key.get()
            );
            self.report(span(key.span()), Kind::PointerDeclares, message);
            found = true;
        }
        found
    }

    /// Parses a pointer file. It returns `None` when the file has no `spec`
    /// key, because such a file declares variables itself.
    fn pointer(&mut self, text: &str) -> Option<Pointer> {
        let document = self.toml(text)?;
        let root = document.as_table();
        let spec = root_field(root, "spec")?;
        self.declares_too(root);
        for (key, _) in entries(root) {
            let name = key.get();
            if !POINTER_FIELDS.contains(&name) && !DECLARING_FIELDS.contains(&name) {
                let message = format!(
                    "the pointer has the field `{name}`, and it takes {}",
                    list(POINTER_FIELDS, "")
                );
                self.report(span(key.span()), Kind::UnknownField, message);
            }
        }
        let target = self.path(&spec);
        let src = match root_field(root, "src") {
            None => Some(vec![PathBuf::from(Pointer::SRC)]),
            Some(field) => self.paths(&field),
        };
        let dotenv = match root_field(root, "dotenv") {
            None => Some(PathBuf::from(Pointer::DOTENV)),
            Some(field) => self.path(&field),
        };
        let tools = match root_field(root, "tool") {
            None => Some(Vec::new()),
            Some(field) => self.tools(&field),
        };
        Some(Pointer {
            spec: target?,
            src: src?,
            dotenv: dotenv?,
            tools: tools?,
        })
    }

    /// Returns the field's text as a path, or reports that it is not text
    /// or is empty.
    fn path(&mut self, field: &Field<'_>) -> Option<PathBuf> {
        let text = self.string(field)?;
        if text.is_empty() {
            let message = format!("`{}` is empty, and it takes a path", field.path);
            self.report(field.span(), Kind::BadValue, message);
            return None;
        }
        Some(PathBuf::from(text))
    }

    /// Returns the field's list of paths, or reports that it is not a list
    /// of text or holds an empty path.
    fn paths(&mut self, field: &Field<'_>) -> Option<Vec<PathBuf>> {
        let Some(array) = field.item.as_array() else {
            self.wrong_type(field, "a list of paths");
            return None;
        };
        let mut paths = Vec::new();
        for value in array {
            match value.as_str() {
                Some(text) if !text.is_empty() => paths.push(PathBuf::from(text)),
                _ => {
                    let message = format!(
                        "`{}` holds {}, and each entry is a path",
                        field.path,
                        value.to_string().trim()
                    );
                    self.report(span(value.span()), Kind::BadValue, message);
                    return None;
                }
            }
        }
        Some(paths)
    }
}
