use std::fs;
use std::io::ErrorKind;

use super::{read_capped, rust_files, write_new};

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

#[test]
fn the_rust_files_under_a_directory_are_listed_in_path_order() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("books")).unwrap();
    for file in ["main.rs", "books.rs", "books/remove.rs", "notes.txt"] {
        fs::write(dir.path().join(file), "").unwrap();
    }
    let found = rust_files(dir.path()).unwrap();
    let names: Vec<_> = found
        .iter()
        .map(|path| path.strip_prefix(dir.path()).unwrap().to_str().unwrap())
        .collect();
    assert_eq!(names, ["books/remove.rs", "books.rs", "main.rs"]);
}

#[test]
fn a_new_file_is_written_and_an_existing_file_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("starmap.toml");
    write_new(&path, "first").unwrap();
    let error = write_new(&path, "second").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&path).unwrap(), "first");
}
