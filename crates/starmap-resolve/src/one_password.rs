//! The `one-password` feature: a [`Resolver`] that reads each 1Password
//! reference with the 1Password CLI's `op read`.

use std::io;
use std::mem;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Instant;

use starmap::{Secret, SecretRef};
use zeroize::Zeroizing;

use crate::process::{self, OUTPUT_LIMIT, OpRead};
use crate::{Lookup, Resolver, SecretError};

/// A [`Resolver`] that reads a secret variable's `op://` reference with
/// `op read`. It leaves every other lookup, such as a generated value, to
/// another resolver.
///
/// ```no_run
/// use std::path::PathBuf;
/// use std::time::{Duration, Instant};
///
/// use starmap_resolve::{Context, OnePassword, Places, resolve};
///
/// # let spec = starmap::Spec::parse("")?;
/// # let caller = starmap::Bindings::new();
/// let one_password = OnePassword::new(PathBuf::from("op"));
/// let deadline = Instant::now() + Duration::from_secs(30);
/// let context = Context::new(Places::new(PathBuf::from(".")), &caller, deadline)
///     .resolver(&one_password);
/// let resolution = resolve(&spec, starmap::Mode::Local, &context);
/// # Ok::<(), starmap::SpecError>(())
/// ```
#[derive(Debug, Clone)]
pub struct OnePassword {
    program: PathBuf,
}

impl OnePassword {
    /// Returns a resolver that runs `program` as the 1Password CLI. A bare
    /// name such as `op` is found on the `PATH` of the process that runs
    /// starmap, and the host trusts whoever installed the program it names.
    #[must_use]
    pub fn new(program: PathBuf) -> Self {
        Self { program }
    }
}

impl Resolver for OnePassword {
    /// Reads a [`SecretRef::OnePassword`] reference with `op read` by
    /// `deadline`, and returns what `op` printed, unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::Unsupported`] for any other lookup,
    /// [`SecretError::Locked`] when `op` is signed out,
    /// [`SecretError::NotFound`] when the reference names no vault, item or
    /// field, [`SecretError::Denied`] when the person dismissed the
    /// 1Password prompt, [`SecretError::Deadline`] when `op` has not
    /// finished by `deadline`, and [`SecretError::Program`] for any other
    /// failure, such as output that is not UTF-8 or is over 64 KiB.
    fn resolve(
        &self,
        lookup: Lookup<'_>,
        deadline: Instant,
    ) -> Result<Secret<String>, SecretError> {
        let Lookup::Reference(SecretRef::OnePassword(reference)) = lookup else {
            return Err(SecretError::Unsupported);
        };
        if Instant::now() >= deadline {
            return Err(SecretError::Deadline);
        }
        match process::op_read(&self.program, reference.as_str(), deadline)
            .map_err(SecretError::Program)?
        {
            OpRead::Printed(bytes) => text(bytes),
            OpRead::Failed { status, message } => Err(failure(status, &message)),
            OpRead::TooLong => Err(invalid(format!(
                "op read printed more than {} KiB",
                OUTPUT_LIMIT / 1024
            ))),
            OpRead::Deadline => Err(SecretError::Deadline),
        }
    }
}

/// Returns the secret that `bytes` hold as text. Bytes that are not UTF-8
/// are zeroed before they are dropped.
fn text(mut bytes: Zeroizing<Vec<u8>>) -> Result<Secret<String>, SecretError> {
    match String::from_utf8(mem::take(&mut *bytes)) {
        Ok(text) => Ok(Secret::new(text)),
        Err(error) => {
            drop(Zeroizing::new(error.into_bytes()));
            Err(invalid("op read printed text that is not UTF-8".to_owned()))
        }
    }
}

/// Returns the error that `op` reported with `status` and `message`.
///
/// `op` exits with 1 for every failure, so the error is chosen from the
/// words of its English message. This misses a message that 1Password
/// rewords, which then reads as [`SecretError::Program`] with the message
/// itself, so the person still sees what `op` said. A machine-readable
/// error from `op` would replace the word lists (BASE-37).
fn failure(status: ExitStatus, message: &str) -> SecretError {
    let lower = message.to_lowercase();
    let says = |phrases: &[&str]| phrases.iter().any(|phrase| lower.contains(phrase));
    // The item, vault and field names appear in a not-found message, so its
    // phrases are checked first, so an item named after another phrase,
    // such as "not currently signed in", still reads as not found.
    if says(&[
        "isn't an item",
        "isn't a vault",
        "does not have a field",
        "could not find field",
    ]) {
        SecretError::NotFound
    } else if says(&["dismissed"]) {
        SecretError::Denied
    } else if says(&["not currently signed in"]) {
        SecretError::Locked
    } else {
        let last = message
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty());
        SecretError::Program(io::Error::other(format!(
            "op read exited with {status}: {}",
            last.unwrap_or("it printed no message")
        )))
    }
}

/// Returns a [`SecretError::Program`] for output that `op` should not have
/// printed.
fn invalid(message: String) -> SecretError {
    SecretError::Program(io::Error::new(io::ErrorKind::InvalidData, message))
}
