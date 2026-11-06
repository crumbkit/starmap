use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::Path;

use super::AgentSocket;

fn uid() -> u32 {
    nix::unistd::getuid().as_raw()
}

/// Returns a new private directory, which stands in for the user's own
/// temporary directory.
fn parent() -> tempfile::TempDir {
    let directory = tempfile::Builder::new().prefix("sa").tempdir().unwrap();
    chmod(directory.path(), 0o700);
    directory
}

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn code(result: Result<AgentSocket, crate::agent::AgentError>) -> &'static str {
    result.unwrap_err().code()
}

#[test]
fn a_private_parent_gets_a_starmap_agent_directory_of_mode_0700_and_the_socket_in_it() {
    let parent = parent();
    let socket = AgentSocket::in_parent(parent.path(), uid()).unwrap();
    let directory = parent.path().join("starmap-agent");
    assert_eq!(socket.path(), directory.join("agent.sock"));
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o7777,
        0o700
    );
    // A second call finds the directory and accepts it.
    assert!(AgentSocket::in_parent(parent.path(), uid()).is_ok());
}

#[test]
fn a_parent_that_group_or_others_can_write_is_refused_and_the_agent_is_off() {
    let parent = parent();
    chmod(parent.path(), 0o770);
    assert_eq!(
        code(AgentSocket::in_parent(parent.path(), uid())),
        "starmap::agent::off"
    );
    chmod(parent.path(), 0o755);
    assert!(AgentSocket::in_parent(parent.path(), uid()).is_ok());
}

#[test]
fn a_shared_sticky_parent_is_refused() {
    let parent = parent();
    chmod(parent.path(), 0o1700);
    assert_eq!(
        code(AgentSocket::in_parent(parent.path(), uid())),
        "starmap::agent::off"
    );
    assert_eq!(
        code(AgentSocket::in_parent(Path::new("/tmp"), uid())),
        "starmap::agent::off"
    );
}

#[test]
fn a_parent_of_another_uid_is_refused() {
    let parent = parent();
    assert_eq!(
        code(AgentSocket::in_parent(parent.path(), uid() + 1)),
        "starmap::agent::off"
    );
}

#[test]
fn a_parent_that_is_a_symbolic_link_is_refused() {
    let parent = parent();
    let real = parent.path().join("real");
    DirBuilder::new().mode(0o700).create(&real).unwrap();
    let link = parent.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert_eq!(
        code(AgentSocket::in_parent(&link, uid())),
        "starmap::agent::off"
    );
    assert!(AgentSocket::in_parent(&real, uid()).is_ok());
}

#[test]
fn a_starmap_agent_directory_with_another_mode_or_a_file_in_its_place_is_unsafe() {
    let parent = parent();
    let directory = parent.path().join("starmap-agent");
    DirBuilder::new().mode(0o700).create(&directory).unwrap();
    chmod(&directory, 0o750);
    assert_eq!(
        code(AgentSocket::in_parent(parent.path(), uid())),
        "starmap::agent::unsafe_directory"
    );
    fs::remove_dir(&directory).unwrap();
    fs::write(&directory, "").unwrap();
    assert_eq!(
        code(AgentSocket::in_parent(parent.path(), uid())),
        "starmap::agent::unsafe_directory"
    );
}

#[test]
fn a_socket_path_that_does_not_fit_in_sun_path_is_refused() {
    let parent = parent();
    let long = parent.path().join("x".repeat(100));
    DirBuilder::new().mode(0o700).create(&long).unwrap();
    assert_eq!(
        code(AgentSocket::in_parent(&long, uid())),
        "starmap::agent::path_too_long"
    );
    assert!(AgentSocket::in_parent(parent.path(), uid()).is_ok());
}

#[cfg(target_os = "macos")]
#[test]
fn on_macos_the_socket_lies_in_the_users_own_temporary_directory() {
    let socket = AgentSocket::for_user().unwrap();
    let path = socket.path().to_str().unwrap();
    assert!(path.starts_with("/var/folders/"), "{path}");
    assert!(path.ends_with("/T/starmap-agent/agent.sock"), "{path}");
}

#[test]
fn a_socket_at_a_path_in_a_directory_of_mode_0700_is_accepted() {
    let parent = parent();
    let path = parent.path().join("agent.sock");
    let socket = AgentSocket::at(path.clone()).unwrap();
    assert_eq!(socket.path(), path);
    assert!(socket.check_directory().is_ok());
}

#[test]
fn a_socket_at_a_path_whose_directory_group_or_others_can_open_is_unsafe() {
    let parent = parent();
    for mode in [0o755, 0o770, 0o750, 0o701] {
        chmod(parent.path(), mode);
        assert_eq!(
            code(AgentSocket::at(parent.path().join("agent.sock"))),
            "starmap::agent::unsafe_directory",
            "{mode:o}"
        );
    }
}

#[test]
fn a_socket_at_a_path_whose_directory_is_a_symbolic_link_is_unsafe() {
    let parent = parent();
    let real = parent.path().join("real");
    DirBuilder::new().mode(0o700).create(&real).unwrap();
    let link = parent.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert_eq!(
        code(AgentSocket::at(link.join("agent.sock"))),
        "starmap::agent::unsafe_directory"
    );
    assert!(AgentSocket::at(real.join("agent.sock")).is_ok());
}

#[test]
fn a_socket_at_a_path_with_no_directory_or_a_missing_one_is_unsafe() {
    let parent = parent();
    assert_eq!(
        code(AgentSocket::at("agent.sock".into())),
        "starmap::agent::unsafe_directory"
    );
    assert_eq!(
        code(AgentSocket::at(parent.path().join("missing/agent.sock"))),
        "starmap::agent::unsafe_directory"
    );
}

#[test]
fn a_socket_at_a_path_that_does_not_fit_in_sun_path_is_refused() {
    let parent = parent();
    assert_eq!(
        code(AgentSocket::at(parent.path().join("x".repeat(110)))),
        "starmap::agent::path_too_long"
    );
}

#[test]
fn the_directory_check_refuses_a_directory_opened_to_others_after_the_socket_was_found() {
    let parent = parent();
    let socket = AgentSocket::at(parent.path().join("agent.sock")).unwrap();
    chmod(parent.path(), 0o755);
    assert_eq!(
        socket.check_directory().unwrap_err().code(),
        "starmap::agent::unsafe_directory"
    );
}
