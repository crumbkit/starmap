//! The seams a host fills: a provider of one kind of provided value, and a
//! resolver of secrets. Tests fill them with fakes, so resolution is tested
//! without 1Password or a Keychain (BASE-33).

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::time::Instant;

use starmap::{Name, ProvidedRef, ProviderKind, Secret, SecretBytes, SecretRef};

use crate::{AppName, WorkspaceName};

/// A source of one kind of provided value, such as the ports a host hands
/// out for `multiverse/port`.
pub trait Provider {
    /// Returns the kind of value the provider supplies.
    fn kind(&self) -> &ProviderKind;

    /// Returns the value of the variable `name`, whose bind is `reference`.
    /// The provider returns by `deadline`, which covers the whole
    /// resolution.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] when the provider has no value to give.
    fn provide(
        &self,
        name: &Name,
        reference: &ProvidedRef,
        deadline: Instant,
    ) -> Result<OsString, ProviderError>;
}

/// The reason a [`Provider`] has no value. Its message states the problem
/// and the provider's own next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    message: String,
}

impl ProviderError {
    /// Builds the error with `message`, which states the problem and the
    /// next step.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ProviderError {}

/// A source of secrets: the values of secret variables and the generated
/// values a host keeps.
pub trait Resolver {
    /// Returns the secret that `lookup` names. The resolver returns by
    /// `deadline`, which covers the whole resolution.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::Unsupported`] for a lookup the resolver does
    /// not handle, so that resolution asks the next resolver, and another
    /// [`SecretError`] when the secret cannot be read.
    fn resolve(&self, lookup: Lookup<'_>, deadline: Instant)
    -> Result<Secret<String>, SecretError>;
}

/// The secret that resolution asks a [`Resolver`] for.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum Lookup<'a> {
    /// The value a secret variable's bind refers to.
    Reference(&'a SecretRef),
    /// The generated value of the variable `name` of `app` in `workspace`,
    /// which the resolver creates with `bytes` random bytes on first use.
    Generated {
        /// The workspace the app runs in.
        workspace: &'a WorkspaceName,
        /// The app that reads the variable.
        app: &'a AppName,
        /// The variable.
        name: &'a Name,
        /// How many random bytes a new value has.
        bytes: SecretBytes,
    },
}

/// The reason a [`Resolver`] could not return a secret.
#[derive(Debug)]
#[non_exhaustive]
pub enum SecretError {
    /// The store is locked or signed out.
    Locked,
    /// The reference names no item or field.
    NotFound,
    /// The person refused the store's prompt.
    Denied,
    /// The resolver does not handle this kind of lookup, or the platform
    /// has no such store.
    Unsupported,
    /// The program that reads the store failed.
    Program(io::Error),
    /// The deadline passed before the store answered.
    Deadline,
}

impl fmt::Display for SecretError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locked => formatter.write_str("the secret store is locked"),
            Self::NotFound => formatter.write_str("the secret store has no such item or field"),
            Self::Denied => formatter.write_str("access to the secret store was refused"),
            Self::Unsupported => formatter.write_str("no resolver handles this secret"),
            Self::Program(error) => write!(formatter, "the secret program failed: {error}"),
            Self::Deadline => formatter.write_str("the secret store did not answer in time"),
        }
    }
}

impl Error for SecretError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Program(error) => Some(error),
            _ => None,
        }
    }
}
