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
or removed and each field that changed. `starmap run -- <program>` resolves
every declared variable, checks the values and starts the program with each
declared variable set or removed. A value comes from `--set`, then the
shell, then the `.env` file beside the spec, then a setting's path bind,
then the default, and `.env` and `--set` never give a secret. A secret
comes from its 1Password bind, or from the shell or the Keychain when it
has none. At a terminal, `run` asks for a required secret that nothing
supplies, with hidden input, and offers to save it to the Keychain. The program's exit code becomes `starmap`'s.
[docs/commands.md](https://github.com/crumbkit/starmap/blob/main/crates/starmap-cli/docs/commands.md) is the
reference of every command. The repository's
[README](https://github.com/crumbkit/starmap) describes the project.

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
