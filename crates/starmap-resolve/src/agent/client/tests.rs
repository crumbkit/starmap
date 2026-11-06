use nix::errno::Errno;

use super::connect_error;

#[test]
fn a_connect_refused_for_a_full_backlog_reads_as_busy() {
    assert_eq!(connect_error(Errno::EAGAIN).code(), "starmap::agent::busy");
}

#[test]
fn any_other_connect_failure_reads_as_not_running() {
    for errno in [Errno::ECONNREFUSED, Errno::ENOENT, Errno::EACCES] {
        assert_eq!(
            connect_error(errno).code(),
            "starmap::agent::not_running",
            "{errno}"
        );
    }
}
