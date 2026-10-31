use std::str::FromStr as _;

use starmap::{Name, Spec, SpecProblemKind};

use super::{Guess, guess, spec_text};
use crate::scan::Reads;

/// Returns the guess for `name`, read at one place.
fn guessed(name: &str) -> Guess {
    guess(
        Name::from_str(name).unwrap(),
        vec!["src/main.rs:1".to_owned()],
    )
}

/// Returns the type, unset rule, show and source of a guess.
fn fields(guess: &Guess) -> [&str; 4] {
    [&guess.form, &guess.unset, &guess.show, &guess.source]
}

#[test]
fn a_name_guesses_its_type_from_its_last_word() {
    assert_eq!(guessed("SHELF_PORT").form, "port?");
    assert_eq!(guessed("SHELF_CATALOGUE_URL").form, "url?");
    for name in ["CACHE_DIR", "CONFIG_PATH", "LOG_FILE", "JAVA_HOME"] {
        assert_eq!(guessed(name).form, "path?", "{name}");
    }
    assert_eq!(guessed("SHELF_PAGE_SIZE").form, "string?");
    assert_eq!(guessed("PORT").form, "string?");
}

#[test]
fn a_setting_is_guessed_required_and_shown() {
    assert_eq!(
        fields(&guessed("SHELF_PORT")),
        ["port?", "required?", "shown?", "setting?"]
    );
}

#[test]
fn a_name_with_a_secret_word_is_guessed_a_withheld_optional_string() {
    for name in [
        "SHELF_TOKEN",
        "CLIENT_SECRET",
        "SESSION_KEY",
        "DB_PASSWORD",
        "KEY",
    ] {
        assert_eq!(
            fields(&guessed(name)),
            ["string?", "optional?", "withheld?", "secret?"],
            "{name}"
        );
    }
    assert_eq!(guessed("SHELF_KEYBOARD").source, "setting?");
}

#[test]
fn a_variable_of_the_shell_or_the_xdg_directories_is_guessed_inherited() {
    for name in ["HOME", "PATH", "LANG", "TZ", "XDG_CONFIG_HOME"] {
        let guess = guessed(name);
        assert_eq!(
            [&guess.unset, &guess.source],
            ["optional?", "inherited?"],
            "{name}"
        );
    }
    assert_eq!(guessed("XDG_STATE_HOME").form, "path?");
}

#[test]
fn every_value_that_init_writes_is_a_guess_that_the_parser_refuses() {
    let mut reads = Reads::new();
    for name in ["SHELF_PORT", "SHELF_TOKEN", "XDG_STATE_HOME"] {
        reads.insert(
            Name::from_str(name).unwrap(),
            vec!["src/main.rs:1".to_owned()],
        );
    }
    let guesses: Vec<Guess> = reads
        .into_iter()
        .map(|(name, places)| guess(name, places))
        .collect();
    let error = Spec::parse(&spec_text(&guesses)).unwrap_err();
    let kinds: Vec<SpecProblemKind> = error
        .problems()
        .iter()
        .map(|problem| problem.kind())
        .collect();
    // Each variable has five guesses: purpose, type, unset, show and source.
    assert_eq!(kinds, vec![SpecProblemKind::Guess; 15]);
}

#[test]
fn a_file_without_guesses_is_an_empty_spec() {
    let spec = Spec::parse(&spec_text(&[])).unwrap();
    assert_eq!(spec.variables().count(), 0);
}
