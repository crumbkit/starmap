use super::{AgentError, Generation};

#[test]
fn two_drawn_generations_differ() {
    assert_ne!(Generation::draw(), Generation::draw());
}

#[test]
fn a_start_failure_has_the_start_failed_code_and_names_its_reason() {
    let error = AgentError::StartFailed("no such file".to_owned());
    assert_eq!(error.code(), "starmap::agent::start_failed");
    assert!(error.to_string().contains("no such file"));
}
