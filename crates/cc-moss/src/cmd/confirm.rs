//! Asking before doing something the user should agree to.

use std::io::{self, BufRead, IsTerminal, Write};

use anyhow::{bail, Result};

#[derive(Debug, PartialEq, Eq)]
enum Confirm {
    Proceed,
    Ask,
    Refuse,
}

/// A yes comes from `--yes`, or is typed at a terminal. Without either (CI,
/// pipes) there is nobody to ask, so refuse.
fn decide(yes: bool, stdin_is_tty: bool) -> Confirm {
    if yes {
        Confirm::Proceed
    } else if stdin_is_tty {
        Confirm::Ask
    } else {
        Confirm::Refuse
    }
}

fn is_yes(answer: &str) -> bool {
    matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes" | "是" | "好" | "确定"
    )
}

/// `Ok(true)` to go ahead, `Ok(false)` if the user said no, an error when no
/// one can be asked; `refused` tells the user what to do instead.
pub fn ask(yes: bool, question: &str, refused: &str) -> Result<bool> {
    prompt(yes, question, refused, false)
}

/// Like [`ask`], but an empty answer means yes (`[Y/n]`).
pub fn ask_default_yes(yes: bool, question: &str, refused: &str) -> Result<bool> {
    prompt(yes, question, refused, true)
}

fn prompt(yes: bool, question: &str, refused: &str, default_yes: bool) -> Result<bool> {
    match decide(yes, io::stdin().is_terminal()) {
        Confirm::Proceed => Ok(true),
        Confirm::Refuse => bail!(tr!(
            format!("stdin is not a terminal, so cc-moss cannot ask; {refused}"),
            format!("当前不是交互式终端，没法问你；{refused}")
        )),
        Confirm::Ask => {
            let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
            print!("{question} {hint} ");
            io::stdout().flush()?;
            let mut answer = String::new();
            io::stdin().lock().read_line(&mut answer)?;
            Ok(is_yes(&answer) || (default_yes && answer.trim().is_empty()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_needs_yes_or_a_terminal() {
        assert_eq!(decide(true, false), Confirm::Proceed);
        assert_eq!(decide(true, true), Confirm::Proceed);
        assert_eq!(decide(false, true), Confirm::Ask);
        assert_eq!(decide(false, false), Confirm::Refuse);
    }

    #[test]
    fn only_explicit_yes_confirms() {
        for y in ["y\n", "Y", " yes \n", "YES"] {
            assert!(is_yes(y), "{y:?}");
        }
        for n in ["", "\n", "n", "no", "yep", "sure"] {
            assert!(!is_yes(n), "{n:?}");
        }
    }
}
