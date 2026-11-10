//! Reading the `.env` file beside the spec, which `starmap run` takes
//! setting, inherited and provided values from. It is the only module that
//! opens `.env`, and apps never read the file themselves.
//!
//! starmap reads a small part of the `.env` format: `NAME=value` lines, an
//! optional `export ` before the name, values in single or double quotes,
//! blank lines and `#` comments. It never expands anything, so every `$`
//! is literal and the process environment is never read. Any other line,
//! such as a value over several lines or an escape in double quotes, is
//! refused.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::Path;

use crumbkit::{Failure, Recovery};
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
        hint = "make {file} a readable text file of at most 256 KiB",
        alternative = Recovery::Hint(format!("remove {file}")),
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
        hint = "write the line as NAME=value on one line, and put a value that holds spaces, # or quotes in single quotes, without escapes",
        field(at = at.as_str()),
    )]
    Syntax {
        /// The file and the line, such as `.env:4`.
        at: String,
    },
    /// The file sets a secret or generated variable, which only its bind,
    /// the shell or the Keychain supplies.
    #[failure(
        code = DOTENV_SECRET,
        hint = "remove {name} from {file}, and give it a bind in starmap.toml, such as {{ one-password = \"op://vault/item/field\" }}",
        alternative = Recovery::Hint(format!(
            "remove {name} from {file}, and let starmap run ask for it and save it to the Keychain"
        )),
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
    let Ok(text) = std::str::from_utf8(&bytes) else {
        let error = io::Error::other("the file is not UTF-8 text");
        return Err(vec![DotEnvFailure::Unreadable { file: shown, error }]);
    };
    let mut dotenv = DotEnv::default();
    let mut secrets = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let Some((key, value)) = entry(line).map_err(|()| {
            let at = format!("{shown}:{}", number + 1);
            vec![DotEnvFailure::Syntax { at }]
        })?
        else {
            continue;
        };
        let key = key.to_owned();
        let value = Zeroizing::new(value.to_owned());
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

/// Returns the name and value of one line of `.env`, or `None` for a
/// blank line or a comment. The value is the text between its quotes, or
/// the unquoted text before a `#` that follows whitespace, with
/// whitespace trimmed.
///
/// # Errors
///
/// Returns `Err` for a line that is not in the format starmap reads.
fn entry(line: &str) -> Result<Option<(&str, &str)>, ()> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let line = line.strip_prefix("export ").unwrap_or(line);
    let (key, raw) = line.split_once('=').ok_or(())?;
    let key = key.trim();
    let named = key
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !named {
        return Err(());
    }
    let raw = raw.trim();
    let quoted = ['\'', '"']
        .into_iter()
        .find_map(|quote| raw.strip_prefix(quote).map(|inner| (quote, inner)));
    let value = match quoted {
        Some((quote, inner)) => {
            let (value, rest) = inner.split_once(quote).ok_or(())?;
            let rest = rest.trim_start();
            if (quote == '"' && value.contains('\\')) || !(rest.is_empty() || rest.starts_with('#'))
            {
                return Err(());
            }
            value
        }
        None => {
            let comment = raw.char_indices().find(|&(at, c)| {
                c == '#'
                    && raw
                        .get(..at)
                        .is_some_and(|before| before.ends_with(char::is_whitespace))
            });
            let value = comment
                .and_then(|(at, _)| raw.get(..at))
                .unwrap_or(raw)
                .trim_end();
            if value.contains(['\'', '"']) {
                return Err(());
            }
            value
        }
    };
    Ok(Some((key, value)))
}
