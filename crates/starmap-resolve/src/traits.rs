//! The seams a host fills: a provider of one kind of provided value, and a
//! resolver of secrets. Tests fill them with fakes, so resolution is tested
//! without 1Password or a Keychain (BASE-33).

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::time::Instant;

use starmap::{
    Cache, Mode, Name, ProvidedRef, ProviderKind, Secret, SecretBind, SecretBytes, SecretRef,
};

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
    /// Readies the secrets of `lookups` before [`Resolver::resolve`] asks
    /// for them one at a time, so a resolver can read them together, such
    /// as several 1Password references with one `op` call.
    /// [`crate::resolve`] calls it once on each resolver, before it asks for
    /// any secret, with the lookup of every secret variable that has a
    /// bind. The resolver returns by `deadline`.
    ///
    /// The default readies nothing. A resolver that readies a secret keeps
    /// it until `resolve` asks for it or [`Resolver::finish`] drops it. A
    /// failure is reported by the `resolve` of each lookup it concerns, so
    /// `prepare` returns nothing.
    ///
    /// The `prepare`, every `resolve` and the `finish` of one resolution
    /// run on the same thread, as [`crate::resolve`] runs them. A resolver
    /// may keep what it readied for the calling thread alone, as
    /// `OnePassword` does.
    fn prepare(&self, lookups: &[Lookup<'_>], deadline: Instant) {
        let _ = (lookups, deadline);
    }

    /// Drops every secret that [`Resolver::prepare`] readied and
    /// [`Resolver::resolve`] did not take, and forgets every failure it
    /// recorded. Before it drops what it read, a resolver may hand it off,
    /// such as storing it in a cache. [`crate::resolve`] calls it on each
    /// resolver when the resolution ends. The default has nothing to drop.
    fn finish(&self) {}

    /// Returns the secret that `lookup` names. The resolver returns by
    /// `deadline`, which covers the whole resolution.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::Unsupported`] for a lookup the resolver does
    /// not handle, so that resolution asks the next resolver, and another
    /// [`SecretError`] when the secret cannot be read.
    fn resolve(&self, lookup: Lookup<'_>, deadline: Instant) -> Result<Secret, SecretError>;

    /// Keeps `value` as the secret that `lookup` names, so a later
    /// [`Resolver::resolve`] of the same lookup returns it. A host calls it
    /// with a [`Lookup::Saved`] after a person typed a secret that has no
    /// bind. The resolver returns by `deadline`.
    ///
    /// The default keeps nothing and returns [`SecretError::Unsupported`],
    /// so a resolver that only reads, such as one of 1Password, needs no
    /// code for it.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::Unsupported`] for a lookup the resolver does
    /// not keep, and another [`SecretError`] when the store refuses the
    /// value.
    fn save(
        &self,
        lookup: Lookup<'_>,
        value: &Secret,
        deadline: Instant,
    ) -> Result<(), SecretError> {
        let _ = (lookup, value, deadline);
        Err(SecretError::Unsupported)
    }
}

/// The secret that resolution asks a [`Resolver`] for.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum Lookup<'a> {
    /// The value a secret variable's bind refers to. The variant is
    /// non-exhaustive, so code outside starmap-resolve builds it only with
    /// [`Lookup::reference`], which has no [`Cacheable`] mark, and cannot
    /// copy a mark from one lookup onto another reference.
    ///
    /// ```compile_fail
    /// use starmap::SecretRef;
    /// use starmap_resolve::Lookup;
    ///
    /// let reference = SecretRef::OnePassword("op://Personal/shelf/token".parse().unwrap());
    /// let lookup = Lookup::Reference { reference: &reference, cacheable: None };
    /// ```
    #[non_exhaustive]
    Reference {
        /// The reference the bind names.
        reference: &'a SecretRef,
        /// Present when a cache of resolved values may keep the value.
        /// Only [`crate::resolve`] sets it, so a host that builds a lookup
        /// itself passes `None`.
        cacheable: Option<Cacheable>,
    },
    /// The generated value of `slot`, which the resolver creates with
    /// `bytes` random bytes on first use.
    Generated {
        /// The variable and the app and workspace it belongs to.
        slot: Slot<'a>,
        /// How many random bytes a new value has.
        bytes: SecretBytes,
    },
    /// The value that a person typed for the secret variable of `slot`,
    /// which has no bind, and that a host kept with [`Resolver::save`].
    Saved {
        /// The variable and the app and workspace it belongs to.
        slot: Slot<'a>,
    },
}

/// The variable that a generated or saved value belongs to: the variable
/// `name` of `app` in `workspace`. A resolver that keeps such values keys
/// each one by its slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Slot<'a> {
    /// The workspace the app runs in.
    pub workspace: &'a WorkspaceName,
    /// The app that reads the variable.
    pub app: &'a AppName,
    /// The variable.
    pub name: &'a Name,
}

impl<'a> Slot<'a> {
    /// Returns the slot of the variable `name` of `app` in `workspace`.
    #[must_use]
    pub fn new(workspace: &'a WorkspaceName, app: &'a AppName, name: &'a Name) -> Self {
        Self {
            workspace,
            app,
            name,
        }
    }
}

impl<'a> Lookup<'a> {
    /// Returns the lookup of `reference` without a [`Cacheable`] mark, as
    /// a host builds one by hand.
    #[must_use]
    pub fn reference(reference: &'a SecretRef) -> Self {
        Self::Reference {
            reference,
            cacheable: None,
        }
    }

    /// Returns the lookup of `bind` in `mode`. It is [`Cacheable`] only
    /// when the bind allows a cache and the mode is local, so no value
    /// resolved in production mode is ever marked for a cache.
    pub(crate) fn of_bind(bind: &'a SecretBind, mode: Mode) -> Self {
        let cacheable =
            (mode == Mode::Local && bind.cache() == Cache::Allowed).then_some(Cacheable(()));
        Self::Reference {
            reference: bind.reference(),
            cacheable,
        }
    }
}

/// The mark on a [`Lookup::Reference`] that a cache of resolved values may
/// keep its value: the bind does not say `cache = false`, and the
/// resolution is in local mode. Only [`crate::resolve`] can build one.
///
/// The mark keeps a host that builds lookups by hand from caching a value
/// by mistake. It does not stop a program that reaches a cache directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cacheable(());

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
