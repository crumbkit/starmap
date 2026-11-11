# Commands of `starmap`

This reference is generated from the help of `starmap`. Edit the help in the tool's code, and not this file.

## `starmap`

Write, check, explain and run the environment variables that an application declares in starmap.toml

### Commands

- [`starmap init`](#starmap-init): Write starmap.toml with a guess for each variable that src reads
- [`starmap check`](#starmap-check): Check that starmap.toml follows every rule of the file, and that it declares every variable that src reads
- [`starmap explain`](#starmap-explain): Describe each declared variable and the value it has in this shell
- [`starmap diff`](#starmap-diff): List each variable that was added, changed or removed since a git revision's spec
- [`starmap run`](#starmap-run): Run a program with the environment that starmap.toml declares
- [`starmap commands`](#starmap-commands): Explore starmap's commands
- [`starmap schemas`](#starmap-schemas): Explore the JSON documents that starmap prints
- [`starmap self`](#starmap-self): Review starmap's own use
- [`starmap skills`](#starmap-skills): Give coding agents starmap's skill
- [`starmap completion`](#starmap-completion): Set up starmap's completion in a shell

### Arguments

- `--json`: Print the result as a JSON document. The default prints text. Works after every subcommand.
- `--fields <FIELD>`: Print only these comma-separated fields, in this order. The default prints the compact columns. Takes more than one value. Works after every subcommand.
- `--long`: Add the long columns after the compact ones. The default prints only the compact columns. Works after every subcommand.
- `--full`: With --json, print every field of every row. The default prints only the fields of the columns. Works after every subcommand.
- `--no-input`: Fail instead of asking a question. Works after every subcommand.

## `starmap init`

Write starmap.toml with a guess for each variable that src reads.

The scan skips test code: files named tests.rs or ending in \_tests.rs, a file that begins with #!\[cfg(test)\], and the item after #\[cfg(test)\]. check scans src the same way.

### Arguments

- `--spec <FILE>`: Write the spec to this file, and scan the src directory beside it; for a pointer, write the file it names and scan the directories it lists. Default: `starmap.toml`.
- `--dry-run`: Report what the command would change, and change nothing

### Examples

Write starmap.toml with a guess for each variable that src reads.

```sh
starmap init
```

Print the guesses and write nothing.

```sh
starmap init --dry-run
```

### Columns

- `name`: The variable's name.
- `type`: The guessed type, such as port?.
- `unset`: The guessed unset rule: required? for a setting, and optional? otherwise.
- `source`: The guessed source: setting?, secret? or inherited?.
- `read_at`: The places in src that read the variable, such as src/serve.rs:22.

### Columns that `--long` adds

- `show`: The guessed show: withheld? for a secret, and shown? otherwise.

Effects: additive, not idempotent, closed-world.

## `starmap check`

Check that starmap.toml follows every rule of the file, and that it declares every variable that src reads.

check counts a literal name passed to var or var\_os, a #\[starmap("NAME")\] field, and clap's env argument, as in #\[arg(env = "NAME")\]; a bare clap env reads the upper-case form of its field's name. It does not count a name held in a constant.

A read whose name format! builds from a name, an underscore and one trailing placeholder, such as format!("SHELF\_DB\_URL\_{region}"), reads a member of that family, and check fails when the file declares no such family. check passes on any other format! name and names the place of each one it cannot resolve.

### Arguments

- `--spec <FILE>`: Read the spec from this file. Default: `starmap.toml`.

### Examples

Check starmap.toml in the current directory.

```sh
starmap check
```

Print the result of the check as JSON.

```sh
starmap check --json
```

Effects: read-only, closed-world.

## `starmap explain`

Describe each declared variable and the value it has in this shell

### Arguments

- `<NAME>`: The variables to explain, separated by spaces; without names, every declared variable. Takes more than one value.
- `--spec <FILE>`: Read the spec from this file. Default: `starmap.toml`.

### Examples

Explain every variable and its value in this shell.

```sh
starmap explain
```

Explain one variable.

```sh
starmap explain SHELF_MIRROR_TOKEN
```

Print one variable as JSON, where a withheld value has no text.

```sh
starmap explain SHELF_MIRROR_TOKEN --json
```

### Columns

- `name`: The variable's name.
- `type`: The type of the variable's value, such as port or integer 1..=100.
- `unset`: What happens when the variable has no value, in local mode and, when it differs, in production.
- `source`: Where the variable's value comes from, with the kind of its bind.
- `value`: The variable's value in this shell, or its default; a withheld value prints as eight dots and has no text in JSON.

### Columns that `--long` adds

- `show`: Whether the value may be shown or is withheld.
- `bind`: The variable's bind, such as a 1Password reference, when it has one.
- `cache`: `never` when the bind keeps the value out of any cache of resolved values; empty, and left out of `--json`, when a cache may keep it.
- `purpose`: The sentence that says what the variable is for.
- `effect`: The sentence that says what the application does when the variable is unset; empty, and left out of `--json`, when the spec states none.
- `values`: Each value of a named enumeration with its description; empty, and left out of `--json`, for any other type.
- `check`: The sentence that states the check the application's own code makes of the value, which starmap never runs; empty, and left out of `--json`, when the spec states none.
- `group`: The name of the group of variables that are set together or not at all, which the variable belongs to; empty, and left out of `--json`, when it belongs to none.

Effects: read-only, closed-world.

## `starmap diff`

List each variable that was added, changed or removed since a git revision's spec.

diff compares the spec with the file at the same path in the revision. A revision without the file counts as an empty spec.

### Arguments

- `<REVISION>`: The git revision to compare with, such as main or HEAD\~3. Required.
- `--spec <FILE>`: Read the spec from this file. Default: `starmap.toml`.

### Examples

List each variable that changed since the spec on main.

```sh
starmap diff main
```

### Columns

- `change`: Whether the variable was added, changed or removed.
- `name`: The variable's name.
- `field`: The field that changed, such as type or unset; null for a variable that was added or removed.
- `was`: The field's value at the revision, or the type, show and source of a removed variable; null for an added variable.
- `now`: The field's value in the working tree, or the type, show and source of an added variable; null for a removed variable.

Effects: read-only, closed-world.

## `starmap run`

Run a program with the environment that starmap.toml declares.

run resolves every variable, checks the values and starts the program with each declared variable set or removed. A value comes from --set, then the shell, then .env beside the spec or the file that a pointer names, then a setting's path bind, then the default. A secret comes from its bind, or from the shell or the Keychain when it has none; at a terminal, run asks for a missing one and offers to save it to the Keychain. .env and --set never give a secret.

### Arguments

- `--spec <FILE>`: Read the spec from this file. Default: `starmap.toml`.
- `--workspace <NAME>`: Name the Keychain items of this checkout after this workspace; default: the checkout's directory name
- `--set <NAME=VALUE>`: Give a setting or inherited variable this value for this run, as NAME=VALUE; repeat it for more. Takes more than one value.
- `<PROGRAM>`: The program to run and its arguments, after --. Required. Takes more than one value.

### Examples

Run the app with the environment that starmap.toml declares.

```sh
starmap run -- cargo run
```

Run the app with one setting changed for this run.

```sh
starmap run --set SHELF_DEBUG=true -- cargo run
```

Effects: additive, not idempotent, closed-world.

### Exit codes

- `3-255`: starmap run passes on the exit code of the program it runs. The program's exit codes 0, 1 and 2 pass on as well, and are then the same numbers as starmap's own success, failure and refused command line.

## `starmap commands`

Explore starmap's commands

### Commands

- [`starmap commands list`](#starmap-commands-list): List a branch of starmap's commands with their arguments, examples and columns

## `starmap commands list`

List a branch of starmap's commands with their arguments, examples and columns

### Arguments

- `<PATH>`: The names of the commands below the root that lead to the branch, such as `books list`. Takes more than one value.
- `--limit <N>`: Print at most this many rows. Default: `25`.
- `--all`: Print every row

### Examples

List every command.

```sh
starmap commands list
```

### Columns

- `path`: The command line that runs the command, such as `shelf books list`.
- `about`: The command's one-line description, or its full description when it has no one-line description.

Effects: read-only, closed-world.

## `starmap schemas`

Explore the JSON documents that starmap prints

### Commands

- [`starmap schemas list`](#starmap-schemas-list): List the JSON documents that starmap prints, with a description of each
- [`starmap schemas show`](#starmap-schemas-show): Print the JSON Schema of each document

## `starmap schemas list`

List the JSON documents that starmap prints, with a description of each

### Arguments

- `--limit <N>`: Print at most this many rows. Default: `25`.
- `--all`: Print every row

### Examples

List every document that starmap prints.

```sh
starmap schemas list
```

### Columns

- `id`: The document's id, such as `shelf/books-list/v1`, which `schemas show` takes.
- `kind`: output when a command prints the document with its schema id, unwrapped when a command prints it without the envelope that holds its schema id, and file when it is a file format that users write and no command prints.
- `form`: stream when a command prints the document as a stream, one document on each line as each row arrives, and document when a command prints it once or it is a file format.
- `description`: What the document holds, in one line.

### Columns that `--long` adds

- `commands`: The command lines that print the document. It is empty for the failure document, which every command prints when it fails with --json, and for a file format.

Effects: read-only, closed-world.

## `starmap schemas show`

Print the JSON Schema of each document

### Arguments

- `<ID>`: The ids of the documents that `schemas list` lists, separated by spaces. Required. Takes more than one value.

### Examples

Print the JSON Schema of the document that init prints.

```sh
starmap schemas show starmap/init/v1
```

### Columns

- `id`: The document's id, such as `shelf/books-list/v1`.
- `schema`: The document's 2020-12 JSON Schema.

Effects: read-only, closed-world.

## `starmap self`

Review starmap's own use

### Commands

- [`starmap self usage`](#starmap-self-usage): Report the patterns in starmap's usage log where callers waste effort

## `starmap self usage`

Report the patterns in starmap's usage log where callers waste effort

### Arguments

- `--since <DURATION>`: How far back to read the usage log, such as 12h, 7d or 2w. Default: `7d`.
- `--log <FILE>`: Read this usage log instead of the tool's own

### Examples

Report the findings of the last 7 days.

```sh
starmap self usage
```

### Columns

- `finding`: What the pattern is.
- `runs`: The runs that show the pattern.
- `command`: The command path below the tool's name, such as `books show`, or the tool's name for its root command.
- `suggestion`: What the pattern suggests, or the figure that raised it.

Effects: read-only, closed-world.

## `starmap skills`

Give coding agents starmap's skill

### Commands

- [`starmap skills install`](#starmap-skills-install): Install starmap's skill into each agent skills directory that exists
- [`starmap skills check`](#starmap-skills-check): Check that each installed copy of starmap's skill matches this starmap
- [`starmap skills print`](#starmap-skills-print): Print starmap's skill

## `starmap skills install`

Install starmap's skill into each agent skills directory that exists

### Arguments

- `--dir <DIR>`: Install into this skills directory in place of the agent skills directories
- `--force`: Replace a file or a link that install did not write with the skill's own folder

### Examples

Install starmap's skill into each agent skills directory that exists.

```sh
starmap skills install
```

Install it, replacing a file or a link that install did not write.

```sh
starmap skills install --force
```

### Columns

- `state`: What install did: failed, skipped, written or unchanged.
- `dirs`: The skills directories that share this copy, as named. Directories that resolve to one place through a symbolic link share one copy.
- `reason`: Why install skipped or failed, or what it replaced under --force, such as `linked to /src/shelf/skill`, or null.

### Columns that `--long` adds

- `path`: The SKILL.md file that install wrote or left, at its real path.

Effects: additive, idempotent, closed-world.

## `starmap skills check`

Check that each installed copy of starmap's skill matches this starmap

### Arguments

- `--dir <DIR>`: Check this skills directory in place of the agent skills directories

### Examples

Report each agent skills directory whose copy is missing or differs.

```sh
starmap skills check
```

### Columns

- `state`: Whether the copy is the binary's: missing, differs, unreadable, linked or current.
- `dirs`: The skills directories that share this copy, as named. Directories that resolve to one place through a symbolic link share one copy.
- `reason`: Why the copy differs, cannot be read or is linked, such as `written by shelf 0.2.0` or `linked to /src/shelf/skill`, or null.

### Columns that `--long` adds

- `path`: The SKILL.md file that was checked, at its real path.

Effects: read-only, closed-world.

## `starmap skills print`

Print starmap's skill

### Examples

Print starmap's skill.

```sh
starmap skills print
```

Effects: read-only, closed-world.

## `starmap completion`

Set up starmap's completion in a shell

### Commands

- [`starmap completion install`](#starmap-completion-install): Install starmap's completion where the shell loads it
- [`starmap completion check`](#starmap-completion-check): Check that starmap's installed completion file matches this starmap
- [`starmap completion print`](#starmap-completion-print): Print starmap's completion registration script for a shell

## `starmap completion install`

Install starmap's completion where the shell loads it

### Arguments

- `--shell <SHELL>`: Install for this shell in place of the one that SHELL names. One of `bash`, `elvish`, `fish`, `powershell`, `zsh`.
- `--force`: Replace a file or a link that install did not write; a link's target stays as it is

### Examples

Install starmap's completion for fish.

```sh
starmap completion install --shell fish
```

Install it for zsh, replacing a file or a link that install did not write.

```sh
starmap completion install --shell zsh --force
```

### Columns

- `state`: What install did: failed, skipped, written or unchanged.
- `shell`: The shell: bash, elvish, fish, powershell or zsh.
- `path`: The completion file that install wrote or left. When the file is a link to a file that install wrote, the path of that file.
- `reason`: Why install skipped or failed, or what it replaced under --force, such as `linked to /src/shelf/shelf.fish`, or null.

Effects: additive, idempotent, closed-world.

## `starmap completion check`

Check that starmap's installed completion file matches this starmap

### Arguments

- `--shell <SHELL>`: Check the file of this shell in place of the one that SHELL names. One of `bash`, `elvish`, `fish`, `powershell`, `zsh`.

### Examples

Report zsh's completion file when it is missing or differs.

```sh
starmap completion check --shell zsh
```

### Columns

- `state`: Whether the file holds the script that install writes: missing, differs, unreadable, linked or current.
- `shell`: The shell: bash, elvish, fish, powershell or zsh.
- `path`: The completion file that was checked. When the file is a link to a file that install wrote, the path of that file.
- `reason`: Why the file differs, cannot be read or is linked, such as `linked to /src/shelf/shelf.fish`, or null.

Effects: read-only, closed-world.

## `starmap completion print`

Print starmap's completion registration script for a shell

### Arguments

- `--shell <SHELL>`: Print the script of this shell in place of the one that SHELL names. One of `bash`, `elvish`, `fish`, `powershell`, `zsh`.

### Examples

Print the registration script for fish.

```sh
starmap completion print --shell fish
```

Effects: read-only, closed-world.
