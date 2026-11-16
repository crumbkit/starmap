# crumbkit-starmap

crumbkit-starmap gives a crumbkit tool that derives `starmap::Env` the
built-in `self environment list` command. The command prints each variable
that the tool declares, with the columns of `starmap explain` and a `state`
of `set`, `default`, `unset` or `invalid`. An installed tool ships without its
`starmap.toml`, and the command reads the copy that `starmap::Env::SPEC`
embeds in the tool.

The command reads the tool's own process environment and resolves nothing. A
bind is named and never read, and the value of a withheld variable has no
text. It also lists the variables that the `crumbkit` crate reads in every
tool, `<TOOL>_OUTPUT_BUDGET`, `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE` and
`TERM`, each with the owner `crumbkit` in its `owner` column. The variables that
only `crumbkit-usage` reads are not listed, because a tool that does not use
that crate never reads them. A variable that the tool declares itself keeps the tool's
declaration.

A tool adds an `environment` resource with a `list` verb that takes no
arguments to its `self` group, adds `list_leaf` to its help record, and passes
the result of `environment` to `crumbkit::finish` with
`SELF_ENVIRONMENT_LIST`:

```ignore
SelfCommand::Environment { verb: EnvironmentVerb::List } => {
    let result = crumbkit_starmap::environment(&mut run, <Settings as starmap::Env>::SPEC, &vars);
    crumbkit::finish(run, crumbkit_starmap::SELF_ENVIRONMENT_LIST, result)
}
```

`starmap explain` builds its rows with the same `row` function, so the two
documents describe a variable alike. The repository's
[README](https://github.com/crumbkit/starmap) describes the project.

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
