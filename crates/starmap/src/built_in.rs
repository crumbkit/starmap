//! The built-in variables: the names that pass from the caller's
//! environment into an environment of only declared variables, because the
//! operating system and common tools need them. The glossary entry
//! "built-in variable" in `docs/glossary.md` states why each name passes.

use std::ffi::OsStr;

#[cfg(test)]
mod tests;

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
/// assert!(!built_ins.passes(OsStr::new("LC_TERMINAL")));
/// assert!(!built_ins.passes(OsStr::new("AWS_PROFILE")));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltIns {
    names: &'static [&'static str],
    /// Whether a name matches in any ASCII case, as Windows compares the
    /// names of environment variables.
    any_case: bool,
}

impl BuiltIns {
    /// The built-in variables of Unix systems, macOS and Linux among them.
    /// A name matches only in the case written here.
    pub const UNIX: Self = Self {
        names: &[
            "PATH",
            "HOME",
            "USER",
            "LOGNAME",
            "SHELL",
            "TMPDIR",
            "LANG",
            // The locale categories of POSIX.
            "LC_ALL",
            "LC_COLLATE",
            "LC_CTYPE",
            "LC_MESSAGES",
            "LC_MONETARY",
            "LC_NUMERIC",
            "LC_TIME",
            // The locale categories that glibc adds.
            "LC_ADDRESS",
            "LC_IDENTIFICATION",
            "LC_MEASUREMENT",
            "LC_NAME",
            "LC_PAPER",
            "LC_TELEPHONE",
            "TERM",
            "TZ",
        ],
        any_case: false,
    };

    /// The built-in variables of Windows. A name matches in any ASCII
    /// case, so `Path` and `PATH` are the same name.
    pub const WINDOWS: Self = Self {
        names: &[
            "PATH",
            "PATHEXT",
            "SystemRoot",
            "SystemDrive",
            "windir",
            "ComSpec",
            "TEMP",
            "TMP",
            "USERPROFILE",
            "USERNAME",
            "APPDATA",
            "LOCALAPPDATA",
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

    /// Returns the name of each built-in variable, in the order the list
    /// states them.
    pub fn names(&self) -> impl Iterator<Item = &'static str> {
        self.names.iter().copied()
    }

    /// Whether the variable `name` is built in. A name that is not Unicode
    /// never is.
    #[must_use]
    pub fn passes(&self, name: &OsStr) -> bool {
        name.to_str()
            .is_some_and(|name| self.names.iter().any(|built_in| self.same(name, built_in)))
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
