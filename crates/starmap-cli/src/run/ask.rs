//! Asking a person at the terminal for a secret that nothing else supplies,
//! and whether to save it. [`Ask`] is the seam, so the tests answer
//! through a fake and need no terminal.

use std::io::{self, IsTerminal as _};

use crumbkit::{Run, Yes};
use starmap::{Name, Secret};
use zeroize::Zeroizing;

/// A person who can be asked for a secret.
pub(crate) trait Ask {
    /// Whether a person can answer: standard input and standard error are
    /// terminals, and the caller did not pass `--no-input`.
    fn can_ask(&self) -> bool;

    /// Asks for the value of the secret `name` with hidden input. It
    /// returns `None` when the person typed nothing or the answer cannot
    /// be read.
    fn secret(&mut self, name: &Name) -> Option<Secret>;

    /// Asks `question`, and returns whether the person answered yes.
    fn save(&mut self, question: &str) -> bool;
}

/// The person at the terminal of the process that runs starmap.
#[derive(Debug)]
pub(crate) struct Terminal<'a> {
    /// The run's output, which asks the yes-or-no question as crumbkit
    /// asks every question.
    output: &'a mut Run,
    /// Whether a person can answer.
    interactive: bool,
}

impl<'a> Terminal<'a> {
    /// Returns the person at the terminal, who can answer when
    /// `interactive` is true.
    pub(crate) fn new(output: &'a mut Run, interactive: bool) -> Self {
        Self {
            output,
            interactive,
        }
    }
}

impl Ask for Terminal<'_> {
    fn can_ask(&self) -> bool {
        self.interactive
    }

    /// rpassword writes the prompt to the terminal and reads the answer
    /// from it with echo turned off, so the value never shows on screen.
    fn secret(&mut self, name: &Name) -> Option<Secret> {
        let prompt = format!("{name} has no bind and no saved value. Type it (input is hidden): ");
        let mut typed = Zeroizing::new(rpassword::prompt_password(prompt).ok()?);
        if typed.is_empty() {
            return None;
        }
        Some(Secret::new(std::mem::take(&mut *typed)))
    }

    /// crumbkit asks the question on standard error and reads the answer
    /// from standard input, and any answer but yes declines.
    fn save(&mut self, question: &str) -> bool {
        self.output.confirm(Yes::default(), question).is_ok()
    }
}

/// Reports whether standard input and standard error are both terminals,
/// where a person can answer a question.
pub(crate) fn terminals() -> bool {
    #[expect(
        clippy::disallowed_methods,
        reason = "starmap run asks for a secret only when a person sits at the terminal, and reads nothing here"
    )]
    let input = io::stdin().is_terminal();
    #[expect(
        clippy::disallowed_methods,
        reason = "starmap run asks for a secret only when a person sits at the terminal, and writes nothing here"
    )]
    let error = io::stderr().is_terminal();
    input && error
}
