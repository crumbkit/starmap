use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};
#[cfg(feature = "agent-server")]
use std::time::{SystemTime, UNIX_EPOCH};

use starmap::{OnePasswordRef, Secret};

use super::{
    Expect, FRAME_LIMIT, Response, VALUE_LIMIT, get, put, put_groups, read_frame, response,
    write_frame,
};
#[cfg(feature = "agent-server")]
use super::{Request, cleared, hits, refused, request, status_answer, stored};
use crate::agent::{AgentError, CacheScope, Generation};
#[cfg(feature = "agent-server")]
use crate::agent::{AgentStatus, PutOutcome};

fn scope() -> CacheScope {
    CacheScope::new(PathBuf::from("/opt/op"), [("OP_ACCOUNT", "my")]).unwrap()
}

/// Returns a Hits body of `generation` with each of `entries`, an outcome
/// byte and, for a hit, the value.
fn hits_body(generation: u64, entries: &[(u8, &[u8])]) -> Vec<u8> {
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

/// Returns the body of a Put of generation 5 in [`scope`] with the count
/// `count` and `entries`, each a reference and a value.
fn put_body(count: u16, entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut body = vec![1, 0x02, 0, 2, b'm', b'y', 0, 7];
    body.extend_from_slice(b"/opt/op");
    body.extend_from_slice(&5_u64.to_be_bytes());
    body.extend_from_slice(&count.to_be_bytes());
    for (reference, value) in entries {
        body.extend_from_slice(&u16::try_from(reference.len()).unwrap().to_be_bytes());
        body.extend_from_slice(reference.as_bytes());
        body.extend_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
        body.extend_from_slice(value);
    }
    body
}

/// Returns the body of `frame`, after its 4-byte length, once the length
/// is checked to be the body's.
fn body(frame: &[u8]) -> &[u8] {
    let length = usize::try_from(u32::from_be_bytes(frame[..4].try_into().unwrap())).unwrap();
    assert_eq!(length, frame.len() - 4, "the length prefix is the body's");
    &frame[4..]
}

fn code<T: std::fmt::Debug>(result: Result<T, AgentError>) -> &'static str {
    result.unwrap_err().code()
}

#[expect(
    clippy::disallowed_methods,
    reason = "the tests check the values that a frame carries"
)]
fn text(value: &Secret) -> String {
    value.expose().to_owned()
}

fn texts(response: Response) -> (Generation, Vec<Option<String>>) {
    let Response::Hits(generation, values) = response else {
        panic!("not a Hits: {response:?}");
    };
    (
        generation,
        values
            .iter()
            .map(|value| value.as_ref().map(text))
            .collect(),
    )
}

fn secret(text: &str) -> Secret {
    Secret::new(text.to_owned())
}

/// Returns 64 KiB of `x`, the largest value one frame may carry.
fn large_bytes() -> &'static [u8] {
    static LARGE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    LARGE.get_or_init(|| vec![b'x'; 64 * 1024])
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
fn a_put_frame_carries_the_generation_count_and_each_reference_and_value_at_its_full_size() {
    let (one, two) = (secret("v4lue"), secret("other"));
    let frame = put(
        &scope(),
        &[("op://a/b/c", &one), ("op://d/e/f", &two)],
        Generation(5),
    )
    .unwrap();
    let expected = put_body(2, &[("op://a/b/c", b"v4lue"), ("op://d/e/f", b"other")]);
    assert_eq!(body(&frame), expected);
    assert_eq!(frame.capacity(), frame.len());
}

#[test]
fn a_put_of_no_values_or_257_values_is_refused_and_one_of_256_is_written() {
    let value = secret("v");
    assert_eq!(
        code(put(&scope(), &[], Generation(0))),
        "starmap::agent::protocol"
    );
    let values = vec![("op://a/b/c", &value); 257];
    assert_eq!(
        code(put(&scope(), &values, Generation(0))),
        "starmap::agent::too_many_references"
    );
    assert!(put(&scope(), &values[..256], Generation(0)).is_ok());
}

#[test]
fn a_put_of_a_value_over_64_kib_is_refused_and_one_of_64_kib_is_written() {
    let refused = secret(&"x".repeat(64 * 1024 + 1));
    let admitted = secret(&"x".repeat(64 * 1024));
    assert_eq!(
        code(put(&scope(), &[("op://a/b/c", &refused)], Generation(0))),
        "starmap::agent::protocol"
    );
    assert!(put(&scope(), &[("op://a/b/c", &admitted)], Generation(0)).is_ok());
}

#[test]
fn a_put_whose_values_pass_the_frame_limit_is_refused() {
    let large = secret(&"x".repeat(64 * 1024));
    let values = vec![("op://a/b/c", &large); 16];
    assert_eq!(
        code(put(&scope(), &values, Generation(0))),
        "starmap::agent::protocol"
    );
    assert!(put(&scope(), &values[..15], Generation(0)).is_ok());
}

/// Returns `count` copies of the reference `op://a/b/c`, each paired with
/// `value`, as the splitter takes them.
fn group_input<'a>(
    reference: &'a OnePasswordRef,
    value: &'a Secret,
    count: usize,
) -> Vec<(&'a OnePasswordRef, &'a Secret)> {
    vec![(reference, value); count]
}

/// Returns the reference text and value of each entry of `group`, as
/// `put` takes them.
fn put_entries<'a>(group: &[(&'a OnePasswordRef, &'a Secret)]) -> Vec<(&'a str, &'a Secret)> {
    group
        .iter()
        .map(|(reference, value)| (reference.as_str(), *value))
        .collect()
}

#[test]
fn values_are_split_so_no_put_holds_more_than_256() {
    let reference: OnePasswordRef = "op://a/b/c".parse().unwrap();
    let value = secret("v");
    let groups = put_groups(&scope(), &group_input(&reference, &value, 300)).unwrap();
    assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(), [256, 44]);
    for group in &groups {
        assert!(put(&scope(), &put_entries(group), Generation(0)).is_ok());
    }
    let groups = put_groups(&scope(), &group_input(&reference, &value, 256)).unwrap();
    assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(), [256]);
}

#[test]
fn values_that_fill_a_frame_exactly_stay_in_one_put_and_one_more_byte_starts_another() {
    let reference: OnePasswordRef = "op://a/b/c".parse().unwrap();
    let large = secret(std::str::from_utf8(large_bytes()).unwrap());
    // The body holds the version and kind, the account, the op path, the
    // generation and the count, then each entry's reference, length and
    // value.
    let head = 2 + (2 + "my".len()) + (2 + "/opt/op".len()) + 8 + 2;
    let entry = 2 + reference.as_str().len() + 4;
    let filling = FRAME_LIMIT - head - 15 * (entry + VALUE_LIMIT) - entry;
    let fits = secret(&"y".repeat(filling));
    let over = secret(&"y".repeat(filling + 1));
    let values = |last: &'_ Secret| -> usize {
        let mut values = group_input(&reference, &large, 15);
        values.push((&reference, last));
        let groups = put_groups(&scope(), &values).unwrap();
        for group in &groups {
            assert!(put(&scope(), &put_entries(group), Generation(0)).is_ok());
        }
        groups.len()
    };
    assert_eq!(values(&fits), 1);
    assert_eq!(values(&over), 2);
}

#[test]
fn a_value_over_64_kib_is_left_out_of_every_put_and_no_group_is_empty() {
    let reference: OnePasswordRef = "op://a/b/c".parse().unwrap();
    let limit = secret(std::str::from_utf8(large_bytes()).unwrap());
    let over = secret(&"x".repeat(VALUE_LIMIT + 1));
    let groups = put_groups(&scope(), &[(&reference, &over), (&reference, &limit)]).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].len(), 1);
    assert_eq!(text(groups[0][0].1).len(), VALUE_LIMIT);
    assert!(
        put_groups(&scope(), &[(&reference, &over)])
            .unwrap()
            .is_empty()
    );
}

#[cfg(feature = "agent-server")]
#[test]
fn the_agent_parses_a_get_that_the_client_wrote() {
    let frame = get(&scope(), &["op://a/b/c", "op://d/e/f"]).unwrap();
    let Request::Get {
        account,
        op,
        references,
    } = request(body(&frame)).unwrap()
    else {
        panic!("not a Get");
    };
    assert_eq!((account.as_str(), op.as_str()), ("my", "/opt/op"));
    assert_eq!(references, ["op://a/b/c", "op://d/e/f"]);
}

#[cfg(feature = "agent-server")]
#[test]
fn the_agent_parses_a_put_that_the_client_wrote() {
    let (one, two) = (secret("v4lue"), secret("é"));
    let frame = put(
        &scope(),
        &[("op://a/b/c", &one), ("op://d/e/f", &two)],
        Generation(5),
    )
    .unwrap();
    let Request::Put {
        account,
        op,
        generation,
        values,
    } = request(body(&frame)).unwrap()
    else {
        panic!("not a Put");
    };
    assert_eq!((account.as_str(), op.as_str()), ("my", "/opt/op"));
    assert_eq!(generation, Generation(5));
    let values: Vec<(String, String)> = values
        .iter()
        .map(|(reference, value)| (reference.clone(), text(value)))
        .collect();
    assert_eq!(
        values,
        [
            ("op://a/b/c".to_owned(), "v4lue".to_owned()),
            ("op://d/e/f".to_owned(), "é".to_owned())
        ]
    );
}

#[cfg(feature = "agent-server")]
#[test]
fn the_agent_refuses_a_put_of_no_values_and_one_of_257_before_reading_any() {
    assert_eq!(code(request(&put_body(0, &[]))), "starmap::agent::protocol");
    // The count says 257 and no entry follows, so a parser that read for
    // the count first would fail another way.
    assert_eq!(
        code(request(&put_body(257, &[]))),
        "starmap::agent::too_many_references"
    );
    assert!(request(&put_body(1, &[("op://a/b/c", b"v")])).is_ok());
}

#[cfg(feature = "agent-server")]
#[test]
fn the_agent_refuses_a_put_value_over_64_kib_before_reading_it() {
    let mut body = put_body(1, &[]);
    body.extend_from_slice(&[0, 1, b'r']);
    body.extend_from_slice(&u32::try_from(64 * 1024 + 1).unwrap().to_be_bytes());
    body.resize(body.len() + 64 * 1024 + 1, b'x');
    let refused = request(&body).unwrap_err();
    assert_eq!(refused.code(), "starmap::agent::protocol");
    assert!(refused.to_string().contains("over 64 KiB"), "{refused}");
}

#[cfg(feature = "agent-server")]
#[test]
fn the_agent_refuses_an_account_op_path_or_reference_that_is_not_utf8() {
    let scoped = |account: &[u8], op: &[u8], reference: &[u8]| {
        let mut body = vec![1, 0x01];
        for text in [account, op] {
            body.extend_from_slice(&u16::try_from(text.len()).unwrap().to_be_bytes());
            body.extend_from_slice(text);
        }
        body.extend_from_slice(&[0, 1]);
        body.extend_from_slice(&u16::try_from(reference.len()).unwrap().to_be_bytes());
        body.extend_from_slice(reference);
        body
    };
    for body in [
        scoped(b"\xFF", b"/opt/op", b"op://a/b/c"),
        scoped(b"my", b"/opt/\xFF", b"op://a/b/c"),
        scoped(b"my", b"/opt/op", b"op://a/\xFF/c"),
    ] {
        let refused = request(&body).unwrap_err();
        assert_eq!(refused.code(), "starmap::agent::protocol");
        assert!(refused.to_string().contains("not UTF-8"), "{refused}");
    }
    assert!(request(&scoped(b"my", b"/opt/op", b"op://a/b/c")).is_ok());
}

#[cfg(feature = "agent-server")]
#[test]
fn the_agent_refuses_a_put_value_that_is_not_utf8() {
    assert_eq!(
        code(request(&put_body(1, &[("op://a/b/c", &[0xff, 0xfe])]))),
        "starmap::agent::protocol"
    );
}

#[cfg(feature = "agent-server")]
#[test]
fn the_agent_refuses_a_get_of_257_references_before_reading_any() {
    let mut body = vec![1, 0x01, 0, 0, 0, 0];
    body.extend_from_slice(&257_u16.to_be_bytes());
    assert_eq!(code(request(&body)), "starmap::agent::too_many_references");
}

#[cfg(feature = "agent-server")]
#[test]
fn the_agent_refuses_a_request_with_bytes_after_its_last_field() {
    let frame = get(&scope(), &["op://a/b/c"]).unwrap();
    let mut body = body(&frame).to_vec();
    body.push(0);
    assert_eq!(code(request(&body)), "starmap::agent::protocol");
    assert!(matches!(request(&[1, 0x04]), Ok(Request::Status)));
    assert_eq!(code(request(&[1, 0x04, 0])), "starmap::agent::protocol");
}

#[cfg(feature = "agent-server")]
#[test]
fn the_agent_accepts_a_clear_of_any_version_and_refuses_other_kinds_of_another_version() {
    assert!(matches!(request(&[9, 0x03]), Ok(Request::Clear)));
    assert!(matches!(request(&[1, 0x03]), Ok(Request::Clear)));
    assert_eq!(code(request(&[9, 0x04])), "starmap::agent::protocol");
    let frame = get(&scope(), &["op://a/b/c"]).unwrap();
    let mut body = body(&frame).to_vec();
    body[0] = 9;
    assert_eq!(code(request(&body)), "starmap::agent::protocol");
}

#[cfg(feature = "agent-server")]
#[test]
fn the_agent_refuses_an_unknown_kind_and_an_empty_body() {
    assert_eq!(code(request(&[1, 0x05])), "starmap::agent::protocol");
    assert_eq!(code(request(&[1, 0x81])), "starmap::agent::protocol");
    assert_eq!(code(request(&[])), "starmap::agent::protocol");
}

#[test]
fn a_hits_returns_each_hit_in_request_order_and_misses_and_over_budget_as_none() {
    let body = hits_body(9, &[(0, b"one"), (1, b""), (2, b""), (0, b"two")]);
    let (generation, values) = texts(response(&body, Expect::Hits(4)).unwrap());
    assert_eq!(generation, Generation(9));
    assert_eq!(
        values,
        [Some("one".to_owned()), None, None, Some("two".to_owned())]
    );
}

#[cfg(feature = "agent-server")]
#[expect(
    clippy::disallowed_methods,
    reason = "BASE-24: the test checks the frames that the Hits writer builds"
)]
#[test]
fn the_client_reads_the_hits_that_the_agent_wrote() {
    let (one, two) = (secret("one"), secret("two"));
    let frame = hits(Generation(9), &[Some(&one), None, Some(&two)]).unwrap();
    assert_eq!(frame.capacity(), frame.len());
    let (generation, values) = texts(response(body(&frame), Expect::Hits(3)).unwrap());
    assert_eq!(generation, Generation(9));
    assert_eq!(
        values,
        [Some("one".to_owned()), None, Some("two".to_owned())]
    );
}

#[cfg(feature = "agent-server")]
#[expect(
    clippy::disallowed_methods,
    reason = "BASE-24: the test checks the frames that the Hits writer builds"
)]
#[test]
fn the_hits_writer_sends_every_hit_after_the_first_over_budget_as_over_budget() {
    let large = secret(&"x".repeat(64 * 1024));
    let small = secret("small");
    // Eight values of 64 KiB fill the 512 KiB budget. The ninth would pass
    // it, and the small one after it is over budget too.
    let mut values = vec![Some(&large); 9];
    values.push(None);
    values.push(Some(&small));
    let frame = hits(Generation(1), &values).unwrap();
    assert_eq!(frame.capacity(), frame.len());
    let mut expected: Vec<(u8, &[u8])> = vec![(0, large_bytes()); 8];
    expected.extend([(2, &b""[..]), (1, b""), (2, b"")]);
    assert_eq!(body(&frame), hits_body(1, &expected));
}

#[cfg(feature = "agent-server")]
#[expect(
    clippy::disallowed_methods,
    reason = "BASE-24: the test checks the frames that the Hits writer builds"
)]
#[test]
fn the_hits_writer_refuses_more_than_256_entries() {
    assert_eq!(
        code(hits(Generation(0), &[None; 257])),
        "starmap::agent::too_many_references"
    );
}

#[test]
fn a_hits_with_another_count_than_the_get_is_refused() {
    let body = hits_body(0, &[(1, b""), (1, b"")]);
    assert_eq!(
        code(response(&body, Expect::Hits(3))),
        "starmap::agent::protocol"
    );
    assert!(response(&body, Expect::Hits(2)).is_ok());
}

#[test]
fn a_hits_count_over_256_is_refused_before_anything_is_read_for_it() {
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
        code(response(&hits_body(0, &[(3, b"")]), Expect::Hits(1))),
        "starmap::agent::protocol"
    );
    assert!(response(&hits_body(0, &[(2, b"")]), Expect::Hits(1)).is_ok());
}

#[test]
fn a_length_past_the_end_of_the_frame_is_refused() {
    let mut body = hits_body(0, &[(0, b"abc")]);
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
    let mut body = hits_body(0, &[(0, b"abc")]);
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
        code(response(
            &hits_body(0, &[(0, &[0xff, 0xfe])]),
            Expect::Hits(1)
        )),
        "starmap::agent::protocol"
    );
    assert!(response(&hits_body(0, &[(0, "é".as_bytes())]), Expect::Hits(1)).is_ok());
}

#[test]
fn hits_over_the_512_kib_budget_are_refused() {
    let at_budget: Vec<(u8, &[u8])> = (0..8).map(|_| (0, large_bytes())).collect();
    assert!(response(&hits_body(0, &at_budget), Expect::Hits(8)).is_ok());
    let over: Vec<(u8, &[u8])> = (0..9).map(|_| (0, large_bytes())).collect();
    assert_eq!(
        code(response(&hits_body(0, &over), Expect::Hits(9))),
        "starmap::agent::protocol"
    );
}

#[test]
fn an_unknown_version_byte_is_refused() {
    let mut body = hits_body(0, &[]);
    body[0] = 9;
    assert_eq!(
        code(response(&body, Expect::Hits(0))),
        "starmap::agent::protocol"
    );
}

#[test]
fn an_answer_of_another_kind_than_the_request_is_refused() {
    assert_eq!(
        code(response(&[1, 0x82, 0, 0], Expect::Hits(0))),
        "starmap::agent::protocol"
    );
    assert_eq!(
        code(response(&[1, 0x83, 0, 0, 0, 0], Expect::Status)),
        "starmap::agent::protocol"
    );
}

#[cfg(feature = "agent-server")]
#[test]
fn the_client_reads_the_stored_outcomes_that_the_agent_wrote() {
    let frame = stored(&[PutOutcome::Stored, PutOutcome::AlreadyHeld]).unwrap();
    let Response::Stored(outcomes) = response(body(&frame), Expect::Stored(2)).unwrap() else {
        panic!("not a Stored");
    };
    assert_eq!(outcomes, [PutOutcome::Stored, PutOutcome::AlreadyHeld]);
}

#[cfg(feature = "agent-server")]
#[test]
fn a_stored_with_another_count_than_the_put_is_refused() {
    let frame = stored(&[PutOutcome::Stored]).unwrap();
    assert_eq!(
        code(response(body(&frame), Expect::Stored(2))),
        "starmap::agent::protocol"
    );
}

#[test]
fn a_stored_outcome_byte_of_2_is_refused_and_1_is_admitted() {
    assert_eq!(
        code(response(&[1, 0x82, 0, 1, 2], Expect::Stored(1))),
        "starmap::agent::protocol"
    );
    assert!(response(&[1, 0x82, 0, 1, 1], Expect::Stored(1)).is_ok());
}

#[cfg(feature = "agent-server")]
#[test]
fn the_client_reads_the_count_of_a_cleared_that_the_agent_wrote() {
    let frame = cleared(3);
    assert!(matches!(
        response(body(&frame), Expect::Cleared),
        Ok(Response::Cleared(3))
    ));
}

#[cfg(feature = "agent-server")]
#[test]
fn the_client_reads_the_status_that_the_agent_wrote() {
    let unlocked = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let lapses = unlocked + Duration::from_secs(12 * 3600);
    let full = AgentStatus::new(Generation(7), 4, 321, Some(unlocked), Some(lapses));
    let empty = AgentStatus::new(Generation(8), 0, 322, None, None);
    for status in [full, empty] {
        let frame = status_answer(&status);
        let Response::Status(read) = response(body(&frame), Expect::Status).unwrap() else {
            panic!("not a Status");
        };
        assert_eq!(read, status);
    }
}

#[cfg(feature = "agent-server")]
#[test]
fn a_status_time_keeps_only_whole_seconds() {
    let unlocked = UNIX_EPOCH + Duration::from_millis(1_800_000_000_500);
    let status = AgentStatus::new(Generation(7), 1, 1, Some(unlocked), Some(SystemTime::now()));
    let Response::Status(read) = response(body(&status_answer(&status)), Expect::Status).unwrap()
    else {
        panic!("not a Status");
    };
    assert_eq!(
        read.unlocked_at(),
        Some(UNIX_EPOCH + Duration::from_secs(1_800_000_000))
    );
}

#[cfg(feature = "agent-server")]
#[test]
fn a_refusal_that_the_agent_wrote_returns_the_error_of_its_code() {
    for error in [
        AgentError::PeerRefused,
        AgentError::StaleGeneration,
        AgentError::Busy,
        AgentError::TooManyReferences,
        AgentError::Protocol("a reason".to_owned()),
    ] {
        let frame = refused(&error);
        assert_eq!(
            code(response(body(&frame), Expect::Status)),
            error.code(),
            "{error:?}"
        );
    }
}

#[test]
fn a_refusal_with_an_unknown_code_is_a_protocol_error_that_keeps_no_text() {
    let mut body = vec![1, 0xFF];
    let text = "op://Personal/shelf/token";
    body.extend_from_slice(&u16::try_from(text.len()).unwrap().to_be_bytes());
    body.extend_from_slice(text.as_bytes());
    let unknown = response(&body, Expect::Stored(1)).unwrap_err();
    assert_eq!(unknown.code(), "starmap::agent::protocol");
    assert!(!unknown.to_string().contains("op://"));
}

#[test]
fn a_frame_written_on_one_end_is_read_whole_on_the_other() {
    let (mut one, mut two) = UnixStream::pair().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    write_frame(&mut one, &[0, 0, 0, 6, 1, 0x83, 0, 0, 0, 2], deadline).unwrap();
    let read = read_frame(&mut two, deadline).unwrap();
    assert_eq!(*read, [1, 0x83, 0, 0, 0, 2]);
    assert_eq!(read.capacity(), read.len());
}

#[test]
fn a_frame_length_over_1_mib_is_refused_before_its_body_is_read() {
    let (mut one, mut two) = UnixStream::pair().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let length = u32::try_from(FRAME_LIMIT + 1).unwrap().to_be_bytes();
    write_frame(&mut one, &length, deadline).unwrap();
    let refused = read_frame(&mut two, deadline).unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::InvalidData);
}
