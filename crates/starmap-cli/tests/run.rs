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
#[cfg(unix)]
fn set_refuses_a_value_that_is_not_unicode_and_starts_nothing() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;

    let state = tempfile::tempdir().unwrap();
    let assignment = OsString::from_vec(b"SHELF_PAGE_SIZE=3\xff".to_vec());
    let run = Command::new(env!("CARGO_BIN_EXE_starmap"))
        .args(["run", "--no-input", "--set"])
        .arg(assignment)
        .args(["--", "sh", "-c", "echo started"])
        .current_dir(FIXTURES)
        .env_clear()
        .env("XDG_STATE_HOME", state.path())
        .env("PATH", "/usr/bin:/bin")
        .assert()
        .failure();
    let output = run.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(stderr.contains("UTF-8"), "{stderr}");
    assert!(output.stdout.is_empty());
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
fn json_is_refused_after_the_spec_and_before_any_secret_or_the_program() {
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
fn json_prints_a_failure_of_dotenv_as_json() {
    let run = starmap(
        Path::new(FIXTURES),
        &[
            "run",
            "--json",
            "--spec",
            "dotenv-secret/starmap.toml",
            "--",
            "/usr/bin/true",
        ],
        &[],
    )
    .code(1);
    let stdout = String::from_utf8_lossy(&run.get_output().stdout).into_owned();
    assert!(stdout.contains("starmap::dotenv::secret"), "{stdout}");
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

#[test]
fn a_shown_value_that_does_not_fit_is_quoted_and_a_withheld_one_is_not() {
    let dir = checkout("");
    std::fs::write(
        dir.path().join("starmap.toml"),
        SPEC.replace(
            "type = \"string\"",
            "type = { string = { pattern = \"sk-[a-z0-9]+\" } }",
        ),
    )
    .unwrap();
    let run = starmap(
        dir.path(),
        &[
            "run",
            "--no-input",
            "--set",
            "SHELF_PAGE_SIZE=10\n00",
            "--",
            "/usr/bin/true",
        ],
        &[("SHELF_UPSTREAM_TOKEN", "pk-live-hidden")],
    )
    .code(1);
    let stderr = String::from_utf8_lossy(&run.get_output().stderr).into_owned();

    assert!(
        stderr.contains(
            "SHELF_PAGE_SIZE: the value is not a whole number (the value is \"10\\n00\")"
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains("SHELF_UPSTREAM_TOKEN: the value does not match the pattern sk-[a-z0-9]+"),
        "{stderr}"
    );
    assert!(!stderr.contains("pk-live-hidden"), "{stderr}");
}

/// The script that prints an undeclared variable, two built-in ones and a
/// declared setting.
const PRINT_ONLY_DECLARED: &str =
    r#"printf '%s %s %s %s' "${SHELF_THEME-unset}" "$LANG" "$LC_ALL" "$SHELF_PAGE_SIZE""#;

/// The shell of the tests of `--only-declared`: an undeclared variable, two
/// built-in ones and a declared setting.
const ONLY_DECLARED_SHELL: &[(&str, &str)] = &[
    ("SHELF_THEME", "dark"),
    ("LANG", "C"),
    ("LC_ALL", "POSIX"),
    ("SHELF_PAGE_SIZE", "40"),
];

#[test]
fn without_only_declared_an_undeclared_variable_of_the_shell_reaches_the_program() {
    starmap(
        Path::new(FIXTURES),
        &["run", "--no-input", "--", "sh", "-c", PRINT_ONLY_DECLARED],
        ONLY_DECLARED_SHELL,
    )
    .code(0)
    .stdout_eq("dark C POSIX 40");
}

#[test]
fn only_declared_passes_declared_values_and_built_in_variables_and_nothing_else() {
    starmap(
        Path::new(FIXTURES),
        &[
            "run",
            "--no-input",
            "--only-declared",
            "--",
            "sh",
            "-c",
            PRINT_ONLY_DECLARED,
        ],
        ONLY_DECLARED_SHELL,
    )
    .code(0)
    .stdout_eq("unset C POSIX 40");
}

#[test]
fn only_declared_finds_the_program_on_the_path_of_the_shell() {
    // The harness sets PATH, and the program `sh` has no directory, so the
    // run starts only when PATH reaches the search for the program.
    starmap(
        Path::new(FIXTURES),
        &[
            "run",
            "--no-input",
            "--only-declared",
            "--",
            "sh",
            "-c",
            r#"printf '%s' "$PATH""#,
        ],
        &[],
    )
    .code(0)
    .stdout_eq("/usr/bin:/bin");
}

#[test]
fn only_declared_passes_a_default_and_each_member_of_a_family_that_is_set() {
    let root = tempfile::tempdir().unwrap();
    let spec = format!(
        "{SPEC}\n[families.SHELF_DB_URL]\npurpose = \"The database of one region, keyed by the region's code.\"\ntype = \"url\"\nunset = \"optional\"\nshow = \"shown\"\nsource = \"setting\"\n"
    );
    std::fs::write(root.path().join("starmap.toml"), spec).unwrap();
    starmap(
        root.path(),
        &[
            "run",
            "--no-input",
            "--only-declared",
            "--",
            "sh",
            "-c",
            r#"printf '%s %s %s' "$SHELF_DEBUG" "$SHELF_DB_URL_EU" "${SHELF_DB-unset}""#,
        ],
        &[
            ("SHELF_DB_URL_EU", "https://eu.example/"),
            ("SHELF_DB", "undeclared"),
        ],
    )
    .code(0)
    .stdout_eq("false https://eu.example/ unset");
}

#[test]
fn only_declared_leaves_a_declared_built_in_variable_unset_when_the_check_does() {
    // The spec declares the built-in name TZ as optional, and an empty
    // value counts as unset, so the check leaves TZ unset although the
    // shell sets it. HOME is built in and undeclared, so it still passes.
    let root = tempfile::tempdir().unwrap();
    let spec = format!(
        "{SPEC}\n[variables.TZ]\npurpose = \"The time zone shelf prints dates in.\"\ntype = \"string\"\nunset = \"optional\"\nshow = \"shown\"\nsource = \"inherited\"\n"
    );
    std::fs::write(root.path().join("starmap.toml"), spec).unwrap();
    starmap(
        root.path(),
        &[
            "run",
            "--no-input",
            "--only-declared",
            "--",
            "sh",
            "-c",
            r#"printf '%s %s' "${TZ-unset}" "$HOME""#,
        ],
        &[("TZ", ""), ("HOME", "/home/shelf")],
    )
    .code(0)
    .stdout_eq("unset /home/shelf");
}

#[test]
fn a_variable_that_reads_its_presence_reaches_the_program_as_it_was_set_even_when_empty() {
    let root = tempfile::tempdir().unwrap();
    let spec = format!(
        "{SPEC}\n[variables.NO_COLOR]\npurpose = \"Turns colour off when it is set.\"\ntype = \"boolean\"\npresence = true\nunset = \"optional\"\nshow = \"shown\"\nsource = \"inherited\"\n"
    );
    std::fs::write(root.path().join("starmap.toml"), spec).unwrap();
    let run = |env: &[(&str, &str)]| {
        let script = r#"printf '%s' "${NO_COLOR-unset}""#;
        let words = [
            "run",
            "--no-input",
            "--only-declared",
            "--",
            "sh",
            "-c",
            script,
        ];
        starmap(root.path(), &words, env).code(0)
    };

    run(&[("NO_COLOR", "")]).stdout_eq("");
    run(&[("NO_COLOR", "anything")]).stdout_eq("anything");
    run(&[]).stdout_eq("unset");

    std::fs::write(root.path().join(".env"), "NO_COLOR=\n").unwrap();
    run(&[]).stdout_eq("");
}
