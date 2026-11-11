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
resolve. `check_binds` asks each provider to check the binds of its kind
without resolving a value, so a host can report a wrong provided bind,
naming the variable and the provider, before the application starts. A
provider that does not check accepts every bind. The `one-password` feature adds `OnePassword`, a resolver that
reads all of a resolution's `op://` references with one `op inject`, so one
1Password prompt covers them, and reads a reference that a template would
change, such as one holding `$`, with its own `op read`. The `keychain` feature adds
`Keychain`, which creates each generated value on first use in the encoding
that `Lookup::Generated` names and returns it unchanged afterwards, keeps a secret without a bind that a host saves with
`Resolver::save`, and `Keychain::forget` deletes a workspace's values.
On a platform other than macOS, every call of `Keychain` returns
`SecretError::Unsupported`. The `agent` feature, on macOS and Linux, adds
`agent::AgentSocket::for_user`, the path of a per-user agent's socket,
which a sandbox such as phase's denies to the apps it starts, and
`agent::Agent`, a resolver that reads 1Password values from that agent
when one is running and reads the rest with `op`. A process started
outside a sandbox, including a child of `starmap run`, can reach the
socket. The repository's
[README](https://github.com/crumbkit/starmap) describes the project.

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
