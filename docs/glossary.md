# Glossary

This page names each starmap concept once. Where a `starmap.toml` and the
Rust API spell a concept differently, the entry gives both spellings.

## type

The `type` field of a variable in `starmap.toml` states what the variable's
value is. The Rust API names it `starmap::Type`, and `Variable::value_type`
returns it.

The field takes one of the words `path`, `port`, `url`, `socket-address`,
`boolean` and `string`, or a table with one key:

- `{ path = { absolute = true } }` is a path that must be absolute. A
  relative path, including an empty one, then counts as unset.
- `{ integer = { min = 1, max = 100 } }` is a whole number from `min` to
  `max`, both included.
- `{ enumeration = ["dev", "prod"] }` is one of the listed values.
- `{ string = { pattern = "[0-9a-f]{40}" } }` is Unicode text that the
  pattern matches as a whole. `{ string = {} }` is the same type as the word
  `string`, which takes any text, including text that is not Unicode.

A pattern is written in the syntax of the Rust `regex` crate, which is close
to RE2's. It has no look-around and no backreferences, and matching takes
time linear in the value's length. A pattern that another implementation
must read should keep to the syntax that both the `regex` crate and RE2
accept. starmap anchors the pattern as `\A(?:pattern)\z`, so it matches the
whole value, and `^` and `$` are allowed and unnecessary. A pattern holds at
most 1024 bytes. The parser refuses a longer pattern, and one that the
`regex` crate does not compile within its default size limit, with
`starmap::spec::bad_pattern`, which quotes the pattern. A value that does not
match fails with the text `the value does not match the pattern <pattern>`,
which never holds the value. `starmap run` refuses such a value, and
`starmap explain` marks it invalid. A pattern that sets the `x` flag and ends
inside a `#` comment is refused, because the comment would hide the anchors;
ending the comment with a newline fixes it. The Rust API names the pattern
`starmap::Pattern`.

`Type::parse` reads a raw value in the type and returns a `Value`, nothing
when the value counts as unset, or a `TypeError` when the value does not fit
the type. `starmap explain` and
`starmap diff` print a variable's type under the name `type`.

## withheld

Withheld is how a variable's value is shown. A variable states it with
`show = "withheld"`, and the Rust API spells it `Show::Withheld`. The other
word is `shown`, which is `Show::Shown`.

A withheld value is held in a `starmap::Secret`, whose `Debug` prints
`Secret(••••••••)` whatever the value. `starmap explain` prints eight dots
in place of a withheld value and gives it no text in JSON. A withheld
variable has no default, because a default would sit in `starmap.toml` in
plain text, and its type is `string`, with or without a pattern. A check
reports a withheld value that does not match its pattern by naming the
pattern alone. The parser refuses a withheld
variable that breaks either rule.

The parser also forces `show = "withheld"` on every variable whose source is
`secret` or `generated`, and it refuses `show = "shown"` on them. A variable
with any other source, which is `setting`, `inherited` or `provided`, can be
withheld too. Such a variable gets its value as any other variable of its
source does: from the shell, from `.env` beside the spec or from `--set`, from
its path when a `setting` has a `bind`, or from a provider. Starmap does not
ask for it at a terminal. Only a `secret` with no `bind` is asked for there.
A withheld variable is kept out of output although it is not a secret in the
sense of the next entry.

## secret

Secret names a source: `source = "secret"` in `starmap.toml`, and
`Source::Secret` in the Rust API. A `secret` variable is always withheld. Its
optional `bind` is a `one-password` reference that a resolver reads the value
from, with an optional `cache = false` that keeps the value out of any cache
of resolved values.

A secret differs from a withheld `setting`. The word `secret` says where the
value comes from, and `withheld` says only how the value is shown. A
`setting` takes only a `path` bind, which is a template that a host expands
into the value.

`starmap::Secret` is the Rust type that holds any withheld value, whichever
source the value comes from.

## provided

Provided has two spellings in a `starmap.toml`, and they sit in different
places.

- The source is the word `provided` in a variable's `source` field:
  `source = "provided"`. The Rust API spells it `Source::Provided`.
- The bind key is `provided` inside the variable's `bind` table, as in
  `bind = { provided = "multiverse/port", process = "api" }`. Its value is a
  provider kind written `<host>/<kind>`, and the other keys of the table are
  qualifiers that tell the provider which value to supply. The Rust API spells
  the whole bind `ProvidedRef`, and the kind `ProviderKind`.

A source of `provided` needs a bind with the `provided` key, and a bind with
the `provided` key is allowed only on a source of `provided`. A spec that
pairs them any other way fails with `starmap::spec::bind_mismatch`. To tell
the two apart, look at the key: the source is the value of `source`, and the
bind key is a key inside `bind`.

## Unset, UnsetRule and the unset field

The `unset` field of a variable in `starmap.toml` says what happens when the
variable has no value. It is one of the words `required` and `optional`, or
a table that has either `local` (one of those words) or `default` (text
that the variable's type reads), and an optional `production` (one of those
words).

`Unset` is the Rust enum for the rule in one mode, with the variants
`Required`, `Optional` and `Default(Value)`.

`UnsetRule` holds an `Unset` for local mode and one for production mode.
`UnsetRule::in_mode` returns the rule of a mode, and `Variable::unset`
returns a variable's `UnsetRule`. When the file states no `production`, the
production rule equals the local one. A stated `production` is `required`
or `optional`, because a production rule takes no default of its own.

An empty value counts as unset, wherever it comes from: the process
environment, `.env`, `--set`, a provider or a resolver. So `FOO=` gives a
variable its default, leaves an optional variable unset, and leaves a
required variable missing. An empty value in a later layer does not hide a
value from an earlier one, so `FOO=` in the shell leaves the value that
`.env` gives `FOO` in place. `starmap::capture` binds nothing for an empty
value, and `check` treats an empty bound value as unset before any type
reads it.

## pointer

A pointer is a `starmap.toml` that names the file that declares the
variables, in place of declaring them itself. A file is a pointer when its
top level has the key `spec`. The Rust API names it `starmap::Pointer`, and
`Pointer::parse` returns it, or nothing for a file without `spec`.

```toml
spec = "config/environment.toml"
src = ["src"]
dotenv = ".env"
```

Each path is relative to the directory of the pointer file. `spec` is the
declaration file. `src` lists the directories whose source `starmap init`
and `starmap check` scan, is `["src"]` when the pointer leaves it out, and
scans nothing when it is an empty list.
`dotenv` is the file that `starmap run` reads values from, and is `.env`
when the pointer leaves it out. `starmap init`, `check`, `explain`, `diff`
and `run` follow a pointer as if `--spec` named the declaration file, and
`#[derive(Env)]` embeds the declaration file and names it in
`Env::SPEC_FILE`. `starmap run` keeps the pointer's directory as the
checkout.

A file either points or declares. A pointer that also has `variables` or
`mode` fails with `starmap::spec::pointer_declares`. A pointer reached where
a declaration file belongs, such as the file that another pointer names,
fails with `starmap::spec::pointer_chain`, so starmap follows at most one
pointer. A pointer has no digest of its own, and `starmap check` prints the
digest of the declaration file.
