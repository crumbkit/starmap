//! The `starmap` command checks, explains, compares and runs an application's
//! declared environment. The commands arrive in later releases, and until
//! then the program does nothing and exits with success.

use std::process::ExitCode;

/// Returns the exit code, so destructors run before the process ends
/// (BASE-7).
fn main() -> ExitCode {
    ExitCode::SUCCESS
}
