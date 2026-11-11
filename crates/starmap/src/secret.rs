//! The wrappers that hold a withheld value: its text, and the bytes a
//! withheld bytes value decodes to.

use std::fmt;

use secrecy::{ExposeSecret as _, SecretBox};

use crate::{ByteRule, Pattern, TypeError};

#[cfg(test)]
mod tests;

/// A text value that is never shown, such as a token. Its `Debug` prints
/// `Secret(••••••••)` whatever the value, and its memory is zeroed when it
/// is dropped.
///
/// `Secret` has no `Display`, no `Serialize` and no `Clone`, so a secret
/// reaches text, a file or a second copy only through
/// [`Secret::expose`]. The files of this crate that call `expose` are
/// listed in the BASE-24 binding of its `rust-standards.toml`.
///
/// ```
/// let secret = starmap::Secret::new("hunter2".to_owned());
/// assert_eq!(format!("{secret:?}"), "Secret(••••••••)");
/// ```
///
/// A secret cannot be cloned or displayed:
///
/// ```compile_fail
/// let secret = starmap::Secret::new("hunter2".to_owned());
/// let copy = secret.clone();
/// ```
///
/// ```compile_fail
/// let secret = starmap::Secret::new("hunter2".to_owned());
/// let text = format!("{secret}");
/// ```
pub struct Secret(SecretBox<String>);

impl Secret {
    /// Holds `value` as a secret.
    #[must_use]
    pub fn new(value: String) -> Self {
        // The String moves into the box as it is, so no shorter copy
        // reallocates it and leaves its old bytes unzeroed.
        Self(SecretBox::new(Box::new(value)))
    }

    /// Returns the value. Call it only where the value leaves the program,
    /// such as the environment of a child process.
    #[must_use]
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-24: Secret is the crate's one wrapper of secrecy"
    )]
    pub fn expose(&self) -> &str {
        self.0.expose_secret().as_str()
    }

    /// Returns whether the value is empty, without exposing it, so that
    /// [`check`](crate::check) can treat an empty secret as unset.
    #[must_use]
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-24: only the length of the value leaves the wrapper"
    )]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.expose_secret().is_empty()
    }

    /// Returns a second secret that holds a copy of the value, without the
    /// value passing through any other type. It is the one way to copy a
    /// secret: `Secret` has no `Clone`, so a copy is a deliberate call.
    ///
    /// ```
    /// let secret = starmap::Secret::new(String::from("token"));
    /// let copy = secret.duplicate();
    /// assert_eq!(format!("{copy:?}"), "Secret(••••••••)");
    /// ```
    #[must_use]
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-24: the copy goes straight into a new Secret"
    )]
    pub fn duplicate(&self) -> Self {
        Self::new(self.0.expose_secret().clone())
    }
}

impl Secret {
    /// Decodes the value by `rule` into bytes held as a secret. The refusal
    /// names only the rule's encoding or length, so the value stays inside
    /// the secret.
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-24: the value is decoded in place, and its bytes go straight into a SecretBytes"
    )]
    pub(crate) fn decode(&self, rule: ByteRule) -> Result<SecretBytes, TypeError> {
        rule.decode(self.0.expose_secret()).map(SecretBytes::new)
    }

    /// Refuses the value unless `pattern` matches all of it. The refusal
    /// names only the pattern, so the value stays inside the secret.
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-24: the value is matched in place and never leaves the secret"
    )]
    pub(crate) fn check(&self, pattern: &Pattern) -> Result<(), TypeError> {
        pattern.check(self.0.expose_secret())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Eight dots whatever the value, so the text does not reveal the
        // value's length.
        formatter.write_str("Secret(••••••••)")
    }
}

/// The bytes that a withheld bytes value decodes to, such as a signing key.
/// Its `Debug` prints `SecretBytes(••••••••)` whatever the bytes, and its
/// memory is zeroed when it is dropped.
///
/// Like [`Secret`], it has no `Display`, no `Serialize` and no `Clone`, so
/// the bytes leave it only through [`SecretBytes::expose`].
///
/// ```
/// let key = starmap::SecretBytes::new(vec![1, 2, 3]);
/// assert_eq!(format!("{key:?}"), "SecretBytes(••••••••)");
/// ```
///
/// ```compile_fail
/// let key = starmap::SecretBytes::new(vec![1, 2, 3]);
/// let copy = key.clone();
/// ```
pub struct SecretBytes(SecretBox<Vec<u8>>);

impl SecretBytes {
    /// Holds `bytes` as a secret.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        // The Vec moves into the box as it is, so its buffer is not copied.
        Self(SecretBox::new(Box::new(bytes)))
    }

    /// Returns the bytes. Call it only where the bytes are used, such as
    /// where a key signs or decrypts.
    #[must_use]
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-24: SecretBytes is the crate's wrapper of secrecy for decoded bytes"
    )]
    pub fn expose(&self) -> &[u8] {
        self.0.expose_secret()
    }

    /// Returns a second holder of a copy of the bytes, without the bytes
    /// passing through any other type.
    ///
    /// ```
    /// let key = starmap::SecretBytes::new(vec![1, 2, 3]);
    /// let copy = key.duplicate();
    /// assert_eq!(format!("{copy:?}"), "SecretBytes(••••••••)");
    /// ```
    #[must_use]
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-24: the copy goes straight into a new SecretBytes"
    )]
    pub fn duplicate(&self) -> Self {
        Self::new(self.0.expose_secret().clone())
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Eight dots whatever the bytes, so the text does not reveal their
        // length.
        formatter.write_str("SecretBytes(••••••••)")
    }
}
