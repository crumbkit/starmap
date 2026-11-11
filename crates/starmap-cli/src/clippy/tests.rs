use std::fs;
use std::path::{Path, PathBuf};

use crumbkit::Failure as _;
use snapbox::file;

use super::{BANNED, Ban, ClippyFailure, add, missing, open};
use crate::codes::CLIPPY_INVALID;

/// Returns `text`, the text of a clippy.toml, with an entry added for each
/// banned method that it lacks.
fn edited(text: &str) -> String {
    let mut document = text.parse().unwrap();
    let lacking = missing(&document).unwrap();
    add(&mut document, &lacking);
    document.to_string()
}

/// Returns the banned methods that `text`, the text of a clippy.toml,
/// lacks.
fn lacking(text: &str) -> Vec<&'static str> {
    missing(&text.parse().unwrap()).unwrap()
}

#[test]
fn an_empty_file_gains_one_entry_for_each_banned_method() {
    snapbox::assert_data_eq!(
        edited(""),
        file!["../../../starmap/tests/example/clippy.toml"]
    );
}

#[test]
fn the_entries_follow_the_existing_ones_and_every_comment_stays() {
    let text = "\
# The team's lints.
too-many-lines-threshold = 150 # agreed in March

disallowed-methods = [
  # Exit from main.
  { path = \"std::process::exit\", reason = \"return from main\" }, # keep
  # The last comment.
]

[[disallowed-types]]
path = \"std::process::Child\"
";
    let edited = edited(text);
    let (before, after) = text.split_once("  # The last comment.\n").unwrap();
    let (edited_before, rest) = edited.split_at(before.len());
    assert_eq!(edited_before, before);
    let (added, edited_after) = rest.split_once("  # The last comment.\n").unwrap();
    assert_eq!(edited_after, after);
    // Each new entry takes the indentation of the entry before it.
    let lines: Vec<&str> = added.lines().collect();
    assert_eq!(lines.len(), 4);
    for (line, path) in lines.iter().zip(BANNED) {
        assert!(
            line.starts_with(&format!("  {{ path = \"{path}\", reason = \"")),
            "{line}"
        );
        assert!(line.ends_with("\" },"), "{line}");
    }
    assert_eq!(lacking(&edited), [""; 0]);
}

#[test]
fn a_comment_beside_a_last_entry_without_a_comma_stays_beside_it() {
    let edited = edited("disallowed-methods = [\n\t\"std::env::var\" # mine\n]\n");
    assert!(
        edited.starts_with(
            "disallowed-methods = [\n\t\"std::env::var\", # mine\n\t{ path = \"std::env::var_os\", reason = "
        ),
        "{edited}"
    );
    assert!(edited.ends_with("\" },\n]\n"), "{edited}");
    assert_eq!(lacking(&edited), [""; 0]);
}

#[test]
fn an_array_on_one_line_stays_on_one_line() {
    let edited = edited("disallowed-methods = [\"std::process::exit\"]\n");
    assert!(
        edited.starts_with(
            "disallowed-methods = [\"std::process::exit\", { path = \"std::env::var\", reason = "
        ),
        "{edited}"
    );
    assert_eq!(edited.lines().count(), 1);
    assert_eq!(lacking(&edited), [""; 0]);
}

#[test]
fn an_entry_counts_as_a_string_or_as_a_table_with_a_path() {
    let text = "disallowed-methods = [\n    \"std::env::var\",\n    { path = \"std::env::vars_os\", replacement = \"crate::env\" },\n]\n";
    assert_eq!(lacking(text), ["std::env::var_os", "std::env::vars"]);
    let edited = edited(text);
    assert!(edited.starts_with(text.trim_end_matches("]\n")), "{edited}");
    assert_eq!(edited.matches("std::env::var\"").count(), 1);
    assert_eq!(lacking(&edited), [""; 0]);
}

#[test]
fn a_file_that_lists_the_methods_as_tables_gains_a_table_for_each_missing_one() {
    let text = "[[disallowed-methods]]\npath = \"std::env::var\"\nreason = \"mine\"\n";
    assert_eq!(lacking(text).len(), 3);
    let edited = edited(text);
    assert!(edited.starts_with(text), "{edited}");
    assert_eq!(edited.matches("[[disallowed-methods]]").count(), 4);
    assert_eq!(lacking(&edited), [""; 0]);
}

#[test]
fn a_file_with_every_entry_is_left_as_it_is() {
    let text = edited("");
    assert_eq!(edited(&text), text);
}

/// Opens the one clippy.toml that Clippy reads for a crate whose source is
/// in `src` beside the spec in `dir`.
fn beside(dir: &Path) -> Result<Ban, ClippyFailure> {
    fs::create_dir_all(dir.join("src")).unwrap();
    let mut bans = open(&dir.join("starmap.toml"), &[dir.join("src")]);
    assert_eq!(bans.len(), 1);
    bans.remove(0)
}

/// Returns the failure of opening the ban beside a spec in a directory
/// whose clippy.toml holds `text`, and checks that the file is unchanged.
fn refused(text: &[u8]) -> ClippyFailure {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("clippy.toml");
    fs::write(&file, text).unwrap();
    let failure = beside(dir.path()).unwrap_err();
    assert_eq!(fs::read(&file).unwrap(), text);
    failure
}

#[test]
fn a_file_that_is_not_toml_is_refused_and_kept() {
    let failure = refused(b"disallowed-methods = [\n");
    assert_eq!(failure.code(), &CLIPPY_INVALID);
    assert!(
        failure.to_string().contains("is not valid TOML"),
        "{failure}"
    );
    let failure = refused(b"\xff");
    assert!(failure.to_string().ends_with("is not UTF-8 text"));
}

#[test]
fn a_disallowed_methods_that_is_not_an_array_is_refused_and_kept() {
    for text in [
        "disallowed-methods = \"std::env::var\"\n",
        "[disallowed-methods]\npath = \"std::env::var\"\n",
    ] {
        let failure = refused(text.as_bytes());
        assert_eq!(failure.code(), &CLIPPY_INVALID, "{text}");
        assert!(
            failure
                .to_string()
                .ends_with("sets disallowed-methods to a value that is not an array"),
            "{failure}"
        );
    }
}

#[test]
fn a_file_over_256_kib_is_refused_and_kept() {
    let failure = refused("#".repeat(256 * 1024 + 1).as_bytes());
    assert_eq!(failure.code(), &CLIPPY_INVALID);
    assert!(failure.to_string().ends_with("is over 256 KiB"));
}

/// Opens the bans for the source directories `sources` and the spec
/// `spec`, each a path below `root`, and returns each file as a path below
/// `root` with whether the file exists. It creates each source directory.
fn found(root: &Path, spec: &str, sources: &[&str]) -> Vec<(String, bool)> {
    let sources: Vec<PathBuf> = sources.iter().map(|src| root.join(src)).collect();
    for src in &sources {
        fs::create_dir_all(src).unwrap();
    }
    open(&root.join(spec), &sources)
        .into_iter()
        .map(|ban| {
            let Ban { file, exists, .. } = ban.unwrap();
            let file = file.strip_prefix(root).unwrap().display().to_string();
            (file, exists)
        })
        .collect()
}

/// Returns a directory with every symbolic link in its path resolved, as
/// the files above a spec are named.
fn root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    (dir, root)
}

/// Makes each directory of `crates` below `root` a crate, with a
/// Cargo.toml.
fn crates(root: &Path, crates: &[&str]) {
    for krate in crates {
        fs::create_dir_all(root.join(krate)).unwrap();
        fs::write(root.join(krate).join("Cargo.toml"), "").unwrap();
    }
}

#[test]
fn the_file_is_the_nearest_one_from_the_crate_up_to_the_repository_root() {
    let (_dir, root) = root();
    fs::create_dir(root.join(".git")).unwrap();
    crates(&root, &["crates/app"]);
    let found = || found(&root, "crates/app/starmap.toml", &["crates/app/src"]);
    // No file anywhere: init writes one beside the spec.
    assert_eq!(found(), [("crates/app/clippy.toml".to_owned(), false)]);
    fs::write(root.join("clippy.toml"), "").unwrap();
    assert_eq!(found(), [("clippy.toml".to_owned(), true)]);
    fs::write(root.join("crates/.clippy.toml"), "").unwrap();
    assert_eq!(found(), [("crates/.clippy.toml".to_owned(), true)]);
    // Clippy skips a directory that has the file's name.
    fs::create_dir(root.join("crates/app/.clippy.toml")).unwrap();
    assert_eq!(found(), [("crates/.clippy.toml".to_owned(), true)]);
    fs::write(root.join("crates/app/clippy.toml"), "").unwrap();
    assert_eq!(found(), [("crates/app/clippy.toml".to_owned(), true)]);
}

#[test]
fn each_crate_has_its_own_nearest_file_and_a_shared_file_is_listed_once() {
    let (_dir, root) = root();
    crates(&root, &["crates/a", "crates/b", "crates/c"]);
    let sources = ["crates/a/src", "crates/b/src", "crates/c/src"];
    // No file anywhere: init writes one beside the spec for every crate.
    assert_eq!(
        found(&root, "starmap.toml", &sources),
        [("clippy.toml".to_owned(), false)]
    );
    fs::write(root.join("clippy.toml"), "").unwrap();
    fs::write(root.join("crates/b/clippy.toml"), "").unwrap();
    assert_eq!(
        found(&root, "starmap.toml", &sources),
        [
            ("clippy.toml".to_owned(), true),
            ("crates/b/clippy.toml".to_owned(), true)
        ]
    );
}

#[test]
fn a_crate_outside_the_directory_of_the_spec_gains_a_file_in_its_own_directory() {
    let (_dir, root) = root();
    fs::create_dir(root.join(".git")).unwrap();
    fs::create_dir_all(root.join("config")).unwrap();
    crates(&root, &["app"]);
    assert_eq!(
        found(&root, "config/starmap.toml", &["app/src"]),
        [("app/clippy.toml".to_owned(), false)]
    );
}

#[test]
fn the_crate_of_a_source_file_is_the_nearest_directory_above_it_with_a_cargo_toml() {
    let (_dir, root) = root();
    crates(&root, &["crates/a", "crates/b"]);
    fs::write(root.join("crates/b/clippy.toml"), "").unwrap();
    // The scan of `src = ["crates"]` reads files in these directories.
    let sources = ["crates/a/src", "crates/b/src/net", "crates/b/tests"];
    assert_eq!(
        found(&root, "starmap.toml", &sources),
        [
            ("clippy.toml".to_owned(), false),
            ("crates/b/clippy.toml".to_owned(), true)
        ]
    );
    // A directory with no Cargo.toml above it is in the crate of the spec.
    assert_eq!(
        found(&root, "starmap.toml", &["scripts"]),
        [("clippy.toml".to_owned(), false)]
    );
}

#[test]
fn a_file_above_the_repository_root_is_not_read() {
    let (_dir, root) = root();
    fs::write(root.join("clippy.toml"), "").unwrap();
    crates(&root, &["repo/app"]);
    // A worktree and a submodule keep `.git` as a file.
    fs::write(root.join("repo/.git"), "gitdir: elsewhere\n").unwrap();
    assert_eq!(
        found(&root, "repo/app/starmap.toml", &["repo/app/src"]),
        [("repo/app/clippy.toml".to_owned(), false)]
    );
    assert_eq!(
        found(&root, "repo/starmap.toml", &["repo/app/src"]),
        [("repo/clippy.toml".to_owned(), false)]
    );
}

#[test]
fn without_a_repository_the_search_stops_at_the_directory_of_the_spec() {
    let (_dir, root) = root();
    fs::write(root.join("clippy.toml"), "").unwrap();
    crates(&root, &["app/core"]);
    assert_eq!(
        found(&root, "app/starmap.toml", &["app/core/src"]),
        [("app/clippy.toml".to_owned(), false)]
    );
    fs::write(root.join("app/.clippy.toml"), "").unwrap();
    assert_eq!(
        found(&root, "app/starmap.toml", &["app/core/src"]),
        [("app/.clippy.toml".to_owned(), true)]
    );
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_that_leads_to_no_file_or_to_itself_is_refused() {
    for target in ["missing.toml", "clippy.toml"] {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(target, dir.path().join("clippy.toml")).unwrap();
        let failure = beside(dir.path()).unwrap_err();
        assert_eq!(failure.code(), &CLIPPY_INVALID, "{target}");
        assert!(failure.to_string().contains("cannot be read"), "{failure}");
    }
}

#[cfg(unix)]
#[test]
fn a_file_that_is_a_symbolic_link_is_named_as_the_file_it_leads_to() {
    let (_dir, root) = root();
    fs::create_dir(root.join("shared")).unwrap();
    fs::write(root.join("shared/lints.toml"), "").unwrap();
    std::os::unix::fs::symlink("shared/lints.toml", root.join("clippy.toml")).unwrap();
    assert_eq!(
        found(&root, "starmap.toml", &["src"]),
        [("shared/lints.toml".to_owned(), true)]
    );
}

#[test]
fn no_file_is_opened_when_no_source_directory_holds_rust() {
    let dir = tempfile::tempdir().unwrap();
    assert!(open(&dir.path().join("starmap.toml"), &[]).is_empty());
}

#[test]
fn writing_adds_the_entries_to_the_file_and_then_nothing_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let spec = dir.path().join("starmap.toml");
    let ban = beside(dir.path()).unwrap();
    assert_eq!(ban.missing, BANNED);
    ban.write(&spec).unwrap();
    let ban = beside(dir.path()).unwrap();
    assert!(ban.exists);
    assert_eq!(ban.missing, [""; 0]);
    let written = fs::read_to_string(dir.path().join("clippy.toml")).unwrap();
    ban.write(&spec).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("clippy.toml")).unwrap(),
        written
    );
}
