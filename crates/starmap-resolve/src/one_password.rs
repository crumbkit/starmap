//! The `one-password` feature: a [`Resolver`] that reads 1Password
//! references with the 1Password CLI, all of a resolution's references
//! with one `op inject` where it can, and each other one with `op read`.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::mem;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use starmap::{Secret, SecretRef};
use zeroize::Zeroizing;

use crate::process::{self, OUTPUT_LIMIT, OpRun};
use crate::{Lookup, Resolver, SecretError};

mod batch;

use batch::{Template, batchable};

/// A [`Resolver`] that reads a secret variable's `op://` reference with the
/// 1Password CLI. It leaves every other lookup, such as a generated value,
/// to another resolver.
///
/// [`Resolver::prepare`] reads every reference that a template can hold
/// with one `op inject`, so one 1Password prompt covers them, and keeps
/// each value until [`Resolver::resolve`] takes it or
/// [`Resolver::finish`] drops it. `resolve` reads any other reference with
/// its own `op read`.
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
pub struct OnePassword {
    program: PathBuf,
    prepared: Mutex<Prepared>,
}

/// What [`OnePassword::prepare`] readied for the lookups of one
/// resolution.
#[derive(Default)]
struct Prepared {
    /// The entries of each reference, one for each lookup that names it.
    entries: HashMap<String, Vec<Entry>>,
    /// The failure that ends every read still to come, once `op` reported
    /// that 1Password is locked, that the person dismissed its prompt, or
    /// that the deadline passed. A further run would only show the person
    /// another prompt.
    stopped: Option<Stop>,
}

impl Prepared {
    /// Adds `entry` for one lookup of `reference`.
    fn add(&mut self, reference: &str, entry: Entry) {
        self.entries
            .entry(reference.to_owned())
            .or_default()
            .push(entry);
    }
}

/// What a prepared lookup returns.
enum Entry {
    /// The value that `op inject` printed.
    Value(Secret<String>),
    /// The reference is read with its own `op read`, unless the
    /// preparation has stopped.
    Read,
    /// The batch failed in a way that another run would not mend, such as
    /// output over its limit, and this message says how. It names no value.
    Failed(String),
}

/// A failure that ends the reads of a preparation.
#[derive(Debug, Clone, Copy)]
enum Stop {
    Locked,
    Denied,
    Deadline,
}

impl Stop {
    /// Returns the stop that `error` brings, if any.
    fn of(error: &SecretError) -> Option<Self> {
        match error {
            SecretError::Locked => Some(Self::Locked),
            SecretError::Denied => Some(Self::Denied),
            SecretError::Deadline => Some(Self::Deadline),
            _ => None,
        }
    }

    fn error(self) -> SecretError {
        match self {
            Self::Locked => SecretError::Locked,
            Self::Denied => SecretError::Denied,
            Self::Deadline => SecretError::Deadline,
        }
    }
}

impl fmt::Debug for OnePassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OnePassword")
            .field("program", &self.program)
            .finish_non_exhaustive()
    }
}

impl OnePassword {
    /// Returns a resolver that runs `program` as the 1Password CLI. A bare
    /// name such as `op` is found on the `PATH` of the process that runs
    /// starmap, and the host trusts whoever installed the program it names.
    #[must_use]
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            prepared: Mutex::default(),
        }
    }

    /// Returns what [`Resolver::prepare`] readied. A thread that panicked
    /// while it held the lock leaves entries that are still whole, so the
    /// lock is taken anyway.
    fn prepared(&self) -> MutexGuard<'_, Prepared> {
        self.prepared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reads every reference of `batch` with one `op inject` by
    /// `deadline`, and returns an entry for each, in order, and the stop
    /// that the run brought, if any.
    ///
    /// A batch that `op` reports as not found, or that fails for any other
    /// reason that names no reference, such as a template it refuses,
    /// leaves each reference to its own `op read`, which names the
    /// reference that failed. A locked store, a dismissed prompt or the
    /// deadline stops every read. Output over the limit, or output that
    /// does not split into one value for each reference, fails each
    /// reference of the batch.
    fn inject(&self, batch: &[&str], deadline: Instant) -> (Vec<Entry>, Option<Stop>) {
        let each = |entry: &dyn Fn() -> Entry| batch.iter().map(|_| entry()).collect();
        let Ok(template) = Template::new(batch) else {
            // Without random markers there is no template, and nothing
            // was asked of 1Password yet.
            return (each(&|| Entry::Read), None);
        };
        let run = process::op_inject(
            &self.program,
            template.text().to_owned(),
            batch.len(),
            deadline,
        );
        match run {
            // op did not start, so each op read reports the same error
            // against its own variable.
            Err(_) => (each(&|| Entry::Read), None),
            Ok(OpRun::Printed(output)) => match template.split(&output) {
                Some(values) => {
                    let entries = values
                        .into_iter()
                        .map(
                            |value| match text(Zeroizing::new(value.to_vec()), "op inject") {
                                Ok(secret) => Entry::Value(secret),
                                Err(_) => Entry::Failed(
                                    "op inject printed text that is not UTF-8".to_owned(),
                                ),
                            },
                        )
                        .collect();
                    (entries, None)
                }
                None => (
                    each(&|| {
                        Entry::Failed(
                            "op inject printed output that does not hold each of its markers \
                             once and in order"
                                .to_owned(),
                        )
                    }),
                    None,
                ),
            },
            Ok(OpRun::Failed { status, message }) => (
                each(&|| Entry::Read),
                Stop::of(&failure("op inject", status, &message)),
            ),
            Ok(OpRun::TooLong) => (
                each(&|| {
                    Entry::Failed(format!(
                        "op inject printed more than {} KiB for each reference",
                        OUTPUT_LIMIT / 1024
                    ))
                }),
                None,
            ),
            Ok(OpRun::Deadline) => (each(&|| Entry::Read), Some(Stop::Deadline)),
        }
    }

    /// Reads `reference` with its own `op read` by `deadline`.
    fn read(&self, reference: &str, deadline: Instant) -> Result<Secret<String>, SecretError> {
        match process::op_read(&self.program, reference, deadline).map_err(SecretError::Program)? {
            OpRun::Printed(bytes) => text(bytes, "op read"),
            OpRun::Failed { status, message } => Err(failure("op read", status, &message)),
            OpRun::TooLong => Err(invalid(format!(
                "op read printed more than {} KiB",
                OUTPUT_LIMIT / 1024
            ))),
            OpRun::Deadline => Err(SecretError::Deadline),
        }
    }
}

impl Resolver for OnePassword {
    /// Reads every [`SecretRef::OnePassword`] reference of `lookups` that a
    /// template can hold with one `op inject` by `deadline`, and leaves
    /// each other one to its own `op read`. One reference alone is left to
    /// `op read`. It replaces whatever an earlier `prepare` readied.
    fn prepare(&self, lookups: &[Lookup<'_>], deadline: Instant) {
        let mut prepared = Prepared::default();
        let mut batch = Vec::new();
        for lookup in lookups {
            if let Lookup::Reference(SecretRef::OnePassword(reference)) = lookup {
                let reference = reference.as_str();
                if batchable(reference) {
                    batch.push(reference);
                } else {
                    prepared.add(reference, Entry::Read);
                }
            }
        }
        match batch.as_slice() {
            [] => {}
            [reference] => prepared.add(reference, Entry::Read),
            _ => {
                let (entries, stopped) = self.inject(&batch, deadline);
                for (reference, entry) in batch.iter().zip(entries) {
                    prepared.add(reference, entry);
                }
                prepared.stopped = stopped;
            }
        }
        *self.prepared() = prepared;
    }

    /// Drops every value that `prepare` readied and `resolve` did not take,
    /// which zeroes it, and forgets a stop.
    fn finish(&self) {
        *self.prepared() = Prepared::default();
    }

    /// Returns the value that [`Resolver::prepare`] readied for a
    /// [`SecretRef::OnePassword`] reference, or reads it with `op read` by
    /// `deadline` and returns what `op` printed, unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::Unsupported`] for any other lookup,
    /// [`SecretError::Locked`] when `op` is signed out,
    /// [`SecretError::NotFound`] when the reference names no vault, item or
    /// field, [`SecretError::Denied`] when the person dismissed the
    /// 1Password prompt, [`SecretError::Deadline`] when `op` has not
    /// finished by `deadline`, and [`SecretError::Program`] for any other
    /// failure, such as output that is not UTF-8 or is over 64 KiB. Once a
    /// prepared reference is locked, denied or past the deadline, every
    /// other prepared reference returns the same error without running
    /// `op` again.
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
        let reference = reference.as_str();
        let (entry, stopped) = {
            let mut prepared = self.prepared();
            let entry = prepared.entries.get_mut(reference).and_then(Vec::pop);
            (entry, prepared.stopped)
        };
        match entry {
            Some(Entry::Value(secret)) => Ok(secret),
            Some(Entry::Failed(message)) => Err(invalid(message)),
            Some(Entry::Read) => {
                if let Some(stop) = stopped {
                    return Err(stop.error());
                }
                let read = self.read(reference, deadline);
                if let Some(stop) = read.as_ref().err().and_then(Stop::of) {
                    self.prepared().stopped = Some(stop);
                }
                read
            }
            None => self.read(reference, deadline),
        }
    }
}

/// Returns the secret that `bytes`, which `command` printed, hold as text.
/// Bytes that are not UTF-8 are zeroed before they are dropped.
fn text(mut bytes: Zeroizing<Vec<u8>>, command: &str) -> Result<Secret<String>, SecretError> {
    match String::from_utf8(mem::take(&mut *bytes)) {
        Ok(text) => Ok(Secret::new(text)),
        Err(error) => {
            drop(Zeroizing::new(error.into_bytes()));
            Err(invalid(format!("{command} printed text that is not UTF-8")))
        }
    }
}

/// Returns the error that `command`, a run of `op`, reported with `status`
/// and `message`.
///
/// `op` exits with 1 for every failure, so the error is chosen from the
/// words of its English message. This misses a message that 1Password
/// rewords, which then reads as [`SecretError::Program`] with the message
/// itself, so the person still sees what `op` said. A machine-readable
/// error from `op` would replace the word lists (BASE-37).
fn failure(command: &str, status: ExitStatus, message: &str) -> SecretError {
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
            "{command} exited with {status}: {}",
            last.unwrap_or("it printed no message")
        )))
    }
}

/// Returns a [`SecretError::Program`] for output that `op` should not have
/// printed.
fn invalid(message: String) -> SecretError {
    SecretError::Program(io::Error::new(io::ErrorKind::InvalidData, message))
}
