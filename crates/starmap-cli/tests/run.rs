//! End-to-end tests of `starmap run`. Each test runs the built binary in a
//! checkout of its own or in `tests/fixtures/run`, with a cleared
//! environment, `--no-input` and a state directory of its own. The specs
//! declare no 1Password bind and no generated value, so no test runs `op`
//! or creates a Keychain item.

#![cfg(test)]

use std::path::Path;

use snapbox::cmd::{Command, OutputAssert};

/// The directory that holds the fixtures of `run`, relative to this
/// package.
const FIXTURES: &str = "tests/fixtures/run";

/// The base spec of the fixtures, which a test writes into a checkout of
/// its own.
const SPEC: &str = include_str!("fixtures/run/starmap.toml");

/// Runs the built `starmap` binary with `args` in `dir`, with a cleared
/// environment plus `env` and its usage log in a temporary directory.
fn starmap(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> OutputAssert {
    let state = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_starmap"))
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("XDG_STATE_HOME", state.path())
        .env("PATH", "/usr/bin:/bin")
        .envs(env.iter().copied())
        .assert()
}

/// Returns a checkout named `shelf` in a temporary directory with the base
/// spec and `dotenv` as its `.env`.
fn checkout(dotenv: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("starmap.toml"), SPEC).unwrap();
    std::fs::write(root.path().join(".env"), dotenv).unwrap();
    root
}

/// The script that prints the two settings of the base spec.
const PRINT: &str = r#"printf '%s %s' "$SHELF_PAGE_SIZE" "$SHELF_DEBUG""#;

#[test]
fn the_program_receives_set_over_the_shell_over_dotenv_over_the_default() {
    let dir = checkout("SHELF_PAGE_SIZE=30\nSHELF_DEBUG=true\n");
    let run = |args: &[&str], env: &[(&str, &str)]| {
        let mut words = vec!["run", "--no-input"];
        words.extend_from_slice(args);
        words.extend_from_slice(&["--", "sh", "-c", PRINT]);
        starmap(dir.path(), &words, env).code(0)
    };

    run(&[], &[]).stdout_eq("30 true");
    run(&[], &[("SHELF_PAGE_SIZE", "40")]).stdout_eq("40 true");
    run(
        &["--set", "SHELF_PAGE_SIZE=50"],
        &[("SHELF_PAGE_SIZE", "40")],
    )
    .stdout_eq("50 true");

    std::fs::remove_file(dir.path().join(".env")).unwrap();
    run(&[], &[]).stdout_eq("20 false");
}

#[test]
fn an_undeclared_variable_in_dotenv_does_not_reach_the_program() {
    let dir = checkout("SHELF_THEME=dark\n");
    starmap(
        dir.path(),
        &[
            "run",
            "--no-input",
            "--",
            "sh",
            "-c",
            r#"printf '%s' "${SHELF_THEME-unset}""#,
        ],
        &[],
    )
    .code(0)
    .stdout_eq("unset");
}

#[test]
fn the_program_exit_code_becomes_the_exit_code_of_starmap() {
    starmap(
        Path::new(FIXTURES),
        &["run", "--no-input", "--", "sh", "-c", "exit 7"],
        &[],
    )
    .code(7);
}

#[test]
fn a_program_that_a_signal_ends_exits_with_128_plus_the_signal() {
    starmap(
        Path::new(FIXTURES),
        &["run", "--no-input", "--", "sh", "-c", "kill -TERM $$"],
        &[],
    )
    .code(143);
}

#[test]
fn a_secret_in_dotenv_is_refused_without_printing_its_value_and_starts_nothing() {
    let run = starmap(
        &Path::new(FIXTURES).join("dotenv-secret"),
        &["run", "--no-input", "--", "sh", "-c", "echo started"],
        &[],
    )
    .code(1)
    .stdout_eq("");
    let stderr = String::from_utf8_lossy(&run.get_output().stderr).into_owned();
    assert!(stderr.contains("starmap::dotenv::secret"), "{stderr}");
    assert!(stderr.contains("SHELF_UPSTREAM_TOKEN"), "{stderr}");
    assert!(!stderr.contains("a-fixture-value"), "{stderr}");
}

#[test]
fn a_secret_in_the_shell_reaches_the_program_and_never_its_arguments() {
    starmap(
        Path::new(FIXTURES),
        &[
            "run",
            "--no-input",
            "--",
            "sh",
            "-c",
            r#"printf '%s' "$SHELF_UPSTREAM_TOKEN""#,
        ],
        &[("SHELF_UPSTREAM_TOKEN", "a-test-token")],
    )
    .code(0)
    .stdout_eq("a-test-token");
}

#[test]
fn set_for_a_secret_is_refused() {
    let run = starmap(
        Path::new(FIXTURES),
        &[
            "run",
            "--no-input",
            "--set",
            "SHELF_UPSTREAM_TOKEN=a-test-token",
            "--",
            "/usr/bin/true",
        ],
        &[],
    )
    .code(1);
    let stderr = String::from_utf8_lossy(&run.get_output().stderr).into_owned();
    assert!(stderr.contains("starmap::variable::secret"), "{stderr}");
    assert!(!stderr.contains("a-test-token"), "{stderr}");
}

#[test]
fn a_required_secret_without_a_bind_is_refused_under_no_input() {
    let run = starmap(
        Path::new(FIXTURES),
        &[
            "run",
            "--no-input",
            "--spec",
            "unbound.toml",
            "--workspace",
            "starmap-test-run",
            "--",
            "/usr/bin/true",
        ],
        &[],
    )
    .code(1);
    let stderr = String::from_utf8_lossy(&run.get_output().stderr).into_owned();
    assert!(stderr.contains("starmap::secret::unbound"), "{stderr}");
    assert!(stderr.contains("starmap started nothing"), "{stderr}");
}

#[test]
fn json_is_refused_before_anything_starts() {
    let run = starmap(
        Path::new(FIXTURES),
        &["run", "--json", "--", "sh", "-c", "echo started"],
        &[],
    )
    .code(2);
    let stdout = String::from_utf8_lossy(&run.get_output().stdout).into_owned();
    assert!(stdout.contains("starmap::cli::usage"), "{stdout}");
    assert!(!stdout.contains("started\n"), "{stdout}");
}

#[test]
fn a_program_that_does_not_exist_fails_as_not_found() {
    let run = starmap(
        Path::new(FIXTURES),
        &["run", "--no-input", "--", "no-such-program-of-starmap"],
        &[],
    )
    .code(1);
    let stderr = String::from_utf8_lossy(&run.get_output().stderr).into_owned();
    assert!(stderr.contains("starmap::child::not_found"), "{stderr}");
}
