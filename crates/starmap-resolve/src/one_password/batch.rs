//! The template that reads several 1Password references with one run of
//! `op inject`, and the split of what `op` printed back into one value for
//! each reference.
//!
//! The template is a marker, then each reference in `{{ }}` followed by a
//! marker of its own. Each marker is 32 hex digits from the operating
//! system's random source, so no value is expected to hold one. `op`
//! replaces each reference with its value and leaves the markers as they
//! are, so each value is the bytes between two markers.

use std::io;

#[cfg(test)]
mod tests;

/// The random bytes of each marker, which it writes as twice as many hex
/// digits.
const MARKER_BYTES: usize = 16;

/// Whether `reference` may go into a template. Each part after `op://`
/// holds only ASCII letters, digits, spaces, `_`, `.` and `-`, and starts
/// and ends with something other than a space.
///
/// `op inject` expands `$var`, `${var}` and `${VAR:-default}`, trims the
/// spaces around a reference, and refuses names with `? ! " { } [ ] ( )`,
/// so a template would read a reference outside this rule as another one
/// or not at all. `op read` takes the reference as one argument, so such a
/// reference is read on its own.
pub(super) fn batchable(reference: &str) -> bool {
    let Some(path) = reference.strip_prefix("op://") else {
        return false;
    };
    path.split('/').all(|part| {
        !part.is_empty()
            && !part.starts_with(' ')
            && !part.ends_with(' ')
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b" _.-".contains(&byte))
    })
}

/// An `op inject` template for some references, and the markers that
/// [`Template::split`] looks for in what `op` printed.
pub(super) struct Template {
    text: String,
    markers: Vec<String>,
    placeholders: Vec<String>,
}

impl Template {
    /// Returns the template of `references`, each of which is
    /// [`batchable`].
    ///
    /// # Errors
    ///
    /// Returns the error of the operating system's random source.
    pub(super) fn new(references: &[&str]) -> io::Result<Self> {
        let mut random = vec![0_u8; MARKER_BYTES * (references.len() + 1)];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let markers: Vec<String> = random.chunks(MARKER_BYTES).map(hex).collect();
        let placeholders: Vec<String> = references
            .iter()
            .map(|reference| format!("{{{{ {reference} }}}}"))
            .collect();
        let mut text = String::new();
        for (index, marker) in markers.iter().enumerate() {
            if let Some(placeholder) = index.checked_sub(1).and_then(|i| placeholders.get(i)) {
                text.push_str(placeholder);
            }
            text.push_str(marker);
        }
        Ok(Self {
            text,
            markers,
            placeholders,
        })
    }

    /// Returns the text that `op inject` reads on its standard input.
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    /// Returns the value of each reference, in order, from `output`, what
    /// `op inject` printed. Every marker must appear exactly once and in
    /// order, the output must start with the first, and only a newline may
    /// follow the last, and no value may be its reference's own `{{ }}`
    /// placeholder, which `op` would have left unreplaced. Otherwise it
    /// returns `None`, and no value is taken.
    ///
    /// A value that held a marker would add a marker, so the count catches
    /// it before the split could cut the value in the wrong place.
    pub(super) fn split<'o>(&self, output: &'o [u8]) -> Option<Vec<&'o [u8]>> {
        let (first, rest) = self.markers.split_first()?;
        if self
            .markers
            .iter()
            .any(|marker| count(output, marker.as_bytes()) != 1)
        {
            return None;
        }
        let mut left = output.strip_prefix(first.as_bytes())?;
        let mut values = Vec::with_capacity(rest.len());
        for (marker, placeholder) in rest.iter().zip(&self.placeholders) {
            let at = find(left, marker.as_bytes())?;
            let value = left.get(..at)?;
            if value == placeholder.as_bytes() {
                return None;
            }
            values.push(value);
            left = left.get(at + marker.len()..)?;
        }
        matches!(left, b"" | b"\n").then_some(values)
    }
}

/// Returns `bytes` as lower case hex digits.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Returns how many times `needle` appears in `haystack`.
fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

/// Returns where `needle` first appears in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
