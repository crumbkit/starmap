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
- `{ enumeration = "log-level" }` is one of the values of the named
  enumeration `log-level`, which the next entry describes.
- `{ url = { schemes = ["postgres", "postgresql"] } }` is a URL whose scheme
  is one of the listed schemes. Each scheme is written in lower case, and a
  value's scheme matches in any case. `{ url = {} }` means the same as
  `url`.
- `{ string = { pattern = "[0-9a-f]{40}" } }` is Unicode text that the
  pattern matches as a whole. `{ string = {} }` is the same type as the word
  `string`, which takes any text, including text that is not Unicode.

- `{ bytes = { encoding = "base64", length = 32 } }` is bytes written as
  text in an encoding, whose length after decoding is exactly `length`, or
  at least `min-length` when the table sets that key instead. A bytes table
  takes exactly one of the two lengths.

A bytes type's `encoding` is `hex`, `base64` or `base64url`. `hex` takes two
digits per byte in upper or lower case. `base64` is RFC 4648 section 4 with
its padding required, and `base64url` is RFC 4648 section 5 with all its
padding or none, and partly padded text is refused. Each encoding refuses
whitespace. A length runs from 1 to 1024 bytes, and the parser refuses one
outside that range with `starmap::spec::bad_bytes`. A value that does not
decode fails with the text `the value is not base64`, and one of the wrong
size with `the value does not decode to 32 bytes` or
`the value does not decode to at least 16 bytes`. Neither text holds the
value or its decoded size. A shown value is held decoded, as `Value::Bytes`,
and is written back in its encoding's one spelling: lower case hex, padded
base64 or unpadded base64url. A shown value therefore reaches the child
process in that spelling, so a value written in upper case hex arrives in
lower case, and padded base64url arrives without its padding. A generated
variable cannot have a bytes type yet, and the parser refuses one with
`starmap::spec::bad_bytes`. The Rust API names the rule `starmap::ByteRule`,
its encoding `starmap::Encoding`, its length `starmap::ByteLength` and the
count in a length `starmap::ByteCount`.

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

## enumeration

An enumeration type takes one of a fixed list of values. Each value is text
that is not empty and holds no whitespace. A variable writes the list inline,
as `type = { enumeration = ["dev", "prod"] }`, or names an enumeration that
the file declares, as `type = { enumeration = "log-level" }`. The Rust API
names the type `Type::Enumeration` and its values `starmap::Choices`.

A file declares a named enumeration in a top-level table, with each value as
a key and a description of that value as the key's value:

```toml
[enumerations.log-level]
debug = "Logs every request and its timing."
info = "Logs each request once."
warn = "Logs only failures and slow requests."
```

The values keep the file's order. A name starts with a lower case ASCII
letter and holds only lower case ASCII letters, digits and hyphens; the Rust
API names it `starmap::EnumerationName`. Every value has a description, which
follows the rules of `purpose`. The parser refuses a bad name, a value with
whitespace and an empty or non-text description with
`starmap::spec::bad_value`, a description of `?` with
`starmap::spec::guess`, and an empty table with
`starmap::spec::empty_enumeration`. A variable that names an enumeration the
file does not declare fails with `starmap::spec::enumeration_undeclared`.
Any number of variables may share one named enumeration, and a file may
declare one that no variable uses. An inline list has no descriptions.

A named enumeration follows every rule of an inline one: a default must be
one of its values, and a withheld variable cannot have it as its type. The
canonical form writes each `[enumerations.<name>]` table after `[mode]` and
before the variables, in name order, so renaming an enumeration or changing
a description changes the digest. `Spec::enumerations` lists the named
enumerations, `Choices::name` returns a named enumeration's name, and
`Choices::description` returns a value's description. `starmap explain`
prints the type as `log-level: one of debug, info, warn`, and its `--long`
column `values` lists each value with its description. `starmap diff`
reports a changed description as a change of `values`.

## withheld

Withheld is how a variable's value is shown. A variable states it with
`show = "withheld"`, and the Rust API spells it `Show::Withheld`. The other
word is `shown`, which is `Show::Shown`.

A withheld value is held in a `starmap::Secret`, whose `Debug` prints
`Secret(••••••••)` whatever the value. `starmap explain` prints eight dots
in place of a withheld value and gives it no text in JSON. A withheld
variable has no default, because a default would sit in `starmap.toml` in
plain text, and its type is `string`, with or without a pattern, or
`bytes`. A check reports a withheld value that does not match its pattern by
naming the pattern alone, and one that does not decode or has the wrong size
by naming the encoding or the length alone. A check holds the bytes that a
withheld bytes value decodes to in a `starmap::SecretBytes`, whose memory is
zeroed when it is dropped, beside its text, and `Environment::apply` gives a
child the text. The parser refuses a withheld
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

## effect

The optional `effect` field of a variable in `starmap.toml` is one sentence
that says what the application does when the variable is unset, such as
`effect = "Calls are not recorded."`. Any variable may have one. Its text
follows the rules of `purpose`: the parser refuses an empty or blank
sentence with `starmap::spec::bad_value`, and `?` with
`starmap::spec::guess`. `starmap init` does not write it.
`Variable::effect` returns the sentence, or nothing when the file states
none.

The effect counts toward the canonical form and the digest, as the purpose
does. The canonical form writes it right after `purpose`, and only when the
variable has one, so a file without effects keeps its digest.
`starmap explain --long` prints it in the `effect` column after `purpose`,
and `starmap explain --json` leaves the field out when the variable has
none. `starmap diff` reports an effect that was added, changed or removed
as a change of the field `effect`.

## group

A group is a set of variables that must be set together or not at all,
such as the account, key and secret of one external service. A file
declares one as a top-level table with one field, `variables`:

```toml
[groups.mirror]
variables = ["SHELF_MIRROR_ACCOUNT", "SHELF_MIRROR_KEY", "SHELF_MIRROR_SECRET"]
```

A group name starts with a lower case ASCII letter and holds only lower
case ASCII letters, digits and hyphens, as an enumeration name does, and a
bad name fails with `starmap::spec::bad_value`. A member that the file does
not declare fails with `starmap::spec::group_undeclared`, which names the
group and the variable. `starmap::spec::bad_group` refuses a group with
fewer than two members or a repeated member, a variable in two groups, and
a member with a default or a generated value, because a default or a
generated value always counts as set. The Rust API names a group
`starmap::Group` and its name `starmap::GroupName`. `Spec::groups` lists
the groups in name order, `Spec::group` returns one by name, and
`Spec::group_of` returns the group a variable belongs to.

`check` applies the rule after it has read each variable. A member counts
as set when the check holds a value for it, so `FOO=` counts as unset. When
at least one member is set and another is unset, each unset member that
has no finding of its own gets `Problem::GroupIncomplete`, which names the
group and the members that are set. A member that is missing because it is
required gets only that finding. A group that is wholly unset or wholly
set passes. A member may be required in production and optional locally;
the group is then required in production and all or none locally.
`starmap run` reports the finding with the code `starmap::group::incomplete`,
and `#[derive(Env)]` refuses the read with the same text.

Groups count toward the canonical form and the digest, because a group
changes what `check` accepts. The canonical form writes each group after
the named enumerations and before the variables, in name order, with its
members in the file's order. A file without groups keeps its digest.
`starmap explain --long` prints the variable's group in the `group`
column, and `starmap explain --json` leaves the field out for a variable
without one. `starmap diff` reports a variable that joins, leaves or
changes group as a change of the field `group`.

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

A file either points or declares. A pointer that also has `variables`,
`groups` or `mode` fails with `starmap::spec::pointer_declares`. A pointer reached where
a declaration file belongs, such as the file that another pointer names,
fails with `starmap::spec::pointer_chain`, so starmap follows at most one
pointer. A pointer has no digest of its own, and `starmap check` prints the
digest of the declaration file.

## tool table

A tool table is a `[tool.<name>]` table, where a tool such as a generator,
a linter or a host keeps settings of its own in the file that starmap reads.
A file that declares variables may hold tool tables at its top level and in
each variable, as `[tool.codegen]` and `[variables.SHELF_PORT.tool.codegen]`,
and a pointer may hold them at its top level. The Rust API names one
`starmap::ToolTable`, which holds its keys and values as the text of a TOML
document. `Spec::tool`, `Variable::tool` and `Pointer::tool` return the
table of one tool, and `tools` lists them in name order.

starmap checks only that `tool` is a table whose values are tables, and
refuses any other value with `starmap::spec::bad_value`. It never reads the
keys inside a tool table. The canonical form and the digest leave tool
tables out, so an edit to a tool table never changes a file's digest, and
`starmap explain`, `check`, `diff` and `init` print the same output for a
file with tool tables as for the same file without them. starmap never
merges a pointer's tool tables into those of the file it names.
