use std::path::PathBuf;

use starmap::Secret;

use super::{Expect, Response, get, put, response};
use crate::agent::{AgentError, CacheScope, Generation};

fn scope() -> CacheScope {
    CacheScope::new(PathBuf::from("/opt/op"), [("OP_ACCOUNT", "my")]).unwrap()
}

/// Returns a Hits body of `generation` with each of `entries`, an outcome
/// byte and, for a hit, the value.
fn hits(generation: u64, entries: &[(u8, &[u8])]) -> Vec<u8> {
    let mut body = vec![1, 0x81];
    body.extend_from_slice(&generation.to_be_bytes());
    body.extend_from_slice(&u16::try_from(entries.len()).unwrap().to_be_bytes());
    for (outcome, value) in entries {
        body.push(*outcome);
        if *outcome == 0 {
            body.extend_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
            body.extend_from_slice(value);
        }
    }
    body
}

fn code(result: Result<Response, AgentError>) -> &'static str {
    result.unwrap_err().code()
}

#[expect(
    clippy::disallowed_methods,
    reason = "the tests check the values a Hits returned"
)]
fn texts(response: Response) -> (Generation, Vec<Option<String>>) {
    let Response::Hits(generation, values) = response else {
        panic!("not a Hits: {response:?}");
    };
    let values = values
        .iter()
        .map(|value| value.as_ref().map(|value| value.expose().clone()))
        .collect();
    (generation, values)
}

#[test]
fn a_get_frame_holds_its_length_version_kind_scope_and_references_in_order() {
    let frame = get(&scope(), &["op://a/b/c", "op://d/e/f"]).unwrap();
    let mut body = vec![1, 0x01, 0, 2, b'm', b'y', 0, 7];
    body.extend_from_slice(b"/opt/op");
    body.extend_from_slice(&[0, 2, 0, 10]);
    body.extend_from_slice(b"op://a/b/c");
    body.extend_from_slice(&[0, 10]);
    body.extend_from_slice(b"op://d/e/f");
    let mut expected = u32::try_from(body.len()).unwrap().to_be_bytes().to_vec();
    expected.extend_from_slice(&body);
    assert_eq!(*frame, expected);
    assert_eq!(frame.capacity(), frame.len());
}

#[test]
fn a_get_of_257_references_is_refused_and_one_of_256_is_written() {
    let references = vec!["op://a/b/c"; 257];
    assert_eq!(
        get(&scope(), &references).unwrap_err().code(),
        "starmap::agent::too_many_references"
    );
    assert!(get(&scope(), &references[..256]).is_ok());
}

#[test]
fn a_put_frame_carries_the_generation_and_the_value_in_a_buffer_of_its_full_size() {
    let frame = put(
        &scope(),
        "op://a/b/c",
        &Secret::new("v4lue".to_owned()),
        Generation(7),
    )
    .unwrap();
    let tail: Vec<u8> = [&7_u64.to_be_bytes()[..], &5_u32.to_be_bytes()[..], b"v4lue"].concat();
    assert!(frame.ends_with(&tail));
    assert_eq!(frame.capacity(), frame.len());
}

#[test]
fn a_put_of_a_value_over_64_kib_is_refused_and_one_of_64_kib_is_written() {
    let refused = Secret::new("x".repeat(64 * 1024 + 1));
    let admitted = Secret::new("x".repeat(64 * 1024));
    assert_eq!(
        put(&scope(), "op://a/b/c", &refused, Generation(0))
            .unwrap_err()
            .code(),
        "starmap::agent::protocol"
    );
    assert!(put(&scope(), "op://a/b/c", &admitted, Generation(0)).is_ok());
}

#[test]
fn a_hits_returns_each_hit_in_request_order_and_misses_and_over_budget_as_none() {
    let body = hits(9, &[(0, b"one"), (1, b""), (2, b""), (0, b"two")]);
    let (generation, values) = texts(response(&body, Expect::Hits(4)).unwrap());
    assert_eq!(generation, Generation(9));
    assert_eq!(
        values,
        [Some("one".to_owned()), None, None, Some("two".to_owned())]
    );
}

#[test]
fn a_hits_with_another_count_than_the_get_is_refused() {
    let body = hits(0, &[(1, b""), (1, b"")]);
    assert_eq!(
        code(response(&body, Expect::Hits(3))),
        "starmap::agent::protocol"
    );
    assert!(response(&body, Expect::Hits(2)).is_ok());
}

#[test]
fn a_hits_count_over_256_is_refused_before_anything_is_read_for_it() {
    // The count says 257 and no entry follows, so a parser that allocated
    // or read for the count first would fail another way.
    let mut body = vec![1, 0x81];
    body.extend_from_slice(&0_u64.to_be_bytes());
    body.extend_from_slice(&257_u16.to_be_bytes());
    assert_eq!(
        code(response(&body, Expect::Hits(257))),
        "starmap::agent::protocol"
    );
}

#[test]
fn an_outcome_byte_of_3_is_refused_and_2_is_admitted() {
    assert_eq!(
        code(response(&hits(0, &[(3, b"")]), Expect::Hits(1))),
        "starmap::agent::protocol"
    );
    assert!(response(&hits(0, &[(2, b"")]), Expect::Hits(1)).is_ok());
}

#[test]
fn a_length_past_the_end_of_the_frame_is_refused() {
    let mut body = hits(0, &[(0, b"abc")]);
    body.truncate(body.len() - 1);
    assert_eq!(
        code(response(&body, Expect::Hits(1))),
        "starmap::agent::protocol"
    );
    // A generation of 7 bytes, one short of its 8.
    let short = [1, 0x81, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        code(response(&short, Expect::Hits(0))),
        "starmap::agent::protocol"
    );
}

#[test]
fn a_frame_with_bytes_after_its_last_field_is_refused() {
    let mut body = hits(0, &[(0, b"abc")]);
    assert!(response(&body, Expect::Hits(1)).is_ok());
    body.push(0);
    assert_eq!(
        code(response(&body, Expect::Hits(1))),
        "starmap::agent::protocol"
    );
}

#[test]
fn a_value_that_is_not_utf8_is_refused_and_a_utf8_one_is_admitted() {
    assert_eq!(
        code(response(&hits(0, &[(0, &[0xff, 0xfe])]), Expect::Hits(1))),
        "starmap::agent::protocol"
    );
    assert!(response(&hits(0, &[(0, "é".as_bytes())]), Expect::Hits(1)).is_ok());
}

#[test]
fn hits_over_the_512_kib_budget_are_refused() {
    let value = vec![b'x'; 64 * 1024];
    let at_budget: Vec<(u8, &[u8])> = (0..8).map(|_| (0, value.as_slice())).collect();
    assert!(response(&hits(0, &at_budget), Expect::Hits(8)).is_ok());
    let over: Vec<(u8, &[u8])> = (0..9).map(|_| (0, value.as_slice())).collect();
    assert_eq!(
        code(response(&hits(0, &over), Expect::Hits(9))),
        "starmap::agent::protocol"
    );
}

#[test]
fn an_unknown_version_byte_is_refused() {
    let mut body = hits(0, &[]);
    body[0] = 9;
    assert_eq!(
        code(response(&body, Expect::Hits(0))),
        "starmap::agent::protocol"
    );
}

#[test]
fn an_answer_of_another_kind_than_the_request_is_refused() {
    assert_eq!(
        code(response(&[1, 0x82], Expect::Hits(0))),
        "starmap::agent::protocol"
    );
    assert!(matches!(
        response(&[1, 0x82], Expect::Stored),
        Ok(Response::Stored)
    ));
}

#[test]
fn a_refusal_returns_the_error_of_its_code_and_an_unknown_code_is_a_protocol_error() {
    let refusal = |code: &str| {
        let mut body = vec![1, 0xFF];
        body.extend_from_slice(&u16::try_from(code.len()).unwrap().to_be_bytes());
        body.extend_from_slice(code.as_bytes());
        body
    };
    assert_eq!(
        code(response(
            &refusal("starmap::agent::stale_generation"),
            Expect::Stored
        )),
        "starmap::agent::stale_generation"
    );
    assert_eq!(
        code(response(&refusal("starmap::agent::exists"), Expect::Stored)),
        "starmap::agent::exists"
    );
    let unknown = response(&refusal("op://Personal/shelf/token"), Expect::Stored).unwrap_err();
    assert_eq!(unknown.code(), "starmap::agent::protocol");
    assert!(!unknown.to_string().contains("op://"));
}
