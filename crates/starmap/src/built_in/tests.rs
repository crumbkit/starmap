//! Tests for the built-in variables that pass into an environment of only
//! declared variables.

use std::ffi::OsStr;

use super::BuiltIns;

/// Returns the patterns of `built_ins`, in the order the list holds them.
fn patterns(built_ins: &BuiltIns) -> Vec<&'static str> {
    built_ins.iter().map(super::BuiltIn::pattern).collect()
}

#[test]
fn the_unix_list_holds_exactly_the_names_the_documentation_states() {
    assert_eq!(
        patterns(&BuiltIns::UNIX),
        [
            "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TMPDIR", "LANG", "LC_*", "TERM", "TZ",
        ]
    );
}

#[test]
fn the_windows_list_holds_exactly_the_names_the_documentation_states() {
    assert_eq!(
        patterns(&BuiltIns::WINDOWS),
        [
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
        ]
    );
}

#[test]
fn every_built_in_states_its_reason_in_a_sentence() {
    for built_in in BuiltIns::UNIX.iter().chain(BuiltIns::WINDOWS.iter()) {
        let reason = built_in.reason();
        assert!(reason.ends_with('.'), "{}: {reason}", built_in.pattern());
        assert!(reason.len() > 20, "{}: {reason}", built_in.pattern());
    }
}

#[test]
fn a_unix_name_passes_only_in_the_case_the_list_writes() {
    assert!(BuiltIns::UNIX.passes(OsStr::new("PATH")));
    assert!(!BuiltIns::UNIX.passes(OsStr::new("Path")));
    assert!(!BuiltIns::UNIX.passes(OsStr::new("PATHEXT")));
    assert!(!BuiltIns::UNIX.passes(OsStr::new("SHELF_THEME")));
}

#[test]
fn a_pattern_that_ends_in_a_star_passes_every_name_with_its_prefix() {
    assert!(BuiltIns::UNIX.passes(OsStr::new("LC_ALL")));
    assert!(BuiltIns::UNIX.passes(OsStr::new("LC_CTYPE")));
    assert!(!BuiltIns::UNIX.passes(OsStr::new("LC")));
    assert!(!BuiltIns::UNIX.passes(OsStr::new("LCD_SIZE")));
}

#[test]
fn a_windows_name_passes_in_any_case() {
    assert!(BuiltIns::WINDOWS.passes(OsStr::new("Path")));
    assert!(BuiltIns::WINDOWS.passes(OsStr::new("SYSTEMROOT")));
    assert!(BuiltIns::WINDOWS.passes(OsStr::new("comspec")));
    assert!(!BuiltIns::WINDOWS.passes(OsStr::new("HOME")));
}

#[cfg(unix)]
#[test]
fn a_name_that_is_not_unicode_never_passes() {
    use std::os::unix::ffi::OsStrExt as _;

    assert!(!BuiltIns::UNIX.passes(OsStr::from_bytes(b"LC_\xff")));
}

#[test]
fn the_current_list_is_the_list_of_the_platform_the_crate_is_built_for() {
    let expected = if cfg!(windows) {
        &BuiltIns::WINDOWS
    } else {
        &BuiltIns::UNIX
    };
    assert_eq!(patterns(BuiltIns::current()), patterns(expected));
}
