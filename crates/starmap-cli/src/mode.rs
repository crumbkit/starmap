//! The mode that a spec's mode rule decides from this shell's values, and
//! the sentence that says why.

use starmap::{Bindings, Bound, Mode, Spec, Unset};

use crate::explain::value_text;

#[cfg(test)]
mod tests;

/// Returns the mode that `bindings` decide, as `starmap::mode` decides it,
/// with a note that names the reason, such as `Mode: local, because
/// SHELF_ENV is unset and its default is dev.`
pub(crate) fn decide(spec: &Spec, bindings: &Bindings) -> (Mode, String) {
    let mode = starmap::mode(spec, bindings);
    let because = reason(spec, bindings);
    (mode, format!("Mode: {mode}, because {because}."))
}

/// Returns why the mode is what it is, as a clause.
fn reason(spec: &Spec, bindings: &Bindings) -> String {
    let Some(rule) = spec.mode() else {
        return "the spec has no [mode] table".to_owned();
    };
    let name = rule.variable();
    let Some(variable) = spec.variable(name) else {
        return format!("the spec does not declare {name}");
    };
    if let Some(Bound::Plain(raw)) = bindings.get(name) {
        match variable.form().parse(raw) {
            Ok(Some(_)) => return format!("{name} is {}", raw.to_string_lossy()),
            Ok(None) => {}
            Err(_) => return format!("the value of {name} does not fit its type"),
        }
    }
    match variable.unset().in_mode(Mode::Local) {
        Unset::Default(value) => {
            format!("{name} is unset and its default is {}", value_text(value))
        }
        Unset::Required | Unset::Optional => format!("{name} is unset"),
    }
}
