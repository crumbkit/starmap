//! The agent's wire protocol, version 1, with both halves: the client's
//! requests and its parser of the agent's answers, and the agent's parser
//! of requests and its answers. starmap's agent parses requests with
//! [`request`] and writes each answer with [`hits`], [`stored`],
//! [`cleared`], [`status_answer`] or [`refused`].
//!
//! Each connection carries one request and one answer, then closes. Every
//! frame is a 4-byte big-endian length, at most [`FRAME_LIMIT`] bytes, and
//! a body that starts with the version byte (1) and a kind byte.
//! [`read_frame`] and [`write_frame`] move one frame over a socket by a
//! deadline. Text, such as an account, an `op` path or a reference, is a
//! `u16` length and UTF-8.
//!
//! | Kind | Direction | Body after the version and kind |
//! | --- | --- | --- |
//! | 0x01 Get | client to agent | account; `op` path; `u16` count, at most 256; each reference |
//! | 0x02 Put | client to agent | account; `op` path; `u64` generation; `u16` count, 1 to 256; each reference, then a `u32` length, at most 64 KiB, and the value |
//! | 0x03 Clear | client to agent | empty, with any version byte |
//! | 0x04 Status | client to agent | empty |
//! | 0x81 Hits | agent to client | `u64` generation; `u16` count, the Get's; each entry in request order is an outcome byte, 0 hit, 1 miss or 2 over budget, and for a hit a `u32` length and the value |
//! | 0x82 Stored | agent to client | `u16` count, the Put's; each entry in request order is an outcome byte, 0 stored or 1 already held |
//! | 0x83 Cleared | agent to client | `u32` count of the values dropped |
//! | 0x84 Status | agent to client | `u64` generation; `u32` values held; `u32` process id; `i64` unlocked-at and `i64` lapses-at in Unix seconds, 0 when none |
//! | 0xFF Refused | agent to client | the code as text, such as `starmap::agent::stale_generation` |
//!
//! The agent accepts Clear with any version byte, so a client of any
//! version can clear an agent of any version. A Hits carries at most
//! [`BUDGET`] bytes of values in all. Once the next hit would pass the
//! budget, [`hits`] sends it and every later hit as over budget, and the
//! client reads those from `op` as it reads a miss.
//!
//! Both parsers apply the same rules. A count over 256 is refused before
//! anything is allocated for it. Every length is checked against the
//! bytes left in the frame, and a value's length against [`VALUE_LIMIT`],
//! before it is read. A frame with bytes after its last field is refused.
//! Every text and value must be UTF-8, and a value that is not is zeroed
//! before it is dropped. An unknown version byte on anything but Clear is
//! refused.
//!
//! The format is written by hand, so no value passes through serde, and
//! every buffer that may hold a value is a [`Zeroizing`] buffer allocated
//! at its full size, so writing or reading never reallocates it and leaves
//! a copy behind.
//!
//! The generation is an opaque token of each agent instance that
//! [`Generation::draw`] makes: the agent draws one when it starts and
//! again on every clear. A Get returns it, and a Put carries back the one
//! its Get returned. The agent stores a Put's values only when the token
//! equals its own, so a value read before a clear, or read from an agent
//! that has since stopped and started again, never lands. The client
//! never compares or increases a generation; it only echoes it.

use std::io::{self, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use starmap::Secret;
use zeroize::Zeroizing;

use super::{AgentError, AgentStatus, CacheScope, Generation, PutOutcome};

#[cfg(test)]
mod tests;

/// The protocol's version.
const VERSION: u8 = 1;

const GET: u8 = 0x01;
const PUT: u8 = 0x02;
const CLEAR: u8 = 0x03;
const STATUS: u8 = 0x04;
const HITS: u8 = 0x81;
const STORED: u8 = 0x82;
const CLEARED: u8 = 0x83;
const STATUS_ANSWER: u8 = 0x84;
const REFUSED: u8 = 0xFF;

/// The outcome bytes of a Hits entry.
const HIT: u8 = 0;
const MISS: u8 = 1;
const OVER_BUDGET: u8 = 2;

/// The outcome bytes of a Stored entry.
const STORED_VALUE: u8 = 0;
const ALREADY_HELD: u8 = 1;

/// The most bytes of one frame's body. A reader refuses a longer frame
/// before it allocates anything for it.
pub const FRAME_LIMIT: usize = 1024 * 1024;

/// The most references of one Get or one Put.
pub const REFERENCE_LIMIT: usize = 256;

/// The most bytes of one value.
pub const VALUE_LIMIT: usize = 64 * 1024;

/// The most bytes of all the values of one Hits.
pub const BUDGET: usize = 512 * 1024;

/// The longest that one wait to connect, or to write or read part of a
/// frame, may take, so a peer that stops answering cannot hold the other
/// end. Each wait is also cut to the time left before the deadline.
const WAIT_LIMIT: Duration = Duration::from_secs(2);

/// A request that [`request`] parsed.
#[derive(Debug)]
#[non_exhaustive]
pub enum Request {
    /// Asks for the value of each of `references` in the scope of
    /// `account` and the `op` program at `op`.
    Get {
        /// The 1Password account.
        account: String,
        /// The absolute path of the `op` program.
        op: String,
        /// The references, in request order.
        references: Vec<String>,
    },
    /// Stores each of `values`, a reference and its value, in the scope of
    /// `account` and `op`, read after the Get that returned `generation`.
    Put {
        /// The 1Password account.
        account: String,
        /// The absolute path of the `op` program.
        op: String,
        /// The generation that the client's Get returned.
        generation: Generation,
        /// Each reference and its value, in request order.
        values: Vec<(String, Secret)>,
    },
    /// Drops every value.
    Clear,
    /// Asks for the agent's [`AgentStatus`].
    Status,
}

/// The answer a client's request expects, besides a refusal.
#[derive(Debug, Clone, Copy)]
pub(super) enum Expect {
    /// The hits of a Get of this many references.
    Hits(usize),
    /// The outcomes of a Put of this many values.
    Stored(usize),
    Cleared,
    Status,
}

/// What the agent answered a client.
#[derive(Debug)]
pub(super) enum Response {
    /// The generation and, in request order, each hit's value or `None`
    /// for a miss or a hit over the budget.
    Hits(Generation, Vec<Option<Secret>>),
    /// The outcome of each value of the Put, in request order.
    Stored(Vec<PutOutcome>),
    /// How many values the Clear dropped.
    Cleared(u32),
    Status(AgentStatus),
}

/// Returns the frame of a Get of `references` in `scope`.
///
/// # Errors
///
/// Returns [`AgentError::TooManyReferences`] for more than 256 references,
/// and [`AgentError::Protocol`] for a text over 64 KiB or a frame over its
/// limit.
pub(super) fn get(
    scope: &CacheScope,
    references: &[&str],
) -> Result<Zeroizing<Vec<u8>>, AgentError> {
    if references.len() > REFERENCE_LIMIT {
        return Err(AgentError::TooManyReferences);
    }
    let op = op_text(scope)?;
    let size = text_size(scope.account())
        + text_size(op)
        + 2
        + references.iter().map(|r| text_size(r)).sum::<usize>();
    let mut frame = Frame::new(GET, size)?;
    frame.text(scope.account())?;
    frame.text(op)?;
    frame.u16(references.len())?;
    for reference in references {
        frame.text(reference)?;
    }
    Ok(frame.0)
}

/// Returns the frame of one Put of `values`, each a reference and its
/// value, in `scope`, carrying `generation`.
///
/// # Errors
///
/// Returns [`AgentError::TooManyReferences`] for more than 256 values, and
/// [`AgentError::Protocol`] for no value, a value over 64 KiB, a text over
/// 64 KiB or a frame over its limit.
pub(super) fn put(
    scope: &CacheScope,
    values: &[(&str, &Secret)],
    generation: Generation,
) -> Result<Zeroizing<Vec<u8>>, AgentError> {
    if values.is_empty() {
        return Err(malformed("a Put holds no value"));
    }
    if values.len() > REFERENCE_LIMIT {
        return Err(AgentError::TooManyReferences);
    }
    let op = op_text(scope)?;
    let mut size = text_size(scope.account()) + text_size(op) + 8 + 2;
    for (reference, value) in values {
        let value = exposed(value);
        if value.len() > VALUE_LIMIT {
            return Err(malformed("a value is over 64 KiB"));
        }
        size += text_size(reference) + 4 + value.len();
    }
    let mut frame = Frame::new(PUT, size)?;
    frame.text(scope.account())?;
    frame.text(op)?;
    frame.bytes(&generation.0.to_be_bytes());
    frame.u16(values.len())?;
    for (reference, value) in values {
        frame.text(reference)?;
        frame.value(exposed(value));
    }
    Ok(frame.0)
}

/// Returns the frame of a Clear.
pub(super) fn clear() -> Zeroizing<Vec<u8>> {
    Frame::empty(CLEAR)
}

/// Returns the frame of a Status request.
pub(super) fn status() -> Zeroizing<Vec<u8>> {
    Frame::empty(STATUS)
}

/// Returns the frame of a Hits at `generation` with an entry for each of
/// `values`, in request order: the value of a hit, or `None` for a miss.
/// Once the next hit would pass the [`BUDGET`], it and every later hit are
/// sent as over budget, and so is a value over [`VALUE_LIMIT`], which no
/// client would accept.
///
/// # Errors
///
/// Returns [`AgentError::TooManyReferences`] for more than 256 values,
/// because no Get asks for more.
pub fn hits(
    generation: Generation,
    values: &[Option<&Secret>],
) -> Result<Zeroizing<Vec<u8>>, AgentError> {
    if values.len() > REFERENCE_LIMIT {
        return Err(AgentError::TooManyReferences);
    }
    let mut total = 0_usize;
    let mut over = false;
    // For each entry: `None` for a miss, `Some(None)` for a hit over the
    // budget, and `Some(Some(bytes))` for a hit that is sent.
    let entries: Vec<Option<Option<&[u8]>>> = values
        .iter()
        .map(|value| {
            let bytes = exposed((*value)?);
            over = over || bytes.len() > VALUE_LIMIT || total + bytes.len() > BUDGET;
            if over {
                return Some(None);
            }
            total += bytes.len();
            Some(Some(bytes))
        })
        .collect();
    let sent = entries.iter().flatten().flatten().count();
    let mut frame = Frame::new(HITS, 8 + 2 + entries.len() + 4 * sent + total)?;
    frame.bytes(&generation.0.to_be_bytes());
    frame.u16(entries.len())?;
    for entry in entries {
        match entry {
            Some(Some(bytes)) => {
                frame.bytes(&[HIT]);
                frame.value(bytes);
            }
            Some(None) => frame.bytes(&[OVER_BUDGET]),
            None => frame.bytes(&[MISS]),
        }
    }
    Ok(frame.0)
}

/// Returns the frame of a Stored with `outcomes`, one for each value of
/// the Put, in request order.
///
/// # Errors
///
/// Returns [`AgentError::TooManyReferences`] for more than 256 outcomes,
/// because no Put holds more.
pub fn stored(outcomes: &[PutOutcome]) -> Result<Zeroizing<Vec<u8>>, AgentError> {
    if outcomes.len() > REFERENCE_LIMIT {
        return Err(AgentError::TooManyReferences);
    }
    let mut frame = Frame::new(STORED, 2 + outcomes.len())?;
    frame.u16(outcomes.len())?;
    for outcome in outcomes {
        frame.bytes(&[match outcome {
            PutOutcome::Stored => STORED_VALUE,
            PutOutcome::AlreadyHeld => ALREADY_HELD,
        }]);
    }
    Ok(frame.0)
}

/// Returns the frame of a Cleared that dropped `values` values.
#[must_use]
pub fn cleared(values: u32) -> Zeroizing<Vec<u8>> {
    let mut frame = Frame::fixed(CLEARED, 4);
    frame.bytes(&values.to_be_bytes());
    frame.0
}

/// Returns the frame of the agent's answer to a Status request.
#[must_use]
pub fn status_answer(status: &AgentStatus) -> Zeroizing<Vec<u8>> {
    let mut frame = Frame::fixed(STATUS_ANSWER, 8 + 4 + 4 + 8 + 8);
    frame.bytes(&status.generation().0.to_be_bytes());
    frame.bytes(&status.values().to_be_bytes());
    frame.bytes(&status.pid().to_be_bytes());
    frame.bytes(&unix_seconds(status.unlocked_at()).to_be_bytes());
    frame.bytes(&unix_seconds(status.lapses_at()).to_be_bytes());
    frame.0
}

/// Returns the frame of a refusal with the code of `error`. The frame
/// carries the code alone, so no message, reference or value reaches the
/// client.
#[must_use]
pub fn refused(error: &AgentError) -> Zeroizing<Vec<u8>> {
    let code = error.code();
    let mut frame = Frame::fixed(REFUSED, text_size(code));
    // A code is a short constant, so its length always fits in a u16.
    let _ = frame.text(code);
    frame.0
}

/// Returns the bytes of `value`, which a Put and a Hits write into their
/// frames. It is the one place in the module that reads a value's text.
fn exposed(value: &Secret) -> &[u8] {
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-24: a Put and a Hits write the value into the frame that the agent's socket carries"
    )]
    value.expose().as_bytes()
}

/// Returns the op path of `scope` as text.
fn op_text(scope: &CacheScope) -> Result<&str, AgentError> {
    scope
        .op()
        .to_str()
        .ok_or_else(|| malformed("the op path is not UTF-8"))
}

/// Returns the bytes that `text` takes in a frame.
fn text_size(text: &str) -> usize {
    2 + text.len()
}

/// Returns `time` in whole Unix seconds, or 0 for `None`. A time before
/// 1970 is negative.
fn unix_seconds(time: Option<SystemTime>) -> i64 {
    match time.map(|time| time.duration_since(UNIX_EPOCH)) {
        None => 0,
        Some(Ok(after)) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Some(Err(before)) => {
            i64::try_from(before.duration().as_secs()).map_or(i64::MIN, |seconds| -seconds)
        }
    }
}

/// Returns the time `seconds` after the Unix epoch, or `None` for 0.
fn from_unix_seconds(seconds: i64) -> Result<Option<SystemTime>, AgentError> {
    let span = Duration::from_secs(seconds.unsigned_abs());
    let time = match seconds {
        0 => return Ok(None),
        1.. => UNIX_EPOCH.checked_add(span),
        _ => UNIX_EPOCH.checked_sub(span),
    };
    time.map(Some)
        .ok_or_else(|| malformed("a time is out of range"))
}

/// A frame being written, whose buffer holds its full size from the start.
struct Frame(Zeroizing<Vec<u8>>);

impl Frame {
    /// Starts the frame of `kind` whose fields take `size` bytes.
    fn new(kind: u8, size: usize) -> Result<Self, AgentError> {
        if size + 2 > FRAME_LIMIT {
            return Err(malformed("the frame is over 1 MiB"));
        }
        Ok(Self::fixed(kind, size))
    }

    /// Starts the frame of `kind` whose fields take `size` bytes, a size
    /// that the caller knows to be far within [`FRAME_LIMIT`].
    fn fixed(kind: u8, size: usize) -> Self {
        let body = size + 2;
        let mut bytes = Zeroizing::new(Vec::with_capacity(4 + body));
        bytes.extend_from_slice(&u32::try_from(body).unwrap_or(u32::MAX).to_be_bytes());
        bytes.extend_from_slice(&[VERSION, kind]);
        Self(bytes)
    }

    /// Returns the frame of `kind` with no field.
    fn empty(kind: u8) -> Zeroizing<Vec<u8>> {
        Self::fixed(kind, 0).0
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn u16(&mut self, value: usize) -> Result<(), AgentError> {
        let value = u16::try_from(value).map_err(|_| malformed("a text is over 64 KiB"))?;
        self.bytes(&value.to_be_bytes());
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<(), AgentError> {
        self.u16(text.len())?;
        self.bytes(text.as_bytes());
        Ok(())
    }

    /// Writes a `u32` length and `value`, which the caller has checked
    /// against [`VALUE_LIMIT`].
    fn value(&mut self, value: &[u8]) {
        self.bytes(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
        self.bytes(value);
    }
}

/// Returns the request that `body`, a frame's body, holds, after every
/// parser rule of the module.
///
/// # Errors
///
/// Returns [`AgentError::TooManyReferences`] for a Get or a Put of more
/// than 256 references, and [`AgentError::Protocol`] for any other broken
/// rule, an unknown kind, or an unknown version on anything but Clear.
pub fn request(body: &[u8]) -> Result<Request, AgentError> {
    let mut reader = Reader(body);
    let version = reader.u8()?;
    let kind = reader.u8()?;
    if kind == CLEAR {
        reader.end()?;
        return Ok(Request::Clear);
    }
    if version != VERSION {
        return Err(malformed("the request is of another protocol version"));
    }
    let request = match kind {
        GET => {
            let (account, op) = (reader.owned_text()?, reader.owned_text()?);
            let count = reader.count()?;
            let mut references = Vec::with_capacity(count);
            for _ in 0..count {
                references.push(reader.owned_text()?);
            }
            Request::Get {
                account,
                op,
                references,
            }
        }
        PUT => {
            let (account, op) = (reader.owned_text()?, reader.owned_text()?);
            let generation = Generation(reader.u64()?);
            let count = reader.count()?;
            if count == 0 {
                return Err(malformed("a Put holds no value"));
            }
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                let reference = reader.owned_text()?;
                values.push((reference, reader.value()?));
            }
            Request::Put {
                account,
                op,
                generation,
                values,
            }
        }
        STATUS => Request::Status,
        _ => return Err(malformed("the request is of an unknown kind")),
    };
    reader.end()?;
    Ok(request)
}

/// Returns the answer that `body`, a frame's body, holds, when it is the
/// answer `expect` names or a refusal, after every parser rule of the
/// module. A Hits or a Stored must have the count of its request, and
/// outcome bytes of its own kind.
///
/// # Errors
///
/// Returns the error a refusal names, and [`AgentError::Protocol`] for a
/// malformed frame, an unknown version, or an answer of another kind.
pub(super) fn response(body: &[u8], expect: Expect) -> Result<Response, AgentError> {
    let mut reader = Reader(body);
    if reader.u8()? != VERSION {
        return Err(malformed("the agent speaks another protocol version"));
    }
    let response = match (reader.u8()?, expect) {
        (REFUSED, _) => {
            let code = reader.text()?;
            reader.end()?;
            return Err(AgentError::refused(code));
        }
        (HITS, Expect::Hits(asked)) => read_hits(&mut reader, asked)?,
        (STORED, Expect::Stored(asked)) => read_stored(&mut reader, asked)?,
        (CLEARED, Expect::Cleared) => Response::Cleared(reader.u32()?),
        (STATUS_ANSWER, Expect::Status) => Response::Status(AgentStatus::new(
            Generation(reader.u64()?),
            reader.u32()?,
            reader.u32()?,
            from_unix_seconds(reader.i64()?)?,
            from_unix_seconds(reader.i64()?)?,
        )),
        _ => return Err(malformed("the agent answered with another kind of frame")),
    };
    reader.end()?;
    Ok(response)
}

/// Reads the body of a Hits that answers a Get of `asked` references.
fn read_hits(reader: &mut Reader<'_>, asked: usize) -> Result<Response, AgentError> {
    let generation = Generation(reader.u64()?);
    let count = usize::from(reader.u16()?);
    if count > REFERENCE_LIMIT || count != asked {
        return Err(malformed("the hits do not match the references asked for"));
    }
    let mut values = Vec::with_capacity(count);
    let mut total = 0_usize;
    for _ in 0..count {
        values.push(match reader.u8()? {
            HIT => {
                let value = reader.value()?;
                total = total.saturating_add(exposed(&value).len());
                if total > BUDGET {
                    return Err(malformed("the hits are over their budget"));
                }
                Some(value)
            }
            MISS | OVER_BUDGET => None,
            _ => return Err(malformed("a hit has an unknown outcome")),
        });
    }
    Ok(Response::Hits(generation, values))
}

/// Reads the body of a Stored that answers a Put of `asked` values.
fn read_stored(reader: &mut Reader<'_>, asked: usize) -> Result<Response, AgentError> {
    let count = usize::from(reader.u16()?);
    if count > REFERENCE_LIMIT || count != asked {
        return Err(malformed("the outcomes do not match the values put"));
    }
    let mut outcomes = Vec::with_capacity(count);
    for _ in 0..count {
        outcomes.push(match reader.u8()? {
            STORED_VALUE => PutOutcome::Stored,
            ALREADY_HELD => PutOutcome::AlreadyHeld,
            _ => return Err(malformed("a stored value has an unknown outcome")),
        });
    }
    Ok(Response::Stored(outcomes))
}

/// Returns the protocol error for a frame that breaks a parser rule.
fn malformed(reason: &str) -> AgentError {
    AgentError::Protocol(reason.to_owned())
}

/// The bytes of a frame's body still to read.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    /// Takes the next `count` bytes, once they are there.
    fn bytes(&mut self, count: usize) -> Result<&'a [u8], AgentError> {
        let (taken, left) = self
            .0
            .split_at_checked(count)
            .ok_or_else(|| malformed("a field runs past the end of the frame"))?;
        self.0 = left;
        Ok(taken)
    }

    /// Takes the next `N` bytes as an array.
    fn array<const N: usize>(&mut self) -> Result<[u8; N], AgentError> {
        let mut array = [0; N];
        array.copy_from_slice(self.bytes(N)?);
        Ok(array)
    }

    fn u8(&mut self) -> Result<u8, AgentError> {
        Ok(u8::from_be_bytes(self.array()?))
    }

    fn u16(&mut self) -> Result<u16, AgentError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, AgentError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, AgentError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn i64(&mut self) -> Result<i64, AgentError> {
        Ok(i64::from_be_bytes(self.array()?))
    }

    /// Takes a request's `u16` count of references, refused over 256
    /// before anything is allocated for it.
    fn count(&mut self) -> Result<usize, AgentError> {
        let count = usize::from(self.u16()?);
        if count > REFERENCE_LIMIT {
            return Err(AgentError::TooManyReferences);
        }
        Ok(count)
    }

    /// Takes a `u16` length and that much UTF-8.
    fn text(&mut self) -> Result<&'a str, AgentError> {
        let length = usize::from(self.u16()?);
        std::str::from_utf8(self.bytes(length)?).map_err(|_| malformed("a text is not UTF-8"))
    }

    /// Takes a text and returns it as an owned string.
    fn owned_text(&mut self) -> Result<String, AgentError> {
        self.text().map(str::to_owned)
    }

    /// Takes a `u32` length, at most [`VALUE_LIMIT`], and that many bytes
    /// as a secret. A value that is not UTF-8 is zeroed and refused.
    fn value(&mut self) -> Result<Secret, AgentError> {
        let length = usize::try_from(self.u32()?).unwrap_or(usize::MAX);
        if length > VALUE_LIMIT {
            return Err(malformed("a value is over 64 KiB"));
        }
        match String::from_utf8(self.bytes(length)?.to_vec()) {
            Ok(text) => Ok(Secret::new(text)),
            Err(error) => {
                drop(Zeroizing::new(error.into_bytes()));
                Err(malformed("a value is not UTF-8"))
            }
        }
    }

    /// Checks that no byte is left.
    fn end(&self) -> Result<(), AgentError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(malformed("the frame has bytes after its last field"))
        }
    }
}

/// Returns the time left before `deadline` for one wait, at most two
/// seconds, or `None` once it has passed.
pub(super) fn wait(deadline: Instant) -> Option<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    (!left.is_zero()).then(|| left.min(WAIT_LIMIT))
}

/// Returns the time left before `deadline` for one wait, as [`wait`]
/// does, or a timed-out error once it has passed.
fn left(deadline: Instant) -> io::Result<Duration> {
    wait(deadline).ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "the deadline passed"))
}

/// Accepts the outcome of setting a socket's timeout. macOS refuses a new
/// timeout with EINVAL once the peer has closed its end. A read or a write
/// then returns at once, with what is buffered, the end of the stream or
/// an error, so no wait is left to limit.
fn timeout_set(outcome: io::Result<()>) -> io::Result<()> {
    match outcome {
        Err(error) if error.kind() != io::ErrorKind::InvalidInput => Err(error),
        _ => Ok(()),
    }
}

/// Reads one frame from `stream` by `deadline` and returns its body. The
/// body's buffer is allocated at its full size, and a length over
/// [`FRAME_LIMIT`] is refused before anything is allocated. Each wait for
/// more bytes takes at most two seconds and ends at `deadline`, so a peer
/// that sends one byte at a time cannot hold the reader past it.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidData`] for a length over the limit,
/// [`io::ErrorKind::TimedOut`] once `deadline` passes,
/// [`io::ErrorKind::UnexpectedEof`] when the peer closes early, and the
/// socket's own errors.
pub fn read_frame(stream: &mut UnixStream, deadline: Instant) -> io::Result<Zeroizing<Vec<u8>>> {
    let mut length = [0_u8; 4];
    read_by(stream, &mut length, deadline)?;
    let length = usize::try_from(u32::from_be_bytes(length)).unwrap_or(usize::MAX);
    if length > FRAME_LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the frame is over 1 MiB",
        ));
    }
    let mut body = Zeroizing::new(vec![0_u8; length]);
    read_by(stream, &mut body, deadline)?;
    Ok(body)
}

/// Writes `frame`, its length and body, to `stream` by `deadline`. Each
/// wait for the peer to take more bytes takes at most two seconds and ends
/// at `deadline`.
///
/// # Errors
///
/// Returns [`io::ErrorKind::TimedOut`] once `deadline` passes,
/// [`io::ErrorKind::WriteZero`] when the socket takes no byte, and the
/// socket's own errors.
pub fn write_frame(stream: &mut UnixStream, frame: &[u8], deadline: Instant) -> io::Result<()> {
    let mut written = 0;
    while let Some(rest) = frame.get(written..).filter(|rest| !rest.is_empty()) {
        timeout_set(stream.set_write_timeout(Some(left(deadline)?)))?;
        match stream.write(rest) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(count) => written += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Fills `buffer` from `stream`, and fails once `deadline` passes. Each
/// read waits only for the time left, because a socket's read timeout
/// limits one read, and a peer that sends one byte at a time would
/// otherwise hold the reader for a timeout per byte.
fn read_by(stream: &mut UnixStream, buffer: &mut [u8], deadline: Instant) -> io::Result<()> {
    let mut filled = 0;
    while let Some(rest) = buffer.get_mut(filled..).filter(|rest| !rest.is_empty()) {
        timeout_set(stream.set_read_timeout(Some(left(deadline)?)))?;
        match stream.read(rest) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
