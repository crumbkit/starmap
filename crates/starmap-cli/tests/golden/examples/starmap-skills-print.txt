---
name: starmap
# crumbkit:generated description
description: "Write, check, explain and run the environment variables that an application declares in starmap.toml"
# /crumbkit:generated description
---

# Working with starmap

starmap keeps an application's environment variables in one file, starmap.toml,
that names each variable's type, whether it is a setting, a secret or inherited,
what happens when it is unset, and where its value comes from. The commands
below check that file against the code and run a program with the environment
it declares.

Use starmap when a repository has a starmap.toml, when code in `src` reads an
environment variable that no file declares, or when a program needs its
variables set from a spec. A task that never reads or sets an environment
variable does not need it.

The commands read `starmap.toml` in the current directory. `--spec FILE` names
another file, and a starmap.toml that holds only `spec = "path"` points to the
file that declares the variables.

## Choose the command that matches the job

| Job | Command | Behaviour |
| --- | --- | --- |
| Start a spec for code that reads variables | `starmap init` | starmap scans `.rs` files under `src` and writes starmap.toml with a guess for each variable, each ending in `?`. `starmap init --dry-run` prints the guesses and writes nothing. It refuses to replace an existing file. |
| Confirm the spec is valid and complete | `starmap check` | starmap checks every rule of the file and fails for each variable that `src` reads and the file does not declare. It refuses a guess that still ends in `?`. |
| See what each variable is and its value now | `starmap explain` | starmap prints each variable's type, unset rule, source and value in this shell. A withheld value prints as eight dots and has no text in JSON. `starmap explain NAME` explains one variable. |
| See what a branch changed in the spec | `starmap diff REVISION` | starmap lists each variable that was added, changed or removed since the spec at that git revision, such as `starmap diff main`. |
| Run a program with the declared environment | `starmap run -- COMMAND` | starmap resolves every variable, checks the values and starts the program. `--set NAME=VALUE` changes one setting for the run. The program's exit code becomes starmap's. |

The scan counts `var` and `var_os` with a literal name, `#[starmap("NAME")]`
fields and clap's `#[arg(env = "NAME")]`. A bare clap `env` reads the
upper-case field name. The scan misses a name in a constant.

Add `--json` to any command to print JSON. `starmap check --json` and
`starmap explain --json` give the documents `starmap/check/v1` and
`starmap/explain/v1`, and a failure prints `starmap/failure/v1` with a code and
a recovery.

## Where a variable's value comes from

`starmap run` takes a value from `--set`, then the shell, then the `.env` file
beside the spec, then a setting's path bind, then the default. A secret comes
from its 1Password bind, or from the shell or the macOS Keychain when it has
none. `.env` and `--set` never give a secret.

`starmap run` asks for a missing required secret only at a terminal. An agent
has no terminal, so pass `--no-input` and expect `starmap::secret::unbound`
when a secret has no value, then ask the person to supply it. Never print a
secret or write one to a file.

## Recover from common failures

- `starmap::spec::not_found` means there is no starmap.toml in the current
  directory. Run the command in the application's directory, name the file with
  `--spec`, or run `starmap init` to write one.
- `starmap::spec::guess` means a value in the spec still ends in `?`. Replace
  each guess with the real type or source, then run `starmap check` again.
- `starmap::variable::undeclared` means `src` reads a variable that starmap.toml
  does not declare. Declare it in the spec, and run `starmap check`.
- `starmap::variable::unknown` means the name given to `starmap explain` is not
  declared. Run `starmap explain` with no name to list the declared ones.
- `starmap::variable::missing` and `starmap::variable::invalid` mean `starmap
  run` found a required variable unset or a value of the wrong type. Run
  `starmap explain` to see each variable's rule, then pass the value with
  `--set NAME=VALUE`, or set it in the shell.
- `starmap::spec::exists` means `starmap init` found a starmap.toml already and
  refuses to replace it. Edit the existing file, or pass `--spec FILE` with a
  path that is free.
- `starmap::secret::unreadable` means 1Password or the Keychain could not
  return a secret: it is locked, the item is missing, `op` is missing or
  signed out, the bind is wrong, or there is no login keychain. Check the bind
  and that `op` is signed in, ask the person to unlock it or add the item,
  then run `starmap run` again.
- `starmap::revision::not_found` means the revision given to `starmap diff` is
  not in the repository. Fetch it, or pass a branch or commit that exists.

Every `starmap::spec::*` code for a rule of the file, such as `bad_value` or
`unknown_field`, names a rule that the file breaks. The
failure's message gives the file, the line and the rule, so fix that line and
run `starmap check` again.

## Complete starmap's commands in a shell

`starmap completion install` writes the shell completion for starmap's commands
where your shell loads it, with `--shell bash|zsh|fish` when `SHELL` does not
name it.

## Keep this skill current

starmap embeds this skill, so the installed copy matches the starmap that wrote
it. `starmap skills install` writes it into each agent skills directory that
exists, and `starmap skills check` exits with 1 when a copy is missing or
differs from this starmap's. `starmap skills print` prints it.

## Find everything else in the tool

`starmap commands list --all` lists every command, and `starmap COMMAND --help`
describes one. `starmap schemas list` lists the JSON documents, and
`starmap schemas show ID` prints one document's schema.
