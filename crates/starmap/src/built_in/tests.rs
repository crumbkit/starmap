//! Tests for the built-in variables that pass into an environment of only
//! declared variables.

use std::ffi::OsStr;

use super::BuiltIns;

#[test]
fn the_unix_list_names_the_basic_variables_and_each_locale_category() {
    assert_eq!(
        BuiltIns::UNIX.names().collect::<Vec<_>>(),
        [
            "PATH",
            "HOME",
            "USER",
            "LOGNAME",
            "SHELL",
            "TMPDIR",
            "LANG",
            "LC_ALL",
            "LC_COLLATE",
            "LC_CTYPE",
            "LC_MESSAGES",
            "LC_MONETARY",
            "LC_NUMERIC",
            "LC_TIME",
            "LC_ADDRESS",
            "LC_IDENTIFICATION",
            "LC_MEASUREMENT",
            "LC_NAME",
            "LC_PAPER",
            "LC_TELEPHONE",
            "TERM",
            "TZ",
        ]
    );
}

#[test]
fn the_windows_list_names_the_variables_that_windows_programs_need() {
    assert_eq!(
        BuiltIns::WINDOWS.names().collect::<Vec<_>>(),
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
fn a_unix_name_passes_only_in_the_case_the_list_writes() {
    assert!(BuiltIns::UNIX.passes(OsStr::new("PATH")));
    assert!(!BuiltIns::UNIX.passes(OsStr::new("Path")));
    assert!(!BuiltIns::UNIX.passes(OsStr::new("PATHEXT")));
    assert!(!BuiltIns::UNIX.passes(OsStr::new("SHELF_THEME")));
}

#[test]
fn a_name_that_starts_with_lc_passes_only_when_it_is_a_locale_category() {
    assert!(BuiltIns::UNIX.passes(OsStr::new("LC_ALL")));
    assert!(BuiltIns::UNIX.passes(OsStr::new("LC_CTYPE")));
    assert!(BuiltIns::UNIX.passes(OsStr::new("LC_PAPER")));
    assert!(!BuiltIns::UNIX.passes(OsStr::new("LC_TERMINAL")));
    assert!(!BuiltIns::UNIX.passes(OsStr::new("LC_")));
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
    assert_eq!(BuiltIns::current(), expected);
}
