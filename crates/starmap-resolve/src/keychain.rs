//! The Keychain resolver, which keeps each generated value as a generic
//! password in the macOS login keychain.

use std::time::Instant;

use starmap::Secret;

use crate::{Lookup, Resolver, SecretError, WorkspaceName};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(test)]
mod tests;

/// A [`Resolver`] of generated values that keeps each one in the macOS
/// login keychain.
///
/// The first lookup of a generated variable creates its value from the
/// variable's `bytes` random bytes, written as base64url without padding,
/// and every later lookup returns the same value. The value is a generic
/// password whose service is `starmap.<workspace>` and whose account is
/// `<app>/<NAME>`. The resolver leaves every lookup by reference to another
/// resolver.
///
/// On a platform other than macOS, every call returns
/// [`SecretError::Unsupported`].
#[derive(Debug, Clone)]
pub struct Keychain {
    prefix: &'static str,
}

impl Keychain {
    /// Returns the resolver of the login keychain.
    #[must_use]
    pub const fn new() -> Self {
        Self { prefix: "starmap" }
    }

    /// Returns a resolver whose services start with `prefix` in place of
    /// `starmap`, so the tests never touch a real workspace's items.
    #[cfg(all(test, target_os = "macos"))]
    const fn with_prefix(prefix: &'static str) -> Self {
        Self { prefix }
    }

    /// Deletes every generated value of `workspace`, and returns how many
    /// it deleted. A host calls it when it deletes the workspace.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::Unsupported`] on a platform other than
    /// macOS, [`SecretError::Denied`] when the person refuses the
    /// Keychain's prompt, [`SecretError::Locked`] when the keychain is
    /// locked and cannot prompt, and [`SecretError::Program`] for any
    /// other Keychain failure.
    pub fn forget(&self, workspace: &WorkspaceName) -> Result<usize, SecretError> {
        platform::forget(&self.service(workspace))
    }

    /// Returns the Keychain service that holds the values of `workspace`.
    fn service(&self, workspace: &WorkspaceName) -> String {
        format!("{}.{}", self.prefix, workspace.as_str())
    }
}

impl Default for Keychain {
    fn default() -> Self {
        Self::new()
    }
}

impl Resolver for Keychain {
    fn resolve(
        &self,
        lookup: Lookup<'_>,
        deadline: Instant,
    ) -> Result<Secret<String>, SecretError> {
        let Lookup::Generated {
            workspace,
            app,
            name,
            bytes,
        } = lookup
        else {
            return Err(SecretError::Unsupported);
        };
        let account = format!("{}/{}", app.as_str(), name.as_str());
        platform::generated(&self.service(workspace), &account, bytes, deadline)
    }
}

/// The calls on a platform without a Keychain, each of which returns
/// [`SecretError::Unsupported`].
#[cfg(not(target_os = "macos"))]
mod platform {
    use std::time::Instant;

    use starmap::{Secret, SecretBytes};

    use crate::SecretError;

    pub(super) fn generated(
        _service: &str,
        _account: &str,
        _bytes: SecretBytes,
        _deadline: Instant,
    ) -> Result<Secret<String>, SecretError> {
        Err(SecretError::Unsupported)
    }

    pub(super) fn forget(_service: &str) -> Result<usize, SecretError> {
        Err(SecretError::Unsupported)
    }
}
