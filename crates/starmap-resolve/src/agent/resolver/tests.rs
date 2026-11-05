//! Tests of [`Agent`] and [`AgentClient`] against a fake agent that runs on
//! a thread of the test, in a private directory, and a fake `op`, a POSIX
//! `sh` script that prints a canary the test generates at run time and
//! records its arguments, environment and runs. No test reaches a real
//! agent or the real 1Password CLI.

mod fake;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use starmap::{Bound, Mode, Secret, Spec};

use super::Agent;
use crate::agent::{AgentClient, AgentError, CacheScope, Generation};
use crate::{Context, Places, resolve};

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
    Agent::new(AgentClient::new(fake.socket.clone()), op.scope())
}

fn codes(warnings: &[AgentError]) -> Vec<&'static str> {
    warnings.iter().map(AgentError::code).collect()
}

#[test]
fn a_lookup_of_a_cache_false_bind_or_in_production_mode_never_reaches_the_socket() {
    let (fake, op) = (FakeAgent::start(), Op::new());
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
    let (fake, op) = (FakeAgent::start(), Op::new());
    resolve_text(&agent_of(&fake, &op), &spec(""), Mode::Local);
    assert_eq!(fake.requests()[0], Request::Get(vec![REFERENCE.to_owned()]));
}

#[test]
fn a_hit_returns_the_agents_value_and_op_never_runs() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    fake.hold(&op.program, REFERENCE, "held value");
    let agent = agent_of(&fake, &op);
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), "held value");
    assert_eq!(op.runs(), 0);
    assert_eq!(fake.requests(), [Request::Get(vec![REFERENCE.to_owned()])]);
    assert!(agent.warnings().is_empty());
}

#[test]
fn a_value_held_for_another_op_program_is_a_miss() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    fake.hold(Path::new("/usr/local/bin/op"), REFERENCE, "held value");
    assert_eq!(
        resolve_text(&agent_of(&fake, &op), &spec(""), Mode::Local),
        op.canary
    );
    assert_eq!(op.runs(), 1);
}

#[test]
fn a_miss_is_read_from_op_and_then_put_with_the_generation_of_the_get() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    let agent = agent_of(&fake, &op);
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(
        fake.requests(),
        [
            Request::Get(vec![REFERENCE.to_owned()]),
            Request::Put {
                reference: REFERENCE.to_owned(),
                value: op.canary.as_bytes().to_vec(),
                generation: fake.generation(),
            },
        ]
    );
    // The next resolution hits, so op runs once in all.
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(op.runs(), 1);
    assert!(agent.warnings().is_empty());
}

/// Returns a deadline far enough away for a test.
fn later() -> Instant {
    Instant::now() + PATIENT
}

#[test]
fn a_put_from_before_a_clear_is_refused_and_one_after_it_is_stored() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    let client = AgentClient::new(fake.socket.clone());
    let reference = REFERENCE.parse().unwrap();
    let value = Secret::new("read value".to_owned());
    let before = client
        .get(&op.scope(), &[&reference], later())
        .unwrap()
        .generation();
    fake.clear();
    let stale = client
        .put(&op.scope(), &reference, &value, before, later())
        .unwrap_err();
    assert_eq!(stale.code(), "starmap::agent::stale_generation");
    let after = client.get(&op.scope(), &[&reference], later()).unwrap();
    assert_eq!(after.generation(), Generation(fake.generation()));
    let after = after.generation();
    client
        .put(&op.scope(), &reference, &value, after, later())
        .unwrap();
    let exists = client
        .put(&op.scope(), &reference, &value, after, later())
        .unwrap_err();
    assert_eq!(exists.code(), "starmap::agent::exists");
}

#[test]
fn a_put_across_a_restart_of_the_agent_is_refused() {
    let op = Op::new();
    let reference = REFERENCE.parse().unwrap();
    let value = Secret::new("read value".to_owned());
    let first = FakeAgent::start();
    let client = AgentClient::new(first.socket.clone());
    let before = client
        .get(&op.scope(), &[&reference], later())
        .unwrap()
        .generation();
    // A new instance at the same socket draws a generation of its own.
    first.clear();
    let stale = client
        .put(&op.scope(), &reference, &value, before, later())
        .unwrap_err();
    assert_eq!(stale.code(), "starmap::agent::stale_generation");
    // The Get after the restart returns the new instance's generation, and
    // a Put with it is stored.
    let after = client
        .get(&op.scope(), &[&reference], later())
        .unwrap()
        .generation();
    assert_ne!(after, before);
    client
        .put(&op.scope(), &reference, &value, after, later())
        .unwrap();
}

#[test]
fn an_agent_that_never_answers_holds_the_client_only_until_the_deadline() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    fake.fall_silent();
    let client = AgentClient::new(fake.socket.clone());
    let reference = REFERENCE.parse().unwrap();
    let started = Instant::now();
    let late = client
        .get(
            &op.scope(),
            &[&reference],
            started + Duration::from_millis(200),
        )
        .unwrap_err();
    assert_eq!(late.code(), "starmap::agent::protocol");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_get_whose_deadline_has_passed_sends_nothing_and_one_before_it_is_sent() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    let client = AgentClient::new(fake.socket.clone());
    let reference = REFERENCE.parse().unwrap();
    let passed = client
        .get(&op.scope(), &[&reference], Instant::now())
        .unwrap_err();
    assert_eq!(passed.code(), "starmap::agent::protocol");
    assert_eq!(fake.requests(), []);
    client.get(&op.scope(), &[&reference], later()).unwrap();
    assert_eq!(fake.requests().len(), 1);
}

#[test]
fn with_no_agent_resolution_reads_from_op_and_warns_not_running() {
    let op = Op::new();
    let (socket, _directory) = FakeAgent::absent();
    let agent = Agent::new(AgentClient::new(socket), op.scope());
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(codes(&agent.warnings()), ["starmap::agent::not_running"]);
    // The next resolution starts with no warnings of its own yet.
    assert!(agent.warnings().is_empty());
}

#[test]
fn each_resolution_keeps_only_its_own_warnings() {
    let op = Op::new();
    let (socket, _directory) = FakeAgent::absent();
    let agent = Agent::new(AgentClient::new(socket), op.scope());
    resolve_text(&agent, &spec(""), Mode::Local);
    resolve_text(&agent, &spec(""), Mode::Local);
    assert_eq!(codes(&agent.warnings()), ["starmap::agent::not_running"]);
}

#[test]
fn a_malformed_answer_fails_closed_and_resolution_reads_from_op_without_a_put() {
    let (fake, op) = (FakeAgent::start(), Op::new());
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
    let (fake, op) = (FakeAgent::start(), Op::new());
    fake.reply(vec![1, 0x81, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 3]);
    let agent = agent_of(&fake, &op);
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(codes(&agent.warnings()), ["starmap::agent::protocol"]);
}

#[test]
fn a_service_account_token_or_an_op_session_turns_the_agent_off() {
    let op = PathBuf::from("/opt/op");
    for name in ["OP_SERVICE_ACCOUNT_TOKEN", "OP_SESSION_my"] {
        let off = CacheScope::new(op.clone(), [("OP_ACCOUNT", "my"), (name, "dummy")]).unwrap_err();
        assert_eq!(off.code(), "starmap::agent::off", "{name}");
    }
    let scope = CacheScope::new(op, [("OP_ACCOUNT", "my"), ("HOME", "/home/luke")]).unwrap();
    assert_eq!(scope.account(), "my");
}

#[test]
fn the_agent_is_off_unless_op_account_names_an_account() {
    let op = PathBuf::from("/opt/op");
    for environment in [
        vec![],
        vec![("OP_ACCOUNT", "")],
        vec![("HOME", "/home/luke")],
    ] {
        let off = CacheScope::new(op.clone(), environment.clone()).unwrap_err();
        assert_eq!(off.code(), "starmap::agent::off", "{environment:?}");
    }
    assert!(CacheScope::new(op, [("OP_ACCOUNT", "my")]).is_ok());
}

#[test]
fn an_op_program_that_is_not_an_absolute_path_turns_the_agent_off() {
    let relative = CacheScope::new(PathBuf::from("op"), [("OP_ACCOUNT", "my")]).unwrap_err();
    assert_eq!(relative.code(), "starmap::agent::off");
    assert!(CacheScope::new(PathBuf::from("/opt/op"), [("OP_ACCOUNT", "my")]).is_ok());
}

#[test]
fn the_canary_never_appears_in_argv_env_files_or_warnings() {
    let (fake, op) = (FakeAgent::start(), Op::new());
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
