//! Tests of reading a resolution's 1Password references with one
//! `op inject`. Each fake `op` records each run's subcommand in `runs`, its
//! arguments in `args` and its environment in `env`. An inject run keeps its
//! template in `template.<pid>`, so runs at the same time do not mix, and
//! appends it to `template`. The fake's values hold a canary that the test
//! generates at run time, which appears only in the fake's own files, `op`
//! and `inject.awk`.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a helper that meets a broken fixture fails the test"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use starmap::{Bound, Mode, SecretRef, Spec};
use starmap_resolve::{
    Context, Lookup, OnePassword, Places, Resolution, ResolveProblem, Resolver, SecretError,
    resolve,
};

use super::{PATIENT, fake_op, text};

/// The awk program that plays `op inject`. It replaces each `{{ op://… }}`
/// of the template with the canary, a hyphen and the reference's last part,
/// and the part `lines` with a value of three lines and `bytes` with a byte
/// that is not UTF-8.
const INJECT_AWK: &str = r#"{
    rest = $0
    while (match(rest, /[{][{] op:[^}]* [}][}]/)) {
        reference = substr(rest, RSTART + 3, RLENGTH - 6)
        n = split(reference, parts, "/")
        value = "{canary}-" parts[n]
        if (parts[n] == "lines") value = "{canary}\nline two\n"
        if (parts[n] == "bytes") value = "\377"
        printf "%s%s", substr(rest, 1, RSTART - 1), value
        rest = substr(rest, RSTART + RLENGTH)
    }
    printf "%s", rest
}"#;

/// Returns a canary of 32 random hex digits that no fixture holds.
fn canary() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).unwrap();
    let digits: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("STARMAP-CANARY-{digits}")
}

/// A fake `op` for the test `test`, which answers `op inject` with
/// `inject` and `op read` with `read`, and the canary it prints.
struct Fake {
    program: PathBuf,
    canary: String,
}

impl Fake {
    /// Writes the fake. `{inject}` in `inject` runs [`INJECT_AWK`] on the
    /// template, and `{read}` in `read` prints what `op read` would print
    /// for the reference.
    fn new(test: &str, inject: &str, read: &str) -> Self {
        let canary = canary();
        let script = format!(
            r#"echo "$1" >> '{{dir}}/runs'
printf '%s\n' "$@" >> '{{dir}}/args'
env >> '{{dir}}/env'
case "$1" in
inject)
  cat > "{{dir}}/template.$$"
  cat "{{dir}}/template.$$" >> '{{dir}}/template'
  {inject}
  ;;
read)
  {read}
  ;;
esac"#
        )
        .replace(
            "{inject}",
            "LC_ALL=C awk -f '{dir}/inject.awk' \"{dir}/template.$$\"",
        )
        .replace(
            "{read}",
            &format!(r#"reference="$4"; printf '%s-%s' '{canary}' "${{reference##*/}}""#),
        );
        let program = fake_op(test, &script);
        fs::write(
            program.with_file_name("inject.awk"),
            INJECT_AWK.replace("{canary}", &canary),
        )
        .unwrap();
        Self { program, canary }
    }

    /// Returns a fake whose `op inject` and `op read` both answer with the
    /// canary.
    fn answering(test: &str) -> Self {
        Self::new(test, "{inject}", "{read}")
    }

    /// Returns the file `name` beside the program, or an empty string.
    fn file(&self, name: &str) -> String {
        fs::read_to_string(self.program.with_file_name(name)).unwrap_or_default()
    }

    /// Returns each subcommand the fake ran, in order.
    fn runs(&self) -> Vec<String> {
        self.file("runs").lines().map(str::to_owned).collect()
    }

    /// Returns the value the fake gives the reference that ends in `part`.
    fn value(&self, part: &str) -> String {
        format!("{}-{part}", self.canary)
    }

    /// Resolves `spec` in local mode with this fake as the 1Password CLI,
    /// by `deadline`.
    fn resolve(&self, spec: &Spec, deadline: Instant) -> Resolution {
        let one_password = OnePassword::new(self.program.clone());
        let caller = starmap::Bindings::new();
        let context = Context::new(Places::new(PathBuf::from("/checkout")), &caller, deadline)
            .resolver(&one_password);
        resolve(spec, Mode::Local, &context)
    }

    /// Fails the test when the canary, or its hex or base64 form, appears
    /// in `text`.
    fn assert_absent(&self, place: &str, text: &str) {
        for form in [
            self.canary.clone(),
            hex(self.canary.as_bytes()),
            base64(self.canary.as_bytes()),
        ] {
            assert!(!text.contains(&form), "the canary appears in {place}");
        }
    }
}

impl Fake {
    /// Fails the test when the canary appears in the arguments or
    /// environment of any run of the fake, in an inject template, in any
    /// file beside the fake other than its own program and awk script, or
    /// in the debug output of `resolution`.
    fn assert_no_leak(&self, resolution: &Resolution) {
        self.assert_absent("the arguments of op", &self.file("args"));
        self.assert_absent("the environment of op", &self.file("env"));
        self.assert_absent("the template", &self.file("template"));
        self.assert_absent("the resolution's debug output", &format!("{resolution:?}"));
        // The fake's own program and awk script hold the canary, as they
        // print it. No other file does.
        for path in files(self.program.parent().unwrap()) {
            let name = path.file_name().unwrap().to_str().unwrap();
            if name == "op" || name == "inject.awk" {
                continue;
            }
            let bytes = fs::read(&path).unwrap();
            self.assert_absent(name, &String::from_utf8_lossy(&bytes));
        }
    }
}

/// Returns `bytes` as lower case hex digits.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Returns `bytes` in standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let word = chunk.iter().enumerate().fold(0_u32, |word, (i, byte)| {
            word | u32::from(*byte) << (16 - 8 * i)
        });
        for i in 0..4 {
            if i <= chunk.len() {
                let digit = (word >> (18 - 6 * i)) & 0x3f;
                text.push(char::from(ALPHABET[usize::try_from(digit).unwrap()]));
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// Returns a spec with one withheld secret for each `(name, reference)`.
fn spec(variables: &[(&str, &str)]) -> Spec {
    let text: String = variables
        .iter()
        .map(|(name, reference)| {
            format!(
                r#"
[variables.{name}]
purpose = "A secret the test reads."
type = "string"
unset = "required"
show = "withheld"
source = "secret"
bind = {{ one-password = "{reference}" }}
"#
            )
        })
        .collect();
    Spec::parse(&text).unwrap()
}

/// Returns the text bound to `name`.
fn bound(resolution: &Resolution, name: &str) -> String {
    match resolution.bindings.get(&name.parse().unwrap()) {
        Some(Bound::Secret(secret)) => text(secret).to_owned(),
        other => panic!("{name}: {other:?}"),
    }
}

/// Returns each failed variable and its problem, as text.
fn failures(resolution: &Resolution) -> Vec<(String, String)> {
    resolution
        .failures
        .iter()
        .map(|failure| (failure.name.to_string(), format!("{:?}", failure.problem)))
        .collect()
}

/// Returns the paths of every file below `directory`.
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

/// Three references a template can hold, the last of which has a value of
/// three lines.
const THREE: [(&str, &str); 3] = [
    ("SHELF_MIRROR_TOKEN", "op://Personal/shelf-mirror/token"),
    ("SHELF_API_KEY", "op://Dev_vault/catalogue_api.v2/key"),
    ("SHELF_CERTIFICATE", "op://Personal/shelf-tls/lines"),
];

#[test]
fn one_resolution_reads_every_reference_a_template_can_hold_with_one_op_inject() {
    let fake = Fake::answering("batch-one-run");
    let resolution = fake.resolve(&spec(&THREE), Instant::now() + PATIENT);

    assert!(resolution.failures.is_empty(), "{:?}", resolution.failures);
    assert_eq!(fake.runs(), ["inject"]);
    assert_eq!(
        bound(&resolution, "SHELF_MIRROR_TOKEN"),
        fake.value("token")
    );
    assert_eq!(bound(&resolution, "SHELF_API_KEY"), fake.value("key"));
    assert_eq!(
        bound(&resolution, "SHELF_CERTIFICATE"),
        format!("{}\nline two\n", fake.canary)
    );
}

#[test]
fn no_value_reaches_the_arguments_environment_input_or_any_file_of_op() {
    let fake = Fake::answering("batch-no-leak");
    let resolution = fake.resolve(&spec(&THREE), Instant::now() + PATIENT);
    assert!(resolution.failures.is_empty(), "{:?}", resolution.failures);

    assert_eq!(fake.file("args"), "inject\n");
    let template = fake.file("template");
    for (_, reference) in THREE {
        assert!(
            template.contains(&format!("{{{{ {reference} }}}}")),
            "{template}"
        );
    }
    fake.assert_no_leak(&resolution);
}

#[test]
fn a_reference_a_template_would_change_is_read_with_its_own_op_read() {
    let fake = Fake::answering("batch-unbatchable");
    let expanded = "op://Personal/${ITEM:-shelf}/token";
    let spaced = "op://Personal/shelf mirror/my field";
    let resolution = fake.resolve(
        &spec(&[
            ("SHELF_MIRROR_TOKEN", "op://Personal/shelf-mirror/token"),
            ("SHELF_API_KEY", "op://Personal/catalogue/key"),
            ("SHELF_EXPANDED", expanded),
            ("SHELF_SPACED", spaced),
        ]),
        Instant::now() + PATIENT,
    );

    assert!(resolution.failures.is_empty(), "{:?}", resolution.failures);
    assert_eq!(fake.runs(), ["inject", "read", "read"]);
    let template = fake.file("template");
    assert!(
        !template.contains('$') && !template.contains("my field"),
        "{template}"
    );
    let args = fake.file("args");
    assert!(args.contains(&format!("--\n{expanded}\n")), "{args}");
    assert!(args.contains(&format!("--\n{spaced}\n")), "{args}");
    assert_eq!(bound(&resolution, "SHELF_EXPANDED"), fake.value("token"));
    assert_eq!(bound(&resolution, "SHELF_SPACED"), fake.value("my field"));
    assert_eq!(bound(&resolution, "SHELF_API_KEY"), fake.value("key"));
    fake.assert_no_leak(&resolution);
}

#[test]
fn a_single_reference_is_read_with_op_read() {
    let fake = Fake::answering("batch-single");
    let resolution = fake.resolve(
        &spec(&[("SHELF_MIRROR_TOKEN", "op://Personal/shelf-mirror/token")]),
        Instant::now() + PATIENT,
    );
    assert!(resolution.failures.is_empty(), "{:?}", resolution.failures);
    assert_eq!(fake.runs(), ["read"]);
}

#[test]
fn two_variables_of_one_reference_each_receive_its_value_from_the_one_op_inject() {
    let fake = Fake::answering("batch-shared");
    let resolution = fake.resolve(
        &spec(&[
            ("SHELF_MIRROR_TOKEN", "op://Personal/shelf-mirror/token"),
            (
                "SHELF_MIRROR_TOKEN_AGAIN",
                "op://Personal/shelf-mirror/token",
            ),
        ]),
        Instant::now() + PATIENT,
    );
    assert!(resolution.failures.is_empty(), "{:?}", resolution.failures);
    assert_eq!(fake.runs(), ["inject"]);
    assert_eq!(
        bound(&resolution, "SHELF_MIRROR_TOKEN"),
        fake.value("token")
    );
    assert_eq!(
        bound(&resolution, "SHELF_MIRROR_TOKEN_AGAIN"),
        fake.value("token")
    );
}

#[test]
fn a_locked_or_dismissed_batch_fails_every_reference_without_running_op_again() {
    let cases = [
        ("You are not currently signed in.", "Locked"),
        ("authorization prompt dismissed, please try again", "Denied"),
    ];
    for (index, (message, problem)) in cases.into_iter().enumerate() {
        let fake = Fake::new(
            &format!("batch-stopped-{index}"),
            &format!("echo '[ERROR] {message}' >&2; exit 1"),
            "{read}",
        );
        let mut variables = THREE.to_vec();
        variables.push(("SHELF_EXPANDED", "op://Personal/$ITEM/token"));
        let resolution = fake.resolve(&spec(&variables), Instant::now() + PATIENT);

        assert_eq!(fake.runs(), ["inject"], "{message}");
        let failed = failures(&resolution);
        assert_eq!(failed.len(), 4, "{failed:?}");
        for (name, shown) in failed {
            assert_eq!(shown, format!("Secret({problem})"), "{name}");
        }
    }
}

#[test]
fn a_batch_that_op_reports_not_found_reads_each_reference_on_its_own() {
    let fake = Fake::new(
        "batch-not-found",
        r#"echo '[ERROR] "catalogue_api.v2" isn'"'"'t an item in the "Dev vault" vault.' >&2; exit 1"#,
        "{read}",
    );
    let resolution = fake.resolve(&spec(&THREE), Instant::now() + PATIENT);
    assert!(resolution.failures.is_empty(), "{:?}", resolution.failures);
    assert_eq!(fake.runs(), ["inject", "read", "read", "read"]);
    assert_eq!(bound(&resolution, "SHELF_API_KEY"), fake.value("key"));
    fake.assert_no_leak(&resolution);
}

#[test]
fn an_unrecognised_batch_failure_is_final_and_names_the_last_line_op_printed() {
    let fake = Fake::new(
        "batch-unrecognised",
        "echo 'first line' >&2; echo '[ERROR] the template has a syntax error' >&2; exit 1",
        "{read}",
    );
    let resolution = fake.resolve(&spec(&THREE), Instant::now() + PATIENT);

    assert_eq!(fake.runs(), ["inject"]);
    assert_eq!(resolution.failures.len(), 3);
    for failure in &resolution.failures {
        let ResolveProblem::Secret(SecretError::Program(error)) = &failure.problem else {
            panic!("{failure:?}");
        };
        let message = error.to_string();
        assert!(message.starts_with("op inject exited with"), "{message}");
        assert!(
            message.ends_with("the template has a syntax error"),
            "{message}"
        );
    }
}

#[test]
fn the_same_batch_failure_worded_as_not_found_falls_back_to_op_read() {
    // The twin of the unrecognised failure: only the words of op's last
    // line differ, and they decide whether each reference is read again.
    let fake = Fake::new(
        "batch-recognised",
        r#"echo 'first line' >&2; echo '[ERROR] "shelf-tls" isn'"'"'t an item in the "Personal" vault.' >&2; exit 1"#,
        "{read}",
    );
    let resolution = fake.resolve(&spec(&THREE), Instant::now() + PATIENT);
    assert!(resolution.failures.is_empty(), "{:?}", resolution.failures);
    assert_eq!(fake.runs(), ["inject", "read", "read", "read"]);
}

#[test]
fn a_batch_that_cannot_start_op_reads_each_reference_on_its_own() {
    // Each op read fails to start the same missing program, and reports it
    // against its own variable.
    let program = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("no-such-op");
    let one_password = OnePassword::new(program);
    let caller = starmap::Bindings::new();
    let context = Context::new(
        Places::new(PathBuf::from("/checkout")),
        &caller,
        Instant::now() + PATIENT,
    )
    .resolver(&one_password);
    let resolution = resolve(&spec(&THREE), Mode::Local, &context);

    assert_eq!(resolution.failures.len(), 3);
    for failure in &resolution.failures {
        let ResolveProblem::Secret(SecretError::Program(error)) = &failure.problem else {
            panic!("{failure:?}");
        };
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");
    }
}

#[test]
fn a_dismissed_op_read_after_a_failed_batch_ends_the_remaining_reads() {
    let fake = Fake::new(
        "batch-then-dismissed",
        r#"echo '[ERROR] "shelf-tls" isn'"'"'t an item in the "Personal" vault.' >&2; exit 1"#,
        "echo '[ERROR] authorization prompt dismissed' >&2; exit 1",
    );
    let resolution = fake.resolve(&spec(&THREE), Instant::now() + PATIENT);
    assert_eq!(fake.runs(), ["inject", "read"]);
    let failed = failures(&resolution);
    assert_eq!(failed.len(), 3, "{failed:?}");
    assert!(
        failed.iter().all(|(_, shown)| shown == "Secret(Denied)"),
        "{failed:?}"
    );
}

#[test]
fn output_without_each_marker_once_fails_every_reference_and_names_no_value() {
    let fake = Fake::new(
        "batch-malformed",
        "printf '%s' \"$(cat '{dir}/inject.awk')\"",
        "{read}",
    );
    let resolution = fake.resolve(&spec(&THREE), Instant::now() + PATIENT);

    assert_eq!(fake.runs(), ["inject"]);
    assert_eq!(resolution.failures.len(), 3);
    for failure in &resolution.failures {
        let ResolveProblem::Secret(SecretError::Program(error)) = &failure.problem else {
            panic!("{failure:?}");
        };
        assert!(error.to_string().contains("markers"), "{error}");
        fake.assert_absent("an error", &format!("{failure:?} {error}"));
    }
}

#[test]
fn a_value_that_is_not_unicode_fails_only_its_own_variable() {
    let fake = Fake::answering("batch-bytes");
    let resolution = fake.resolve(
        &spec(&[
            ("SHELF_MIRROR_TOKEN", "op://Personal/shelf-mirror/token"),
            ("SHELF_DOCUMENT", "op://Personal/shelf-document/bytes"),
        ]),
        Instant::now() + PATIENT,
    );
    let [failure] = resolution.failures.as_slice() else {
        panic!("{:?}", resolution.failures);
    };
    assert_eq!(failure.name.as_str(), "SHELF_DOCUMENT");
    let ResolveProblem::Secret(SecretError::Program(error)) = &failure.problem else {
        panic!("{failure:?}");
    };
    assert!(error.to_string().contains("not UTF-8"), "{error}");
    assert_eq!(
        bound(&resolution, "SHELF_MIRROR_TOKEN"),
        fake.value("token")
    );
}

#[test]
fn a_batch_that_outlives_the_deadline_fails_every_reference_without_running_op_again() {
    let fake = Fake::new("batch-slow", "sleep 5", "{read}");
    let resolution = fake.resolve(&spec(&THREE), Instant::now() + Duration::from_millis(300));
    assert_eq!(fake.runs(), ["inject"]);
    let failed = failures(&resolution);
    assert_eq!(failed.len(), 3, "{failed:?}");
    assert!(
        failed
            .iter()
            .all(|(_, shown)| shown == "Secret(Deadline)" || shown == "Deadline"),
        "{failed:?}"
    );
}

#[test]
fn finish_drops_what_prepare_readied_so_a_later_resolve_reads_again() {
    let fake = Fake::answering("batch-finish");
    let one_password = OnePassword::new(fake.program.clone());
    let references: Vec<SecretRef> = THREE
        .iter()
        .map(|(_, reference)| SecretRef::OnePassword(reference.parse().unwrap()))
        .collect();
    let lookups: Vec<Lookup<'_>> = references.iter().map(Lookup::reference).collect();
    let deadline = Instant::now() + PATIENT;

    one_password.prepare(&lookups, deadline);
    one_password.finish();
    let secret = one_password.resolve(lookups[0], deadline).unwrap();

    assert_eq!(fake.runs(), ["inject", "read"]);
    assert_eq!(text(&secret), fake.value("token"));
}

#[test]
fn two_threads_that_share_one_resolver_each_keep_the_values_they_prepared() {
    let fake = Fake::answering("batch-threads");
    let one_password = OnePassword::new(fake.program.clone());
    let barrier = std::sync::Barrier::new(2);
    let batches = [
        ["op://Personal/first/alpha", "op://Personal/first/beta"],
        ["op://Personal/second/gamma", "op://Personal/second/delta"],
    ];
    std::thread::scope(|scope| {
        for batch in &batches {
            let (one_password, barrier, fake) = (&one_password, &barrier, &fake);
            scope.spawn(move || {
                let references: Vec<SecretRef> = batch
                    .iter()
                    .map(|reference| SecretRef::OnePassword(reference.parse().unwrap()))
                    .collect();
                let lookups: Vec<Lookup<'_>> = references.iter().map(Lookup::reference).collect();
                let deadline = Instant::now() + PATIENT;
                one_password.prepare(&lookups, deadline);
                // Both threads have prepared before either resolves.
                barrier.wait();
                for (lookup, reference) in lookups.iter().zip(batch) {
                    let secret = one_password.resolve(*lookup, deadline).unwrap();
                    let part = reference.rsplit('/').next().unwrap();
                    assert_eq!(text(&secret), fake.value(part));
                }
                one_password.finish();
            });
        }
    });
    assert_eq!(fake.runs(), ["inject", "inject"]);
}
