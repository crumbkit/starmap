//! [`Agent`], the [`Resolver`] that reads hits from the agent, reads misses
//! from `op`, and puts the misses back.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};
use std::time::Instant;

use starmap::{OnePasswordRef, Secret, SecretRef};

use super::{AgentClient, AgentError, AgentStarter, CacheScope, Generation, Hits};
use crate::{Lookup, OnePassword, Resolver, SecretError};

/// A [`Resolver`] that asks the agent for each 1Password lookup that
/// [`crate::resolve`] marked [`crate::Cacheable`], and reads every other
/// lookup and every miss through [`OnePassword`], with one batched `op`
/// call. It then puts each miss it read into the agent, with the
/// generation of the Get, so a value read before a clear never lands
/// after it.
///
/// A lookup without the mark, such as one of a `cache = false` bind or
/// one resolved in production mode, never reaches the socket. When no
/// agent answers, it runs the starter and asks once more. Every failure
/// of the agent leaves resolution to `op`, and is kept as a warning that
/// [`Agent::warnings`] returns.
pub struct Agent {
    client: AgentClient,
    scope: CacheScope,
    starter: AgentStarter,
    one_password: OnePassword,
    prepared: Mutex<HashMap<ThreadId, Prepared>>,
    warnings: Mutex<HashMap<ThreadId, Vec<AgentError>>>,
}

/// What [`Agent::prepare`] readied for one resolution.
#[derive(Default)]
struct Prepared {
    /// The agent's values of each reference, one for each lookup it hit.
    hits: HashMap<String, Vec<Secret<String>>>,
    /// The generation of the Get, when the agent answered it.
    generation: Option<Generation>,
    /// The references the agent missed, which are put once read.
    misses: HashSet<String>,
}

impl fmt::Debug for Agent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Agent")
            .field("client", &self.client)
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

impl Agent {
    /// Returns a resolver that asks the agent `client` for values in
    /// `scope`, reads misses with the `op` program of `scope`, and starts
    /// the agent with `starter` when none answers.
    #[must_use]
    pub fn new(client: AgentClient, scope: CacheScope, starter: AgentStarter) -> Self {
        let one_password = OnePassword::new(scope.op().to_owned());
        Self {
            client,
            scope,
            starter,
            one_password,
            prepared: Mutex::default(),
            warnings: Mutex::default(),
        }
    }

    /// Returns and forgets the failures of the agent that resolutions on
    /// this thread met, in order. Each names a code and no value.
    pub fn warnings(&self) -> Vec<AgentError> {
        lock(&self.warnings)
            .remove(&thread::current().id())
            .unwrap_or_default()
    }

    /// Keeps `error` as a warning of this thread.
    fn warn(&self, error: AgentError) {
        lock(&self.warnings)
            .entry(thread::current().id())
            .or_default()
            .push(error);
    }

    /// Asks the agent for `references`, and starts it and asks once more
    /// when none answers.
    fn get(&self, references: &[&OnePasswordRef], deadline: Instant) -> Result<Hits, AgentError> {
        match self.client.get(&self.scope, references) {
            Err(AgentError::NotRunning) => {
                self.starter.start(deadline)?;
                self.client.get(&self.scope, references)
            }
            result => result,
        }
    }
}

impl Resolver for Agent {
    /// Asks the agent with one Get for every marked 1Password lookup of
    /// `lookups`, and prepares every other lookup and every miss with
    /// [`OnePassword`]'s one batched `op` call.
    fn prepare(&self, lookups: &[Lookup<'_>], deadline: Instant) {
        let mut prepared = Prepared::default();
        let asked: Vec<&OnePasswordRef> = lookups.iter().filter_map(marked).collect();
        let mut values = Vec::new();
        if !asked.is_empty() {
            match self.get(&asked, deadline) {
                Ok(hits) => {
                    prepared.generation = Some(hits.generation());
                    values = hits.into_values();
                }
                Err(error) => self.warn(error),
            }
        }
        let mut values = values.into_iter();
        let mut rest = Vec::with_capacity(lookups.len());
        for lookup in lookups {
            let Some(reference) = marked(lookup) else {
                rest.push(*lookup);
                continue;
            };
            let reference = reference.as_str().to_owned();
            match values.next().flatten() {
                Some(value) => prepared.hits.entry(reference).or_default().push(value),
                None => {
                    prepared.misses.insert(reference);
                    rest.push(*lookup);
                }
            }
        }
        self.one_password.prepare(&rest, deadline);
        lock(&self.prepared).insert(thread::current().id(), prepared);
    }

    /// Drops every value that `prepare` readied on this thread and
    /// `resolve` did not take.
    fn finish(&self) {
        self.one_password.finish();
        lock(&self.prepared).remove(&thread::current().id());
    }

    /// Returns the agent's value of a marked lookup that it hit, or reads
    /// the lookup through [`OnePassword`]. A marked miss that `op` read is
    /// put into the agent once, with the generation of the Get.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`OnePassword`]'s `resolve`. A failure of the
    /// agent is never one, and [`Agent::warnings`] returns it.
    fn resolve(
        &self,
        lookup: Lookup<'_>,
        deadline: Instant,
    ) -> Result<Secret<String>, SecretError> {
        let Some(reference) = marked(&lookup) else {
            return self.one_password.resolve(lookup, deadline);
        };
        let thread = thread::current().id();
        let (hit, generation) = match lock(&self.prepared).get_mut(&thread) {
            Some(prepared) => {
                let hit = prepared.hits.get_mut(reference.as_str()).and_then(Vec::pop);
                let put = hit.is_none() && prepared.misses.remove(reference.as_str());
                (hit, prepared.generation.filter(|_| put))
            }
            None => (None, None),
        };
        if let Some(value) = hit {
            return Ok(value);
        }
        let value = self.one_password.resolve(lookup, deadline)?;
        if let Some(generation) = generation
            && let Err(error) = self.client.put(&self.scope, reference, &value, generation)
        {
            self.warn(error);
        }
        Ok(value)
    }
}

/// Returns the 1Password reference of `lookup` when [`crate::resolve`]
/// marked it [`crate::Cacheable`].
fn marked<'a>(lookup: &Lookup<'a>) -> Option<&'a OnePasswordRef> {
    match lookup {
        Lookup::Reference {
            reference: SecretRef::OnePassword(reference),
            cacheable: Some(_),
        } => Some(reference),
        _ => None,
    }
}

/// Takes `mutex`'s lock. A thread that panicked while it held the lock
/// leaves entries that are still whole, so the lock is taken anyway.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
