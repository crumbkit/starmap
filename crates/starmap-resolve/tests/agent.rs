//! Tests of the `agent` feature. Each test runs a fake agent on a thread
//! of its own, in a private directory, and a fake `op`, a POSIX `sh`
//! script that prints a canary the test generates at run time and records
//! its arguments, environment and runs. No test reaches a real agent or
//! the real 1Password CLI.

#![cfg(all(feature = "agent", any(target_os = "macos", target_os = "linux")))]
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a helper that meets a broken fixture fails the test"
)]

#[path = "agent/fake.rs"]
mod fake;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use starmap::{Bound, Mode, Secret, Spec};
use starmap_resolve::agent::{Agent, AgentClient, AgentError, AgentStarter, CacheScope};
use starmap_resolve::{Context, Places, resolve};

use fake::{FakeAgent, Request, private_directory, program};

/// The reference that every test reads.
const REFERENCE: &str = "op://Personal/shelf mirror/token";

/// A deadline far enough away that only a fake that never ends reaches it.
const PATIENT: Duration = Duration::from_secs(60);

/// Returns a canary of 32 random hex digits that no fixture holds.
fn canary() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).unwrap();
    let digits: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("STARMAP-CANARY-{digits}")
}

/// A fake `op` in a directory of its own, which prints the canary for
/// `op read` and records each run in `runs`, its arguments in `args` and
/// its environment in `env`.
struct Op {
    program: PathBuf,
    canary: String,
    directory: tempfile::TempDir,
}

impl Op {
    fn new() -> Self {
        let directory = private_directory();
        let canary = canary();
        let program = program(
            directory.path(),
            "op",
            &format!(
                r#"echo "$1" >> '{{dir}}/runs'
printf '%s\n' "$@" >> '{{dir}}/args'
env >> '{{dir}}/env'
printf '%s' '{canary}'"#
            ),
        );
        Self {
            program,
            canary,
            directory,
        }
    }

    fn file(&self, name: &str) -> String {
        fs::read_to_string(self.directory.path().join(name)).unwrap_or_default()
    }

    fn runs(&self) -> usize {
        self.file("runs").lines().count()
    }

    fn scope(&self) -> CacheScope {
        CacheScope::new(self.program.clone(), [("OP_ACCOUNT", "my")]).unwrap()
    }

    /// Fails the test when the canary, or its hex or base64 form, appears
    /// in `text`.
    fn assert_absent(&self, place: &str, text: &str) {
        let bytes = self.canary.as_bytes();
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        for form in [self.canary.clone(), hex, base64(bytes)] {
            assert!(!text.contains(&form), "the canary appears in {place}");
        }
    }
}

/// Returns `bytes` in standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let word = chunk.iter().enumerate().fold(0_u32, |word, (index, byte)| {
            word | u32::from(*byte) << (16 - 8 * index)
        });
        for index in 0..4 {
            if index <= chunk.len() {
                text.push(char::from(
                    ALPHABET[usize::try_from(word >> (18 - 6 * index) & 63).unwrap()],
                ));
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// Returns a starter whose `starmap` does nothing but record its arguments
/// and environment in `directory`.
fn starter(directory: &Path, environment: Vec<(OsString, OsString)>) -> AgentStarter {
    let starmap = program(
        directory,
        "starmap",
        "printf '%s\\n' \"$@\" >> '{dir}/starmap-args'\nenv >> '{dir}/starmap-env'",
    );
    AgentStarter::new(starmap, environment)
}

/// Returns the spec of one secret variable bound to [`REFERENCE`], with
/// `extra` after its bind's reference, such as `, cache = false`.
fn spec(extra: &str) -> Spec {
    Spec::parse(&format!(
        r#"
[variables.SHELF_MIRROR_TOKEN]
purpose = "The token shelf sends to the catalogue mirror."
type = "string"
unset = "required"
show = "withheld"
source = "secret"
bind = {{ one-password = "{REFERENCE}"{extra} }}
"#
    ))
    .unwrap()
}

/// Resolves `spec` in `mode` with `agent`, and returns the variable's text.
#[expect(
    clippy::disallowed_methods,
    reason = "the tests check the value that resolution bound"
)]
fn resolve_text(agent: &Agent, spec: &Spec, mode: Mode) -> String {
    let caller = starmap::Bindings::new();
    let context = Context::new(
        Places::new(PathBuf::from("/checkout")),
        &caller,
        Instant::now() + PATIENT,
    )
    .resolver(agent);
    let resolution = resolve(spec, mode, &context);
    assert!(resolution.failures.is_empty(), "{:?}", resolution.failures);
    let Some(Bound::Secret(secret)) = resolution
        .bindings
        .get(&"SHELF_MIRROR_TOKEN".parse().unwrap())
    else {
        panic!("{:?}", resolution.bindings);
    };
    secret.expose().clone()
}

fn agent_of(fake: &FakeAgent, op: &Op) -> Agent {
    Agent::new(
        AgentClient::new(fake.socket.clone()),
        op.scope(),
        starter(op.directory.path(), Vec::new()),
    )
}

fn codes(warnings: &[AgentError]) -> Vec<&'static str> {
    warnings.iter().map(AgentError::code).collect()
}

#[test]
fn a_lookup_of_a_cache_false_bind_or_in_production_mode_never_reaches_the_socket() {
    let (fake, op) = (FakeAgent::start(0), Op::new());
    let agent = agent_of(&fake, &op);
    assert_eq!(
        resolve_text(&agent, &spec(", cache = false"), Mode::Local),
        op.canary
    );
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Production), op.canary);
    assert_eq!(fake.requests(), []);
    assert_eq!(op.runs(), 2);
}

#[test]
fn a_marked_lookup_in_local_mode_asks_the_agent_with_one_get() {
    let (fake, op) = (FakeAgent::start(0), Op::new());
    resolve_text(&agent_of(&fake, &op), &spec(""), Mode::Local);
    assert_eq!(fake.requests()[0], Request::Get(vec![REFERENCE.to_owned()]));
}

#[test]
fn a_hit_returns_the_agents_value_and_op_never_runs() {
    let (fake, op) = (FakeAgent::start(0), Op::new());
    fake.hold(&op.program, REFERENCE, "held value");
    let agent = agent_of(&fake, &op);
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), "held value");
    assert_eq!(op.runs(), 0);
    assert_eq!(fake.requests(), [Request::Get(vec![REFERENCE.to_owned()])]);
    assert!(agent.warnings().is_empty());
}

#[test]
fn a_value_held_for_another_op_program_is_a_miss() {
    let (fake, op) = (FakeAgent::start(0), Op::new());
    fake.hold(Path::new("/usr/local/bin/op"), REFERENCE, "held value");
    assert_eq!(
        resolve_text(&agent_of(&fake, &op), &spec(""), Mode::Local),
        op.canary
    );
    assert_eq!(op.runs(), 1);
}

#[test]
fn a_miss_is_read_from_op_and_then_put_with_the_generation_of_the_get() {
    let (fake, op) = (FakeAgent::start(4), Op::new());
    let agent = agent_of(&fake, &op);
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(
        fake.requests(),
        [
            Request::Get(vec![REFERENCE.to_owned()]),
            Request::Put {
                reference: REFERENCE.to_owned(),
                value: op.canary.as_bytes().to_vec(),
                generation: 4,
            },
        ]
    );
    // The next resolution hits, so op runs once in all.
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(op.runs(), 1);
    assert!(agent.warnings().is_empty());
}

#[test]
fn a_put_from_before_a_clear_is_refused_and_one_after_it_is_stored() {
    let (fake, op) = (FakeAgent::start(0), Op::new());
    let client = AgentClient::new(fake.socket.clone());
    let reference = REFERENCE.parse().unwrap();
    let value = Secret::new("read value".to_owned());
    let before = client.get(&op.scope(), &[&reference]).unwrap().generation();
    assert_eq!(client.clear().unwrap().values(), 0);
    let stale = client
        .put(&op.scope(), &reference, &value, before)
        .unwrap_err();
    assert_eq!(stale.code(), "starmap::agent::stale_generation");
    let after = client.get(&op.scope(), &[&reference]).unwrap();
    assert_eq!(after.into_values().len(), 1);
    let after = client.get(&op.scope(), &[&reference]).unwrap().generation();
    client.put(&op.scope(), &reference, &value, after).unwrap();
    let exists = client
        .put(&op.scope(), &reference, &value, after)
        .unwrap_err();
    assert_eq!(exists.code(), "starmap::agent::exists");
}

#[test]
fn a_malformed_answer_fails_closed_and_resolution_reads_from_op_without_a_put() {
    let (fake, op) = (FakeAgent::start(0), Op::new());
    // A Hits of one hit with a value, then a byte after its last field.
    let mut body = vec![1, 0x81, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 4];
    body.extend_from_slice(b"fake");
    body.push(0);
    fake.reply(body);
    let agent = agent_of(&fake, &op);
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(codes(&agent.warnings()), ["starmap::agent::protocol"]);
    assert_eq!(fake.requests(), [Request::Get(vec![REFERENCE.to_owned()])]);
}

#[test]
fn an_unknown_outcome_byte_fails_closed() {
    let (fake, op) = (FakeAgent::start(0), Op::new());
    fake.reply(vec![1, 0x81, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 3]);
    let agent = agent_of(&fake, &op);
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(codes(&agent.warnings()), ["starmap::agent::protocol"]);
}

#[test]
fn with_no_agent_the_starter_runs_agent_start_in_a_cleared_environment_and_op_still_answers() {
    let op = Op::new();
    let (socket, _directory) = FakeAgent::absent();
    let environment = vec![
        (OsString::from("HOME"), OsString::from("/home/luke")),
        (OsString::from("PATH"), OsString::from("/usr/bin")),
        (OsString::from("SHELF_TOKEN"), OsString::from(&op.canary)),
    ];
    let agent = Agent::new(
        AgentClient::new(socket),
        op.scope(),
        starter(op.directory.path(), environment),
    );
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(op.file("starmap-args"), "agent\nstart\n");
    let started_with = op.file("starmap-env");
    assert!(started_with.contains("HOME=/home/luke"), "{started_with}");
    assert!(!started_with.contains("PATH=/usr/bin"), "{started_with}");
    assert!(!started_with.contains("SHELF_TOKEN"), "{started_with}");
    // The agent never came up, so the second Get found none either.
    assert_eq!(codes(&agent.warnings()), ["starmap::agent::not_running"]);
}

#[test]
fn a_starter_whose_program_does_not_exist_reports_start_failed() {
    let op = Op::new();
    let (socket, directory) = FakeAgent::absent();
    let agent = Agent::new(
        AgentClient::new(socket),
        op.scope(),
        AgentStarter::new(
            directory.path().join("missing"),
            Vec::<(OsString, OsString)>::new(),
        ),
    );
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(codes(&agent.warnings()), ["starmap::agent::start_failed"]);
}

#[test]
fn a_service_account_token_or_an_op_session_turns_the_agent_off_and_an_account_does_not() {
    let op = PathBuf::from("/opt/op");
    for name in ["OP_SERVICE_ACCOUNT_TOKEN", "OP_SESSION_my"] {
        let off = CacheScope::new(op.clone(), [(name, "dummy")]).unwrap_err();
        assert_eq!(off.code(), "starmap::agent::off", "{name}");
    }
    let scope =
        CacheScope::new(op.clone(), [("OP_ACCOUNT", "my"), ("HOME", "/home/luke")]).unwrap();
    assert_eq!(scope.account(), "my");
    let relative = CacheScope::new(PathBuf::from("op"), Vec::<(&str, &str)>::new()).unwrap_err();
    assert_eq!(relative.code(), "starmap::agent::off");
}

#[test]
fn the_canary_never_appears_in_argv_env_files_or_warnings() {
    let (fake, op) = (FakeAgent::start(0), Op::new());
    let agent = agent_of(&fake, &op);
    resolve_text(&agent, &spec(""), Mode::Local);
    // The positive control: the canary did travel to the agent.
    let Request::Put { value, .. } = &fake.requests()[1] else {
        panic!("{:?}", fake.requests());
    };
    assert_eq!(value, op.canary.as_bytes());
    op.assert_absent("op's arguments", &op.file("args"));
    op.assert_absent("op's environment", &op.file("env"));
    for entry in fs::read_dir(op.directory.path()).unwrap() {
        let path = entry.unwrap().path();
        // The fake op holds the canary it prints.
        if path != op.program {
            op.assert_absent(
                &path.display().to_string(),
                &fs::read_to_string(&path).unwrap_or_default(),
            );
        }
    }
    op.assert_absent("the agent's debug form", &format!("{agent:?}"));
    let warnings = agent.warnings();
    op.assert_absent(
        "the warnings",
        &format!(
            "{warnings:?} {}",
            warnings.iter().map(ToString::to_string).collect::<String>()
        ),
    );
}
