use std::fs;
use std::io::ErrorKind;

use super::read_capped;

#[test]
fn a_file_longer_than_the_limit_is_read_up_to_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.toml");
    fs::write(&path, "0123456789").unwrap();
    assert_eq!(read_capped(&path, 4).unwrap(), b"0123");
}

#[test]
fn a_file_shorter_than_the_limit_is_read_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("short.toml");
    fs::write(&path, "0123").unwrap();
    assert_eq!(read_capped(&path, 10).unwrap(), b"0123");
}

#[test]
fn a_missing_file_returns_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let error = read_capped(&dir.path().join("missing.toml"), 10).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::NotFound);
}
