use std::path::Path;

use crumbkit_process::StopReason;

use super::*;

#[test]
fn a_tree_entry_reads_its_mode_id_and_size() {
    let listed = b"100644 blob 3b18e512dba79e4c8300dd08aeb37f8e728b8dad     412\tstarmap.toml\0";
    assert_eq!(
        tree_entry(listed),
        Some(TreeEntry {
            mode: "100644",
            id: "3b18e512dba79e4c8300dd08aeb37f8e728b8dad",
            size: Some(412),
        })
    );
}

#[test]
fn a_tree_entry_of_a_directory_has_no_size() {
    let listed = b"040000 tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904       -\tstarmap.toml\0";
    let entry = tree_entry(listed).unwrap();
    assert_eq!((entry.mode, entry.size), ("040000", None));
}

#[test]
fn an_empty_listing_has_no_tree_entry() {
    assert_eq!(tree_entry(b""), None);
}

#[test]
fn a_revision_that_starts_with_a_dash_names_no_commit_and_git_never_runs() {
    let missing = Path::new("/a/directory/that/does/not/exist");
    let result = read_at(missing, "--output=/tmp/x", "starmap.toml");
    assert!(matches!(result, Err(GitError::NotFound)), "{result:?}");
}

#[test]
fn a_git_run_that_prints_more_than_its_limit_fails_and_says_so() {
    // git may exit before the run notices the limit, or be stopped first;
    // either way the run fails with the same message.
    let deadline = Instant::now() + DEADLINE;
    let error = git(Path::new("."), &["--version"], 1, deadline).unwrap_err();
    assert_eq!(error.to_string(), "git printed more than 1 bytes");
}

#[test]
fn a_run_that_exits_with_success_after_passing_its_limit_fails() {
    let error = ended(Outcome::Ended(Status::Code(0)), true, 1).unwrap_err();
    assert_eq!(error.to_string(), "git printed more than 1 bytes");
}

#[test]
fn a_run_stopped_for_passing_its_limit_fails() {
    let error = ended(Outcome::Stopped(StopReason::OverLimit), true, 1).unwrap_err();
    assert_eq!(error.to_string(), "git printed more than 1 bytes");
}

#[test]
fn a_run_within_its_limit_reports_its_exit_status() {
    assert!(ended(Outcome::Ended(Status::Code(0)), false, 1).unwrap());
    assert!(!ended(Outcome::Ended(Status::Code(128)), false, 1).unwrap());
}

#[test]
fn a_run_that_times_out_after_passing_its_limit_reports_the_timeout() {
    let error = ended(Outcome::TimedOut, true, 1).unwrap_err();
    assert_eq!(error.to_string(), "git did not finish within 30 seconds");
}

#[test]
fn a_run_ended_by_a_signal_says_so() {
    let error = ended(Outcome::Ended(Status::Signal(9)), false, 1).unwrap_err();
    assert_eq!(error.to_string(), "git was ended by a signal");
}

#[test]
fn a_git_run_still_going_at_its_deadline_fails_and_says_so() {
    // The alias runs a shell that sleeps, so git is still running when the
    // run first checks the deadline.
    let sleeps = ["-c", "alias.wait=!sleep 5", "wait"];
    let error = git(Path::new("."), &sleeps, SMALL, Instant::now()).unwrap_err();
    assert_eq!(error.to_string(), "git did not finish within 30 seconds");
}
