# starmap

starmap declares every environment variable an application reads. An
application keeps a `starmap.toml` beside its code, reads its environment
into a typed struct, and a test fails when the struct and the file
disagree. This crate parses the declaration, checks values against it and
applies a checked environment to a child process.

The crate holds a variable's name, the forms its value takes and the mode rule. The repository's
[README](https://github.com/crumbkit/starmap) describes the project.

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
