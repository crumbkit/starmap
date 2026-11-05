# Changelog

This file records the notable changes to the published starmap crates. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
the crates follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
- `starmap::ModeRule` decides between local and production `Mode` from the
  mode variable's value.
- `starmap::Spec::parse` reads a `starmap.toml` of at most 256 KiB into a
  `Spec` of `Variable`s and an optional `ModeRule`. A file that breaks a
  rule returns a `SpecError` that lists every `SpecProblem`, each with its
  `SpecProblemKind` and the `Span` of text it is about.
- `starmap::Source` merges a variable's source with the bind that source
  allows: a `PathTemplate` for a setting, a `SecretRef` for a secret,
  `SecretBytes` for a generated value and a `ProvidedRef` for a provided
  one.
- `Spec::to_toml` writes a spec's canonical form, and `Spec::digest` returns
  its SHA-256 `Digest`, which prints as `sha256:<hex>`.
- `starmap::Secret<T>` holds a withheld value in a `secrecy` box. Its
  `Debug` prints `Secret(••••••••)`, and it has no `Display`, `Serialize`
  or `Clone`.
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
  reads as `http-server`, and two variants with the same value fail to
  compile.
- A field's type is `PathBuf`, `Port`, `url::Url`, `SocketAddr`, `bool`, an
  integer type of at most 64 bits, `String`, `OsString`, `Secret<String>`
  or an `Enumeration`, each inside an `Option` or not. Any other type
  fails to compile with a message that lists them. `starmap::Field`
  describes each field with its `Shape`, `Presence` and `Show`, and a
  `FieldError` says which field cannot hold its value and why, as a
  `FieldProblem`.
- `starmap_test::check` and `check_allowing` compare a derived struct with
  its `starmap.toml` and panic with every disagreement at once: names the
  struct reads that the spec does not declare and the reverse, field types
  that cannot hold the variable's type or range, fields whose `Option`
  does not match the unset rule in either mode, fields whose show differs,
  and `Allow` entries that excuse nothing. Both panic when the spec text
  differs from the text the struct embeds.
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
  `ResolveFailure` for each variable it could not resolve.
  `Resolution::new` and `ResolveFailure::new` build them for a host's own
  tests.
- `starmap_resolve::expand` fills a path template's placeholders from
  `Places`, and names the first placeholder that has no place.
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
  lookup without the mark, as a host builds one by hand.
- starmap-resolve's `agent` feature, on macOS and Linux, reads 1Password
  values from a per-user agent when one is running. `agent::Agent` is a
  resolver that asks the agent with one Get for each lookup marked
  `Cacheable`, reads every other lookup and every miss with one batched
  `op` call of its own, and puts each miss it read back with the
  generation of the Get, so a value read before a clear is refused. When
  no agent answers it runs `starmap agent start` through
  `agent::AgentStarter`, in an environment cleared of all but `HOME`,
  `USER`, `LOGNAME`, `LANG`, `XDG_STATE_HOME` and `STARMAP_AGENT_LAPSE`,
  and asks once more. Every failure of the agent leaves resolution to
  `op`, and `Agent::warnings` returns it as an `agent::AgentError` with a
  code such as `starmap::agent::protocol`.
- `agent::AgentSocket::for_user` returns the socket's path:
  `starmap-agent/agent.sock` in the directory of
  `confstr(_CS_DARWIN_USER_TEMP_DIR)` on macOS and in `/run/user/<uid>` on
  Linux. `TMPDIR` and `XDG_RUNTIME_DIR` play no part. It refuses a parent
  that is shared, sticky or writable by others, and a `starmap-agent/`
  directory that is not the user's own with mode 0700. On Linux the agent
  is off unless the kernel's Landlock can deny a sandboxed app the socket.
- `agent::CacheScope` names the 1Password account and the absolute `op`
  path that each cache entry belongs to, and turns the agent off when
  `OP_SERVICE_ACCOUNT_TOKEN` or any `OP_SESSION_` variable is set.
- `agent::AgentClient` sends one Get, Put, Clear or Status per connection
  in the agent's wire protocol, version 1, after it checks that the
  process behind the socket runs as the current user. Its parser refuses a
  count over 256 before it allocates for it, a length past the end of the
  frame, bytes after the last field, a Hits whose count differs from its
  Get, an unknown outcome byte, text or a value that is not UTF-8, and an
  unknown version.
- `Variable::capture` binds one raw value by the variable's show, as
  `starmap::capture` binds each value. `PathTemplate::fill` returns the path
  a template names when a function gives each placeholder's directory.
- starmap-resolve's `keychain` feature adds `Keychain`, a `Resolver` that
  keeps each generated value as a generic password in the macOS login
  keychain, under the service `starmap.<workspace>` and the account
  `<app>/<NAME>`. The first lookup creates the value from the variable's
  random bytes, written as base64url, and later lookups return it
  unchanged. `Keychain::forget` deletes every value of one workspace and
  returns how many it deleted. On a platform other than macOS, every call
  returns `SecretError::Unsupported`.
- The `starmap` command, built on crumbkit. `starmap check` reads
  `starmap.toml`, or the file that `--spec` names, and prints the number of
  variables it declares, its digest and the mode that the shell's values
  decide. A file that cannot be read or parsed fails with a
  `starmap::spec::<kind>` code for its first problem and an excerpt of the
  file at every problem. `starmap explain` prints each declared variable,
  or each named one, with its type, unset rule, source and value in this
  shell. A withheld value prints as eight dots whatever its length, and its
  JSON holds only its state. `commands list` and `self usage` come from
  crumbkit, and every run appends a line to the command's usage log.
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
  else passes the check, and offers to save it to the Keychain. `--json`
  is refused before any secret store is asked. run passes on the
  program's exit code.
- `starmap`'s root help groups its commands under four goal headings.
- `starmap explain` and `starmap diff` name a `socket-address` variable's
  type as `ip socket address`, where they printed `another type`, and
  `starmap explain` prints its value, such as `0.0.0.0:8080`, where it
  printed `a value`.

### Removed

- `OnePassword` no longer implements `Clone`, because it holds the values
  that `prepare` readied, which cannot be copied. A host that needs two
  resolvers calls `OnePassword::new` twice.
