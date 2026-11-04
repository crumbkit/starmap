//! Reading the `.env` file beside the spec, which `starmap run` takes
//! setting, inherited and provided values from. It is the only module that
//! opens `.env`, and apps never read the file themselves.
//!
//! dotenvy parses the file as the ecosystem's tools do, including `$NAME`
//! and `${NAME}`, which it expands from earlier lines of the file and then
//! from the environment of the process that runs starmap.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::Path;

use crumbkit::Failure;
use starmap::{Name, Source, Spec};
use zeroize::Zeroizing;

use crate::codes::{DOTENV_SECRET, DOTENV_SYNTAX, DOTENV_UNREADABLE};
use crate::files;

#[cfg(test)]
mod tests;

/// The most bytes that starmap reads of a `.env` file (BASE-34).
pub(crate) const MAX_BYTES: usize = 256 * 1024;

/// The values that `.env` gives the variables of a spec.
#[derive(Debug, Default)]
pub(crate) struct DotEnv {
    /// Each value of a declared variable that `.env` may give, in the
    /// file's order.
    pub(crate) values: Vec<(OsString, OsString)>,
    /// Each name in the file that the spec does not declare.
    pub(crate) undeclared: Vec<String>,
}

/// A `.env` file that `starmap run` refuses.
#[derive(Debug, Failure)]
pub(crate) enum DotEnvFailure {
    /// The file exists and cannot be read, or is over [`MAX_BYTES`].
    #[failure(
        code = DOTENV_UNREADABLE,
        hint = "make {file} a readable text file of at most 256 KiB, or remove it",
        field(at = file.as_str()),
    )]
    Unreadable {
        /// The file, as a path beside the spec.
        file: String,
        /// Why it cannot be read.
        error: io::Error,
    },
    /// A line does not follow the `.env` format.
    #[failure(
        code = DOTENV_SYNTAX,
        hint = "write the line as NAME=value, and quote a value that holds spaces or #",
        field(at = at.as_str()),
    )]
    Syntax {
        /// The file and the line, such as `.env:4`, or the file alone
        /// when the line cannot be found.
        at: String,
    },
    /// The file sets a secret or generated variable, which only its bind,
    /// the shell or the Keychain supplies.
    #[failure(
        code = DOTENV_SECRET,
        hint = "remove {name} from {file}, and give it a bind in starmap.toml, such as {{ one-password = \"op://vault/item/field\" }}, or let starmap run ask for it and save it to the Keychain",
        field(key = name.as_str()),
        field(at = file.as_str()),
    )]
    Secret {
        /// The variable.
        name: Name,
        /// The file, as a path beside the spec.
        file: String,
    },
}

impl fmt::Display for DotEnvFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { file, .. } => write!(formatter, "{file} cannot be read"),
            Self::Syntax { at } => write!(formatter, "{at} is not a line of the .env format"),
            Self::Secret { name, file } => write!(
                formatter,
                "{file} sets {name}, which starmap.toml declares as a secret"
            ),
        }
    }
}

impl std::error::Error for DotEnvFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { error, .. } => Some(error),
            Self::Syntax { .. } | Self::Secret { .. } => None,
        }
    }
}

/// Reads `file`, the `.env` file beside the spec, and returns the values it
/// gives the variables of `spec`. A missing file gives no values. The
/// file's bytes and the value of each refused secret are zeroed once read.
///
/// # Errors
///
/// Returns every variable of `spec` with a secret or generated source that
/// the file sets, or the one failure of a file that cannot be read or
/// parsed.
pub(crate) fn read(file: &Path, spec: &Spec) -> Result<DotEnv, Vec<DotEnvFailure>> {
    let shown = file.display().to_string();
    let bytes = match files::read_capped(file, MAX_BYTES + 1) {
        Ok(bytes) => Zeroizing::new(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(DotEnv::default()),
        Err(error) => return Err(vec![DotEnvFailure::Unreadable { file: shown, error }]),
    };
    if bytes.len() > MAX_BYTES {
        let error = io::Error::other("the file is over 256 KiB");
        return Err(vec![DotEnvFailure::Unreadable { file: shown, error }]);
    }
    let mut dotenv = DotEnv::default();
    let mut secrets = Vec::new();
    for entry in dotenvy::from_read_iter(&bytes[..]) {
        let (key, value) = entry.map_err(|error| vec![parse_failure(&shown, &bytes, error)])?;
        let value = Zeroizing::new(value);
        let declared = key
            .parse::<Name>()
            .ok()
            .and_then(|name| spec.variable(&name).map(|variable| (name, variable)));
        match declared {
            None => dotenv.undeclared.push(key),
            Some((name, variable)) => match variable.source() {
                Source::Secret { .. } | Source::Generated { .. } => {
                    secrets.push(DotEnvFailure::Secret {
                        name,
                        file: shown.clone(),
                    });
                }
                _ => dotenv
                    .values
                    .push((OsString::from(key), OsString::from(value.as_str()))),
            },
        }
    }
    if secrets.is_empty() {
        Ok(dotenv)
    } else {
        Err(secrets)
    }
}

/// Returns the failure of a file whose bytes `bytes` dotenvy could not
/// parse. A line that dotenvy names is found in the file by its text, and
/// the failure names its number, never its text, which may hold a secret.
fn parse_failure(file: &str, bytes: &[u8], error: dotenvy::Error) -> DotEnvFailure {
    match error {
        dotenvy::Error::LineParse(line, _) => {
            let line = Zeroizing::new(line);
            let text = Zeroizing::new(String::from_utf8_lossy(bytes).into_owned());
            let at = text.find(line.as_str()).map_or_else(
                || file.to_owned(),
                |offset| {
                    let number = text
                        .get(..offset)
                        .map_or(0, |before| before.matches('\n').count());
                    format!("{file}:{}", number + 1)
                },
            );
            DotEnvFailure::Syntax { at }
        }
        dotenvy::Error::Io(error) => DotEnvFailure::Unreadable {
            file: file.to_owned(),
            error,
        },
        dotenvy::Error::EnvVar(error) => DotEnvFailure::Unreadable {
            file: file.to_owned(),
            error: io::Error::other(error),
        },
        // A kind of error that a later dotenvy adds reads as a file that
        // cannot be read, until this module learns it.
        _ => DotEnvFailure::Unreadable {
            file: file.to_owned(),
            error: io::Error::other("dotenvy cannot parse the file"),
        },
    }
}
