//! starmap declares every environment variable an application reads. An
//! application keeps a `starmap.toml` beside its code, reads its environment
//! into a typed struct, and a test fails when the struct and the file
//! disagree.
//!
//! The crate has no items yet. The declaration, the checks and the
//! environment that applies checked values to a child process arrive in
//! later releases.
