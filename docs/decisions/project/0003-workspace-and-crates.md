# ADR-0003. Workspace and crates

- Date: 2026-10-30
- Status: accepted

## Context

starmap is a library that applications depend on, a derive that generates
each application's environment read, a test that keeps the declaration and
the reads matched, a resolver for the programs that start applications, and
a command. Each part has different dependants and different dependencies.

## Decision

- The repository is a virtual workspace of five published crates under
  `crates/` and an unpublished `xtask`. `starmap` is the core,
  `starmap-derive` is its procedural macro crate, `starmap-test` holds the
  catalog test of BASE-28, `starmap-resolve` turns bindings into values,
  and `starmap-cli` builds the `starmap` binary.
- An application depends only on `starmap` with its `derive` feature, and
  on `starmap-test` as a dev-dependency. Only the CLI depends on crumbkit,
  so crumbkit's own example application can adopt starmap without a
  version cycle.
- starmap-resolve's `one-password` and `keychain` features are off by
  default (LIB-6), because most hosts use at most one of them and each
  brings a program or a platform library.
- The crates share one version, one `CHANGELOG.md` and one tag, and stay at
  0.0.0 until the first release. release-plz publishes them in dependency
  order once the owner has published 0.1.0 by hand.
- `cargo xtask check` holds the repository checks that no off-the-shelf
  tool makes, each as a pure function with its own tests.
- The lanes follow the standards' templates, and the adoption map's
  `lanes` decision records each lane.

## Consequences

A dependency that only the CLI or only a resolver feature needs stays out
of every application's build. A new repository rule is a module of xtask
with its tests, and it runs in the pull-request lane's `repository` step.

Considered: one crate with features for the derive, the test and the CLI;
a separate repository for the CLI; shell scripts in place of xtask.
