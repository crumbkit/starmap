//! starmap-resolve turns the binds of an application's declared variables
//! into values, for the program that starts the application, such as a
//! host or `starmap run`.
//!
//! [`resolve`] takes each variable of a [`starmap::Spec`] from the
//! [`Context`] a host builds: the caller's own values, the [`Places`] that
//! fill a path template, each [`Provider`] of a provided kind, and each
//! [`Resolver`] of secrets. It returns a [`Resolution`] whose bindings
//! [`starmap::check`] then checks, with a [`ResolveFailure`] for each
//! variable that could not be resolved. [`expand`] fills one path template.
//!
//! The `one-password` feature adds `OnePassword`, which reads secrets
//! through the 1Password CLI, and the `keychain` feature adds `Keychain`,
//! which keeps generated secrets in the macOS Keychain. Neither feature is
//! on by default.

#[cfg(feature = "keychain")]
mod keychain;
#[cfg(feature = "one-password")]
mod one_password;
mod places;
#[cfg(feature = "one-password")]
mod process;
mod resolve;
mod traits;

#[cfg(feature = "keychain")]
pub use keychain::Keychain;
#[cfg(feature = "one-password")]
pub use one_password::OnePassword;

pub use places::{AppName, LabelError, Places, WorkspaceName, expand};
pub use resolve::{Context, Resolution, ResolveFailure, ResolveProblem, resolve};
pub use traits::{Lookup, Provider, ProviderError, Resolver, SecretError};
