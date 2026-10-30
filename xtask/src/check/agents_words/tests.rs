//! Tests for the AGENTS.md word check.

use super::check;
use crate::check::{File, Violation};

fn file(path: &str, words: usize) -> File {
    File {
        path: path.to_owned(),
        contents: "word ".repeat(words),
    }
}

#[test]
fn an_agents_file_of_exactly_200_words_passes() {
    assert_eq!(check(&[file("AGENTS.md", 200)]), []);
}

#[test]
fn an_agents_file_of_201_words_is_reported_with_its_count_and_the_limit() {
    assert_eq!(
        check(&[file("AGENTS.md", 201)]),
        [Violation::OverLimit {
            path: "AGENTS.md".to_owned(),
            measured: 201,
            unit: "words",
            limit: 200,
        }]
    );
}

#[test]
fn an_agents_file_in_a_subdirectory_is_checked() {
    assert_eq!(check(&[file("crates/starmap/AGENTS.md", 401)]).len(), 1);
}

#[test]
fn words_are_separated_by_any_whitespace() {
    let agents = File {
        path: "AGENTS.md".to_owned(),
        contents: "one\ttwo\n\nthree   four\n".repeat(50),
    };
    assert_eq!(check(&[agents]), []);
}

#[test]
fn words_separated_by_mixed_whitespace_are_counted_against_the_limit() {
    let agents = File {
        path: "AGENTS.md".to_owned(),
        contents: format!("{}last", "one\ttwo\n\nthree   four\r\n".repeat(50)),
    };
    assert_eq!(
        check(&[agents]),
        [Violation::OverLimit {
            path: "AGENTS.md".to_owned(),
            measured: 201,
            unit: "words",
            limit: 200,
        }]
    );
}

#[test]
fn a_long_file_with_another_name_is_not_checked() {
    assert_eq!(check(&[file("README.md", 1000)]), []);
}
