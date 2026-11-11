//! Resolving each variable of a spec from the caller's values, the path
//! binds, the providers and the resolvers, under one deadline.

use std::error::Error;
use std::fmt;
use std::time::Instant;

use starmap::{
    Bindings, Bound, Declaration, Mode, Name, NoPlace, Placeholder, ProviderKind, Source, Spec,
    Unset, Variable,
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

    /// Registers `resolver`. A secret with a bind or a generated variable
    /// asks each resolver in the order they were registered, until one
    /// handles the lookup.
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

    /// Asks each resolver for `lookup` until one handles it. When none is
    /// registered or every one returns [`SecretError::Unsupported`], the
    /// problem is [`ResolveProblem::NoResolver`].
    fn ask(&self, lookup: Lookup<'_>) -> Result<Option<Bound>, ResolveProblem> {
        for resolver in &self.resolvers {
            self.in_time()?;
            match resolver.resolve(lookup, self.deadline) {
                Ok(secret) => return Ok(Some(Bound::Secret(secret))),
                Err(SecretError::Unsupported) => {}
                Err(error) => return Err(ResolveProblem::Secret(error)),
            }
        }
        Err(ResolveProblem::NoResolver)
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
    /// Each generated variable, and each secret variable with a bind, that
    /// the caller gave a value, in the spec's order. Resolution ignores
    /// such a value and uses the variable's bind, and the host reports a
    /// note that names the variable. A caller value for a secret without a
    /// bind supplies it and is not listed.
    pub ignored: Vec<Name>,
}

impl Resolution {
    /// Returns a resolution of `bindings`, `failures` and `ignored`, such
    /// as a fake that a host's own tests build.
    #[must_use]
    pub fn new(bindings: Bindings, failures: Vec<ResolveFailure>, ignored: Vec<Name>) -> Self {
        Self {
            bindings,
            failures,
            ignored,
        }
    }
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

impl ResolveFailure {
    /// Returns the failure of the variable `name` with `problem`, such as
    /// a fake that a host's own tests build.
    #[must_use]
    pub fn new(name: Name, problem: ResolveProblem) -> Self {
        Self { name, problem }
    }
}

/// Why a variable could not be resolved.
#[derive(Debug)]
#[non_exhaustive]
pub enum ResolveProblem {
    /// The variable is required, and no provider of its kind is registered
    /// and the caller gave no value.
    NoProvider(ProviderKind),
    /// The provider of the variable's kind returned an error. A provider
    /// that refuses a bind in [`Provider::check`] gives the same error
    /// here when the bind is resolved.
    Provider(ProviderError),
    /// The variable is required, and no registered resolver handles its
    /// lookup: none is registered, or each one returned
    /// [`SecretError::Unsupported`].
    NoResolver,
    /// The resolvers could not return the secret.
    Secret(SecretError),
    /// The variable is required, and its path template or generated value
    /// needs a place that the host did not give. A generated value needs
    /// the workspace.
    NoPlace(NoPlace),
    /// The deadline passed before the variable's provider or resolver was
    /// asked.
    Deadline,
    /// The variable is a required secret without a bind, the caller gave
    /// no value, and no resolver holds a value saved for it. A host that
    /// can ask a person for the value asks here, and may keep the answer
    /// with [`Resolver::save`].
    Unbound,
}

impl ResolveProblem {
    /// Whether the problem is an input the host did not give, which fails
    /// only a variable that is required in the mode. A variable that may
    /// stay unset falls back to its default or stays unset. Resolvers that
    /// all leave a lookup unsupported count as no resolver, so registering
    /// one more resolver never fails an optional variable.
    fn lacks_input(&self) -> bool {
        matches!(
            self,
            Self::NoProvider(_) | Self::NoResolver | Self::NoPlace(_) | Self::Unbound
        )
    }
}

/// Resolves the value of each variable `spec` declares, in `mode`, from
/// `context`.
///
/// A caller value wins for a setting, an inherited variable and a secret
/// without a bind. A secret without a bind and without a caller value asks
/// the resolvers with [`Lookup::Saved`], and is
/// [`ResolveProblem::Unbound`] when none holds a value. A setting without one expands its path bind. A provided
/// variable asks the provider of its kind, and takes the caller value only
/// when no provider of that kind is registered. A secret with a bind asks
/// the resolvers for its reference, and a generated variable asks them
/// with [`Lookup::Generated`]. A caller value for either is ignored and
/// listed in [`Resolution::ignored`]. Each member of a family that the
/// caller gives a value keeps that value, because a family's source is a
/// setting without a bind or inherited.
///
/// A missing provider, resolver or place fails only a variable that is
/// required in `mode`. Every provider and resolver is asked under the
/// context's one deadline, and each failure is reported against its own
/// variable, so one call reports every problem.
///
/// Before it asks for any secret, it calls [`Resolver::prepare`] on each
/// resolver with the lookup of every secret variable that has a bind, so a
/// resolver can read them together. When it has asked for every variable,
/// it calls [`Resolver::finish`] on each resolver, which drops what was
/// readied and not taken.
#[must_use]
pub fn resolve(spec: &Spec, mode: Mode, context: &Context<'_>) -> Resolution {
    let mut bindings = Bindings::new();
    let mut failures = Vec::new();
    let mut ignored = Vec::new();
    let _finish = Finish(context);
    prepare(spec, mode, context);
    for (name, variable) in spec.variables() {
        let caller = context.caller.get(name);
        if caller.is_some()
            && matches!(
                variable.source(),
                Source::Secret { bind: Some(_) } | Source::Generated { .. }
            )
        {
            ignored.push(name.clone());
        }
        match find(name, variable, caller, mode, context) {
            Ok(Some(value)) => bindings.bind(name.clone(), value),
            Ok(None) => {}
            Err(problem) if problem.lacks_input() && !required(variable, mode) => {}
            Err(problem) => failures.push(ResolveFailure {
                name: name.clone(),
                problem,
            }),
        }
    }
    for (name, bound) in context.caller.iter() {
        if let Some(Declaration::Member { family, .. }) = spec.declared(name)
            && let Some(bound) = copy(family.variable(), bound)
        {
            bindings.bind(name.clone(), bound);
        }
    }
    Resolution {
        bindings,
        failures,
        ignored,
    }
}

/// A provided variable whose bind a provider refused in
/// [`Provider::check`]. Its text names the variable and the provider and
/// quotes the provider's message, such as `SHELF_PORT: the provider
/// shelf/port refuses the bind: no process is named web`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BindFailure {
    /// The variable.
    pub name: Name,
    /// The kind of the provider that refused the bind.
    pub kind: ProviderKind,
    /// The provider's reason, which states what is wrong and how to fix
    /// the bind.
    pub error: ProviderError,
}

impl BindFailure {
    /// Returns the failure of the variable `name`, whose bind the provider
    /// of `kind` refused with `error`, such as a fake that a host's own
    /// tests build.
    #[must_use]
    pub fn new(name: Name, kind: ProviderKind, error: ProviderError) -> Self {
        Self { name, kind, error }
    }
}

impl fmt::Display for BindFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: the provider {} refuses the bind: {}",
            self.name.as_str(),
            self.kind.as_str(),
            self.error
        )
    }
}

impl Error for BindFailure {}

/// Checks the bind of each provided variable of `spec` with
/// [`Provider::check`], without resolving any value, and returns a
/// [`BindFailure`] for each bind a provider refuses, in the spec's order.
///
/// Each bind asks the first provider in `providers` of its kind, as
/// [`resolve`] chooses one. A variable whose kind has no provider in
/// `providers` is not checked, because a host checks only the kinds it
/// supplies, and [`resolve`] reports [`ResolveProblem::NoProvider`] when
/// the value is needed. Every provided variable is checked whatever its
/// unset rule, because a wrong bind is wrong in every mode.
///
/// [`resolve`] does not call it, so a host that wants both calls it
/// first.
#[must_use]
pub fn check_binds(spec: &Spec, providers: &[&dyn Provider]) -> Vec<BindFailure> {
    spec.variables()
        .filter_map(|(name, variable)| match variable.source() {
            Source::Provided(reference) => Some((name, reference)),
            _ => None,
        })
        .filter_map(|(name, reference)| {
            let kind = reference.kind();
            let provider = providers.iter().find(|p| p.kind() == kind)?;
            provider
                .check(name, reference)
                .err()
                .map(|error| BindFailure::new(name.clone(), kind.clone(), error))
        })
        .collect()
}

/// Calls [`Resolver::finish`] on each resolver of a context when it is
/// dropped, so a resolution that ends early, even by a panic in a
/// resolver, drops what was readied.
struct Finish<'c, 'a>(&'c Context<'a>);

impl Drop for Finish<'_, '_> {
    fn drop(&mut self) {
        for resolver in &self.0.resolvers {
            resolver.finish();
        }
    }
}

/// Calls [`Resolver::prepare`] on each resolver with the lookup in `mode`
/// of every secret variable of `spec` that has a bind, which [`find`] asks
/// for whatever the caller gives. Nothing is readied once the deadline has
/// passed.
fn prepare(spec: &Spec, mode: Mode, context: &Context<'_>) {
    let lookups: Vec<Lookup<'_>> = spec
        .variables()
        .filter_map(|(_, variable)| match variable.source() {
            Source::Secret { bind: Some(bind) } => Some(Lookup::of_bind(bind, mode)),
            _ => None,
        })
        .collect();
    if lookups.is_empty() {
        return;
    }
    for resolver in &context.resolvers {
        if context.in_time().is_err() {
            return;
        }
        resolver.prepare(&lookups, context.deadline);
    }
}

/// Finds the value of `variable` in `mode` from its source, where `caller`
/// is the caller's value. The caller's value is copied only when it is
/// used.
fn find(
    name: &Name,
    variable: &Variable,
    caller: Option<&Bound>,
    mode: Mode,
    context: &Context<'_>,
) -> Result<Option<Bound>, ResolveProblem> {
    match variable.source() {
        Source::Setting {
            bind: Some(template),
        } if caller.is_none() => expand(template, &context.places)
            .map(|path| Some(variable.capture(path.into_os_string())))
            .map_err(ResolveProblem::NoPlace),
        Source::Setting { .. } | Source::Inherited => {
            Ok(caller.and_then(|bound| copy(variable, bound)))
        }
        Source::Secret { bind: None } => match caller {
            Some(bound) => Ok(copy(variable, bound)),
            None => saved(name, context),
        },
        Source::Provided(reference) => {
            let kind = reference.kind();
            match context.providers.iter().find(|p| p.kind() == kind) {
                Some(provider) => {
                    context.in_time()?;
                    provider
                        .provide(name, reference, context.deadline)
                        .map(|value| Some(variable.capture(value)))
                        .map_err(ResolveProblem::Provider)
                }
                None if caller.is_some() => Ok(caller.and_then(|bound| copy(variable, bound))),
                None => Err(ResolveProblem::NoProvider(kind.clone())),
            }
        }
        Source::Secret { bind: Some(bind) } => context.ask(Lookup::of_bind(bind, mode)),
        Source::Generated { bytes } => {
            let workspace = context
                .places
                .workspace
                .as_ref()
                .ok_or(ResolveProblem::NoPlace(NoPlace(Placeholder::Workspace)))?;
            context.ask(Lookup::Generated {
                slot: workspace.slot(name),
                bytes: *bytes,
            })
        }
        // A source that a later version of starmap adds resolves to
        // nothing until this crate learns it.
        _ => Ok(None),
    }
}

/// Asks the resolvers for the value saved for the secret `name`, which has
/// no bind. A context without a workspace has nowhere to look, and a
/// resolver that holds no value, or none that keeps values, leaves the
/// secret [`ResolveProblem::Unbound`].
fn saved(name: &Name, context: &Context<'_>) -> Result<Option<Bound>, ResolveProblem> {
    let Some(workspace) = context.places.workspace.as_ref() else {
        return Err(ResolveProblem::Unbound);
    };
    let lookup = Lookup::Saved {
        slot: workspace.slot(name),
    };
    context.ask(lookup).map_err(|problem| match problem {
        ResolveProblem::NoResolver | ResolveProblem::Secret(SecretError::NotFound) => {
            ResolveProblem::Unbound
        }
        problem => problem,
    })
}

/// Returns a copy of the caller's value `bound` for `variable`. A plain
/// value is bound again by the variable's show.
fn copy(variable: &Variable, bound: &Bound) -> Option<Bound> {
    match bound {
        Bound::Plain(raw) => Some(variable.capture(raw.clone())),
        Bound::Secret(secret) => Some(Bound::Secret(secret.duplicate())),
        Bound::NotUnicode => Some(Bound::NotUnicode),
        // A kind of bound value that a later version of starmap adds is
        // not copied until this crate learns it.
        _ => None,
    }
}

/// Whether `variable` must have a value in `mode`.
fn required(variable: &Variable, mode: Mode) -> bool {
    matches!(variable.unset().in_mode(mode), Unset::Required)
}
