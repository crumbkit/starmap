# Commands of `starmap`

This reference is generated from the help of `starmap`. Edit the help in the tool's code, and not this file.

## `starmap`

Write, check and explain the environment variables that an application declares in starmap.toml

### Commands

- [`starmap init`](#starmap-init): Write starmap.toml with a guess for each variable that src reads
- [`starmap check`](#starmap-check): Check that starmap.toml follows every rule of the file, and that it declares every variable that src reads
- [`starmap explain`](#starmap-explain): Describe each declared variable and the value it has in this shell
- [`starmap commands`](#starmap-commands): Explore starmap's commands
- [`starmap schemas`](#starmap-schemas): Explore the JSON documents that starmap prints
- [`starmap self`](#starmap-self): Review starmap's own use

### Arguments

- `--json`: Print the result as a JSON document. The default prints text. Works after every subcommand.
- `--fields <FIELD>`: Print only these comma-separated fields, in this order. The default prints the compact columns. Takes more than one value. Works after every subcommand.
- `--long`: Add the long columns after the compact ones. The default prints only the compact columns. Works after every subcommand.
- `--full`: With --json, print every field of every row. The default prints only the fields of the columns. Works after every subcommand.
- `--no-input`: Fail instead of asking a question. Works after every subcommand.

## `starmap init`

Write starmap.toml with a guess for each variable that src reads.

The scan skips test code: files named tests.rs or ending in \_tests.rs, and the item after #\[cfg(test)\]. check scans src the same way.

### Arguments

- `--spec <FILE>`: Write the spec to this file, and scan the src directory beside it. Default: `starmap.toml`.
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

Check that starmap.toml follows every rule of the file, and that it declares every variable that src reads

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
- `purpose`: The sentence that says what the variable is for.

Effects: read-only, closed-world.

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
