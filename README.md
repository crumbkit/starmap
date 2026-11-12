# starmap

starmap declares, checks and resolves the environment variables a Rust
application reads. An application keeps a `starmap.toml` beside its code
that names each variable, its type, and what happens when it is absent. The
application reads its environment once, in `main`, into a typed struct that
`#[derive(starmap::Env)]` generates from that file, and a test fails when
the struct and the file disagree. Programs that start the application, such
as a development host, resolve the declared values and apply them to the
child process. The [glossary](docs/glossary.md) names each concept once,
including `type`, `withheld`, `secret`, `provided` and `unset`.

starmap 0.1.0 is the first release. `cargo install starmap-cli` installs
the `starmap` command, and each
[GitHub release](https://github.com/crumbkit/starmap/releases) attaches
binaries for Apple silicon macOS, Linux and Windows.

## Crates

| Crate | What it does |
| --- | --- |
| [`starmap`](crates/starmap) | Parses the declaration, checks values against it and applies a checked environment to a child process, with or without the other variables of the caller. It reads no environment variable itself. |
| [`starmap-derive`](crates/starmap-derive) | Provides `#[derive(Env)]` and `#[derive(Enumeration)]`, which `starmap` re-exports behind its `derive` feature. |
| [`starmap-test`](crates/starmap-test) | Provides the test that keeps an application's declaration and its reads matched. |
| [`starmap-resolve`](crates/starmap-resolve) | Turns bindings into values for the programs that start applications. Its `one-password` and `keychain` features are off by default. |
| [`starmap-cli`](crates/starmap-cli) | Builds the `starmap` command. `starmap init` writes a `starmap.toml` of guesses from the variables that `src` reads, and adds the [Clippy entries](docs/glossary.md#clippy-entries) that disallow other reads of the environment. `starmap check` checks the file against its rules and against `src`, `starmap explain` describes each variable and its value in the shell, `starmap diff` lists what changed since a git revision, and `starmap run` starts a program with the declared environment. `starmap run --only-declared` leaves every undeclared variable of the shell out of that environment, except a fixed list of [built-in variables](docs/glossary.md#built-in-variable) such as `PATH` and `HOME`. |

An application depends on `starmap` with its `derive` feature, and on
`starmap-test` as a dev-dependency.

## Command-line flags

An application that takes a value from a flag or a variable often lets clap
read the variable:

```rust
#[arg(long, env = "SHELF_PORT")]
port: u16,
```

That read happens outside the struct, so the variable's declared type, its
default and its check do not apply to it. Remove `env` from the argument,
make the flag an `Option`, and use the struct's field when the flag is not
given. The flag takes the field's type, `starmap::Port` here, in place of the
`u16` above, so clap refuses the values that type refuses:

```rust
#[derive(clap::Parser)]
struct Cli {
    #[arg(long)]
    port: Option<starmap::Port>,
}
```

In the next block, `ShelfEnv` is the struct that derives `Env` and declares
`SHELF_PORT` as a `Port`, and `cli` is the parsed `Cli`:

```rust
let env = ShelfEnv::load()?;
let port = cli.port.unwrap_or(env.port);
```

starmap adds no function for this. The glossary's
[flag override](docs/glossary.md#flag-override) entry gives the `Option`
method for each kind of variable and the limits of the pattern.

## Several commands that share one file

An application whose commands read different variables gives each command
its own struct over the same `starmap.toml`. A struct's read enforces only
the variables its fields read and the mode variable, so a variable that
only the `serve` command requires does not stop the `api` command's struct
from reading. One test checks that the structs read every declared
variable between them:

```rust
#[test]
fn the_structs_together_read_every_declared_variable() {
    use starmap_test::Reader;
    starmap_test::check_together(&[Reader::of::<Serve>(), Reader::of::<Api>()], &[]);
}
```

`starmap run` still checks every variable, because it does not know which
command the child runs. The glossary's
[scoped read](docs/glossary.md#scoped-read) entry states the rule.

## Checking a value where the application uses it

A field of type `starmap::Deferred<T>`, or `starmap::Deferred<Option<T>>`
for an optional variable, defers its variable's rules to the point of use.
The read never refuses because of that variable, and the field holds the
value or the finding the read would have reported. The application takes
the value when it needs it, in its own order, and builds its own error from
the finding:

```rust
#[derive(starmap::Env)]
struct Serve {
    #[starmap("SHELF_CATALOGUE_URL")]
    catalogue: starmap::Deferred<url::Url>,
    #[starmap("SHELF_TOKEN")]
    token: starmap::Deferred<starmap::Secret>,
    #[starmap("SHELF_PORT")]
    port: starmap::Port,
}

fn start() -> Result<(), ShelfError> {
    let env = Serve::load().map_err(ShelfError::Environment)?; // refuses only SHELF_PORT
    let catalogue = env.catalogue.get()
        .map_err(|finding| ShelfError::new("shelf::catalogue::url_invalid", finding))?;
    let token = env.token.get()
        .map_err(|finding| ShelfError::new("shelf::auth::token_missing", finding))?;
    run(catalogue, token, env.port)
}
```

The finding names the variable and holds the problem, and its text is the
text a refused read writes, such as `SHELF_TOKEN is required and has no
value`. An invalid value of the mode variable still refuses the read. The
glossary's [deferred](docs/glossary.md#deferred) entry states the rule.

## Contributing

[AGENTS.md](AGENTS.md) lists the checks that a change must pass, and
[docs/README.md](docs/README.md) leads to the decision records and the
standards that starmap follows.

## Licence

starmap is licensed under either the MIT licence or the Apache License 2.0,
at your option.
