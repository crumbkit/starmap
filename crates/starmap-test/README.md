# starmap-test

starmap-test checks that an application's `starmap.toml` declares exactly
the environment variables its code reads. An application calls
`starmap_test::check::<Env>()` from an ordinary `#[test]`, as a
dev-dependency, so the declaration serves as its catalog of environment
variables. An application whose commands each read their own struct from
one `starmap.toml` calls `starmap_test::check_together`, which counts a
variable as read when any of the structs reads it. The repository's [README](https://github.com/crumbkit/starmap)
describes the project.

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
