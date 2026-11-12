//! A field whose variable's rules the application checks where it uses
//! the value.

use crate::{Finding, Name};

/// A variable's checked value, or the [`Finding`] that refuses it, which a
/// field of a struct that derives [`Env`](crate::Env) holds when its type
/// is `Deferred<T>`, or `Deferred<Option<T>>` for an optional variable.
///
/// A deferred field defers every rule of its variable: the unset rule, the
/// type, the group rule and the field's own check.
/// [`Env::from_vars`](crate::Env::from_vars) never refuses a read because
/// of a deferred variable. The field holds the finding that the read would
/// have reported, and the application takes the value when it needs it and
/// builds its own error from the finding, in its own order.
///
/// The mode variable is the exception: an invalid value of the mode
/// variable refuses the read even when its field is deferred, because the
/// mode decides the other fields.
///
/// ```
/// # #[cfg(feature = "derive")] {
/// // starmap.toml declares SHELF_CATALOGUE_URL as a required url.
/// #[derive(starmap::Env)]
/// # #[starmap(spec = "tests/example/deferred.toml")]
/// struct Serve {
///     #[starmap("SHELF_CATALOGUE_URL")]
///     catalogue: starmap::Deferred<starmap::url::Url>,
///     #[starmap("SHELF_TOKEN")]
///     token: starmap::Deferred<starmap::Secret>,
///     #[starmap("SHELF_WORKERS")]
///     workers: starmap::Deferred<u8>,
///     #[starmap("SHELF_MIRROR_URL")]
///     mirror: starmap::Deferred<Option<starmap::url::Url>>,
///     #[starmap("SHELF_PORT")]
///     port: starmap::Port,
/// }
///
/// use starmap::Env as _;
/// let vars = [("SHELF_CATALOGUE_URL", "not a url"), ("SHELF_PORT", "8080")]
///     .map(|(name, value)| (name.into(), value.into()));
/// let env = Serve::from_vars(vars).expect("only the port refuses the read");
/// let finding = env.catalogue.get().expect_err("the address is not a url");
/// assert_eq!(finding.name.as_str(), "SHELF_CATALOGUE_URL");
/// assert!(finding.to_string().starts_with("SHELF_CATALOGUE_URL: "));
/// # }
/// ```
///
/// Its `Debug` text names the variable and shows the value as the value's
/// own `Debug` does, so a withheld [`Secret`](crate::Secret) prints only
/// dots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deferred<T> {
    /// The variable the field reads.
    name: Name,
    /// The checked value, or the finding that refuses it.
    result: Result<T, Finding>,
}

impl<T> Deferred<T> {
    /// Returns the field of the variable `name`, which holds `result`.
    pub(crate) fn new(name: Name, result: Result<T, Finding>) -> Self {
        Self { name, result }
    }

    /// Returns the variable the field reads.
    #[must_use]
    pub fn name(&self) -> &Name {
        &self.name
    }

    /// Returns the checked value, or the finding that refuses it.
    ///
    /// # Errors
    ///
    /// Returns the [`Finding`] that a read which did not defer the
    /// variable would have reported, such as [`Problem::Missing`](crate::Problem::Missing)
    /// or [`Problem::Invalid`](crate::Problem::Invalid).
    pub fn get(&self) -> Result<&T, &Finding> {
        self.result.as_ref()
    }

    /// Returns the checked value, or the finding that refuses it, by value.
    ///
    /// # Errors
    ///
    /// Returns the [`Finding`] as [`Deferred::get`] does.
    #[expect(
        clippy::result_large_err,
        reason = "the field holds the finding unboxed, and the application moves it into its own error"
    )]
    pub fn into_result(self) -> Result<T, Finding> {
        self.result
    }
}
