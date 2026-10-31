# starmap-cli

starmap-cli builds the `starmap` command, which checks, explains, compares
and runs an application's declared environment.

`starmap check` reads `starmap.toml` in the current directory, or the file
that `--spec` names, checks every rule of the file and names the mode that
the shell's values decide. `starmap explain` prints each declared variable
with its type, unset rule, source and value in the shell, and prints a
withheld value as eight dots. [docs/commands.md](https://github.com/crumbkit/starmap/blob/main/crates/starmap-cli/docs/commands.md) is the
reference of every command. The commands `init`, `diff` and `run` come in
later releases. The repository's
[README](https://github.com/crumbkit/starmap) describes the project.

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
