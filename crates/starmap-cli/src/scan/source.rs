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
//! A parameter of a function is a source when the function is named
//! `from_source`, when its type names a type or trait whose name ends in
//! `Source`, as in `&impl Source`, `&dyn ConfigSource` or
//! `Box<dyn Source>`, and when its type names a generic parameter whose
//! bound, in the function's generics or its `where` clause, names one, as in
//! `fn f<S: Source>(source: &S)`. Inside the function's body, two shapes
//! read the variable whose name is a string literal:
//!
//! - a method call on the source whose first argument is the literal, such
//!   as `source.get("APP_PORT")`, `source.get_or("APP_HOST", "localhost")`
//!   or `source.parse::<u16>("APP_PORT")`;
//! - a call whose first argument is the source, with or without a leading
//!   `&`, and whose second is the literal, such as
//!   `required(source, "APP_URL")` or `Self::parse(&source, "APP_URL")`.
//!
//! The literal must be a valid name with no lower-case letter, as a
//! variable's name in a spec is by convention, so a lookup such as
//! `source.get("orders")` on a source of something else is not a read. The
//! place of the read is the line of the literal.
//!
//! This is a token match with no type checking. It misses a source held in
//! a struct field (`self.source.get(...)` is not a read), in a closure's
//! parameter or in a local variable, a source whose type is named by an
//! alias that does not end in `Source`, and a name held in a constant. It
//! counts a method call of any name on a source. When a function nested in
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

/// The name of the function that builds a config from a source, each of
/// whose parameters is a source whatever its type.
const FROM_SOURCE: &str = "from_source";

/// The ending of the name of a type or trait whose values are sources.
const SOURCE_SUFFIX: &str = "Source";

/// Returns the reads that the body of the function `name` makes through
/// its sources. `header` holds the tokens between the function's name and
/// its body: its generics, parameters, return type and `where` clause.
pub(super) fn reads(name: &str, header: &[TokenTree], body: &Group) -> Vec<Read> {
    let Some((before, Some((TokenTree::Group(parameters), after)))) = parameters_at(header)
        .and_then(|at| header.split_at_checked(at))
        .map(|(before, rest)| (before, rest.split_first()))
    else {
        return Vec::new();
    };
    // The generics come before the parameters, and the `where` clause
    // follows the return type.
    let generics = after_first(before, |token| is_punct(Some(token), '<'));
    let clause = after_first(
        after,
        |token| matches!(token, TokenTree::Ident(word) if word == "where"),
    );
    let mut bounded = generic_sources(generics);
    bounded.extend(generic_sources(clause));
    let sources: Vec<String> = split_top_level(parameters.stream())
        .into_iter()
        .filter_map(|parameter| {
            let (binding, kind) = binding_and_type(&parameter)?;
            (name == FROM_SOURCE || names_source(&kind, &bounded)).then_some(binding)
        })
        .collect();
    let mut found = Vec::new();
    if !sources.is_empty() {
        collect(body.stream(), &sources, &mut found);
    }
    found
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
        match token {
            TokenTree::Punct(punct) if punct.as_char() == '<' => depth += 1,
            // The `>` of `->`, as in a bound `Fn(u8) -> u8`, closes nothing.
            TokenTree::Punct(punct) if punct.as_char() == '>' && !is_arrow(previous) => {
                depth = depth.saturating_sub(1);
            }
            TokenTree::Group(group)
                if depth == 0 && group.delimiter() == Delimiter::Parenthesis =>
            {
                return Some(at);
            }
            _ => {}
        }
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
        match &token {
            TokenTree::Punct(punct) if punct.as_char() == '<' => depth += 1,
            TokenTree::Punct(punct) if punct.as_char() == '>' && !is_arrow(previous.as_ref()) => {
                depth = depth.saturating_sub(1);
            }
            TokenTree::Punct(punct) if punct.as_char() == ',' && depth == 0 => {
                parts.push(Vec::new());
                previous = None;
                continue;
            }
            _ => {}
        }
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
fn generic_sources(tokens: &[TokenTree]) -> Vec<String> {
    split_top_level(tokens.iter().cloned())
        .into_iter()
        .filter_map(|part| {
            let (generic, bound) = around_colon(&part)?;
            let TokenTree::Ident(name) = generic.last()? else {
                return None;
            };
            names_source(bound, &[]).then(|| name.to_string())
        })
        .collect()
}

/// Reports whether `tokens`, a type or a bound, name a type or trait whose
/// name ends in `Source`, or one of the generic parameters `bounded`.
fn names_source(tokens: &[TokenTree], bounded: &[String]) -> bool {
    tokens.iter().any(|token| match token {
        TokenTree::Ident(word) => {
            let word = word.to_string();
            word.ends_with(SOURCE_SUFFIX) || bounded.contains(&word)
        }
        TokenTree::Group(group) => {
            names_source(&group.stream().into_iter().collect::<Vec<_>>(), bounded)
        }
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

/// Adds each read through one of `sources` in `tokens`, and in every
/// bracket inside them, to `found`.
fn collect(tokens: TokenStream, sources: &[String], found: &mut Vec<Read>) {
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

/// Returns the read when the tokens at `at` are a source, a `.`, a method,
/// an optional turbofish such as `::<u16>` and parentheses whose first
/// argument is a literal name. A source after a `.` is a field of some
/// other value, so it reads nothing.
fn method_read(tokens: &[TokenTree], at: usize, sources: &[String]) -> Option<Read> {
    let Some(TokenTree::Ident(receiver)) = tokens.get(at) else {
        return None;
    };
    if !sources.iter().any(|source| receiver == source)
        || is_punct(at.checked_sub(1).and_then(|before| tokens.get(before)), '.')
        || !is_punct(tokens.get(at + 1), '.')
        || !matches!(tokens.get(at + 2), Some(TokenTree::Ident(_)))
    {
        return None;
    }
    let mut next = at + 3;
    if is_punct(tokens.get(next), ':') && is_punct(tokens.get(next + 1), ':') {
        next += 2;
        if is_punct(tokens.get(next), '<') {
            let mut depth = 0_usize;
            while let Some(token) = tokens.get(next) {
                let previous = next.checked_sub(1).and_then(|before| tokens.get(before));
                next += 1;
                match token {
                    TokenTree::Punct(punct) if punct.as_char() == '<' => depth += 1,
                    TokenTree::Punct(punct) if punct.as_char() == '>' && !is_arrow(previous) => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
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
fn passed_read(tokens: TokenStream, sources: &[String]) -> Option<Read> {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let start = usize::from(is_punct(tokens.first(), '&'));
    let TokenTree::Ident(first) = tokens.get(start)? else {
        return None;
    };
    if !sources.iter().any(|source| first == source) || !is_punct(tokens.get(start + 1), ',') {
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
