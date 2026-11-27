//! The reads that a function makes through a source of variables, such as
//! `source.get("APP_PORT")` inside `Config::from_source(source: &impl
//! Source)`.
//!
//! An application that tests its config with a map in place of the process
//! environment often builds the config from a source: a trait whose method
//! takes a variable's name and returns its value. The call that reads the
//! environment then holds a name that is not a literal, such as
//! `std::env::var(name)`, so the reader shape of the scan misses every
//! variable that the config reads.
//!
//! A parameter of a function is a source when its type says so: when the
//! type names a type or trait whose name ends in `Source`, as in
//! `&impl Source`, `&dyn ConfigSource` or `Box<dyn Source>`, or names a
//! generic parameter whose bound names one. The bound may sit in the
//! function's generics or `where` clause, as in `fn f<S: Source>(source:
//! &S)`, or in those of the `impl` block that holds the function, as in
//! `impl<S: Source> Loader<S>`. The function's name plays no part, so the
//! text that `fn from_source(source: &str)` parses is no source. Inside the
//! function's body, these shapes read the variable whose name is a string
//! literal:
//!
//! - a method call on the source whose first argument is the literal, such
//!   as `source.get("APP_PORT")`, `source.get_or("APP_HOST", "localhost")`
//!   or `source.parse::<u16>("APP_PORT")`;
//! - a call whose first argument is the source, with or without a leading
//!   `&`, and whose second is the literal, such as
//!   `required(source, "APP_URL")` or `Self::parse(&source, "APP_URL")`.
//!
//! A parameter is also a source when it is a function that takes a `&str`
//! and returns an `Option` or `Result` of a `String`, as in
//! `get: impl Fn(&str) -> Option<String>` or a generic bounded by such an
//! `Fn`, `FnMut` or `FnOnce`. A call of it whose first argument is the
//! literal, such as `get("APP_PORT")`, reads that variable, and so does a
//! call that passes it before the literal.
//!
//! The literal must be a valid name with no lower-case letter, as a
//! variable's name in a spec is by convention, so a lookup such as
//! `source.get("orders")` on a source of something else is not a read. The
//! place of the read is the line of the literal.
//!
//! This is a token match with no type checking. It misses a source held in
//! a struct field (`self.source.get(...)` is not a read), in a closure's
//! parameter or in a local variable, a source whose type is named by an
//! alias that does not end in `Source`, a bound on a trait's own generics,
//! and a name held in a constant. A header whose return type or `where`
//! clause holds a block, as a const generic argument such as `Buf<{ N }>`
//! does, ends at that block, so the reads of the function's real body are
//! missed. It counts a method call of any name on a source, and any type
//! whose name ends in `Source`, such as an `EventSource`, is a source. When a function nested in
//! another's body names its source as the outer function names its own,
//! the scan lists a read through it twice at the same place, which changes
//! no count. A type-checked analysis would need the crate's whole build,
//! which the scan does not have, and would replace this match once an
//! application's sources take shapes that it misses.

use proc_macro2::{Delimiter, Group, Spacing, TokenStream, TokenTree};
use starmap::Name;

use super::{Read, Target, is_punct, string_text};

#[cfg(test)]
mod tests;

/// The ending of the name of a type or trait whose values are sources.
const SOURCE_SUFFIX: &str = "Source";

/// The traits of a function that may be a source.
const FUNCTION_TRAITS: &[&str] = &["Fn", "FnMut", "FnOnce"];

/// How a source gives a variable's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// A value whose method takes the name, as in `source.get("NAME")`.
    Value,
    /// A function that takes the name, as in `get("NAME")`.
    Function,
}

/// A generic parameter whose bound makes a value of it a source.
#[derive(Debug, Clone)]
pub(super) struct Bound {
    /// The generic parameter's name, such as `S`.
    name: String,
    /// How a value of it gives a variable's value.
    kind: Kind,
}

/// Returns the generic parameters that the header of an `impl` block bounds
/// by a source, in its generics or its `where` clause. `header` holds the
/// tokens after the word `impl`, up to the block's body.
pub(super) fn impl_bounds(header: &[TokenTree]) -> Vec<Bound> {
    let generics = if is_punct(header.first(), '<') {
        header
            .get(1..balanced_end(header, 0).unwrap_or(header.len()))
            .unwrap_or_default()
    } else {
        &[]
    };
    let mut bounds = generic_sources(generics);
    bounds.extend(generic_sources(after_first(header, is_where)));
    bounds
}

/// Returns the reads that the body of a function makes through its
/// sources. `header` holds the tokens between the function's name and its
/// body: its generics, parameters, return type and `where` clause, and
/// `outer` holds the bounds of the `impl` block around the function.
pub(super) fn reads(header: &[TokenTree], body: &Group, outer: &[Bound]) -> Vec<Read> {
    let Some((before, Some((TokenTree::Group(parameters), after)))) = parameters_at(header)
        .and_then(|at| header.split_at_checked(at))
        .map(|(before, rest)| (before, rest.split_first()))
    else {
        return Vec::new();
    };
    // The generics come before the parameters, and the `where` clause
    // follows the return type.
    let generics = after_first(before, |token| is_punct(Some(token), '<'));
    let mut bounded = outer.to_vec();
    bounded.extend(generic_sources(generics));
    bounded.extend(generic_sources(after_first(after, is_where)));
    let sources: Vec<Bound> = split_top_level(parameters.stream())
        .into_iter()
        .filter_map(|parameter| {
            let (binding, kind) = binding_and_type(&parameter)?;
            Some(Bound {
                name: binding,
                kind: source_kind(&kind, &bounded)?,
            })
        })
        .collect();
    let mut found = Vec::new();
    if !sources.is_empty() {
        collect(body.stream(), &sources, &mut found);
    }
    found
}

/// Reports whether `token` is the word `where`.
fn is_where(token: &TokenTree) -> bool {
    matches!(token, TokenTree::Ident(word) if word == "where")
}

/// Returns the depth of angle brackets after `token`, when it was `depth`
/// before it and `previous` came before it. The `>` of `->`, as in a bound
/// `Fn(u8) -> u8`, closes nothing.
fn angle_depth(depth: usize, token: &TokenTree, previous: Option<&TokenTree>) -> usize {
    match token {
        TokenTree::Punct(punct) if punct.as_char() == '<' => depth + 1,
        TokenTree::Punct(punct) if punct.as_char() == '>' && !is_arrow(previous) => {
            depth.saturating_sub(1)
        }
        _ => depth,
    }
}

/// Returns the index of the `>` that closes the `<` at `open` in `tokens`.
fn balanced_end(tokens: &[TokenTree], open: usize) -> Option<usize> {
    let mut depth = 0;
    for at in open..tokens.len() {
        let previous = at.checked_sub(1).and_then(|before| tokens.get(before));
        depth = angle_depth(depth, tokens.get(at)?, previous);
        if depth == 0 {
            return Some(at);
        }
    }
    None
}

/// Returns the tokens after the first token of `tokens` that `is_start`
/// accepts, or no tokens when it accepts none.
fn after_first(tokens: &[TokenTree], is_start: impl Fn(&TokenTree) -> bool) -> &[TokenTree] {
    tokens
        .iter()
        .position(is_start)
        .and_then(|at| tokens.get(at + 1..))
        .unwrap_or_default()
}

/// Splits `tokens` around its first lone `:`, as [`lone_colon`] finds it,
/// into the tokens before it and the tokens after it.
fn around_colon(tokens: &[TokenTree]) -> Option<(&[TokenTree], &[TokenTree])> {
    let (before, rest) = tokens.split_at_checked(lone_colon(tokens)?)?;
    Some((before, rest.get(1..)?))
}

/// Returns the index in `header` of the parentheses that hold the
/// parameters: the first parentheses outside the angle brackets of the
/// generics.
fn parameters_at(header: &[TokenTree]) -> Option<usize> {
    let mut depth = 0_usize;
    let mut previous: Option<&TokenTree> = None;
    for (at, token) in header.iter().enumerate() {
        if let TokenTree::Group(group) = token
            && depth == 0
            && group.delimiter() == Delimiter::Parenthesis
        {
            return Some(at);
        }
        depth = angle_depth(depth, token, previous);
        previous = Some(token);
    }
    None
}

/// Reports whether `previous` is the `-` that makes the next `>` an arrow.
fn is_arrow(previous: Option<&TokenTree>) -> bool {
    matches!(previous, Some(TokenTree::Punct(punct)) if punct.as_char() == '-' && punct.spacing() == Spacing::Joint)
}

/// Splits `tokens` at each comma outside angle brackets. Brackets of other
/// kinds are single token groups, so their commas stay inside them.
fn split_top_level(tokens: impl IntoIterator<Item = TokenTree>) -> Vec<Vec<TokenTree>> {
    let mut parts = vec![Vec::new()];
    let mut depth = 0_usize;
    let mut previous: Option<TokenTree> = None;
    for token in tokens {
        if depth == 0 && is_punct(Some(&token), ',') {
            parts.push(Vec::new());
            previous = None;
            continue;
        }
        depth = angle_depth(depth, &token, previous.as_ref());
        if let Some(part) = parts.last_mut() {
            part.push(token.clone());
        }
        previous = Some(token);
    }
    parts
}

/// Returns the position of the first lone `:` in `tokens`, the colon that
/// separates a name from its type or bound. The colons of a path such as
/// `std::env` come in a joint pair and are skipped.
fn lone_colon(tokens: &[TokenTree]) -> Option<usize> {
    let mut at = 0;
    while let Some(token) = tokens.get(at) {
        if let TokenTree::Punct(punct) = token
            && punct.as_char() == ':'
        {
            if punct.spacing() == Spacing::Joint {
                at += 2;
                continue;
            }
            return Some(at);
        }
        at += 1;
    }
    None
}

/// Returns the name that a parameter binds, the last word of its pattern
/// such as `source` in `mut source`, and the tokens of its type. A
/// receiver such as `&self` has no type and binds nothing here.
fn binding_and_type(parameter: &[TokenTree]) -> Option<(String, Vec<TokenTree>)> {
    let (pattern, kind) = around_colon(parameter)?;
    let binding = pattern.iter().rev().find_map(|token| match token {
        TokenTree::Ident(word) => Some(word.to_string()),
        _ => None,
    })?;
    Some((binding, kind.to_vec()))
}

/// Returns each generic parameter that a bound in `tokens` makes a source:
/// a name followed by a lone `:` and a bound that names a source type, in
/// the inside of a list of generics such as `<S: Source>` or of a `where`
/// clause such as `where S: Source`.
fn generic_sources(tokens: &[TokenTree]) -> Vec<Bound> {
    split_top_level(tokens.iter().cloned())
        .into_iter()
        .filter_map(|part| {
            let (generic, bound) = around_colon(&part)?;
            let TokenTree::Ident(name) = generic.last()? else {
                return None;
            };
            Some(Bound {
                name: name.to_string(),
                kind: source_kind(bound, &[])?,
            })
        })
        .collect()
}

/// Returns how a value of the type or bound `tokens` gives a variable's
/// value: as a value when they name a type or trait whose name ends in
/// `Source` or a generic parameter of `bounded` that is one, and as a
/// function when they are a function that takes a `&str` and returns an
/// `Option` or `Result` of a `String`, or name a generic that is one.
fn source_kind(tokens: &[TokenTree], bounded: &[Bound]) -> Option<Kind> {
    if names_value_source(tokens, bounded) {
        return Some(Kind::Value);
    }
    let bounded_function = tokens.iter().any(|token| {
        matches!(token, TokenTree::Ident(word) if bounded.iter().any(|bound| bound.kind == Kind::Function && word == bound.name.as_str()))
    });
    (bounded_function || is_name_function(tokens)).then_some(Kind::Function)
}

/// Reports whether `tokens` name a type or trait whose name ends in
/// `Source`, or a generic parameter of `bounded` whose values are sources.
fn names_value_source(tokens: &[TokenTree], bounded: &[Bound]) -> bool {
    tokens.iter().any(|token| match token {
        TokenTree::Ident(word) => {
            let word = word.to_string();
            word.ends_with(SOURCE_SUFFIX)
                || bounded
                    .iter()
                    .any(|bound| bound.kind == Kind::Value && bound.name == word)
        }
        TokenTree::Group(group) => {
            names_value_source(&group.stream().into_iter().collect::<Vec<_>>(), bounded)
        }
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

/// Reports whether `tokens` hold `Fn`, `FnMut` or `FnOnce` with parentheses
/// that name `str`, then `->` and a return type that names `Option` or
/// `Result` and `String`, as in `impl Fn(&str) -> Option<String>`.
fn is_name_function(tokens: &[TokenTree]) -> bool {
    tokens.windows(2).enumerate().any(|(at, pair)| {
        let [TokenTree::Ident(word), TokenTree::Group(arguments)] = pair else {
            return false;
        };
        let takes_str = arguments
            .stream()
            .into_iter()
            .any(|token| matches!(&token, TokenTree::Ident(word) if word == "str"));
        let returned = tokens.get(at + 2..).unwrap_or_default();
        let returns = is_punct(returned.first(), '-')
            && is_punct(returned.get(1), '>')
            && returned.iter().any(|token| {
                matches!(token, TokenTree::Ident(word) if word == "Option" || word == "Result")
            })
            && returned
                .iter()
                .any(|token| matches!(token, TokenTree::Ident(word) if word == "String"));
        FUNCTION_TRAITS.iter().any(|name| word == name)
            && arguments.delimiter() == Delimiter::Parenthesis
            && takes_str
            && returns
    })
}

/// Adds each read through one of `sources` in `tokens`, and in every
/// bracket inside them, to `found`.
fn collect(tokens: TokenStream, sources: &[Bound], found: &mut Vec<Read>) {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    for (at, token) in tokens.iter().enumerate() {
        let TokenTree::Group(group) = token else {
            if let Some(read) = method_read(&tokens, at, sources) {
                found.push(read);
            }
            continue;
        };
        if group.delimiter() == Delimiter::Parenthesis
            && let Some(read) = passed_read(group.stream(), sources)
        {
            found.push(read);
        }
        collect(group.stream(), sources, found);
    }
}

/// Returns the read when the tokens at `at` are a value source, a `.`, a
/// method, an optional turbofish such as `::<u16>` and parentheses whose
/// first argument is a literal name, or a function source directly followed
/// by such parentheses. A source after a `.` is a field of some other
/// value, so it reads nothing.
fn method_read(tokens: &[TokenTree], at: usize, sources: &[Bound]) -> Option<Read> {
    let Some(TokenTree::Ident(receiver)) = tokens.get(at) else {
        return None;
    };
    let source = sources
        .iter()
        .find(|source| receiver == source.name.as_str())?;
    if is_punct(at.checked_sub(1).and_then(|before| tokens.get(before)), '.') {
        return None;
    }
    let mut next = at + 1;
    if source.kind == Kind::Value {
        if !is_punct(tokens.get(at + 1), '.')
            || !matches!(tokens.get(at + 2), Some(TokenTree::Ident(_)))
        {
            return None;
        }
        next = at + 3;
        if is_punct(tokens.get(next), ':') && is_punct(tokens.get(next + 1), ':') {
            next += 2;
            if is_punct(tokens.get(next), '<') {
                next = balanced_end(tokens, next)? + 1;
            }
        }
    }
    let Some(TokenTree::Group(arguments)) = tokens.get(next) else {
        return None;
    };
    if arguments.delimiter() != Delimiter::Parenthesis {
        return None;
    }
    let arguments: Vec<TokenTree> = arguments.stream().into_iter().collect();
    name_read(&arguments)
}

/// Returns the read when the arguments `tokens` are a source, with or
/// without a leading `&`, a comma and a literal name.
fn passed_read(tokens: TokenStream, sources: &[Bound]) -> Option<Read> {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let start = usize::from(is_punct(tokens.first(), '&'));
    let TokenTree::Ident(first) = tokens.get(start)? else {
        return None;
    };
    if !sources.iter().any(|source| first == source.name.as_str())
        || !is_punct(tokens.get(start + 1), ',')
    {
        return None;
    }
    name_read(tokens.get(start + 2..)?)
}

/// Returns the read of the first argument in `arguments` when it is a
/// string literal whose text is a valid name with no lower-case letter,
/// followed by nothing or a comma.
fn name_read(arguments: &[TokenTree]) -> Option<Read> {
    let Some(TokenTree::Literal(literal)) = arguments.first() else {
        return None;
    };
    if !(arguments.len() == 1 || is_punct(arguments.get(1), ',')) {
        return None;
    }
    let written = literal.to_string();
    let text = string_text(&written)?;
    if text.chars().any(|character| character.is_ascii_lowercase()) {
        return None;
    }
    let name: Name = text.parse().ok()?;
    Some(Read {
        target: Target::Variable(name),
        line: literal.span().start().line,
        checked: false,
    })
}
