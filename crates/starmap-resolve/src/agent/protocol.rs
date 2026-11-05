//! The client's half of the agent's wire protocol, version 1: the frames
//! it sends and the parser of the frames it receives.
//!
//! Every frame is a 4-byte big-endian length, at most 1 MiB, and a body
//! that starts with the version byte and a kind byte. Text is a `u16`
//! length and UTF-8. The format is written by hand, so no value passes
//! through serde, and every buffer that may hold a value is a
//! [`Zeroizing`] buffer allocated at its full size, so writing or reading
//! never reallocates it and leaves a copy behind.
//!
//! This module writes the two requests a resolution needs, Get (0x01) and
//! Put (0x02), and reads their answers, Hits (0x81) and Stored (0x82), and
//! a refusal (0xFF). Clear and Status come with the agent's commands.
//!
//! The generation is an opaque token of each agent instance: 64 random
//! bits that the agent draws when it starts and draws again on every
//! clear. A Get returns it, and a Put carries back the one its Get
//! returned. The agent stores the value only when the token equals its
//! own, so a value read before a clear, or read from an agent that has
//! since stopped and started again, never lands. The client never compares
//! or increases a generation; it only echoes it.

use starmap::Secret;
use zeroize::Zeroizing;

use super::{AgentError, CacheScope, Generation};

#[cfg(test)]
mod tests;

/// The protocol's version.
const VERSION: u8 = 1;

const GET: u8 = 0x01;
const PUT: u8 = 0x02;
const HITS: u8 = 0x81;
const STORED: u8 = 0x82;
const REFUSED: u8 = 0xFF;

/// The most bytes of one frame's body.
pub(super) const FRAME_LIMIT: usize = 1024 * 1024;

/// The most references of one Get.
pub(super) const REFERENCE_LIMIT: usize = 256;

/// The most bytes of one value.
const VALUE_LIMIT: usize = 64 * 1024;

/// The most bytes of all the values of one Hits.
const BUDGET: usize = 512 * 1024;

/// The answer a request expects, besides a refusal.
#[derive(Debug, Clone, Copy)]
pub(super) enum Expect {
    /// The hits of a Get of this many references.
    Hits(usize),
    Stored,
}

/// What the agent answered.
#[derive(Debug)]
pub(super) enum Response {
    /// The generation and, in request order, each hit's value or `None`
    /// for a miss or a hit over the budget.
    Hits(Generation, Vec<Option<Secret<String>>>),
    Stored,
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

/// Returns the frame of a Put of `value` for `reference` in `scope`,
/// carrying `generation`.
///
/// # Errors
///
/// Returns [`AgentError::Protocol`] for a value over 64 KiB, a text over
/// 64 KiB or a frame over its limit.
pub(super) fn put(
    scope: &CacheScope,
    reference: &str,
    value: &Secret<String>,
    generation: Generation,
) -> Result<Zeroizing<Vec<u8>>, AgentError> {
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-24: a Put writes the value into its frame, which the socket carries"
    )]
    let value = value.expose().as_bytes();
    if value.len() > VALUE_LIMIT {
        return Err(AgentError::Protocol("the value is over 64 KiB".to_owned()));
    }
    let op = op_text(scope)?;
    let size =
        text_size(scope.account()) + text_size(op) + text_size(reference) + 8 + 4 + value.len();
    let mut frame = Frame::new(PUT, size)?;
    frame.text(scope.account())?;
    frame.text(op)?;
    frame.text(reference)?;
    frame.0.extend_from_slice(&generation.0.to_be_bytes());
    frame
        .0
        .extend_from_slice(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
    frame.0.extend_from_slice(value);
    Ok(frame.0)
}

/// Returns the op path of `scope` as text.
fn op_text(scope: &CacheScope) -> Result<&str, AgentError> {
    scope
        .op()
        .to_str()
        .ok_or_else(|| AgentError::Protocol("the op path is not UTF-8".to_owned()))
}

/// Returns the bytes that `text` takes in a frame.
fn text_size(text: &str) -> usize {
    2 + text.len()
}

/// A frame being written, whose buffer holds its full size from the start.
struct Frame(Zeroizing<Vec<u8>>);

impl Frame {
    /// Starts the frame of `kind` whose fields take `size` bytes.
    fn new(kind: u8, size: usize) -> Result<Self, AgentError> {
        let body = size + 2;
        if body > FRAME_LIMIT {
            return Err(AgentError::Protocol("the request is over 1 MiB".to_owned()));
        }
        let mut bytes = Zeroizing::new(Vec::with_capacity(4 + body));
        bytes.extend_from_slice(&u32::try_from(body).unwrap_or(u32::MAX).to_be_bytes());
        bytes.extend_from_slice(&[VERSION, kind]);
        Ok(Self(bytes))
    }

    fn u16(&mut self, value: usize) -> Result<(), AgentError> {
        let value = u16::try_from(value)
            .map_err(|_| AgentError::Protocol("a text is over 64 KiB".to_owned()))?;
        self.0.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<(), AgentError> {
        self.u16(text.len())?;
        self.0.extend_from_slice(text.as_bytes());
        Ok(())
    }
}

/// Returns the response that `body`, a frame's body, holds, when it is the
/// answer `expect` names or a refusal. It applies every parser rule: each
/// length is checked against the bytes left before it is read, a count
/// over 256 is refused before anything is allocated for it, a Hits must
/// have the count of its Get and outcome bytes of 0, 1 or 2, every text
/// and value must be UTF-8, and no byte may follow the last field. A value
/// that is not UTF-8 is zeroed before it is dropped.
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
        (HITS, Expect::Hits(asked)) => hits(&mut reader, asked)?,
        (STORED, Expect::Stored) => Response::Stored,
        _ => return Err(malformed("the agent answered with another kind of frame")),
    };
    reader.end()?;
    Ok(response)
}

/// Reads the body of a Hits that answers a Get of `asked` references.
fn hits(reader: &mut Reader<'_>, asked: usize) -> Result<Response, AgentError> {
    let generation = Generation(reader.u64()?);
    let count = usize::from(reader.u16()?);
    if count > REFERENCE_LIMIT || count != asked {
        return Err(malformed("the hits do not match the references asked for"));
    }
    let mut values = Vec::with_capacity(count);
    let mut total = 0_usize;
    for _ in 0..count {
        values.push(match reader.u8()? {
            0 => {
                let length = usize::try_from(reader.u32()?).unwrap_or(usize::MAX);
                total = total.saturating_add(length);
                if length > VALUE_LIMIT || total > BUDGET {
                    return Err(malformed("a value is over its limit"));
                }
                Some(value(reader.bytes(length)?)?)
            }
            1 | 2 => None,
            _ => return Err(malformed("a hit has an unknown outcome")),
        });
    }
    Ok(Response::Hits(generation, values))
}

/// Returns `bytes` as a secret, or zeroes the copy and refuses it when it
/// is not UTF-8.
fn value(bytes: &[u8]) -> Result<Secret<String>, AgentError> {
    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => Ok(Secret::new(text)),
        Err(error) => {
            drop(Zeroizing::new(error.into_bytes()));
            Err(malformed("a value is not UTF-8"))
        }
    }
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

    /// Takes a `u16` length and that much UTF-8.
    fn text(&mut self) -> Result<&'a str, AgentError> {
        let length = usize::from(self.u16()?);
        std::str::from_utf8(self.bytes(length)?).map_err(|_| malformed("a text is not UTF-8"))
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
