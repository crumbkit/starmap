# starmap-cli

starmap-cli builds the `starmap` command, which checks, explains, compares
and runs an application's declared environment.

`starmap init` scans the `.rs` files under `src` for the variables that
the code reads through `std::env::var`, `std::env::var_os` and the
`#[starmap("NAME")]` field attribute, and writes `starmap.toml` with a
guess for each one. Each guessed value ends in `?`, and `starmap check`
refuses the file until every guess is confirmed. `starmap check` reads
`starmap.toml` in the current directory, or the file that `--spec` names,
checks every rule of the file, fails for each variable that `src` reads and
the file does not declare, and names the mode that the shell's values
decide. `starmap explain` prints each declared variable with its type,
unset rule, source and value in the shell, and prints a withheld value as
eight dots. `starmap diff <REVISION>` compares `starmap.toml` with the file
at the same path in a git revision, and lists each variable that was added
or removed and each field that changed. [docs/commands.md](https://github.com/crumbkit/starmap/blob/main/crates/starmap-cli/docs/commands.md) is the
reference of every command. The command `run` comes in a later release. The repository's
[README](https://github.com/crumbkit/starmap) describes the project.

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
