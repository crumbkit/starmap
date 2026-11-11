//! The Keychain resolver, which keeps each generated value as a generic
//! password in the macOS login keychain.

use std::time::Instant;

use starmap::Secret;
use zeroize::Zeroizing;

use crate::{Lookup, Resolver, SecretError, Slot, WorkspaceName};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(test)]
mod tests;

/// A [`Resolver`] of generated values and saved secrets that keeps each
/// one in the macOS login keychain.
///
/// The first lookup of a generated variable creates its value from the
/// lookup's `bytes` random bytes, written in the lookup's encoding with
/// [`starmap::Encoding::encode`], and every later lookup returns the same
/// value unchanged. A stored value whose encoding or length no longer fits
/// the variable's type fails the check after resolution, and deleting it
/// lets the next lookup create a new one. The value is a generic password
/// whose service is `starmap.<workspace>` and whose account is
/// `<app>/<NAME>`.
///
/// A [`Lookup::Saved`] returns the value that [`Resolver::save`] kept for
/// a secret without a bind, at the same service and account, and
/// [`SecretError::NotFound`] when the Keychain holds none. `forget`
/// deletes saved values with the generated ones. The resolver leaves every
/// lookup by reference to another resolver.
///
/// On a platform other than macOS, every call returns
/// [`SecretError::Unsupported`].
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct Keychain;

impl Keychain {
    /// Returns the resolver of the login keychain.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Deletes every generated and saved value of `workspace`, and returns how many
    /// it deleted. A host calls it when it deletes the workspace.
    ///
    /// A failure partway through can leave some of the workspace's values
    /// deleted and others in place. Calling `forget` again deletes the
    /// rest.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::Unsupported`] on a platform other than
    /// macOS or in a session without a login keychain,
    /// [`SecretError::Denied`] when the person refuses the Keychain's
    /// prompt, [`SecretError::Locked`] when the keychain is locked and
    /// cannot prompt, and [`SecretError::Program`] for any other Keychain
    /// failure.
    pub fn forget(&self, workspace: &WorkspaceName) -> Result<usize, SecretError> {
        platform::forget(&service(workspace))
    }
}

/// Returns the Keychain service that holds the values of `workspace`.
fn service(workspace: &WorkspaceName) -> String {
    format!("starmap.{}", workspace.as_str())
}

impl Resolver for Keychain {
    fn resolve(&self, lookup: Lookup<'_>, deadline: Instant) -> Result<Secret, SecretError> {
        match lookup {
            Lookup::Generated {
                slot,
                bytes,
                encoding,
            } => platform::generated(
                &service(slot.workspace),
                &account(slot),
                bytes,
                encoding,
                deadline,
            ),
            Lookup::Saved { slot } => {
                platform::saved(&service(slot.workspace), &account(slot), deadline)
            }
            _ => Err(SecretError::Unsupported),
        }
    }

    /// Keeps `value` for a [`Lookup::Saved`]. When the Keychain already
    /// holds a value at the same service and account, it keeps that value
    /// unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::Unsupported`] for any other lookup and on a
    /// platform other than macOS, and the Keychain's failure otherwise.
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-24: a typed secret leaves the program here, into the Keychain"
    )]
    fn save(
        &self,
        lookup: Lookup<'_>,
        value: &Secret,
        deadline: Instant,
    ) -> Result<(), SecretError> {
        let Lookup::Saved { slot } = lookup else {
            return Err(SecretError::Unsupported);
        };
        let value = Zeroizing::new(value.expose().to_owned());
        platform::save(&service(slot.workspace), &account(slot), value, deadline)
    }
}

/// Returns the Keychain account of `slot`, which names its app and
/// variable.
fn account(slot: Slot<'_>) -> String {
    format!("{}/{}", slot.app.as_str(), slot.name.as_str())
}

/// The calls on a platform without a Keychain, each of which returns
/// [`SecretError::Unsupported`].
#[cfg(not(target_os = "macos"))]
mod platform {
    use std::time::Instant;

    use starmap::{ByteCount, Encoding, Secret};
    use zeroize::Zeroizing;

    use crate::SecretError;

    pub(super) fn generated(
        _service: &str,
        _account: &str,
        _bytes: ByteCount,
        _encoding: Encoding,
        _deadline: Instant,
    ) -> Result<Secret, SecretError> {
        Err(SecretError::Unsupported)
    }

    pub(super) fn saved(
        _service: &str,
        _account: &str,
        _deadline: Instant,
    ) -> Result<Secret, SecretError> {
        Err(SecretError::Unsupported)
    }

    pub(super) fn save(
        _service: &str,
        _account: &str,
        _value: Zeroizing<String>,
        _deadline: Instant,
    ) -> Result<(), SecretError> {
        Err(SecretError::Unsupported)
    }

    pub(super) fn forget(_service: &str) -> Result<usize, SecretError> {
        Err(SecretError::Unsupported)
    }
}
