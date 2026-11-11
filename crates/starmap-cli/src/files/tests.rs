use std::fs;
use std::io::ErrorKind;

use super::{read_capped, replace, rust_files, write_new};

#[test]
fn a_replaced_file_holds_the_new_text_and_no_other_file_is_left() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clippy.toml");
    fs::write(&path, "first").unwrap();
    replace(&path, "second").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

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

#[test]
fn a_file_left_by_a_killed_run_does_not_stop_a_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clippy.toml");
    fs::write(&path, "first").unwrap();
    fs::write(dir.path().join("clippy.toml.starmap-new"), "half").unwrap();
    replace(&path, "second").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn replacing_a_symbolic_link_writes_the_file_it_names_and_keeps_the_link_and_the_mode() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("shared")).unwrap();
    let target = dir.path().join("shared/clippy.toml");
    fs::write(&target, "first").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    let link = dir.path().join("clippy.toml");
    std::os::unix::fs::symlink("shared/clippy.toml", &link).unwrap();

    replace(&link, "second").unwrap();

    assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(&target).unwrap(), "second");
    let mode = fs::metadata(&target).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
    assert_eq!(fs::read_dir(dir.path().join("shared")).unwrap().count(), 1);
}
