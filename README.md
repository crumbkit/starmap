# starmap

starmap declares, checks and resolves the environment variables a Rust
application reads. An application keeps a `starmap.toml` beside its code
that names each variable, its form, and what happens when it is absent. The
application reads its environment once, in `main`, into a typed struct that
`#[derive(starmap::Env)]` generates from that file, and a test fails when
the struct and the file disagree. Programs that start the application, such
as a development host, resolve the declared values and apply them to the
child process.

starmap is under construction, and no crate has been published yet.

## Crates

| Crate | What it does |
| --- | --- |
| [`starmap`](crates/starmap) | Parses the declaration, checks values against it and applies a checked environment to a child process. It reads no environment variable itself. |
| [`starmap-derive`](crates/starmap-derive) | Provides `#[derive(Env)]` and `#[derive(Enumeration)]`, which `starmap` re-exports behind its `derive` feature. |
| [`starmap-test`](crates/starmap-test) | Provides the test that keeps an application's declaration and its reads matched. |
| [`starmap-resolve`](crates/starmap-resolve) | Turns bindings into values for the programs that start applications. Its `one-password` and `keychain` features are off by default. |
| [`starmap-cli`](crates/starmap-cli) | Builds the `starmap` command, with `init`, `check`, `explain`, `diff` and `run`. |

An application depends on `starmap` with its `derive` feature, and on
`starmap-test` as a dev-dependency.

## Contributing

[AGENTS.md](AGENTS.md) lists the checks that a change must pass, and
[docs/README.md](docs/README.md) leads to the decision records and the
standards that starmap follows.

## Licence

starmap is licensed under either the MIT licence or the Apache License 2.0,
at your option.
