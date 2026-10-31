# Changelog

This file records the notable changes to the published starmap crates. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
the crates follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `starmap::Name` checks the name of an environment variable: upper case
  letters, digits and underscores, not starting with a digit, and at most
  128 bytes.
- `starmap::Form::parse` reads a raw value as a path, port, URL, boolean,
  integer in a range, enumeration choice or string, and returns `Value` or
  `FormError`. A path form with `absolute = true` treats a relative value
  as unset.
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
- `starmap_resolve::resolve` resolves each variable of a spec in one mode
  from a `Context` of the caller's values, the `Places` of path templates,
  a `Provider` for each provided kind and a list of `Resolver`s. A caller
  value wins for a setting or inherited variable, and for a provided one
  when no provider of its kind is registered. A caller value for a secret
  or generated variable is ignored and listed in `Resolution::ignored`.
  Every provider and resolver is asked under one deadline, and the
  `Resolution` lists a `ResolveFailure` for each variable it could not
  resolve.
- `starmap_resolve::expand` fills a path template's placeholders from
  `Places`, and names the first placeholder that has no place.
