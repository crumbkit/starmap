//! The wrapper that holds a withheld value.

use std::fmt;

use secrecy::{ExposeSecret as _, SecretBox};
use zeroize::Zeroize;

#[cfg(test)]
mod tests;

/// A value that is never shown, such as a token. Its `Debug` prints
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
pub struct Secret<T: Zeroize>(SecretBox<T>);

impl<T: Zeroize> Secret<T> {
    /// Holds `value` as a secret.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self(SecretBox::new(Box::new(value)))
    }

    /// Returns the value. Call it only where the value leaves the program,
    /// such as the environment of a child process.
    #[must_use]
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-24: Secret is the crate's one wrapper of secrecy"
    )]
    pub fn expose(&self) -> &T {
        self.0.expose_secret()
    }
}

impl<T: Zeroize> fmt::Debug for Secret<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Eight dots whatever the value, so the text does not reveal the
        // value's length.
        formatter.write_str("Secret(••••••••)")
    }
}
