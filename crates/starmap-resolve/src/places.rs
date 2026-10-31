//! The places a path template names, the names of a workspace and an app,
//! and expanding a template.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use starmap::{PathTemplate, Placeholder};

#[cfg(test)]
mod tests;

/// The directories that fill a path template's placeholders, and the
/// workspace and app that name a generated value.
///
/// The checkout always has a place. [`Places::workspace`] and
/// [`Places::home`] add the others.
#[derive(Debug, Clone)]
pub struct Places {
    checkout: PathBuf,
    pub(crate) workspace: Option<Workspace>,
    home: Option<PathBuf>,
}

/// The workspace an app runs in.
#[derive(Debug, Clone)]
pub(crate) struct Workspace {
    pub(crate) name: WorkspaceName,
    pub(crate) app: AppName,
    dir: PathBuf,
}

impl Places {
    /// Returns the places of an app whose checkout is `checkout`, with no
    /// workspace and no home.
    #[must_use]
    pub fn new(checkout: PathBuf) -> Self {
        Self {
            checkout,
            workspace: None,
            home: None,
        }
    }

    /// Places `{workspace}` at `dir`, where `app` runs in the workspace
    /// `name`.
    #[must_use]
    pub fn workspace(mut self, name: WorkspaceName, app: AppName, dir: PathBuf) -> Self {
        self.workspace = Some(Workspace { name, app, dir });
        self
    }

    /// Places `{home}` at `dir`.
    #[must_use]
    pub fn home(mut self, dir: PathBuf) -> Self {
        self.home = Some(dir);
        self
    }

    /// Returns the directory of `placeholder`, if it has a place.
    fn place(&self, placeholder: Placeholder) -> Option<&Path> {
        match placeholder {
            Placeholder::Checkout => Some(&self.checkout),
            Placeholder::Workspace => self.workspace.as_ref().map(|workspace| &*workspace.dir),
            Placeholder::Home => self.home.as_deref(),
            // A placeholder that a later version of starmap adds has no
            // place until this crate learns it.
            _ => None,
        }
    }
}

/// Expands `template` by replacing each placeholder with its directory in
/// `places`. The rest of the template is kept as it is written.
///
/// ```
/// use std::path::PathBuf;
///
/// let template: starmap::PathTemplate = "{checkout}/books.db".parse()?;
/// let places = starmap_resolve::Places::new(PathBuf::from("/src/shelf"));
/// assert_eq!(
///     starmap_resolve::expand(&template, &places),
///     Ok(PathBuf::from("/src/shelf/books.db"))
/// );
/// # Ok::<(), starmap::PathTemplateError>(())
/// ```
///
/// # Errors
///
/// Returns the first placeholder of the template that has no place.
pub fn expand(template: &PathTemplate, places: &Places) -> Result<PathBuf, Placeholder> {
    template.fill(|placeholder| places.place(placeholder))
}

/// The name of the workspace an app runs in, such as `shelf-dev`. It
/// starts with a lower case ASCII letter and holds only lower case ASCII
/// letters, digits and hyphens. [`FromStr`] is the only way to build one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkspaceName(String);

/// The name of an app within its workspace, such as `shelf`. It follows
/// the rule of a [`WorkspaceName`]. [`FromStr`] is the only way to build
/// one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppName(String);

impl WorkspaceName {
    /// Returns the name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AppName {
    /// Returns the name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for WorkspaceName {
    type Err = LabelError;

    /// # Errors
    ///
    /// Returns [`LabelError`] when the text breaks the rule.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        label(text).map(Self)
    }
}

impl FromStr for AppName {
    type Err = LabelError;

    /// # Errors
    ///
    /// Returns [`LabelError`] when the text breaks the rule.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        label(text).map(Self)
    }
}

/// Returns `text` when it starts with a lower case ASCII letter and holds
/// only lower case ASCII letters, digits and hyphens.
///
/// starmap's provider kinds follow the same rule in a private function.
/// The names here are this crate's own type, and sharing the rule would add
/// a public function to starmap that only this crate calls, so the rule is
/// written twice. If starmap ever makes it public, this calls that instead.
fn label(text: &str) -> Result<String, LabelError> {
    let mut characters = text.chars();
    let fits = characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if fits {
        Ok(text.to_owned())
    } else {
        Err(LabelError(()))
    }
}

/// The reason text is not a [`WorkspaceName`] or an [`AppName`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelError(());

impl fmt::Display for LabelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "a workspace or app name starts with a lower case letter and holds only lower case \
             letters, digits and hyphens",
        )
    }
}

impl Error for LabelError {}
