//! The `starmap` binary, which runs the command that the starmap-cli
//! library holds.

use std::process::ExitCode;

/// Reads the process's environment once (BASE-6), starts the usage log and
/// runs the command. Returning the exit code lets destructors run before the
/// process ends (BASE-7).
fn main() -> ExitCode {
    // The guard writes this run's line to the usage log when `main`
    // returns, after the output is flushed (CLI-7).
    let _log = crumbkit_usage::start();
    #[expect(
        clippy::disallowed_methods,
        reason = "BASE-6: main reads the environment once, and check and explain read the declared variables from this snapshot"
    )]
    let vars = std::env::vars_os().collect();
    starmap_cli::run(std::env::args_os(), vars)
}
