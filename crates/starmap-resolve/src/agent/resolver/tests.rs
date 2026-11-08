//! Tests of [`Agent`] and [`AgentClient`] against a fake agent that runs on
//! a thread of the test, in a private directory, and a fake `op`, a POSIX
//! `sh` script that prints a canary the test generates at run time and
//! records its arguments, environment and runs. No test reaches a real
//! agent or the real 1Password CLI.

mod fake;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use starmap::{Bound, Mode, Secret, Spec};

use super::{Agent, lock};
use crate::agent::{AgentClient, AgentError, AgentStarter, CacheScope, Generation, PutOutcome};
use crate::{Context, Lookup, Places, Resolver, SecretError, resolve};

use fake::{FakeAgent, Request, UNLOCKED_AT, private_directory, program};

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
    secret.expose().to_owned()
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
                values: vec![(REFERENCE.to_owned(), op.canary.as_bytes().to_vec())],
                generation: fake.generation(),
            },
        ]
    );
    // The next resolution hits, so op runs once in all.
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(op.runs(), 1);
    assert!(agent.warnings().is_empty());
}

/// The references of [`spec_of_three`].
const THREE: [&str; 3] = [
    "op://Personal/shelf mirror/token",
    "op://Personal/shelf mirror/password",
    "op://Personal/catalogue key/credential",
];

/// Returns the spec of three secret variables, `A`, `B` and `C`, bound to
/// the references of [`THREE`].
fn spec_of_three() -> Spec {
    let mut text = String::new();
    for (name, reference) in ["A", "B", "C"].iter().zip(THREE) {
        text.push_str(&format!(
            r#"
[variables.{name}]
purpose = "A secret of the test."
type = "string"
unset = "required"
show = "withheld"
source = "secret"
bind = {{ one-password = "{reference}" }}
"#
        ));
    }
    Spec::parse(&text).unwrap()
}

/// Resolves `spec` in local mode with `agent`, and checks that every
/// variable is bound.
fn resolve_all(agent: &Agent, spec: &Spec) {
    let caller = starmap::Bindings::new();
    let context = Context::new(
        Places::new(PathBuf::from("/checkout")),
        &caller,
        Instant::now() + PATIENT,
    )
    .resolver(agent);
    let resolution = resolve(spec, Mode::Local, &context);
    assert!(resolution.failures.is_empty(), "{:?}", resolution.failures);
}

#[test]
fn a_resolution_with_several_misses_puts_them_all_with_one_put() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    let agent = agent_of(&fake, &op);
    resolve_all(&agent, &spec_of_three());
    let requests = fake.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    let Request::Put { values, generation } = &requests[1] else {
        panic!("{requests:?}");
    };
    assert_eq!(*generation, fake.generation());
    let mut references: Vec<&str> = values.iter().map(|(r, _)| r.as_str()).collect();
    references.sort_unstable();
    let mut expected = THREE.to_vec();
    expected.sort_unstable();
    assert_eq!(references, expected);
    assert!(
        values
            .iter()
            .all(|(_, value)| value == op.canary.as_bytes())
    );
    assert!(agent.warnings().is_empty());
    // The second resolution hits every reference, so it sends one Get and
    // no Put, and op runs no more.
    let runs = op.runs();
    resolve_all(&agent, &spec_of_three());
    assert_eq!(fake.requests().len(), 3);
    assert!(matches!(fake.requests()[2], Request::Get(_)));
    assert_eq!(op.runs(), runs);
    assert!(agent.warnings().is_empty());
}

#[test]
fn a_put_of_a_value_the_agent_already_holds_is_not_a_warning() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    // Another client stores the reference between this resolution's Get
    // and its Put.
    fake.hold_after_next_get(&op.program, REFERENCE, "first");
    let agent = agent_of(&fake, &op);
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert!(matches!(fake.requests()[1], Request::Put { .. }));
    assert!(agent.warnings().is_empty());
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), "first");
}

#[test]
fn the_client_reports_each_value_of_a_put_as_stored_or_already_held() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    let client = AgentClient::new(fake.socket.clone());
    let (one, two) = (REFERENCE.parse().unwrap(), THREE[1].parse().unwrap());
    let generation = client
        .get(&op.scope(), &[&one], later())
        .unwrap()
        .generation();
    let value = Secret::new("first".to_owned());
    let outcomes = client
        .put(&op.scope(), &[(&one, &value)], generation, later())
        .unwrap();
    assert_eq!(outcomes, [PutOutcome::Stored]);
    let outcomes = client
        .put(
            &op.scope(),
            &[(&one, &value), (&two, &value)],
            generation,
            later(),
        )
        .unwrap();
    assert_eq!(outcomes, [PutOutcome::AlreadyHeld, PutOutcome::Stored]);
}

/// The awk program that plays `op inject` for [`SizedOp`]. It replaces each
/// `{{ op://… }}` of the template with as many `x` as the reference's last
/// part says.
const SIZED_INJECT_AWK: &str = r#"{
    rest = $0
    while (match(rest, /[{][{] op:[^}]* [}][}]/)) {
        reference = substr(rest, RSTART + 3, RLENGTH - 6)
        n = split(reference, parts, "/")
        value = sprintf("%" parts[n] "s", "")
        gsub(/ /, "x", value)
        printf "%s%s", substr(rest, 1, RSTART - 1), value
        rest = substr(rest, RSTART + RLENGTH)
    }
    printf "%s", rest
}"#;

/// A fake `op` in a directory of its own that answers `op inject` with a
/// value for each reference, as long as the reference's last part says.
struct SizedOp {
    program: PathBuf,
    _directory: tempfile::TempDir,
}

impl SizedOp {
    fn new() -> Self {
        let directory = private_directory();
        let program = program(directory.path(), "op", "LC_ALL=C awk -f '{dir}/inject.awk'");
        fs::write(directory.path().join("inject.awk"), SIZED_INJECT_AWK).unwrap();
        Self {
            program,
            _directory: directory,
        }
    }

    fn scope(&self) -> CacheScope {
        CacheScope::new(self.program.clone(), [("OP_ACCOUNT", "my")]).unwrap()
    }
}

/// Returns the reference of variable `index` whose value is `size` bytes
/// long under [`SizedOp`].
fn sized_reference(index: usize, size: usize) -> String {
    format!("op://Personal/item{index}/{size}")
}

/// Returns the spec of one secret variable for each of `sizes`, bound to
/// its [`sized_reference`].
fn spec_of_sizes(sizes: &[usize]) -> Spec {
    let mut text = String::new();
    for (index, size) in sizes.iter().enumerate() {
        let reference = sized_reference(index, *size);
        text.push_str(&format!(
            r#"
[variables.V{index}]
purpose = "A secret of the test."
type = "string"
unset = "required"
show = "withheld"
source = "secret"
bind = {{ one-password = "{reference}" }}
"#
        ));
    }
    Spec::parse(&text).unwrap()
}

#[test]
fn a_value_over_64_kib_is_not_put_and_the_small_value_read_with_it_is_stored() {
    let (fake, op) = (FakeAgent::start(), SizedOp::new());
    let client = AgentClient::new(fake.socket.clone());
    let agent = Agent::new(client.clone(), op.scope());
    resolve_all(&agent, &spec_of_sizes(&[70 * 1024, 5]));
    assert!(agent.warnings().is_empty());
    let small = sized_reference(1, 5);
    let requests = fake.requests();
    assert_eq!(requests.len(), 2, "{} requests", requests.len());
    let Request::Put { values, .. } = &requests[1] else {
        panic!("the second request is not a Put");
    };
    assert_eq!(*values, [(small.clone(), b"xxxxx".to_vec())]);
    let reference = small.parse().unwrap();
    let held = client.get(&op.scope(), &[&reference], later()).unwrap();
    assert!(held.into_values()[0].is_some());
}

#[test]
fn values_too_large_for_one_frame_are_put_with_as_many_puts_as_they_need() {
    let (fake, op) = (FakeAgent::start(), SizedOp::new());
    let agent = Agent::new(AgentClient::new(fake.socket.clone()), op.scope());
    let sizes = [60 * 1024; 20];
    resolve_all(&agent, &spec_of_sizes(&sizes));
    assert!(agent.warnings().is_empty());
    let requests = fake.requests();
    assert!(matches!(requests[0], Request::Get(_)));
    let mut references: Vec<String> = Vec::new();
    for request in &requests[1..] {
        let Request::Put { values, generation } = request else {
            panic!("a request after the Get is not a Put");
        };
        assert_eq!(*generation, fake.generation());
        references.extend(values.iter().map(|(reference, _)| reference.clone()));
    }
    assert_eq!(requests.len(), 3, "{} requests", requests.len());
    references.sort_unstable();
    let mut expected: Vec<String> = (0..20).map(|i| sized_reference(i, 60 * 1024)).collect();
    expected.sort_unstable();
    assert_eq!(references, expected);
}

/// A resolver that runs an [`Agent`] and moves its deadline to now just
/// before its `finish`, as a resolution that used up its time would.
struct OutOfTime<'a>(&'a Agent);

impl Resolver for OutOfTime<'_> {
    fn prepare(&self, lookups: &[Lookup<'_>], deadline: Instant) {
        self.0.prepare(lookups, deadline);
    }

    fn finish(&self) {
        let thread = std::thread::current().id();
        lock(&self.0.prepared).get_mut(&thread).unwrap().deadline = Instant::now();
        self.0.finish();
    }

    fn resolve(&self, lookup: Lookup<'_>, deadline: Instant) -> Result<Secret, SecretError> {
        self.0.resolve(lookup, deadline)
    }
}

#[test]
fn finish_sends_no_put_once_the_deadline_has_passed_and_sends_one_before_it() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    let agent = agent_of(&fake, &op);
    let caller = starmap::Bindings::new();
    let late = OutOfTime(&agent);
    let context =
        Context::new(Places::new(PathBuf::from("/checkout")), &caller, later()).resolver(&late);
    let resolution = resolve(&spec(""), Mode::Local, &context);
    assert!(resolution.failures.is_empty(), "{:?}", resolution.failures);
    assert_eq!(fake.requests(), [Request::Get(vec![REFERENCE.to_owned()])]);
    assert!(agent.warnings().is_empty());
    resolve_text(&agent, &spec(""), Mode::Local);
    assert_eq!(fake.requests().len(), 3);
    assert!(matches!(fake.requests()[2], Request::Put { .. }));
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
    assert_eq!(client.clear(later()).unwrap(), 0);
    let stale = client
        .put(&op.scope(), &[(&reference, &value)], before, later())
        .unwrap_err();
    assert_eq!(stale.code(), "starmap::agent::stale_generation");
    let after = client.get(&op.scope(), &[&reference], later()).unwrap();
    assert_eq!(after.generation(), Generation(fake.generation()));
    let after = after.generation();
    let stored = client
        .put(&op.scope(), &[(&reference, &value)], after, later())
        .unwrap();
    assert_eq!(stored, [PutOutcome::Stored]);
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
        .put(&op.scope(), &[(&reference, &value)], before, later())
        .unwrap_err();
    assert_eq!(stale.code(), "starmap::agent::stale_generation");
    // The Get after the restart returns the new instance's generation, and
    // a Put with it is stored.
    let after = client
        .get(&op.scope(), &[&reference], later())
        .unwrap()
        .generation();
    assert_ne!(after, before);
    let stored = client
        .put(&op.scope(), &[(&reference, &value)], after, later())
        .unwrap();
    assert_eq!(stored, [PutOutcome::Stored]);
}

#[test]
fn a_put_of_no_values_sends_nothing() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    let client = AgentClient::new(fake.socket.clone());
    let outcomes = client
        .put(&op.scope(), &[], Generation(fake.generation()), later())
        .unwrap();
    assert!(outcomes.is_empty());
    assert_eq!(fake.requests(), []);
}

#[test]
fn a_clear_drops_every_value_and_returns_how_many() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    fake.hold(&op.program, REFERENCE, "held value");
    fake.hold(&op.program, THREE[1], "held value");
    let client = AgentClient::new(fake.socket.clone());
    assert_eq!(client.clear(later()).unwrap(), 2);
    assert_eq!(fake.requests(), [Request::Clear]);
    let agent = agent_of(&fake, &op);
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
}

#[test]
fn a_status_returns_the_agents_generation_values_pid_and_times() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    let client = AgentClient::new(fake.socket.clone());
    let empty = client.status(later()).unwrap().unwrap();
    assert_eq!(empty.generation(), Generation(fake.generation()));
    assert_eq!(empty.values(), 0);
    assert_eq!(empty.pid(), std::process::id());
    assert_eq!((empty.unlocked_at(), empty.lapses_at()), (None, None));
    fake.hold(&op.program, REFERENCE, "held value");
    let full = client.status(later()).unwrap().unwrap();
    let unlocked = UNIX_EPOCH + Duration::from_secs(UNLOCKED_AT.unsigned_abs());
    assert_eq!(full.values(), 1);
    assert_eq!(full.unlocked_at(), Some(unlocked));
    assert_eq!(
        full.lapses_at(),
        Some(unlocked + Duration::from_secs(12 * 3600))
    );
    assert_eq!(fake.requests(), [Request::Status, Request::Status]);
}

#[test]
fn a_status_with_no_agent_is_none_and_a_clear_is_not_running() {
    let (socket, _directory) = FakeAgent::absent();
    let client = AgentClient::new(socket);
    assert!(client.status(later()).unwrap().is_none());
    assert_eq!(
        client.clear(later()).unwrap_err().code(),
        "starmap::agent::not_running"
    );
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
fn an_agent_that_sends_one_byte_at_a_time_holds_the_client_only_until_the_deadline() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    // A Hits of one miss is 17 bytes with its length, so 50 ms a byte takes
    // about 850 ms, while each read waits only 50 ms.
    fake.trickle(Duration::from_millis(50));
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
        started.elapsed() < Duration::from_millis(600),
        "{:?}",
        started.elapsed()
    );
    // With time for every byte, the same answer arrives whole.
    let hits = client.get(&op.scope(), &[&reference], later()).unwrap();
    assert_eq!(hits.into_values().len(), 1);
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
    let Request::Put { values, .. } = &fake.requests()[1] else {
        panic!("{:?}", fake.requests());
    };
    assert_eq!(values[0].1, op.canary.as_bytes());
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

/// A fake `starmap` in a directory of its own, a POSIX `sh` script that
/// records its arguments in `args` and its environment in `env`, then
/// creates `done`. It uses only the shell's own commands and programs by
/// absolute path, because the starter gives it no `PATH`.
struct Starmap {
    program: PathBuf,
    directory: tempfile::TempDir,
}

impl Starmap {
    fn new() -> Self {
        let directory = private_directory();
        let program = program(
            directory.path(),
            "starmap",
            r#"printf '%s\n' "$@" > '{dir}/args'
/usr/bin/env > '{dir}/env'
: > '{dir}/done'"#,
        );
        Self { program, directory }
    }

    fn file(&self, name: &str) -> String {
        fs::read_to_string(self.directory.path().join(name)).unwrap_or_default()
    }

    /// Waits until the script has run to its end, for at most 10 seconds.
    fn wait_until_done(&self) {
        let done = self.directory.path().join("done");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done.exists() {
            assert!(Instant::now() < deadline, "the fake starmap never ran");
            #[expect(
                clippy::disallowed_methods,
                reason = "the test waits for a process it cannot be signalled by"
            )]
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn with_no_agent_and_a_starter_resolution_reads_from_op_and_starts_the_agent_with_a_cleared_environment()
 {
    let op = Op::new();
    let starmap = Starmap::new();
    let (socket, _directory) = FakeAgent::absent();
    let starter = AgentStarter::new(
        starmap.program.clone(),
        [
            ("HOME", "/home/luke"),
            ("USER", "luke"),
            ("LOGNAME", "luke"),
            ("LANG", "en_GB.UTF-8"),
            ("XDG_STATE_HOME", "/home/luke/.local/state"),
            ("STARMAP_AGENT_LAPSE", "8h"),
            ("STARMAP_TEST_CANARY", "canary"),
            ("OP_ACCOUNT", "my"),
            ("PATH", "/usr/bin:/bin"),
        ],
    );
    let agent = Agent::new(AgentClient::new(socket.clone()), op.scope()).starter(starter);
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(codes(&agent.warnings()), ["starmap::agent::not_running"]);
    starmap.wait_until_done();
    let args: Vec<String> = starmap.file("args").lines().map(str::to_owned).collect();
    assert_eq!(
        args,
        [
            "agent",
            "start",
            "--socket",
            socket.path().to_str().unwrap()
        ]
    );
    let env = starmap.file("env");
    for line in [
        "HOME=/home/luke",
        "USER=luke",
        "LOGNAME=luke",
        "LANG=en_GB.UTF-8",
        "XDG_STATE_HOME=/home/luke/.local/state",
        "STARMAP_AGENT_LAPSE=8h",
    ] {
        assert!(env.lines().any(|kept| kept == line), "{line} in {env}");
    }
    for name in ["STARMAP_TEST_CANARY=", "OP_ACCOUNT=", "PATH=/usr/bin:/bin"] {
        assert!(
            !env.lines().any(|line| line.starts_with(name)),
            "{name} in {env}"
        );
    }
}

#[test]
fn a_starter_whose_program_cannot_start_warns_start_failed() {
    let op = Op::new();
    let (socket, directory) = FakeAgent::absent();
    let starter = AgentStarter::new(directory.path().join("missing"), [("HOME", "/home/luke")]);
    let agent = Agent::new(AgentClient::new(socket), op.scope()).starter(starter);
    assert_eq!(resolve_text(&agent, &spec(""), Mode::Local), op.canary);
    assert_eq!(
        codes(&agent.warnings()),
        [
            "starmap::agent::not_running",
            "starmap::agent::start_failed"
        ]
    );
}

#[test]
fn a_running_agent_is_never_started_again() {
    let (fake, op) = (FakeAgent::start(), Op::new());
    let starmap = Starmap::new();
    let starter = AgentStarter::new(starmap.program.clone(), [("HOME", "/home/luke")]);
    let agent = agent_of(&fake, &op).starter(starter);
    resolve_text(&agent, &spec(""), Mode::Local);
    assert!(agent.warnings().is_empty());
    assert!(!starmap.directory.path().join("args").exists());
}
