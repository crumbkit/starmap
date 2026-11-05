//! The one module of starmap-resolve with `unsafe` code (BASE-3): it asks
//! macOS for the person's own temporary directory with `confstr`.
//!
//! The invariant it upholds: each `confstr` call is given either a null
//! buffer of size 0, or a buffer it owns of exactly the size it passes, so
//! `confstr` never writes outside memory this module allocated. The text is
//! read only up to its first NUL, inside that buffer.
#![expect(
    unsafe_code,
    reason = "BASE-3: the map's unsafe decision names this module and its invariant"
)]

use std::ffi::{CStr, OsStr};
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;

/// Returns the directory that `confstr(_CS_DARWIN_USER_TEMP_DIR)` names,
/// such as `/var/folders/…/T/`, or `None` when macOS gives none. `TMPDIR`
/// plays no part, so no variable changes the agent's socket path.
pub(super) fn user_temp_dir() -> Option<PathBuf> {
    // SAFETY: a null buffer of size 0 asks only for the size the text
    // needs, including its NUL, and confstr writes nothing.
    let needed = unsafe { libc::confstr(libc::_CS_DARWIN_USER_TEMP_DIR, std::ptr::null_mut(), 0) };
    if needed == 0 {
        return None;
    }
    let mut buffer = vec![0_u8; needed];
    // SAFETY: the buffer is valid for `buffer.len()` bytes, and confstr
    // writes at most that many, including the NUL.
    let written = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    if written == 0 || written > buffer.len() {
        return None;
    }
    let text = CStr::from_bytes_until_nul(&buffer).ok()?;
    Some(PathBuf::from(OsStr::from_bytes(text.to_bytes())))
}
