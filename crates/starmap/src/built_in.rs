//! The built-in variables: the names that pass from the caller's
//! environment into an environment of only declared variables, because the
//! operating system and common tools need them.

use std::ffi::OsStr;
use std::fmt;

#[cfg(test)]
mod tests;

/// One built-in variable: a name, or every name with one prefix, that
/// [`Environment::apply_only_declared`](crate::Environment::apply_only_declared)
/// passes to a child without a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltIn {
    pattern: &'static str,
    reason: &'static str,
}

impl BuiltIn {
    /// Returns the built-in variable that `pattern` names, with the
    /// sentence that says why it passes.
    const fn new(pattern: &'static str, reason: &'static str) -> Self {
        Self { pattern, reason }
    }

    /// Returns the name, such as `PATH`. A pattern that ends in `*`, such
    /// as `LC_*`, stands for every name that starts with the text before
    /// the star.
    #[must_use]
    pub const fn pattern(&self) -> &'static str {
        self.pattern
    }

    /// Returns the sentence that says why the variable passes.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        self.reason
    }
}

impl fmt::Display for BuiltIn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.pattern)
    }
}

/// The built-in variables of one platform. The list is fixed, and no
/// `starmap.toml` adds to it: an application that needs another variable
/// declares it. A file may also declare a built-in name to give it a type,
/// and the checked value then replaces the caller's.
///
/// ```
/// use std::ffi::OsStr;
///
/// let built_ins = starmap::BuiltIns::UNIX;
/// assert!(built_ins.passes(OsStr::new("PATH")));
/// assert!(built_ins.passes(OsStr::new("LC_ALL")));
/// assert!(!built_ins.passes(OsStr::new("AWS_PROFILE")));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltIns {
    entries: &'static [BuiltIn],
    /// Whether a name matches in any ASCII case, as Windows compares the
    /// names of environment variables.
    any_case: bool,
}

impl BuiltIns {
    /// The built-in variables of Unix systems, macOS and Linux among them.
    /// A name matches only in the case written here.
    pub const UNIX: Self = Self {
        entries: &[
            BuiltIn::new(
                "PATH",
                "The program and every program it starts are found on it.",
            ),
            BuiltIn::new(
                "HOME",
                "Tools find their configuration and caches under the home directory.",
            ),
            BuiltIn::new(
                "USER",
                "Tools read the name of the person who runs them from it.",
            ),
            BuiltIn::new(
                "LOGNAME",
                "POSIX names it as the login name, and some tools read it where others read USER.",
            ),
            BuiltIn::new(
                "SHELL",
                "Tools that start a shell for the person start the one it names.",
            ),
            BuiltIn::new(
                "TMPDIR",
                "Temporary files go to the directory it names, which macOS sets per user.",
            ),
            BuiltIn::new(
                "LANG",
                "It sets the locale, which decides the character encoding and the language of messages.",
            ),
            BuiltIn::new(
                "LC_*",
                "Each name, such as LC_ALL and LC_CTYPE, sets one part of the locale and overrides LANG.",
            ),
            BuiltIn::new(
                "TERM",
                "The program holds the terminal, and it names the terminal's kind and what it can draw.",
            ),
            BuiltIn::new(
                "TZ",
                "The C library reads the time zone from it, so local times stay the same.",
            ),
        ],
        any_case: false,
    };

    /// The built-in variables of Windows. A name matches in any ASCII
    /// case, so `Path` and `PATH` are the same name.
    pub const WINDOWS: Self = Self {
        entries: &[
            BuiltIn::new(
                "PATH",
                "The program and every program it starts are found on it.",
            ),
            BuiltIn::new(
                "PATHEXT",
                "It lists the file extensions that a program name may leave out, such as .EXE.",
            ),
            BuiltIn::new(
                "SystemRoot",
                "Windows libraries, networking among them, fail to load without the Windows directory.",
            ),
            BuiltIn::new(
                "SystemDrive",
                "Tools build the paths of system directories from the drive that holds Windows.",
            ),
            BuiltIn::new(
                "windir",
                "Older programs read the Windows directory from it where newer ones read SystemRoot.",
            ),
            BuiltIn::new(
                "ComSpec",
                "A program that runs a shell command starts the command interpreter it names.",
            ),
            BuiltIn::new("TEMP", "Temporary files go to the directory it names."),
            BuiltIn::new(
                "TMP",
                "Windows reads it before TEMP when it chooses the directory of temporary files.",
            ),
            BuiltIn::new(
                "USERPROFILE",
                "It names the home directory, where tools find their configuration.",
            ),
            BuiltIn::new(
                "USERNAME",
                "Tools read the name of the person who runs them from it.",
            ),
            BuiltIn::new(
                "APPDATA",
                "Tools keep each person's roaming configuration under the directory it names.",
            ),
            BuiltIn::new(
                "LOCALAPPDATA",
                "Tools keep each person's caches and local data under the directory it names.",
            ),
        ],
        any_case: true,
    };

    /// Returns the built-in variables of the platform that the crate is
    /// built for: [`BuiltIns::WINDOWS`] on Windows and [`BuiltIns::UNIX`]
    /// everywhere else.
    #[must_use]
    pub const fn current() -> &'static Self {
        if cfg!(windows) {
            &Self::WINDOWS
        } else {
            &Self::UNIX
        }
    }

    /// Returns each built-in variable, in the order the list states them.
    pub fn iter(&self) -> impl Iterator<Item = &'static BuiltIn> {
        self.entries.iter()
    }

    /// Whether the variable `name` is built in. A name that is not Unicode
    /// never is.
    #[must_use]
    pub fn passes(&self, name: &OsStr) -> bool {
        let Some(name) = name.to_str() else {
            return false;
        };
        self.entries
            .iter()
            .any(|entry| match entry.pattern.strip_suffix('*') {
                Some(prefix) => name
                    .get(..prefix.len())
                    .is_some_and(|start| self.same(start, prefix)),
                None => self.same(name, entry.pattern),
            })
    }

    /// Whether `left` and `right` are the same name on this platform.
    fn same(&self, left: &str, right: &str) -> bool {
        if self.any_case {
            left.eq_ignore_ascii_case(right)
        } else {
            left == right
        }
    }
}
