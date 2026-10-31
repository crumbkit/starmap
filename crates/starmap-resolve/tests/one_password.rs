//! Tests of the `one-password` feature. Each test writes a fake `op`
//! program, a POSIX `sh` script, into a directory of its own and passes its
//! path to `OnePassword::new`, so no test runs the real 1Password CLI or
//! reaches an account (PROC-2).

#![cfg(all(unix, feature = "one-password"))]

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use starmap::{Mode, Secret, SecretRef, Spec};
use starmap_resolve::{
    AppName, Context, Lookup, OnePassword, Places, ResolveProblem, Resolver, SecretError,
    WorkspaceName, resolve,
};

/// The reference that every test reads.
const REFERENCE: &str = "op://Personal/shelf mirror/token";

/// A deadline far enough away that only a fake that never ends reaches it.
const PATIENT: Duration = Duration::from_secs(60);

/// Writes a fake `op` program whose body is `script` into a new directory
/// for the test `test`, and returns the program's path. The directory also
/// receives the file `args`, where a fake may record its arguments.
#[expect(
    clippy::unwrap_used,
    reason = "a fixture that cannot be written fails the test"
)]
fn fake_op(test: &str, script: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("one-password")
        .join(test);
    if directory.exists() {
        fs::remove_dir_all(&directory).unwrap();
    }
    fs::create_dir_all(&directory).unwrap();
    let program = directory.join("op");
    let script = script.replace("{dir}", directory.to_str().unwrap());
    fs::write(&program, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    program
}

/// Reads [`REFERENCE`] through `program` with `deadline`.
#[expect(clippy::unwrap_used, reason = "the reference is a valid constant")]
fn read(program: PathBuf, deadline: Instant) -> Result<Secret<String>, SecretError> {
    let reference = SecretRef::OnePassword(REFERENCE.parse().unwrap());
    OnePassword::new(program).resolve(Lookup::Reference(&reference), deadline)
}

/// Returns the text a secret holds.
#[expect(
    clippy::disallowed_methods,
    reason = "the tests check the text that op read returned"
)]
fn text(secret: &Secret<String>) -> &str {
    secret.expose()
}

#[test]
fn op_read_receives_the_reference_as_one_argument_and_returns_its_output_exactly() {
    let program = fake_op(
        "success",
        r#"printf '%s\n' "$@" > '{dir}/args'
printf ' s3cret value\n'"#,
    );
    let secret = read(program.clone(), Instant::now() + PATIENT).unwrap();
    assert_eq!(text(&secret), " s3cret value\n");
    let args = fs::read_to_string(program.with_file_name("args")).unwrap();
    assert_eq!(args, format!("read\n--no-newline\n--\n{REFERENCE}\n"));
}

#[test]
fn op_read_has_no_standard_input_to_wait_on() {
    let program = fake_op(
        "stdin",
        r#"if read -r line; then exit 9; fi
printf 'token'"#,
    );
    let secret = read(program, Instant::now() + PATIENT).unwrap();
    assert_eq!(text(&secret), "token");
}

#[test]
fn a_signed_out_account_reads_as_a_locked_store() {
    let program = fake_op(
        "locked",
        r"echo '[ERROR] 2026/10/31 12:00:00 You are not currently signed in. Please run `op signin --help` for instructions' >&2
exit 1",
    );
    let error = read(program, Instant::now() + PATIENT).unwrap_err();
    assert!(matches!(error, SecretError::Locked), "{error:?}");
}

#[test]
fn a_missing_item_reads_as_not_found() {
    let program = fake_op(
        "missing",
        r#"echo '[ERROR] 2026/10/31 12:00:00 could not read secret '"'"'op://Personal/shelf mirror/token'"'"': could not get item Personal/shelf mirror: "shelf mirror" isn'"'"'t an item in the "Personal" vault. Specify the item with its UUID, name, or domain.' >&2
exit 1"#,
    );
    let error = read(program, Instant::now() + PATIENT).unwrap_err();
    assert!(matches!(error, SecretError::NotFound), "{error:?}");
}

#[test]
fn a_missing_item_whose_name_says_locked_reads_as_not_found() {
    let program = fake_op(
        "missing-locked",
        r#"echo '[ERROR] 2026/10/31 12:00:00 could not read secret '"'"'op://Personal/locked token/token'"'"': could not get item Personal/locked token: "locked token" isn'"'"'t an item in the "Personal" vault. Specify the item with its UUID, name, or domain.' >&2
exit 1"#,
    );
    let error = read(program, Instant::now() + PATIENT).unwrap_err();
    assert!(matches!(error, SecretError::NotFound), "{error:?}");
}

#[test]
fn a_dismissed_prompt_reads_as_denied() {
    let program = fake_op(
        "dismissed",
        r"echo '[ERROR] 2026/10/31 12:00:00 authorization prompt dismissed, please try again' >&2
exit 1",
    );
    let error = read(program, Instant::now() + PATIENT).unwrap_err();
    assert!(matches!(error, SecretError::Denied), "{error:?}");
}

#[test]
fn any_other_failure_names_the_exit_status_and_the_last_line_op_printed() {
    let program = fake_op(
        "other",
        r"echo 'first line' >&2
echo '[ERROR] 2026/10/31 12:00:00 something unexpected' >&2
exit 3",
    );
    let error = read(program, Instant::now() + PATIENT).unwrap_err();
    let SecretError::Program(error) = error else {
        panic!("{error:?}");
    };
    let message = error.to_string();
    assert!(message.contains("exit status: 3"), "{message}");
    assert!(
        message.ends_with("[ERROR] 2026/10/31 12:00:00 something unexpected"),
        "{message}"
    );
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test waits past the fake's own sleep to see that the kill stopped it"
)]
fn op_read_that_outlives_the_deadline_is_stopped() {
    let program = fake_op("slow", "sleep 1; touch '{dir}/survived'");
    let error = read(program.clone(), Instant::now() + Duration::from_millis(200)).unwrap_err();
    assert!(matches!(error, SecretError::Deadline), "{error:?}");
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!program.with_file_name("survived").exists());
}

#[test]
fn a_deadline_that_has_passed_starts_no_program() {
    // Starting a program that does not exist would fail as a program
    // failure, so Deadline shows that nothing was started.
    let program = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("no-such-op");
    let error = read(program, Instant::now()).unwrap_err();
    assert!(matches!(error, SecretError::Deadline), "{error:?}");
}

#[test]
fn output_over_the_limit_is_refused() {
    let program = fake_op("long", "exec cat /dev/zero");
    let error = read(program, Instant::now() + PATIENT).unwrap_err();
    let SecretError::Program(error) = error else {
        panic!("{error:?}");
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn output_that_is_not_unicode_is_refused() {
    let program = fake_op("bytes", r"printf '\377'");
    let error = read(program, Instant::now() + PATIENT).unwrap_err();
    let SecretError::Program(error) = error else {
        panic!("{error:?}");
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn a_program_that_does_not_exist_is_a_program_failure() {
    let program = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("no-such-op");
    let error = read(program, Instant::now() + PATIENT).unwrap_err();
    let SecretError::Program(error) = error else {
        panic!("{error:?}");
    };
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn a_generated_lookup_is_left_to_another_resolver() {
    let program = fake_op("generated", "touch '{dir}/ran'");
    let workspace: WorkspaceName = "shelf-dev".parse().unwrap();
    let app: AppName = "shelf".parse().unwrap();
    let lookup = Lookup::Generated {
        workspace: &workspace,
        app: &app,
        name: &"SHELF_SESSION_KEY".parse().unwrap(),
        bytes: starmap::SecretBytes::new(32).unwrap(),
    };
    let error = OnePassword::new(program.clone())
        .resolve(lookup, Instant::now() + PATIENT)
        .unwrap_err();
    assert!(matches!(error, SecretError::Unsupported), "{error:?}");
    assert!(!program.with_file_name("ran").exists());
}

#[test]
fn resolution_binds_a_secret_variable_from_op_read_and_reports_a_locked_store_against_it() {
    let spec = Spec::parse(
        r#"
[variables.SHELF_MIRROR_TOKEN]
purpose = "The token shelf sends to the catalogue mirror."
type = "string"
unset = "required"
show = "withheld"
source = "secret"
bind = { one-password = "op://Personal/shelf mirror/token" }
"#,
    )
    .unwrap();
    let caller = starmap::Bindings::new();
    let name = "SHELF_MIRROR_TOKEN".parse().unwrap();

    let found = OnePassword::new(fake_op("resolve-found", "printf 'token'"));
    let context = Context::new(
        Places::new(PathBuf::from("/checkout")),
        &caller,
        Instant::now() + PATIENT,
    )
    .resolver(&found);
    let resolution = resolve(&spec, Mode::Local, &context);
    assert!(resolution.failures.is_empty(), "{:?}", resolution.failures);
    let Some(starmap::Bound::Secret(secret)) = resolution.bindings.get(&name) else {
        panic!("{:?}", resolution.bindings);
    };
    assert_eq!(text(secret), "token");

    let locked = OnePassword::new(fake_op(
        "resolve-locked",
        "echo 'You are not currently signed in.' >&2; exit 1",
    ));
    let context = Context::new(
        Places::new(PathBuf::from("/checkout")),
        &caller,
        Instant::now() + PATIENT,
    )
    .resolver(&locked);
    let resolution = resolve(&spec, Mode::Local, &context);
    let [failure] = resolution.failures.as_slice() else {
        panic!("{:?}", resolution.failures);
    };
    assert_eq!(failure.name, name);
    assert!(
        matches!(failure.problem, ResolveProblem::Secret(SecretError::Locked)),
        "{:?}",
        failure.problem
    );
}
