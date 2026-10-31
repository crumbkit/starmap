# starmap-resolve

starmap-resolve turns the bindings of an application's declared variables
into values, for the program that starts the application. The
`one-password` feature reads secrets through the 1Password CLI, and the
`keychain` feature keeps generated secrets in the macOS Keychain. Neither
feature is on by default.

`resolve` takes each variable of a starmap spec from a host's context: the
caller's own values, the directories that fill path templates, a provider
for each provided kind, and resolvers of secrets. It returns the bindings
that `starmap::check` checks, with a failure for each variable it could not
resolve. The `one-password` feature adds `OnePassword`, a resolver that
runs `op read` for each `op://` reference, and the `keychain` feature is
empty in this release. The repository's
[README](https://github.com/crumbkit/starmap) describes the project.

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
