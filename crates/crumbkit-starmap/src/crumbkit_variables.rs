//! The environment variables that crumbkit reads in every tool, declared
//! as a starmap spec so that `self environment list` describes them the way
//! it describes the tool's own variables.
//!
//! The tests check the list against crumbkit's source at the version that
//! Cargo.lock holds.

use crumbkit::ToolName;
use starmap::{Spec, SpecError};

#[cfg(test)]
mod tests;

/// The end of the name of the variable that lowers the tool's output
/// budget, which the tool's name starts, such as `SHELF_OUTPUT_BUDGET`.
const OUTPUT_BUDGET: &str = "OUTPUT_BUDGET";

/// The declaration of the variables that the `crumbkit` crate reads in every
/// tool, with `{OUTPUT_BUDGET}` standing for the name that starts with the
/// tool's name. The variables that only `crumbkit-usage` reads are left out,
/// because a tool that does not use that crate never reads them.
const TEMPLATE: &str = r#"
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

/// Returns the declaration of the variables that crumbkit reads in the tool
/// named `tool`.
///
/// # Errors
///
/// Returns [`SpecError`] when the declaration does not parse, which no
/// tool name causes, because [`ToolName`] spells each name as a valid
/// variable name.
pub(crate) fn spec(tool: &str) -> Result<Spec, SpecError> {
    let name = ToolName::new(tool);
    let text = TEMPLATE.replace("{OUTPUT_BUDGET}", &name.variable(OUTPUT_BUDGET));
    Spec::parse(&text)
}
