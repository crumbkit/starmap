//! The places a path template names, the names of a workspace and an app,
//! and expanding a template.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use starmap::{Name, NoPlace, PathTemplate, Placeholder};

use crate::Slot;

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
    name: WorkspaceName,
    app: AppName,
    dir: PathBuf,
}

impl Workspace {
    /// Returns the slot of the variable `name` of the app in this
    /// workspace.
    pub(crate) fn slot<'a>(&'a self, name: &'a Name) -> Slot<'a> {
        Slot::new(&self.name, &self.app, name)
    }
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
/// Returns [`NoPlace`] with the first placeholder of the template that has
/// no place.
pub fn expand(template: &PathTemplate, places: &Places) -> Result<PathBuf, NoPlace> {
    template.fill(|placeholder| places.place(placeholder))
}

/// The name of the workspace an app runs in, such as `shelf-dev`. It
/// starts with a lower case ASCII letter and holds only lower case ASCII
/// letters, digits and hyphens. [`FromStr`] builds one from text, and
/// `From<&AppName>` names a workspace after the app it holds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkspaceName(String);

impl From<&AppName> for WorkspaceName {
    /// Returns the workspace named after `app`, as a host names the
    /// workspace of a lone app. Both names follow the same rule, so this
    /// never fails.
    fn from(app: &AppName) -> Self {
        Self(app.0.clone())
    }
}

/// The name of an app within its workspace, such as `shelf`. It follows
/// the rule of a [`WorkspaceName`]. [`FromStr`] builds one from text, and
/// [`AppName::of_dir`] gives the default name of a directory.
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

    /// Returns the app name that the directory `dir` gives a host by
    /// default: its name in lower case, with each character other than a
    /// letter, a digit or a hyphen written as a hyphen, and `app-` before a
    /// name that does not start with a letter. So `My_App` gives `my-app`,
    /// `.tmp9` gives `app--tmp9` and `/` gives `app`. The result always
    /// fits the name rule, so this never fails. A host that runs the app
    /// alone names its workspace after it with `WorkspaceName::from`.
    #[must_use]
    pub fn of_dir(dir: &Path) -> Self {
        let text: String = dir
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default()
            .chars()
            .map(|c| {
                let c = c.to_ascii_lowercase();
                if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let text = match text.chars().next() {
            Some(first) if first.is_ascii_lowercase() => text,
            Some(_) => format!("app-{text}"),
            None => "app".to_owned(),
        };
        Self(text)
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
/// The rule keeps `/` out of a name, because the Keychain account of a
/// generated value is `<app>/<NAME>`, and a `/` in the app name would make
/// two accounts read alike.
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
