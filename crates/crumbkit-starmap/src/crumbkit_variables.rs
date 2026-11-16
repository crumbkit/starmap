//! The environment variables that crumbkit reads, declared as a starmap spec
//! so that `self environment list` describes them the way it describes the
//! tool's own variables.
//!
//! The `crumbkit` crate reads its variables in every tool. The
//! `crumbkit-usage` crate reads its own only in a tool that uses it, so the
//! list holds them only for such a tool.
//!
//! The tests check each list against the source of its crate at the version
//! that Cargo.lock holds.

use crumbkit::ToolName;
use starmap::{Spec, SpecError};

#[cfg(test)]
mod tests;

/// The end of the name of the variable that lowers the tool's output
/// budget, which the tool's name starts, such as `SHELF_OUTPUT_BUDGET`.
const OUTPUT_BUDGET: &str = "OUTPUT_BUDGET";

/// The end of the name of the variable that gives the program that runs the
/// tool, which the tool's name starts, such as `SHELF_PROGRAM`.
const PROGRAM: &str = "PROGRAM";

/// The declaration of the variables that the `crumbkit` crate reads in every
/// tool, with `{OUTPUT_BUDGET}` standing for the name that starts with the
/// tool's name.
const CORE: &str = r#"
[variables.{OUTPUT_BUDGET}]
purpose = "The most bytes that one run prints, which lowers the tool's own limit and never raises it."
type = { integer = { min = 1, max = 4294967295 } }
unset = "optional"
effect = "A value that is not a whole number of bytes from 1 up to the tool's limit leaves the limit as it is."
show = "shown"
source = "setting"

[variables.NO_COLOR]
purpose = "Turns colour off in the tool's output when it holds any value."
type = "string"
unset = "optional"
effect = "Colour follows CLICOLOR_FORCE, CLICOLOR, TERM and whether the stream is a terminal."
show = "shown"
source = "inherited"

[variables.CLICOLOR]
purpose = "Turns colour off in the tool's output when it is 0."
type = "string"
unset = "optional"
effect = "Colour follows the terminal."
show = "shown"
source = "inherited"

[variables.CLICOLOR_FORCE]
purpose = "Turns colour on in the tool's output, whatever the stream is, when it holds a value other than 0."
type = "string"
unset = "optional"
effect = "Colour follows the terminal."
show = "shown"
source = "inherited"

[variables.TERM]
purpose = "The kind of terminal the tool prints to; dumb turns colour off in the tool's output."
type = "string"
unset = "optional"
effect = "Colour follows the other colour variables and the terminal."
show = "shown"
source = "inherited"
"#;

/// The declaration of the variables that the `crumbkit-usage` crate reads in
/// a tool that uses it, with `{PROGRAM}` standing for the name that starts
/// with the tool's name.
const USAGE: &str = r#"
[variables.{PROGRAM}]
purpose = "The name of the program that runs the tool, such as flightdeck, which the tool records in its usage log."
type = { string = { pattern = "^[a-z][a-z0-9_-]{0,31}$" } }
unset = "optional"
effect = "The usage log records no program name."
show = "shown"
source = "setting"

[variables.XDG_STATE_HOME]
purpose = "The directory that holds the tool's state, such as its usage log, under a directory named after the tool."
type = { path = { absolute = true } }
unset = "optional"
effect = "The state directory is .local/state under the home directory."
show = "shown"
source = "inherited"

[variables.TRACEPARENT]
purpose = "The W3C trace context of the caller, which the tool's usage log joins."
type = "string"
unset = "optional"
effect = "The run starts a new trace."
show = "shown"
source = "inherited"

[variables.TRACESTATE]
purpose = "The W3C trace state of the caller, which goes with TRACEPARENT."
type = "string"
unset = "optional"
effect = "The run records no trace state."
show = "shown"
source = "inherited"
"#;

/// Returns the declaration of the variables that crumbkit reads in the tool
/// named `tool`: those of the `crumbkit` crate, and those of `crumbkit-usage`
/// when `usage` says the tool uses it.
///
/// # Errors
///
/// Returns [`SpecError`] when the declaration does not parse, which no
/// tool name causes, because [`ToolName`] spells each name as a valid
/// variable name.
pub(crate) fn spec(tool: &str, usage: bool) -> Result<Spec, SpecError> {
    let name = ToolName::new(tool);
    let mut text = CORE.replace("{OUTPUT_BUDGET}", &name.variable(OUTPUT_BUDGET));
    if usage {
        text.push_str(&USAGE.replace("{PROGRAM}", &name.variable(PROGRAM)));
    }
    Spec::parse(&text)
}
