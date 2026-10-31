//! Resolving each variable of a spec from the caller's values, the path
//! binds, the providers and the resolvers, under one deadline.

use std::ffi::OsString;
use std::fmt;
use std::time::Instant;

use starmap::{
    Bindings, Bound, Mode, Name, Placeholder, ProviderKind, Secret, Source, Spec, Unset, Variable,
};

use crate::{Lookup, Places, Provider, ProviderError, Resolver, SecretError, expand};

#[cfg(test)]
mod tests;

/// What [`resolve`] reads values from: the places of path templates, the
/// caller's own values, the providers and resolvers the host registers, and
/// the one deadline that covers the whole resolution (BASE-34).
pub struct Context<'a> {
    places: Places,
    caller: &'a Bindings,
    deadline: Instant,
    providers: Vec<&'a dyn Provider>,
    resolvers: Vec<&'a dyn Resolver>,
}

impl fmt::Debug for Context<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Context")
            .field("places", &self.places)
            .field("caller", &self.caller)
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

impl<'a> Context<'a> {
    /// Returns a context with `places`, the caller's values `caller` and
    /// `deadline`, and no provider or resolver. A host passes its own
    /// values as `caller`, and `starmap run` passes the shell's.
    #[must_use]
    pub fn new(places: Places, caller: &'a Bindings, deadline: Instant) -> Self {
        Self {
            places,
            caller,
            deadline,
            providers: Vec::new(),
            resolvers: Vec::new(),
        }
    }

    /// Registers `provider`. A provided variable asks the first provider
    /// registered for its kind.
    #[must_use]
    pub fn provider(mut self, provider: &'a dyn Provider) -> Self {
        self.providers.push(provider);
        self
    }

    /// Registers `resolver`. A secret or generated variable asks each
    /// resolver in the order they were registered, until one handles the
    /// lookup.
    #[must_use]
    pub fn resolver(mut self, resolver: &'a dyn Resolver) -> Self {
        self.resolvers.push(resolver);
        self
    }

    /// Returns [`ResolveProblem::Deadline`] once the deadline has passed.
    fn in_time(&self) -> Result<(), ResolveProblem> {
        if Instant::now() < self.deadline {
            Ok(())
        } else {
            Err(ResolveProblem::Deadline)
        }
    }

    /// Asks each resolver for `lookup` until one handles it.
    fn ask(&self, lookup: Lookup<'_>) -> Result<Option<Found>, ResolveProblem> {
        let mut problem = ResolveProblem::NoResolver;
        for resolver in &self.resolvers {
            self.in_time()?;
            match resolver.resolve(lookup, self.deadline) {
                Ok(secret) => return Ok(Some(Found::Bound(Bound::Secret(secret)))),
                Err(SecretError::Unsupported) => {
                    problem = ResolveProblem::Secret(SecretError::Unsupported);
                }
                Err(error) => return Err(ResolveProblem::Secret(error)),
            }
        }
        Err(problem)
    }
}

/// The outcome of [`resolve`]: the bindings of every variable it resolved,
/// a failure for each variable it could not, and the variables whose caller
/// value it ignored.
#[derive(Debug)]
#[non_exhaustive]
pub struct Resolution {
    /// The resolved values, which [`starmap::check`] checks. A variable
    /// without a value is not bound, so its default or unset rule applies.
    pub bindings: Bindings,
    /// Each variable that could not be resolved, in the spec's order.
    pub failures: Vec<ResolveFailure>,
    /// Each secret or generated variable that the caller gave a value,
    /// in the spec's order. Resolution ignores such a value and uses the
    /// variable's bind, and the host reports a note that names the
    /// variable.
    pub ignored: Vec<Name>,
}

/// A variable that could not be resolved, and why.
#[derive(Debug)]
#[non_exhaustive]
pub struct ResolveFailure {
    /// The variable.
    pub name: Name,
    /// Why it could not be resolved.
    pub problem: ResolveProblem,
}

/// Why a variable could not be resolved.
#[derive(Debug)]
#[non_exhaustive]
pub enum ResolveProblem {
    /// The variable is required, and no provider of its kind is registered
    /// and the caller gave no value.
    NoProvider(ProviderKind),
    /// The provider of the variable's kind returned an error.
    Provider(ProviderError),
    /// The variable is required, and no resolver is registered.
    NoResolver,
    /// The resolvers could not return the secret.
    Secret(SecretError),
    /// The variable is required, and its path template or generated value
    /// needs a place that the host did not give. A generated value needs
    /// the workspace.
    Placeholder(Placeholder),
    /// The deadline passed before the variable's provider or resolver was
    /// asked.
    Deadline,
}

impl ResolveProblem {
    /// Whether the problem is an input the host did not give, which fails
    /// only a variable that is required in the mode. A variable that may
    /// stay unset falls back to its default or stays unset.
    fn lacks_input(&self) -> bool {
        matches!(
            self,
            Self::NoProvider(_) | Self::NoResolver | Self::Placeholder(_)
        )
    }
}

/// A value that resolution found for one variable.
enum Found {
    /// A raw value, which [`starmap::capture`] binds by the variable's show.
    Raw(OsString),
    /// A value already bound as the variable's show needs.
    Bound(Bound),
}

/// Resolves the value of each variable `spec` declares, in `mode`, from
/// `context`.
///
/// A caller value wins for a setting or an inherited variable. A setting
/// without one expands its path bind. A provided variable asks the provider
/// of its kind, and takes the caller value only when no provider of that
/// kind is registered. A secret variable asks the resolvers for its
/// reference, and a generated variable asks them with
/// [`Lookup::Generated`]. A caller value for a secret or generated
/// variable is ignored and listed in [`Resolution::ignored`].
///
/// A missing provider, resolver or place fails only a variable that is
/// required in `mode`. Every provider and resolver is asked under the
/// context's one deadline, and each failure is reported against its own
/// variable, so one call reports every problem.
#[must_use]
pub fn resolve(spec: &Spec, mode: Mode, context: &Context<'_>) -> Resolution {
    let mut raw = Vec::new();
    let mut bound = Vec::new();
    let mut failures = Vec::new();
    let mut ignored = Vec::new();
    for (name, variable) in spec.variables() {
        let caller = context.caller.get(name);
        if caller.is_some()
            && matches!(
                variable.source(),
                Source::Secret { .. } | Source::Generated { .. }
            )
        {
            ignored.push(name.clone());
        }
        match find(name, variable, caller.and_then(copy), context) {
            Ok(Some(Found::Raw(value))) => raw.push((OsString::from(name.as_str()), value)),
            Ok(Some(Found::Bound(value))) => bound.push((name.clone(), value)),
            Ok(None) => {}
            Err(problem) if problem.lacks_input() && !required(variable, mode) => {}
            Err(problem) => failures.push(ResolveFailure {
                name: name.clone(),
                problem,
            }),
        }
    }
    let mut bindings = starmap::capture(spec, raw);
    for (name, value) in bound {
        bindings.bind(name, value);
    }
    Resolution {
        bindings,
        failures,
        ignored,
    }
}

/// Finds the value of `variable` from its source, where `caller` is the
/// caller's value.
fn find(
    name: &Name,
    variable: &Variable,
    caller: Option<Found>,
    context: &Context<'_>,
) -> Result<Option<Found>, ResolveProblem> {
    match variable.source() {
        Source::Setting {
            bind: Some(template),
        } if caller.is_none() => expand(template, &context.places)
            .map(|path| Some(Found::Raw(path.into_os_string())))
            .map_err(ResolveProblem::Placeholder),
        Source::Setting { .. } | Source::Inherited => Ok(caller),
        Source::Provided(reference) => {
            let kind = reference.kind();
            match context.providers.iter().find(|p| p.kind() == kind) {
                Some(provider) => {
                    context.in_time()?;
                    provider
                        .provide(name, reference, context.deadline)
                        .map(|value| Some(Found::Raw(value)))
                        .map_err(ResolveProblem::Provider)
                }
                None if caller.is_some() => Ok(caller),
                None => Err(ResolveProblem::NoProvider(kind.clone())),
            }
        }
        Source::Secret {
            bind: Some(reference),
        } => context.ask(Lookup::Reference(reference)),
        Source::Generated { bytes } => {
            let workspace = context
                .places
                .workspace
                .as_ref()
                .ok_or(ResolveProblem::Placeholder(Placeholder::Workspace))?;
            context.ask(Lookup::Generated {
                workspace: &workspace.name,
                app: &workspace.app,
                name,
                bytes: *bytes,
            })
        }
        // A secret without a bind has no value to resolve, and a source
        // that a later version of starmap adds resolves to nothing until
        // this crate learns it.
        _ => Ok(None),
    }
}

/// Returns a copy of the caller's value `bound`.
#[expect(
    clippy::disallowed_methods,
    reason = "BASE-24: a caller's withheld value is copied straight into a new Secret"
)]
fn copy(bound: &Bound) -> Option<Found> {
    match bound {
        Bound::Plain(raw) => Some(Found::Raw(raw.clone())),
        Bound::Secret(secret) => Some(Found::Bound(Bound::Secret(Secret::new(
            secret.expose().clone(),
        )))),
        Bound::NotUnicode => Some(Found::Bound(Bound::NotUnicode)),
        // A kind of bound value that a later version of starmap adds is
        // not copied until this crate learns it.
        _ => None,
    }
}

/// Whether `variable` must have a value in `mode`.
fn required(variable: &Variable, mode: Mode) -> bool {
    matches!(variable.unset().in_mode(mode), Unset::Required)
}
