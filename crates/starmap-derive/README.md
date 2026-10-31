# starmap-derive

starmap-derive provides `#[derive(Env)]`, which embeds an application's
`starmap.toml` and generates `Env::load()`, and `#[derive(Enumeration)]`,
which reads a variable into an enum. An application uses them through
starmap's `derive` feature, which documents both derives. The repository's
[README](https://github.com/crumbkit/starmap) describes the project.

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
