# Changelog

This file records the notable changes to the published starmap crates. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
the crates follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `starmap run --only-declared -- <program>` starts the program with an
  environment of only declared variables: each declared variable that has
  a value, each member of a declared family that is set, and each built-in
  variable that the shell sets. The program inherits no other variable, so
  a read of an undeclared variable finds it unset. The built-in variables
  are a fixed list per platform of names that the operating system and
  common tools need: `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `TMPDIR`,
  `LANG`, the locale categories of POSIX and glibc such as `LC_ALL` and
  `LC_CTYPE`, `TERM` and `TZ` on Unix, and `PATH`, `PATHEXT`, `SystemRoot`,
  `SystemDrive`, `windir`, `ComSpec`, `TEMP`, `TMP`, `USERPROFILE`,
  `USERNAME`, `APPDATA` and `LOCALAPPDATA` on Windows. A file may declare a
  built-in name to give it a type. `starmap explain` without a name lists
  the built-in variables in its note. The flag is off by default, and
  `starmap run` without it starts the program as before. The Rust API gains
  `Environment::apply_only_declared` and `BuiltIns`, so a host program can
  start a child the same way.
- `starmap check` reads a name that `format!` builds inside `var(...)` or
  `var_os(...)`, with or without `&` and a path such as `std::format!`. A
  literal that is a name, an underscore and one trailing placeholder, such
  as `format!("SHELF_DB_URL_{region}")`, reads a member of the family with
  that prefix, and the new `starmap::family::undeclared` fails check when
  the file declares no such family. A literal without a placeholder reads
  the name it holds. check passes on any other `format!` name, such as one
  that starts with a placeholder or has two, adds a sentence that names
  each such place, and lists them in the new `unresolved` field of its
  JSON. A name built with `concat!`, `+` or `push_str`, or held in a
  constant, is still not counted.
- A file can declare a family of variables whose names are chosen at run
  time, as `[families.SHELF_DB_URL]` with the fields of a variable. A
  member is any variable named by the prefix, an underscore and a key, such
  as `SHELF_DB_URL_EU`. A family's keys are open unless `keys = [...]`
  lists them, and a listed family refuses a member whose key it does not
  list with `Problem::UnlistedKey`, which `starmap run` reports as
  `starmap::family::unlisted_key`. Only a listed family may be required, no
  family has a default, and a family is a setting without a bind or
  inherited. The new `starmap::spec::bad_family` refuses a file that breaks
  these rules, or whose family shares a member with a variable or another
  family, and `starmap::spec::bad_group` refuses a group that names a
  family or a member. `capture`, `check`, `.env`, `--set` and `starmap
  check` read each member through `Spec::declared`. A struct field reads a
  family with `#[starmap(family = "SHELF_DB_URL")]` into the new
  `starmap::Family<T>`, and `starmap_test::check` compares it with the
  family. The Rust API gains `FamilyDeclaration`, `Declaration`,
  `Spec::families`, `Spec::family`, `Spec::declared`,
  `Environment::members`, `Bindings::iter` and `Field::family`. Families
  count toward the digest, and the digest of a file without families does
  not change.
- A field of a struct that derives `Env` can name the application's own
  check, as `#[starmap("SHELF_PORT", check = unprivileged)]`. A shown
  field's check returns `Result<(), String>` and a withheld field's check
  returns `Result<(), &'static str>`, so its reason cannot hold the secret.
  `Env::from_vars` runs the check on each value, a default included, after
  the value's type and unset rule pass, and a refusal is the new finding
  `Problem::Rejected` among the other findings, written as
  `SHELF_PORT: the port must be above 1024`. A variable may state its
  check for a reader in one sentence in the optional `check` field, which
  starmap never runs. The parser refuses an empty sentence with
  `starmap::spec::bad_value` and `?` with `starmap::spec::guess`.
  `Variable::check` returns it, `starmap explain` prints it in the long
  column `check` and an optional JSON field, and `starmap diff` reports a
  changed check. `starmap check` notes a check in code without a sentence
  and a sentence without a check in code, and lists them in the JSON fields
  `checks_without_sentence` and `sentences_without_check`. The sentence
  counts toward the digest, and the digest of an existing file does not
  change.
- A file can declare a group of variables that must be set together or not
  at all, as `[groups.mirror]` with
  `variables = ["SHELF_MIRROR_ACCOUNT", "SHELF_MIRROR_KEY"]`. `check`
  refuses a partly set group with one finding for each unset member, which
  names the members that are set, and passes a group that is wholly set or
  wholly unset. `starmap run` reports it as `starmap::group::incomplete`.
  A member the file does not declare fails with the new
  `starmap::spec::group_undeclared`, and the new `starmap::spec::bad_group`
  refuses a group of fewer than two variables, a repeated member, a
  variable in two groups, and a member with a default or a generated
  value. A bad group name fails with `starmap::spec::bad_value`. Groups
  count toward the canonical form and the digest. `starmap explain` adds a
  `group` long column and an optional `group` JSON field, and
  `starmap diff` reports a changed group as a change of `group`. The Rust
  API adds `Group`, `GroupName`, `GroupNameError`, `Spec::groups`,
  `Spec::group`, `Spec::group_of`, `Problem::GroupIncomplete`, and
  `SpecProblemKind::GroupUndeclared` and `BadGroup`. The digest of an
  existing file does not change.
- A file can declare named enumerations, each value with a description, as
  `[enumerations.log-level]` with `debug = "Logs every request."`, and a
  variable can name one with `type = { enumeration = "log-level" }`. Any
  number of variables may share one. A name starts with a lower case ASCII
  letter and holds lower case letters, digits and hyphens. A bad name, a
  value with whitespace or a missing, empty or blank description fails with
  `starmap::spec::bad_value`, a description of `?` with
  `starmap::spec::guess`, an empty table with
  `starmap::spec::empty_enumeration`, and a name the file does not declare
  with the new `starmap::spec::enumeration_undeclared`. A default outside
  the values fails with `starmap::spec::bad_default`, and `starmap run`
  refuses a value outside them as it does for an inline list. The names
  and descriptions count toward the canonical form and the digest.
  `starmap explain` prints the type as `log-level: one of debug, info`,
  adds a `values` long column, and adds the optional JSON fields
  `enumeration` and `values`. `starmap diff` reports a changed description
  as a change of `values`. The Rust API adds `EnumerationName`,
  `Choices::name`, `Choices::description`, `Spec::enumeration` and
  `Spec::enumerations`. The digest of an existing file does not change.
- A variable can have a bytes type, such as
  `type = { bytes = { encoding = "base64", length = 32 } }`, which declares
  the encoding of its text (`hex`, `base64` or `base64url`) and its length
  after decoding, exact with `length` or smallest with `min-length`, from 1
  to 1024 bytes. `hex` takes either case, `base64` needs its padding,
  `base64url` takes all of its padding or none, and no encoding takes
  whitespace. `starmap run` refuses a value that does not decode or has the
  wrong size, naming the variable, the encoding and the length and never the
  value or its size, and this covers a withheld value too. `starmap explain`
  marks such a value invalid and never prints a withheld one; `starmap check`
  reads only the spec and the source, so it does not see values. A withheld
  variable may have a bytes type, and a shown one's default must decode and
  fit. A length outside 1 to 1024 fails with `starmap::spec::bad_bytes`.
  `starmap explain` prints
  the type as `32 bytes as base64` or `at least 16 bytes as hex`, and
  `starmap diff` names it `bytes` and reports a changed encoding or length as
  a change of `type`. The digest of an existing file does not change.
- A generated variable may have a bytes type, such as
  `type = { bytes = { encoding = "hex", length = 32 } }`. Its first lookup
  creates as many random bytes as the type's `length`, or its `min-length`,
  and writes them in the declared encoding: lower case `hex`, padded
  `base64` or unpadded `base64url`. The type gives the length, so a `bytes`
  field beside it fails with `starmap::spec::bad_bytes`, and so does a
  length or `min-length` under 16. A generated `string` variable keeps its
  `bytes` field, its default of 32 and its unpadded base64url. A value the
  Keychain already holds is returned unchanged, and `check` refuses one in
  another encoding or of another length, naming the expected encoding and
  length and never the value. `starmap explain` prints a generated
  variable's bind as `32 random bytes as hex` or
  `32 random bytes as base64url`. The canonical form writes no `bytes`
  field for a generated variable with a bytes type. The digest of an
  existing file does not change. `Encoding::encode` is public and returns
  the text in a `Zeroizing<String>`.
- `#[derive(Env)]` reads a shown bytes variable into `Vec<u8>` and a
  withheld one into `starmap::SecretBytes`, which holds the decoded bytes,
  prints only dots in `Debug` and zeroes them on drop.
  `Environment::secret_bytes` returns them. starmap-test matches the new
  `Shape::Bytes` against a bytes type.
- `Type::Bytes`, `starmap::ByteRule`, `starmap::ByteLength`,
  `starmap::Encoding`, `Value::Bytes`, `TypeError::NotEncoded` and
  `TypeError::WrongLength`. `Type::check_secret` checks a withheld bytes
  value too.

### Changed

- **Breaking:** `starmap check` and `starmap init` count clap's `env`
  argument as a read. `#[arg(env = "SHELF_PORT")]` and
  `#[clap(env = "SHELF_PORT")]` read `SHELF_PORT`, and a bare `env`, as in
  `#[arg(long, env)]`, reads the upper-case form of its field's name, as
  clap does. An application that lets clap read a variable that its file
  does not declare now fails `starmap check` with
  `starmap::variable::undeclared`, so its CI fails until the file declares
  the variable. `starmap init` writes a guess for each such variable. An
  `env` whose name is not a string literal, such as `env = NAME` with a
  constant, is still not counted. A bare `env` whose name clap changes
  with `id`, `rename_all_env` or its own word splitting is counted under
  the upper-case form of the field's name, so check reports that name as
  undeclared and the real name as unread. Write the name as
  `env = "NAME"` to have check read the real one.
- A refusal of a shown variable's value quotes the value after its reason,
  as in `SHELF_PORT: the value is not a port from 1 to 65535 (the value is
  "80a")`. This covers the text of `starmap::Refusals`, of
  `ReadError::Refused` from `Env::load` and `Env::from_vars`, and of
  `starmap run`'s failures. The quote holds the first 64 characters of the
  value, counted as Unicode scalar values, then `…` when the value is
  longer, and is written as a JSON string. A withheld variable's value, and
  any value held as a secret, is never quoted. `Finding` gains a `value`
  field that holds the new `starmap::Quote`, and `starmap run`'s JSON
  failure for an invalid value gains a `value` field, which is null for a
  withheld variable. The reason's wording is unchanged, and the digest of an
  existing file does not change.

- **Breaking:** `Source::Generated` and `starmap_resolve::Lookup::Generated`
  gain an `encoding` field, the `starmap::Encoding` a new value is written
  in. The parser sets it to the bytes type's encoding, or to
  `Encoding::Base64Url` for a variable with the type string, and `resolve`
  passes it to the resolver. starmap-resolve no longer depends on `base64`.
- **Breaking:** `starmap::SecretBytes`, the number of random bytes in a
  generated value, is renamed `starmap::ByteCount`, and the name
  `SecretBytes` now holds the decoded bytes of a withheld bytes value.
  `ByteCount::new` accepts 1 to 1024 bytes, and `ByteCount::generated`
  accepts the 16 to 1024 bytes of a generated value, as `SecretBytes::new`
  did.
- **Breaking:** a value that is not valid Unicode is refused with
  `the value is not unicode text` by every type except `path`, which holds
  it as it is. A `string` without a pattern held such a value before, and
  now refuses it, so `starmap run` refuses it and `starmap explain` marks
  it invalid. A variable that names a file that may not be Unicode should
  have the type `path`. `Value::String` holds a `String` in place of an
  `OsString`. The glossary states the rule in its `Unicode` entry. The
  digest of an existing file does not change.

### Removed

- **Breaking:** `FieldProblem::NotUnicode` is gone. A `String` field never
  meets a value that is not Unicode, because the check refuses that value
  with the other findings of a derived struct's read.

## [0.2.0](https://github.com/crumbkit/starmap/compare/v0.1.0...v0.2.0) - 2026-11-11
### Added

- `starmap skills install` writes starmap's agent skill into each agent skills
  directory that exists, `~/.claude/skills`, `~/.agents/skills` and
  `$CODEX_HOME/skills`, or into the directory that `--dir` names.
  `starmap skills check` exits with 1 when an installed copy is missing or
  differs from the installed starmap's, and `starmap skills print` prints the
  skill. The skill is crates/starmap-cli/skill/SKILL.md, embedded in the
  binary.
- `starmap completion install` writes starmap's shell completion where the
  shell loads it, for bash, zsh and fish, with `--shell` naming the shell when
  `SHELL` does not. `starmap completion check` exits with 1 when the installed
  file is missing or differs, and `starmap completion print` prints the
  registration script. starmap also answers a shell's completion request,
  which a shell makes by running starmap with `COMPLETE` set to its name, and
  writes no usage line for it. starmap now builds on crumbkit 0.3.1, so a
  failure's JSON recovery can have the kind `terminal`, which names a command
  that a person must run in a terminal.
- A starmap-resolve `Provider` can check a provided bind without resolving
  a value, with `Provider::check`, whose default accepts every bind, so an
  existing provider needs no change. `starmap_resolve::check_binds` runs
  the first registered provider of each bind's kind over a spec and returns
  a `BindFailure` for each refused bind, in the spec's order. A
  `BindFailure` names the variable and the provider's kind and holds the
  provider's message. The digest of an existing file does not change.
- A url variable can list the schemes it allows, with
  `type = { url = { schemes = ["postgres", "postgresql"] } }`. Each scheme
  is a lower case ASCII letter followed by lower case letters, digits, `+`,
  `-` and `.`, and starmap refuses another with `starmap::spec::bad_value`.
  An empty list or a repeated scheme fails with
  `starmap::spec::empty_enumeration`, and a default with another scheme
  fails with `starmap::spec::bad_default`. `starmap run` refuses a value
  whose scheme is not listed, naming the allowed schemes and never the
  value, and `starmap explain` marks that value invalid. A value's scheme
  matches in any case, so `HTTPS://` matches `https`. `starmap explain`
  prints the type as `url (postgres, postgresql)`, and `starmap diff`
  reports a changed list as a change of `type`. The digest of an existing
  file does not change.
- `starmap::Scheme`, `starmap::SchemeError`, `starmap::Schemes`,
  `starmap::SchemesError` and `TypeError::SchemeNotAllowed`.
- A string variable can require a pattern that its whole value matches,
  with `type = { string = { pattern = "[0-9a-f]{40}" } }`. The pattern uses
  the syntax of the Rust `regex` crate and holds at most 1024 bytes.
  `starmap run` refuses a value that does not match, naming the variable
  and the pattern and never the value, and this covers a withheld value
  too. `starmap explain` marks such a value invalid and never prints a
  withheld one. A shown variable's default must match. starmap refuses a
  pattern that is too long or does not compile with
  `starmap::spec::bad_pattern`. `starmap explain` prints the type as
  `string matching <pattern>`, and `starmap diff` reports a changed pattern
  as a change of `type`. The digest of an existing file does not change.
- `starmap::Pattern`, `starmap::PatternError`, `TypeError::NoMatch`,
  `SpecProblemKind::BadPattern`, and `Type::check_secret`, which checks a
  withheld value against its type's pattern and returns only the refusal.
- A `starmap.toml` may hold `[tool.<name>]` tables at its top level and in
  each variable, such as `[tool.codegen]` or
  `[variables.SHELF_PORT.tool.codegen]`, where a tool keeps settings of its
  own. A pointer may hold top-level tool tables too. starmap checks only
  that each one is a table and refuses any other value with
  `starmap::spec::bad_value`. `starmap::ToolTable` holds one table as TOML
  text, which `Spec::tool`, `Variable::tool` and `Pointer::tool` return.
  The canonical form and the digest leave tool tables out, so the digest
  of an existing file does not change.
- A `starmap.toml` can point to the file that declares the variables, with
  `spec = "config/environment.toml"` and the optional `src = ["src"]` and
  `dotenv = ".env"`, each relative to the pointer file. `starmap init`,
  `check`, `explain`, `diff` and `run` follow the pointer as if `--spec`
  named that file, scan the listed source directories and read the named
  `.env` file. `init` writes a missing declaration file that a pointer
  names. `#[derive(Env)]` embeds the file that the pointer names. starmap
  refuses a file that both points and declares
  (`starmap::spec::pointer_declares`) and a pointer to another pointer
  (`starmap::spec::pointer_chain`). The digest of a file that declares
  variables does not change.
- `starmap::Pointer::parse` reads a pointer, and `Spec::parse` refuses one
  with `starmap::spec::pointer_chain`.
- **Breaking:** the `Env` trait has the associated constant `SPEC_FILE`,
  the path of the embedded file relative to the crate root, which
  `#[derive(Env)]` sets. A hand-written `Env` impl must add it. starmap-test
  names that file in its messages where it named `starmap.toml`.
- A variable may state what the application does when it is unset, in
  one sentence in the optional `effect` field, such as
  `effect = "Calls are not recorded."`. The parser refuses an empty effect
  with `starmap::spec::bad_value` and `?` with `starmap::spec::guess`, as it
  does for `purpose`. `Variable::effect` returns it. `starmap explain`
  prints it in the long column `effect` and in an optional JSON field of
  the same name, and `starmap diff` reports a changed effect. The effect
  counts toward the digest, and the digest of an existing file does not
  change.

### Fixed

- `starmap diff` no longer accepts a git run that exits before its output
  limit is checked and leaves its output cut short. It reports that git
  cannot read the revision, as it does when it stops git for printing too
  much.

### Changed

- **Breaking:** `Type::Url` has the field `schemes: Option<Schemes>`, as
  `Type::Path` has `absolute`. Code that matches it writes
  `Type::Url { .. }`, and code that builds it writes
  `Type::Url { schemes: None }`.
- **Breaking:** `Type::String` has the field `pattern: Option<Pattern>`, as
  `Type::Path` has `absolute`. Code that matches it writes
  `Type::String { .. }`, and code that builds it writes
  `Type::String { pattern: None }`.
- **Breaking:** an empty value, such as `FOO=`, counts as unset wherever it
  comes from, so the variable takes its default, stays unset when it is
  optional, and is missing when it is required. Before, the type decided:
  a required string accepted `FOO=`, and a port, URL, boolean, integer or
  enumeration refused it as invalid. An empty value in the shell, `.env` or
  `--set` no longer hides a value from another of them, and a setting with
  a path bind expands the bind when the shell holds `FOO=`.
  `starmap::capture` binds nothing for an empty value, and `starmap::check`
  treats an empty bound value as unset. `starmap explain` shows the default
  or `unset` for `FOO=`. The digest of a `starmap.toml` does not change.
- A `starmap` failure that offers two different repairs prints each one on
  its own `help:` line, and its JSON lists the second one under
  `alternatives`. This covers `--set` of an undeclared or secret variable, a
  missing provider or place, an unbound secret, an unreadable or secret
  `.env`, an undeclared variable that src reads, a spec file that cannot be
  read or written, and a source file that does not build. A value that does
  not fit its type now recovers with the `starmap explain NAME` command,
  which `--json` gives as a call. `starmap diff` stops a git run that prints
  more output than it expects or takes longer than 30 seconds, and reports
  that git cannot read the revision.
- `starmap-cli` uses crumbkit 0.2. `starmap init --json` and `starmap init
  --dry-run --json` now print a `next` list of next steps. `starmap commands
  list --json --full` gives the root row a `goals` field. The JSON Schemas
  that `starmap schemas show` prints for the `init` and `diff` documents
  describe an optional `next` list, the schema of `commands list` gains
  `goals` and `prints_no_result`, and the schemas of the other documents no
  longer say that a failure's code is in the tool's spelling.
- **Breaking:** the public type that describes a variable's value is named
  `Type`, because the `type` field in `starmap.toml` and the glossary call it
  a type. `starmap::Form` is now `starmap::Type`, `starmap::FormError` is now
  `starmap::TypeError`, and `Variable::form()` is now `Variable::value_type()`.
  `SpecProblemKind::SecretForm` is now `SpecProblemKind::WithheldType`, and
  the failure code `starmap::spec::secret_form` is now
  `starmap::spec::withheld_type`. The `type` field in `starmap.toml` and the
  rest of the output of the `starmap` command are unchanged. Code that
  imports these names must rename its imports and calls.
- **Breaking:** `AgentClient::put` takes a slice of
  `(&OnePasswordRef, &Secret)` and sends all of them in one Put, and it
  returns a `Vec<PutOutcome>` with the outcome of each value, in order.
- **Breaking:** a spec that holds `unset = { default = ... }` on a withheld
  variable now fails to parse with `starmap::spec::withheld_default`.
  Before, `Spec::to_toml` wrote the default in plain text while `explain` and
  `diff` printed dots. Remove the default. A `secret` variable with
  `unset = "required"` and no bind is asked for by `starmap run` at a
  terminal, and a secret with a 1Password bind is read from it. A `generated`
  variable needs no default, because starmap creates the value on first use.
  A withheld `setting` takes its value from the shell.

### Removed

- **Breaking:** `AgentError::Exists` and the code `starmap::agent::exists`
  are gone. A Put reports a value the agent already holds as
  `PutOutcome::AlreadyHeld`.

## [0.1.0] - 2026-11-05

### Added

- `starmap::Name` checks the name of an environment variable: upper case
  letters, digits and underscores, not starting with a digit, and at most
  128 bytes.
- `starmap::Form::parse` reads a raw value as a path, port, URL, IP socket
  address, boolean, integer in a range, enumeration choice or string, and
  returns `Value` or `FormError`. A path form with `absolute = true`
  treats a relative value as unset.
- `starmap::Port`, `IntegerRange`, `Choice` and `Choices` check the values
  of their forms, each through one constructor.
- `starmap::ModeRule`, which only `Spec::parse` builds, decides between
  local and production `Mode` from the mode variable's value.
- `starmap::Spec::parse` reads a `starmap.toml` of at most 256 KiB into a
  `Spec` of `Variable`s and an optional `ModeRule`. A file that breaks a
  rule returns a `SpecError` that lists every `SpecProblem`, each with its
  `SpecProblemKind` and the byte range of text it is about.
  `SpecProblemKind::code` returns the kind's full failure code, such as
  `starmap::spec::bad_default`.
- `starmap::Source` merges a variable's source with the bind that source
  allows: a `PathTemplate` for a setting, a `SecretRef` for a secret,
  `SecretBytes` for a generated value and a `ProvidedRef` for a provided
  one.
- `Spec::to_toml` writes a spec's canonical form, and `Spec::digest` returns
  its SHA-256 `Digest`, which prints as `sha256:<hex>`.
- `starmap::Secret` holds a withheld `String` in a `secrecy` box that
  zeroes its memory on drop. Its `Debug` prints `Secret(••••••••)`, and it
  has no `Display`, `Serialize` or `Clone`. `Secret::expose` returns the
  text, and `Secret::duplicate` is the one way to copy a secret.
- `starmap::capture` binds the variables a spec declares from an
  environment that the caller passes in, as `Bindings` of `Bound` values,
  and `starmap::mode` decides the mode from them.
- `starmap::check` checks bindings against a spec in one mode, after
  defaults apply, and returns `Verdict::Ready` with an `Environment`, or
  `Verdict::Refused` with `Refusals` that name each missing or invalid
  variable, each value bound with the wrong kind for its variable's show,
  and a mode variable whose bindings decide another mode.
- `Environment::apply` sets each checked value on a `Command` and removes
  each declared variable that stays unset. `Environment::value` returns a
  shown value, and only `Environment::secret` returns a withheld one.
- `#[derive(starmap::Env)]`, behind starmap's `derive` feature, embeds an
  application's `starmap.toml` and implements `starmap::Env` for a struct
  whose fields each read one variable. `Env::load` reads the process
  environment, and `Env::from_vars` reads a fixed set of variables. Both
  return `ReadError` when the spec does not parse, a variable is refused,
  or a field cannot hold its checked value.
- `#[derive(starmap::Enumeration)]` reads an enumeration variable into an
  enum of unit variants, each named in kebab case or renamed with
  `#[starmap("...")]`. A run of capitals is one word, so `HTTPServer`
  reads as `http-server`. Two variants with the same value fail to
  compile, and so does a variant with two `#[starmap("...")]` attributes.
- A field's type is `PathBuf`, `Port`, `url::Url`, `SocketAddr`, `bool`, an
  integer type of at most 64 bits, `String`, `OsString`, `Secret` or an
  `Enumeration`, each inside an `Option` or not. Any other type fails to
  compile with a message that lists them. `starmap::Field`, which the
  derive builds, describes each field with its `Shape`, `Presence` and
  `Show`, and a `FieldError` says which field cannot hold its value and
  why, as a `FieldProblem`.
- `starmap_test::check::<E>()` and `check_allowing::<E>(allow)` compare a
  derived struct with the `starmap.toml` it embeds and panic with every
  disagreement at once: names the struct reads that the spec does not
  declare and the reverse, field types that cannot hold the variable's
  type or range, fields whose `Option` does not match the unset rule in
  either mode, fields whose show differs, and `Allow` entries that excuse
  nothing. `Allow::new(name, reason)` builds an entry.
- A variable with `type = "socket-address"` takes an IP socket address,
  such as `0.0.0.0:8080` or `[::1]:8080`, and a field of type
  `std::net::SocketAddr` reads it. Port 0 and a numeric IPv6 zone id, such
  as `[fe80::1%1]:80`, are accepted. A host name such as `localhost:8080`
  and a named zone id such as `[fe80::1%eth0]:80` are refused with
  `FormError::NotSocketAddress`.
- An empty value of an enumeration variable is refused with
  `FormError::EmptyChoice`, whose text says the value is empty and lists
  the choices. Any other value outside the choices is still
  `FormError::NotChoice`.
- `Form`, `Unset` and `Value` implement `Display` for a program's `--help`
  text: a form writes what its values are, such as `an ip socket address`
  or `one of dev, prod`, and an unset rule writes `required`, `optional`
  or `default`. It never writes the default's value, because the variable
  may be withheld, so a caller writes a shown variable's default `Value`
  itself.
- `Refusals` implements `Display`, which writes each finding with its
  variable's name. A missing variable is `required in local mode` only
  when the spec has a `[mode]` table, and `required` otherwise.
  `ReadError::Refused` writes the same text with no prefix, so a program
  can put its own words in front, such as `shelf cannot start:`.
  `ReadError::Field` names the variable whose field cannot hold its value.
- `Enumeration::value` returns a variant's value, and
  `#[derive(Enumeration)]` implements it.
- `starmap_resolve::resolve` resolves each variable of a spec in one mode
  from a `Context` of the caller's values, the `Places` of path templates,
  a `Provider` for each provided kind and a list of `Resolver`s. A caller
  value wins for a setting, an inherited variable or a secret without a
  bind, and for a provided one when no provider of its kind is registered.
  A caller value for a secret with a bind or a generated variable is
  ignored and listed in `Resolution::ignored`. Every provider and resolver
  is asked under one deadline, and the `Resolution` lists a
  `ResolveFailure` for each variable it could not resolve. A lookup that no
  registered resolver handles, because none is registered or each one
  returns `SecretError::Unsupported`, is `ResolveProblem::NoResolver`, and
  a template that needs a place the host did not give is
  `ResolveProblem::NoPlace`.
  `Resolution::new` and `ResolveFailure::new` build them for a host's own
  tests.
- `starmap_resolve::expand` fills a path template's placeholders from
  `Places`, and returns `starmap::NoPlace`, an error that names the first
  placeholder that has no place.
- starmap-resolve's `one-password` feature adds `OnePassword`, a `Resolver`
  that reads each `op://` reference with `op read` under the resolution's
  deadline. A signed-out account returns `SecretError::Locked`, a
  missing vault, item or field returns `SecretError::NotFound`, and a
  dismissed prompt returns `SecretError::Denied`.
- `Resolver::prepare` readies a resolution's secrets before `resolve` asks
  for them one at a time, and `Resolver::finish` drops what was readied and
  not taken when the resolution ends. `starmap_resolve::resolve` calls both
  on each resolver. `OnePassword::prepare` reads every reference whose parts
  hold only ASCII letters, digits, `_`, `.` and `-` with one `op inject`,
  so one 1Password prompt covers them, and leaves each other reference to
  its own `op read`. A locked store, a dismissed prompt or the deadline
  ends every read still to come, so the person sees no further prompt.
  What `prepare` readies belongs to the calling thread.
- A one-password bind takes `cache = false`, which keeps the value out of
  any cache of resolved values. `cache = true` is the same as no `cache`.
  `cache` on a path bind is an unknown field, and `cache` on a provided
  bind is reserved and refused, so it is never read as a qualifier. A
  secret's bind is a `starmap::SecretBind`, which holds its `SecretRef` and
  a `starmap::Cache` of `Allowed` or `Never`. The canonical form writes
  `cache = false`, so it changes the digest. `starmap explain` keeps the
  bind as the plain reference and shows the policy in a `cache` column,
  which `--json` writes as `"cache": "never"` and omits when a cache may
  keep the value. `starmap diff` reports a change of policy as its own
  `cache` change, from `allowed` to `never` or back.
- `Lookup::Reference` holds the `reference` and a `cacheable` mark. The
  variant is non-exhaustive, so code outside starmap-resolve builds a
  lookup with `Lookup::reference` and matches it with `..`.
  `starmap_resolve::resolve` sets the mark only in local mode and only when
  the bind allows a cache, so no lookup is cacheable in production mode.
  Only `resolve` can build a `Cacheable`. `Lookup::reference` builds a
  lookup without the mark, as a host builds one by hand. `agent::Agent`,
  described below, is the resolver that reads the mark.
- starmap-resolve's `agent` feature, on macOS and Linux, adds
  `agent::AgentSocket::for_user`, which returns the path of the per-user
  agent's socket: `starmap-agent/agent.sock` in the directory that
  `/usr/bin/getconf DARWIN_USER_TEMP_DIR` prints on macOS, and in
  `/run/user/<uid>` on Linux. `TMPDIR` and `XDG_RUNTIME_DIR` play no part,
  so a sandbox can deny exactly that path. It creates `starmap-agent/` with
  mode 0700, and refuses one that is not the user's own with that mode
  (`starmap::agent::unsafe_directory`) and a path that does not fit in
  `sun_path` (`starmap::agent::path_too_long`). The agent is off
  (`starmap::agent::off`) when the parent is shared, sticky or writable by
  others, and on Linux when the kernel's Landlock cannot deny a sandboxed
  app the socket. `agent::AgentError` carries each code.
- `agent::Agent` is a resolver that reads 1Password values from the
  per-user agent when one is running. It asks the agent with one Get for
  each lookup marked `Cacheable`, reads every other lookup and every miss
  with one batched `op` call of its own, and puts each miss it read back
  with the generation of the Get, an opaque token the agent draws again on
  every clear and restart, so a value read before either is refused. Every
  failure of the agent, including no agent at the socket
  (`starmap::agent::not_running`), leaves resolution to `op`, and
  `Agent::warnings` returns the latest resolution's failures.
- `agent::CacheScope` names the 1Password account and the absolute `op`
  path that each cache entry belongs to. The agent is off unless
  `OP_ACCOUNT` names an account, and when `OP_SERVICE_ACCOUNT_TOKEN` or any
  `OP_SESSION_` variable is set.
- `agent::AgentClient` sends one Get or Put per connection in the agent's
  wire protocol, version 1, by the resolution's deadline: it connects
  without blocking and cuts every wait to the time left. It checks that
  the process behind the socket runs as the current user, and its parser
  refuses a count over 256 before it allocates for it, a length past the
  end of the frame, bytes after the last field, a Hits whose count differs
  from its Get, an unknown outcome byte, text or a value that is not
  UTF-8, and an unknown version.
- `starmap::url` re-exports the `url` crate, which is part of starmap's
  public API, so a host names the same version.
- `Variable::capture` binds one raw value by the variable's show, as
  `starmap::capture` binds each value. `PathTemplate::fill` returns the path
  a template names when a function gives each placeholder's directory, or
  `NoPlace` for the first placeholder without one.
- starmap-resolve's `keychain` feature adds `Keychain`, a `Resolver` that
  keeps each generated value as a generic password in the macOS login
  keychain, under the service `starmap.<workspace>` and the account
  `<app>/<NAME>`. The first lookup creates the value from the variable's
  random bytes, written as base64url, and later lookups return it
  unchanged. `Keychain::forget` deletes every value of one workspace and
  returns how many it deleted. On a platform other than macOS, every call
  returns `SecretError::Unsupported`. A `Lookup::Generated` or
  `Lookup::Saved` names its value with a `Slot`, the variable of an
  `AppName` in a `WorkspaceName`, which compares and hashes by its three
  names. `AppName::of_dir` gives the default app name of a directory, and
  `WorkspaceName::from(&app)` names a lone app's workspace after it.
- The `starmap` command, built on crumbkit. `starmap check` reads
  `starmap.toml`, or the file that `--spec` names, and prints the number of
  variables it declares, its digest and the mode that the shell's values
  decide. A file that cannot be read or parsed fails with a
  `starmap::spec::<kind>` code for its first problem and an excerpt of the
  file at every problem. Its JSON `problems` field lists the full code of
  every problem, such as `starmap::spec::guess`. A kind that the command
  does not know keeps its own code in `problems`, and the failure's
  top-level code is then `starmap::spec::syntax`. `starmap explain` prints
  each declared variable, or each named one, with its type, unset rule,
  source and value in this shell. A withheld value prints as eight dots
  whatever its length, and its JSON holds only its state. `commands list`
  and `self usage` come from crumbkit, and every run appends a line to the
  command's usage log.
- `starmap init` scans the `.rs` files under `src` for each variable that
  the code reads by a literal name, through `std::env::var`,
  `std::env::var_os` or the `#[starmap("NAME")]` field attribute, and
  writes `starmap.toml` with a guess for each field of each variable. Each
  guess ends in `?`, and `--dry-run` prints the guesses and writes nothing.
  init refuses to replace an existing file with `starmap::spec::exists`.
  The scan skips test code: files named `tests.rs` or ending in
  `_tests.rs`, and the item after `#[cfg(test)]`. It does not count a
  method such as `ctx.var("X")`. It misses a name held in a constant, a
  lookup in `std::env::vars_os`, a variable that a library such as clap
  reads, and a reader that matches names by prefix. It counts a function
  named `var` or `var_os` from another crate.
- `starmap check` prints one failure for a file whose problems are all
  guesses: it counts them and points at the first.
- `starmap check` also scans `src`. Its text says how many of the declared
  variables `src` reads, its JSON gains the fields `read` and
  `undeclared`, and each variable that `src` reads and the file does not
  declare fails with `starmap::variable::undeclared`. A missing spec's next
  step is now `starmap init`.
- `starmap diff REVISION` lists each variable that was added, changed or
  removed since the spec at the same path in a git revision. A revision
  without the file counts as an empty spec. diff reads only regular files,
  never fetches from a remote, and refuses an empty revision.
- `starmap run PROGRAM` resolves every declared variable, checks the
  values and starts the program with each one set or removed. A setting
  comes from `--set NAME=value`, then the shell, then `.env` beside the
  spec, then its path bind, then its default. starmap reads `.env` itself,
  with no `$` expansion, and refuses a secret or generated variable in it
  or in `--set`. A secret comes from its bind, or from the shell or the
  Keychain when it has none. At a terminal, and without `--no-input`, run
  asks for a required secret that nothing supplies, only when everything
  else passes the check, and offers to save it to the Keychain. A
  generated variable in a session without a login keychain fails with
  `starmap::secret::unreadable`, and its hint says that run keeps
  generated values only in the macOS login keychain. `--json` is refused
  before any secret store is asked. run passes on the program's exit code.
- `starmap`'s root help groups its commands under four goal headings.
- `starmap explain` and `starmap diff` name a `socket-address` variable's
  type as `ip socket address`, where they printed `another type`, and
  `starmap explain` prints its value, such as `0.0.0.0:8080`, where it
  printed `a value`.

### Removed

- `OnePassword` no longer implements `Clone`, because it holds the values
  that `prepare` readied, which cannot be copied. A host that needs two
  resolvers calls `OnePassword::new` twice.

[0.1.0]: https://github.com/crumbkit/starmap/releases/tag/v0.1.0
