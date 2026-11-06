//! A fake agent that a test runs on a thread of its own, and a fake `op`
//! program. The fake agent speaks the wire protocol from its own code, so
//! the tests check the client against a second reading of the protocol, and
//! it records every request it receives. Its generation is an opaque token
//! of 64 random bits, drawn when it starts and again on every clear, as the
//! agent draws it. Its status reports an unlock at [`UNLOCKED_AT`] and a
//! lapse 12 hours later while it holds a value.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::agent::AgentSocket;

/// A request the fake agent received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Request {
    Get(Vec<String>),
    /// One Put of each reference and value, in order.
    Put {
        values: Vec<(String, Vec<u8>)>,
        generation: u64,
    },
    Clear,
    Status,
}

#[derive(Default)]
struct State {
    values: HashMap<String, Vec<u8>>,
    generation: u64,
    requests: Vec<Request>,
    /// A body the agent sends in place of its own answer.
    reply: Option<Vec<u8>>,
    /// Whether the agent reads requests and never answers, holding each
    /// connection open.
    silent: bool,
    held: Vec<UnixStream>,
    /// The pause before each byte of an answer, for an agent that sends
    /// one byte at a time.
    trickle: Option<Duration>,
    /// A key and value that the agent stores right after it answers the
    /// next Get, as a second client's Put would.
    after_get: Option<(String, Vec<u8>)>,
}

/// The Unix second at which the fake agent reports its unlock began.
pub(crate) const UNLOCKED_AT: i64 = 1_800_000_000;

/// Returns 64 random bits, a new generation.
fn random() -> u64 {
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes).unwrap();
    u64::from_be_bytes(bytes)
}

/// Returns the socket in `directory/starmap-agent/`, as `for_user` would
/// for that parent.
fn socket_in(directory: &Path) -> AgentSocket {
    AgentSocket::in_parent(directory, nix::unistd::getuid().as_raw()).unwrap()
}

/// A fake agent listening in a private directory of its own.
pub(crate) struct FakeAgent {
    pub(crate) socket: AgentSocket,
    state: Arc<Mutex<State>>,
    _directory: tempfile::TempDir,
}

impl FakeAgent {
    /// Starts a fake agent with a generation of its own.
    pub(crate) fn start() -> Self {
        let directory = private_directory();
        let socket = socket_in(directory.path());
        let listener = UnixListener::bind(socket.path()).unwrap();
        let state = Arc::new(Mutex::new(State {
            generation: random(),
            ..State::default()
        }));
        let shared = Arc::clone(&state);
        thread::spawn(move || {
            for stream in listener.incoming() {
                serve(stream.unwrap(), &shared);
            }
        });
        Self {
            socket,
            state,
            _directory: directory,
        }
    }

    /// Returns a socket in a private directory where no agent listens.
    pub(crate) fn absent() -> (AgentSocket, tempfile::TempDir) {
        let directory = private_directory();
        (socket_in(directory.path()), directory)
    }

    /// Drops every value and draws a new generation, as a clear or a
    /// restart of the agent does.
    pub(crate) fn clear(&self) {
        let mut state = self.state.lock().unwrap();
        state.values.clear();
        state.generation = random();
    }

    /// Reads every request from now on and never answers it.
    pub(crate) fn fall_silent(&self) {
        self.state.lock().unwrap().silent = true;
    }

    /// Sends every answer from now on one byte at a time, `pause` apart.
    pub(crate) fn trickle(&self, pause: Duration) {
        self.state.lock().unwrap().trickle = Some(pause);
    }

    /// Holds `value` for `reference` of the account `my` and the `op` at
    /// `op`.
    pub(crate) fn hold(&self, op: &Path, reference: &str, value: &str) {
        let key = key("my", op.to_str().unwrap(), reference);
        self.state
            .lock()
            .unwrap()
            .values
            .insert(key, value.as_bytes().to_vec());
    }

    /// Holds `value` for `reference`, as [`FakeAgent::hold`] does, right
    /// after the agent answers the next Get.
    pub(crate) fn hold_after_next_get(&self, op: &Path, reference: &str, value: &str) {
        let key = key("my", op.to_str().unwrap(), reference);
        self.state.lock().unwrap().after_get = Some((key, value.as_bytes().to_vec()));
    }

    /// Answers every request with `body` from now on.
    pub(crate) fn reply(&self, body: Vec<u8>) {
        self.state.lock().unwrap().reply = Some(body);
    }

    /// Returns the agent's generation.
    pub(crate) fn generation(&self) -> u64 {
        self.state.lock().unwrap().generation
    }

    /// Returns every request the agent received, in order.
    pub(crate) fn requests(&self) -> Vec<Request> {
        self.state.lock().unwrap().requests.clone()
    }
}

/// Returns a new directory of mode 0700, short enough for a socket path.
pub(crate) fn private_directory() -> tempfile::TempDir {
    let directory = tempfile::Builder::new().prefix("sa").tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

fn key(account: &str, op: &str, reference: &str) -> String {
    format!("{account}\n{op}\n{reference}")
}

/// Reads one request from `stream` and answers it.
fn serve(mut stream: UnixStream, state: &Mutex<State>) {
    let mut length = [0; 4];
    if stream.read_exact(&mut length).is_err() {
        return;
    }
    let mut body = vec![0; usize::try_from(u32::from_be_bytes(length)).unwrap()];
    stream.read_exact(&mut body).unwrap();
    let mut state = state.lock().unwrap();
    if state.silent {
        state.held.push(stream);
        return;
    }
    let answer = answer(&mut Body(&body), &mut state);
    let answer = state.reply.clone().unwrap_or(answer);
    let mut frame = u32::try_from(answer.len()).unwrap().to_be_bytes().to_vec();
    frame.extend_from_slice(&answer);
    let trickle = state.trickle;
    drop(state);
    // A client that gave up has closed its end, so a failed write ends the
    // answer.
    match trickle {
        None => {
            let _ = stream.write_all(&frame);
        }
        Some(pause) => {
            for byte in frame {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "the fake paces its bytes to play an agent that sends them slowly"
                )]
                thread::sleep(pause);
                if stream.write_all(&[byte]).is_err() {
                    return;
                }
            }
        }
    }
}

/// Returns the body of the answer to the request `body`.
fn answer(body: &mut Body<'_>, state: &mut State) -> Vec<u8> {
    assert_eq!(body.take(1), [1], "a request of another version");
    match body.take(1)[0] {
        0x01 => {
            let (account, op) = (body.text(), body.text());
            let count = usize::from(body.u16());
            let references: Vec<String> = (0..count).map(|_| body.text()).collect();
            let mut answer = vec![1, 0x81];
            answer.extend_from_slice(&state.generation.to_be_bytes());
            answer.extend_from_slice(&u16::try_from(count).unwrap().to_be_bytes());
            for reference in &references {
                match state.values.get(&key(&account, &op, reference)) {
                    Some(value) => {
                        answer.push(0);
                        answer
                            .extend_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
                        answer.extend_from_slice(value);
                    }
                    None => answer.push(1),
                }
            }
            state.requests.push(Request::Get(references));
            if let Some((key, value)) = state.after_get.take() {
                state.values.insert(key, value);
            }
            answer
        }
        0x02 => {
            let (account, op) = (body.text(), body.text());
            let generation = u64::from_be_bytes(body.take(8).try_into().unwrap());
            let count = usize::from(body.u16());
            let values: Vec<(String, Vec<u8>)> = (0..count)
                .map(|_| {
                    let reference = body.text();
                    let length = u32::from_be_bytes(body.take(4).try_into().unwrap());
                    (
                        reference,
                        body.take(usize::try_from(length).unwrap()).to_vec(),
                    )
                })
                .collect();
            state.requests.push(Request::Put {
                values: values.clone(),
                generation,
            });
            if generation != state.generation {
                return refused("starmap::agent::stale_generation");
            }
            let mut answer = vec![1, 0x82];
            answer.extend_from_slice(&u16::try_from(count).unwrap().to_be_bytes());
            for (reference, value) in values {
                match state.values.entry(key(&account, &op, &reference)) {
                    Entry::Occupied(_) => answer.push(1),
                    Entry::Vacant(entry) => {
                        entry.insert(value);
                        answer.push(0);
                    }
                }
            }
            answer
        }
        0x03 => {
            state.requests.push(Request::Clear);
            let dropped = u32::try_from(state.values.len()).unwrap();
            state.values.clear();
            state.generation = random();
            let mut answer = vec![1, 0x83];
            answer.extend_from_slice(&dropped.to_be_bytes());
            answer
        }
        0x04 => {
            state.requests.push(Request::Status);
            let held = u32::try_from(state.values.len()).unwrap();
            let (unlocked, lapses) = if held == 0 {
                (0, 0)
            } else {
                (UNLOCKED_AT, UNLOCKED_AT + 12 * 3600)
            };
            let mut answer = vec![1, 0x84];
            answer.extend_from_slice(&state.generation.to_be_bytes());
            answer.extend_from_slice(&held.to_be_bytes());
            answer.extend_from_slice(&std::process::id().to_be_bytes());
            answer.extend_from_slice(&i64::to_be_bytes(unlocked));
            answer.extend_from_slice(&i64::to_be_bytes(lapses));
            answer
        }
        kind => panic!("the fake agent got a request of kind {kind}"),
    }
}

fn refused(code: &str) -> Vec<u8> {
    let mut answer = vec![1, 0xFF];
    answer.extend_from_slice(&u16::try_from(code.len()).unwrap().to_be_bytes());
    answer.extend_from_slice(code.as_bytes());
    answer
}

struct Body<'a>(&'a [u8]);

impl<'a> Body<'a> {
    fn take(&mut self, count: usize) -> &'a [u8] {
        let (taken, left) = self.0.split_at(count);
        self.0 = left;
        taken
    }

    fn u16(&mut self) -> u16 {
        u16::from_be_bytes(self.take(2).try_into().unwrap())
    }

    fn text(&mut self) -> String {
        let length = usize::from(self.u16());
        String::from_utf8(self.take(length).to_vec()).unwrap()
    }
}

/// Writes a POSIX `sh` program `name` with `script` into `directory`, and
/// returns its path.
pub(crate) fn program(directory: &Path, name: &str, script: &str) -> PathBuf {
    let program = directory.join(name);
    let script = script.replace("{dir}", directory.to_str().unwrap());
    fs::write(&program, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    program
}
