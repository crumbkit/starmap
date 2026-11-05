//! Tests of how the outcome of one `op inject` run becomes an entry for
//! each reference. The tests in `tests/one_password/batch.rs` run whole
//! resolutions against fake `op` programs.

use std::io;

use super::batch::Template;
use super::process::OpRun;
use super::{Entry, outcome};

/// Returns the template of two references.
fn template() -> Template {
    Template::new(&["op://Personal/first/alpha", "op://Personal/second/beta"]).unwrap()
}

#[test]
fn a_run_that_could_not_start_op_leaves_each_reference_to_its_own_op_read() {
    let run = Ok(OpRun::NotStarted(io::Error::from(io::ErrorKind::NotFound)));
    let (entries, stopped) = outcome(&template(), 2, run);
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| matches!(entry, Entry::Read)));
    assert!(stopped.is_none());
}

#[test]
fn an_error_after_op_started_is_final_for_every_reference() {
    // The twin of a run that could not start op: the error is the same
    // kind, and only where it happened decides whether op runs again.
    let run = Err(io::Error::from(io::ErrorKind::NotFound));
    let (entries, stopped) = outcome(&template(), 2, run);
    assert_eq!(entries.len(), 2);
    for entry in &entries {
        let Entry::Failed(message) = entry else {
            panic!("an entry is not final");
        };
        assert!(
            message.starts_with("op inject failed after it started"),
            "{message}"
        );
    }
    assert!(stopped.is_none());
}
