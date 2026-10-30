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
