//! An end-to-end test of `starmap run` with 1Password binds. It runs the
//! built binary with a fake `op` first on `PATH`, a shell script that
//! prints a canary the test generates at run time, and with fresh `HOME`,
//! `TMPDIR` and state directories. No test runs the real 1Password CLI.

#![cfg(unix)]

use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use snapbox::cmd::Command;

/// The spec of the test: three secrets that one `op inject` reads.
const SPEC: &str = r#"
[variables.SHELF_MIRROR_TOKEN]
purpose = "The token shelf sends to the catalogue mirror."
type = "string"
unset = "required"
show = "withheld"
source = "secret"
bind = { one-password = "op://Personal/shelf-mirror/token" }

[variables.SHELF_API_KEY]
purpose = "The key shelf sends to the catalogue API."
type = "string"
unset = "required"
show = "withheld"
source = "secret"
bind = { one-password = "op://Personal/catalogue/key" }

[variables.SHELF_SIGNING_KEY]
purpose = "The key that signs shelf's exports."
type = "string"
unset = "required"
show = "withheld"
source = "secret"
bind = { one-password = "op://Personal/exports/signing" }
"#;

/// The awk program that plays `op inject`: it replaces each
/// `{{ op://… }}` of the template with the canary, a hyphen and the
/// reference's last part.
const INJECT_AWK: &str = r#"{
    rest = $0
    while (match(rest, /[{][{] op:[^}]* [}][}]/)) {
        reference = substr(rest, RSTART + 3, RLENGTH - 6)
        n = split(reference, parts, "/")
        printf "%s%s-%s", substr(rest, 1, RSTART - 1), "{canary}", parts[n]
        rest = substr(rest, RSTART + RLENGTH)
    }
    printf "%s", rest
}"#;

/// Returns a canary of 32 random hex digits. Each `RandomState` draws its
/// keys from the operating system's random source.
fn canary() -> String {
    let word = || RandomState::new().build_hasher().finish();
    format!("STARMAP-CANARY-{:016x}{:016x}", word(), word())
}

/// Returns the paths of every file below `directory`.
#[expect(
    clippy::unwrap_used,
    reason = "a directory that cannot be listed fails the test"
)]
fn files(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            found.push(path);
        }
    }
    found
}

#[test]
fn starmap_run_reads_three_secrets_with_one_op_inject_and_leaves_no_value_behind() {
    let canary = canary();
    let fake = tempfile::tempdir().unwrap();
    let dir = fake.path().display().to_string();
    let program = fake.path().join("op");
    fs::write(
        &program,
        format!(
            r#"#!/bin/sh
echo "$1" >> '{dir}/runs'
printf '%s\n' "$@" >> '{dir}/args'
env >> '{dir}/env'
cat > '{dir}/template'
LC_ALL=C awk -f '{dir}/inject.awk' '{dir}/template'
"#
        ),
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        fake.path().join("inject.awk"),
        INJECT_AWK.replace("{canary}", &canary),
    )
    .unwrap();

    let checkout = tempfile::tempdir().unwrap();
    fs::write(checkout.path().join("starmap.toml"), SPEC).unwrap();
    let (home, tmp, state) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    // The program prints the length of each value, so the test sees that
    // the values arrived without the program printing them.
    let expected = ["token", "key", "signing"]
        .map(|part| (canary.len() + 1 + part.len()).to_string())
        .join(" ");
    let output = Command::new(env!("CARGO_BIN_EXE_starmap"))
        .args([
            "run",
            "--no-input",
            "--",
            "sh",
            "-c",
            r#"printf '%s %s %s' "${#SHELF_MIRROR_TOKEN}" "${#SHELF_API_KEY}" "${#SHELF_SIGNING_KEY}""#,
        ])
        .current_dir(checkout.path())
        .env_clear()
        .env("PATH", format!("{dir}:/usr/bin:/bin"))
        .env("HOME", home.path())
        .env("TMPDIR", tmp.path())
        .env("XDG_STATE_HOME", state.path())
        .assert()
        .code(0)
        .stdout_eq(expected)
        .get_output()
        .clone();

    assert_eq!(
        fs::read_to_string(fake.path().join("runs")).unwrap(),
        "inject\n"
    );
    let mut places = vec![
        ("starmap's output".to_owned(), output.stdout.clone()),
        ("starmap's errors".to_owned(), output.stderr.clone()),
    ];
    for directory in [
        fake.path(),
        home.path(),
        tmp.path(),
        state.path(),
        checkout.path(),
    ] {
        for path in files(directory) {
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            // The fake's own awk script holds the canary, as it prints it.
            if name != "inject.awk" {
                places.push((path.display().to_string(), fs::read(&path).unwrap()));
            }
        }
    }
    for (place, bytes) in places {
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(&canary), "the canary appears in {place}");
    }
}
