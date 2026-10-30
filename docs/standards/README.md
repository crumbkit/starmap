# Standards

starmap follows the base, library, cli and child-process profiles of
[Luke Randall's Rust standards](https://github.com/crumbkit/rust-standards).
The rules and their checks live in that repository, and code and docs cite
them by ID, such as BASE-6 or LIB-1.

[rust-standards.toml](../../rust-standards.toml) is starmap's adoption map.
It names the profiles and the commit of the standards it was reviewed
against, and it holds the project's bindings, its decisions, such as the
lanes and the release profile, and any exception to a rule.

The pull-request lane in [ci.yml](../../.github/workflows/ci.yml) runs the
checks that a tool can make, and `cargo xtask check` runs the drift checks
that a script can make, such as the file length of BASE-21.
