# Working on starmap

Start at [docs/README.md](docs/README.md). The rules are the Rust standards
that [rust-standards.toml](rust-standards.toml) adopts.

A change is ready when the pull-request lane passes:

```sh
cargo fmt --check
zizmor .
cargo deny --locked check bans licenses sources
cargo shear
CARGO_BUILD_WARNINGS=deny cargo clippy --locked --workspace --all-targets --all-features
cargo nextest run --locked --workspace --all-features
cargo test --locked --doc --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps
CARGO_BUILD_WARNINGS=deny cargo hack clippy --locked --workspace --each-feature --all-targets
cargo +1.89.0 check --locked --workspace --all-targets --all-features
cargo xtask check
```

- Write the test first, named as a sentence (BASE-36).
- Give each new dependency a reason in `Cargo.toml` (BASE-23).
- Dated decision records are frozen except Status and Children (ADR-0001).

## Writing

Docs, comments and commit messages use plain declarative sentences with a
named subject. Avoid "not X but Y", fragments, metaphors and closing
flourishes. A stated limit says what it counts and what happens when it is
hit. Commit messages describe changed behaviour.
