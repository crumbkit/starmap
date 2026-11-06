//! [`Agent`], the [`Resolver`] that reads hits from the agent, reads misses
//! from `op`, and puts the misses back with one request.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};
use std::time::Instant;

use starmap::{OnePasswordRef, Secret, SecretRef};

use super::{AgentClient, AgentError, AgentStarter, CacheScope, Generation};
use crate::{Lookup, OnePassword, Resolver, SecretError};

#[cfg(test)]
mod tests;

/// A [`Resolver`] that asks the agent for each 1Password lookup that
/// [`crate::resolve`] marked [`crate::Cacheable`], and reads every other
/// lookup and every miss through [`OnePassword`], with one batched `op`
/// call. When the resolution ends, it puts every miss it read into the
/// agent with one Put, carrying the generation of the Get, so a value read
/// before a clear never lands after it.
///
/// A lookup without the mark, such as one of a `cache = false` bind or
/// one resolved in production mode, never reaches the socket. Every
/// failure of the agent, including [`AgentError::NotRunning`] when no
/// agent answers, leaves resolution to `op`, and is kept as a warning that
/// [`Agent::warnings`] returns. With a starter, a resolution that finds no
/// agent starts one and reads from `op` without waiting for it.
pub struct Agent {
    client: AgentClient,
    scope: CacheScope,
    one_password: OnePassword,
    starter: Option<AgentStarter>,
    prepared: Mutex<HashMap<ThreadId, Prepared>>,
    warnings: Mutex<HashMap<ThreadId, Vec<AgentError>>>,
}

/// What [`Agent::prepare`] readied for one resolution, and what
/// [`Agent::resolve`] read for its Put.
struct Prepared {
    /// The agent's values of each reference, one for each lookup it hit.
    hits: HashMap<String, Vec<Secret>>,
    /// The generation of the Get, when the agent answered it.
    generation: Option<Generation>,
    /// The references the agent missed, which are put once read.
    misses: HashSet<String>,
    /// A copy of each miss that `op` read, with its reference, which
    /// [`Agent::finish`] puts.
    read: Vec<(OnePasswordRef, Secret)>,
    /// The resolution's deadline, after which nothing is put.
    deadline: Instant,
}

impl fmt::Debug for Agent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Agent")
            .field("client", &self.client)
            .field("scope", &self.scope)
            .field("starter", &self.starter)
            .finish_non_exhaustive()
    }
}

impl Agent {
    /// Returns a resolver that asks the agent `client` for values in
    /// `scope`, and reads misses with the `op` program of `scope`, so the
    /// program in each cache key is the one that read the value.
    #[must_use]
    pub fn new(client: AgentClient, scope: CacheScope) -> Self {
        let one_password = OnePassword::new(scope.op().to_owned());
        Self {
            client,
            scope,
            one_password,
            starter: None,
            prepared: Mutex::default(),
            warnings: Mutex::default(),
        }
    }

    /// Returns the resolver with `starter`, which starts an agent when a
    /// resolution finds none. The values that resolution reads are not put,
    /// because it has no generation from that agent, and the next
    /// resolution fills the agent.
    #[must_use]
    pub fn starter(mut self, starter: AgentStarter) -> Self {
        self.starter = Some(starter);
        self
    }

    /// Returns and forgets the failures of the agent that the latest
    /// resolution on this thread met, in order. Each names a code and no
    /// value. Each resolution starts with none, so warnings that a host
    /// never reads do not pile up.
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

    /// Asks the agent for `asked` and returns its generation and values.
    /// A failure is a warning, and when no agent answers the starter, if
    /// there is one, starts one.
    fn ask(
        &self,
        asked: &[&OnePasswordRef],
        deadline: Instant,
    ) -> Option<(Generation, Vec<Option<Secret>>)> {
        if asked.is_empty() {
            return None;
        }
        match self.client.get(&self.scope, asked, deadline) {
            Ok(hits) => Some((hits.generation(), hits.into_values())),
            Err(error) => {
                let start = matches!(error, AgentError::NotRunning);
                self.warn(error);
                if start
                    && let Some(starter) = &self.starter
                    && let Err(error) = starter.start(self.client.socket())
                {
                    self.warn(error);
                }
                None
            }
        }
    }
}

impl Resolver for Agent {
    /// Asks the agent with one Get for every marked 1Password lookup of
    /// `lookups`, and prepares every other lookup and every miss with
    /// [`OnePassword`]'s one batched `op` call.
    fn prepare(&self, lookups: &[Lookup<'_>], deadline: Instant) {
        lock(&self.warnings).remove(&thread::current().id());
        let asked: Vec<&OnePasswordRef> = lookups.iter().filter_map(marked).collect();
        let (generation, values) = self
            .ask(&asked, deadline)
            .map_or((None, Vec::new()), |(generation, values)| {
                (Some(generation), values)
            });
        let mut prepared = Prepared {
            hits: HashMap::new(),
            generation,
            misses: HashSet::new(),
            read: Vec::new(),
            deadline,
        };
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

    /// Puts every miss that `resolve` read on this thread into the agent
    /// with one Put, when the Get succeeded and the deadline has not
    /// passed, then drops every value that `prepare` readied and `resolve`
    /// did not take. A failed Put is a warning, and a value the agent
    /// already holds is not.
    ///
    /// The Put carries every value in one frame of at most 1 MiB. Misses
    /// whose values together pass that limit are put by no resolution, and
    /// each resolution warns with `starmap::agent::protocol`; values of
    /// that size are not secrets that a person types, so the limit stands
    /// until such a value appears, and splitting the Put would replace it.
    fn finish(&self) {
        self.one_password.finish();
        let Some(prepared) = lock(&self.prepared).remove(&thread::current().id()) else {
            return;
        };
        let Some(generation) = prepared.generation else {
            return;
        };
        if prepared.read.is_empty() || Instant::now() >= prepared.deadline {
            return;
        }
        let values: Vec<(&OnePasswordRef, &Secret)> = prepared
            .read
            .iter()
            .map(|(reference, value)| (reference, value))
            .collect();
        if let Err(error) = self
            .client
            .put(&self.scope, &values, generation, prepared.deadline)
        {
            self.warn(error);
        }
    }

    /// Returns the agent's value of a marked lookup that it hit, or reads
    /// the lookup through [`OnePassword`]. A copy of a marked miss that
    /// `op` read is kept for the Put that `finish` sends, once for each
    /// reference.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`OnePassword`]'s `resolve`. A failure of the
    /// agent is never one, and [`Agent::warnings`] returns it.
    fn resolve(&self, lookup: Lookup<'_>, deadline: Instant) -> Result<Secret, SecretError> {
        let Some(reference) = marked(&lookup) else {
            return self.one_password.resolve(lookup, deadline);
        };
        let thread = thread::current().id();
        let (hit, put) = match lock(&self.prepared).get_mut(&thread) {
            Some(prepared) => {
                let hit = prepared.hits.get_mut(reference.as_str()).and_then(Vec::pop);
                let put = hit.is_none()
                    && prepared.generation.is_some()
                    && prepared.misses.remove(reference.as_str());
                (hit, put)
            }
            None => (None, false),
        };
        if let Some(value) = hit {
            return Ok(value);
        }
        let value = self.one_password.resolve(lookup, deadline)?;
        if put && let Some(prepared) = lock(&self.prepared).get_mut(&thread) {
            prepared.read.push((reference.clone(), value.duplicate()));
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
