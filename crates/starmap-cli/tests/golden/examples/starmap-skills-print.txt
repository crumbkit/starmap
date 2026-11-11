---
name: starmap
# crumbkit:generated description
description: "Write, check, explain and run the environment variables that an application declares in starmap.toml"
# /crumbkit:generated description
---

# Working with starmap

One file, starmap.toml, declares an application's environment variables: each
one's type, whether it is a setting, a secret or inherited, what happens when
it is unset, and where its value comes from.

Use starmap when a repository has a starmap.toml, when code in `src` reads an
undeclared environment variable, or when a program needs its variables set
from a spec. A task that touches no environment variable does not need it.

The commands read `starmap.toml` in the current directory, or the file that
`--spec FILE` names. A starmap.toml that holds only `spec = "path"` points to
the file that declares the variables.

## Choose a command

- `starmap init` starts a spec for code that reads variables. It scans `.rs`
  files under `src` and writes starmap.toml with a guess for each variable,
  each ending in `?`. `starmap init --dry-run` prints the guesses and writes
  nothing.
- `starmap check` confirms that the spec is valid and complete. It checks
  every rule of the file, fails for each variable that `src` reads and the
  file does not declare, and refuses a guess that still ends in `?`.
- `starmap explain` prints each variable's type, unset rule, source and value
  in this shell. A withheld value prints as eight dots and has no text in
  JSON. `starmap explain NAME` explains one variable.
- `starmap diff REVISION` lists each variable added, changed or removed since
  the spec at that git revision, so `starmap diff main` shows what a branch
  changed.
- `starmap run -- COMMAND` resolves every variable, checks the values and
  starts the program with the declared environment. The program's exit code
  becomes starmap's. `--set NAME=VALUE` changes one setting for the run.

The scan counts `var` and `var_os` with a literal name, `#[starmap("NAME")]`
fields and clap's `#[arg(env = "NAME")]`, reads a bare clap `env` as the
upper-case field name, and misses a name in a constant.

`--json` makes any command print JSON: `starmap check --json` gives
`starmap/check/v1`, `starmap explain --json` gives `starmap/explain/v1`, and a
failure gives `starmap/failure/v1` with a code and a recovery.

## Where a variable's value comes from

`starmap run` takes a value from `--set`, then the shell, then the `.env` file
beside the spec, then a setting's path bind, then the default. A secret comes
from its 1Password bind, or from the shell or the macOS Keychain when it has
none. `.env` and `--set` never give a secret.

`starmap run` asks for a missing required secret only at a terminal. An agent
has no terminal, so pass `--no-input` and expect `starmap::secret::unbound`
when a secret has no value, then ask the person for it. Never print a
secret or write one to a file.

## Recover from failures

- `starmap::spec::not_found` means the current directory has no starmap.toml.
  Run the command in the application's directory, name the file with `--spec`,
  or write one with `starmap init`.
- `starmap::spec::guess` means a value in the spec still ends in `?`. Replace
  each guess with the real type or source.
- `starmap::variable::undeclared` means `src` reads a variable that starmap.toml
  does not declare. Declare it.
- `starmap::variable::unknown` means the name given to `starmap explain` is not
  declared. `starmap explain` with no name lists the declared ones.
- `starmap::variable::missing` and `starmap::variable::invalid` mean `starmap
  run` found a required variable unset or a value of the wrong type.
  `starmap explain` shows each variable's rule. Pass the value with
  `--set NAME=VALUE`, or set it in the shell.
- `starmap::spec::exists` means `starmap init` refuses to replace an existing
  starmap.toml. Edit that file, or pass `--spec FILE` with a free path.
- `starmap::secret::unreadable` means 1Password or the Keychain could not
  return a secret: it is locked, the item is missing, `op` is missing or
  signed out, the bind is wrong, or there is no login keychain. Check the bind
  and that `op` is signed in, ask the person to unlock it or add the item,
  then repeat `starmap run`.
- `starmap::revision::not_found` means the revision given to `starmap diff` is
  not in the repository. Fetch it, or pass a branch or commit that exists.

A `starmap::spec::*` code for a rule of the file, such as `bad_value` or
`unknown_field`, comes with a message that gives the file, the line and the
broken rule, so fix that line. After any fix to the spec, run `starmap check`
again.

## Install completions and this skill

`starmap completion install` writes starmap's shell completion where your
shell loads it. Add `--shell bash|zsh|fish` when `SHELL` does not name the
shell.

starmap embeds this skill, so the installed copy matches the starmap that wrote
it. `starmap skills install` writes it into each existing agent skills
directory, `starmap skills check` exits with 1 when a copy is missing or differs
from this starmap's, and `starmap skills print` prints it.

## Find everything else

`starmap commands list --all` lists every command, and `starmap COMMAND --help`
describes one. `starmap schemas list` lists the JSON documents, and
`starmap schemas show ID` prints one schema.
