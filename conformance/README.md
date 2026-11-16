# starmap conformance cases

These cases let an implementation of the `starmap.toml` format prove that it
reads files as starmap does. A reader written in another language runs them
to show that it parses the same files, refuses the same files with the same
codes, writes the same canonical form and digest, and decides the same
values from the same environment. starmap's own test suite runs every case
on each pull request, in `crates/starmap/tests/conformance.rs`.

This is version 1 of the layout. A change that keeps every existing case
valid, such as a new case or a new optional key, stays in version 1. A
change that a version 1 runner would misread, such as a renamed key or a
new required file, moves the cases to `conformance/v2/`.

## Layout

```text
conformance/
  README.md
  .gitattributes
  parse/<case>/starmap.toml     the input file
  parse/<case>/case.toml        what to expect
  parse/<case>/canonical.toml   the expected canonical form, for a file that parses
  check/<case>/starmap.toml     the input file, which parses
  check/<case>/case.toml        the environment and what to expect
```

Each case is one directory, and its name is a short description in lower
case words joined by hyphens, such as `parse/bad-default`. A runner reads
the cases of `parse` and then those of `check`, each in the byte order of
their names. Every file is UTF-8 text with `\n` line endings, which
`.gitattributes` keeps on every checkout.

A runner refuses an entry at the root of `conformance/` other than
`README.md`, `.gitattributes`, `parse` and `check`, so a mistyped
directory such as `chek` fails. It also refuses a case directory that lacks
a file the layout needs, that holds a file the layout does not name, or
whose `case.toml` holds a key the layout does not name. A mistyped case
then fails instead of passing with nothing checked. A runner reports every
failing case in one run, by its path.

## case.toml

`case.toml` is TOML. Its top-level keys are:

| Key | Kind | Meaning |
| --- | --- | --- |
| `description` | text, required | One sentence that says what the case shows. |
| `environment` | table of text, check cases only, optional | The environment the check reads, as names and values. A case without it reads an empty environment. |
| `expect` | table, required | The result, as the next two sections describe. |

### A parse case

A parse case parses `starmap.toml`, and its `expect` table holds exactly one
of these keys:

| Key | Kind | Meaning |
| --- | --- | --- |
| `digest` | text | The file parses, and its digest is this text. `canonical.toml` holds its canonical form. |
| `problems` | array of text | The file is refused, and these are the codes of every problem, in the order the parser finds them. |

```toml
description = "A default that does not fit the type is refused."

[expect]
problems = ["starmap::spec::bad_default"]
```

A digest is `sha256:` followed by the 64 lower case hex digits of the
SHA-256 hash of the canonical form's UTF-8 bytes. The glossary's
[canonical form](../docs/glossary.md#canonical-form) and
[digest](../docs/glossary.md#digest) entries give the rules that decide
those bytes: the order of tables and fields, the fields and tables the form
leaves out, how it quotes strings, and its line endings. A runner compares
the canonical form it writes with `canonical.toml` byte for byte, and checks
that parsing `canonical.toml` gives the same digest. A case that expects
`problems` has no `canonical.toml`.

### The order of problems

`problems` is an ordered list, and a runner compares it in order. starmap
reports the problems of a file in this order:

1. A file over 262,144 bytes gives the one problem
   `starmap::spec::too_large`, and a file that is not TOML gives the one
   problem `starmap::spec::syntax`. starmap reads nothing more of either.
2. A file whose top level has the key `spec` is a pointer. It gives
   `starmap::spec::pointer_declares` once for each of `mode`,
   `enumerations`, `groups`, `families` and `variables` that it has, in the
   file's order, or else the one problem `starmap::spec::pointer_chain`.
   starmap reports nothing else of a pointer.
3. Any other file first gives each top-level key that starmap does not
   know, in the file's order. starmap then reads the top-level tables in a
   fixed order, whatever their order in the file: `enumerations`,
   `families`, `variables`, `mode`, `groups` and `tool`.
4. Inside `enumerations`, `families`, `variables`, `groups` and `tool`,
   starmap reads the entries in the file's order. After the last family,
   it reports each family whose prefix is a member name of another family,
   in the file's order.

A variable reports its problems in this order, and a family reports them in
the same order:

1. A key that is not a variable name, or not a family prefix.
2. A value that is not a table. A family reports nothing more after this
   step. A variable still reports the family-membership step (step 7), so
   `variables.SHELF_DB_URL = 5` beside `[families.SHELF_DB]` gives
   `starmap::spec::bad_value` and then `starmap::spec::bad_family`.
3. Each key of the table that the entry does not take, in the file's order.
   A family does not take `presence`.
4. The fields `purpose`, `effect`, `check`, `type`, `show`, `unset`,
   `presence`, `empty`, `source`, `bind` and `tool`, in that order. A
   missing required field is reported in its place in this order, so a
   variable that has only a `purpose` gives four
   `starmap::spec::missing_field` problems, for `type`, `show`, `unset` and
   `source`.
5. The rules that join fields: `starmap::spec::shown_secret`, then
   `starmap::spec::withheld_type`, then a `bytes` field that the variable
   does not take, then a bind that does not fit the source or a generated
   length that does not fit its type.
6. For a family, its `keys`, and then a default, a required open family and
   a source that a family does not take, in that order.
7. For a variable, a declared variable that is a member of a family.

The `mode` table reports each key it does not take, then its `variable`,
then its `production`, and then a variable that the file does not declare
or that cannot decide the mode. An enumeration reports its name, then a
value that is not a table and an empty table, and then each value in the
file's order. A group reports its name, then a value that is not a table,
each key it does not take, its `variables` field, a list of fewer than two
members, and then each member's problem in the list's order.

The message and the span of each problem are not part of a case. They are
English text and positions in the file, and each implementation words and
places them for itself.

### A check case

A check case parses `starmap.toml`, which always parses, and reads the
`environment` table as the process environment. It binds the variables the
file declares, decides the mode from that environment, checks the bindings
in that mode, and compares the result with `expect`, which holds either
`mode` and `values`, or `refused`:

| Key | Kind | Meaning |
| --- | --- | --- |
| `mode` | text | The check passes, and the mode it decided is `local` or `production`. |
| `values` | table of text | The text that each variable gives the child process. A variable that stays unset is left out. |
| `refused` | array of tables | The check refuses, and each table names a `variable` and the `code` of its refusal, in the order the check reports them. |

```toml
description = "An empty value counts as unset, so the default applies."

[environment]
SHELF_PORT = ""

[expect]
mode = "local"
values = { SHELF_PORT = "8080" }
```

```toml
description = "A port that is not a number is invalid."

[environment]
SHELF_PORT = "80a"

[expect]
refused = [
  { variable = "SHELF_PORT", code = "starmap::variable::invalid" },
]
```

`values` holds the value of a withheld variable too, because the values of
a case are test data. A member of a family that is set appears under its
own name. The codes in `refused` are the ones that `starmap run` reports:

| Code | The variable |
| --- | --- |
| `starmap::variable::missing` | is required in the mode and has no value. |
| `starmap::variable::invalid` | has a value that does not fit its type. |
| `starmap::group::incomplete` | is unset, and another member of its group is set. |
| `starmap::family::unlisted_key` | is a member of a listed family whose key the family does not list. |

The check reports the declared variables in the file's order, and then the
members of each family, family by family in prefix order and member by
member in name order. The glossary in `docs/glossary.md` gives the rules
that decide each value and each refusal.

## What the cases leave out

The cases cover the file format, its canonical form and digest, and the
check of an environment. They do not cover how starmap gets values from
anywhere other than the environment of a case: binds, providers, resolvers,
`.env` files, pointer files that are followed, and `starmap run`. Those
stay starmap's own behaviour.

A value that is not Unicode cannot be written in TOML, so no case holds
one. The glossary's Unicode entry gives the rule for such a value.

No case expects `starmap::spec::too_large`. A `starmap.toml` may hold at
most 262,144 bytes, which is 256 KiB. starmap refuses a larger file with
`starmap::spec::too_large` before it parses it, and a case would need an
input over that size, which is too large to keep in the repository. An
implementation tests the limit with an input it builds for itself.

## Running the cases from another language

1. Refuse an unknown entry at the root, and list the directories under
   `parse` and under `check`, each in the byte order of their names.
2. For each case, refuse a missing or unknown file and an unknown key in
   `case.toml`, as the layout section says.
3. For a parse case, parse `starmap.toml`. Compare the digest and the
   canonical form with `digest` and `canonical.toml`, or the codes of the
   problems, in order, with `problems`.
4. For a check case, parse `starmap.toml`, check the `environment` table in
   the mode it decides, and compare the mode and the text each variable
   gives the child with `mode` and `values`, or each refused variable and
   its code with `refused`.
5. Report each case whose result differs by its path, with the expected and
   the actual result.

A TOML parser for the language reads `case.toml`. The `starmap.toml` of a
case is input to the implementation under test, so the implementation's own
parser reads it.
