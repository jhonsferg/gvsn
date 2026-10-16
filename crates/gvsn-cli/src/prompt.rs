//! Shared confirmation-prompt helper for destructive commands.

use anyhow::{Context, Result};

/// Prints `message` to stderr and reads a line from stdin, returning `true`
/// if the user answered `y` or `yes` (case-insensitive).
///
/// # Errors
///
/// Returns an error if stdin cannot be read.
pub fn confirm(message: &str) -> Result<bool> {
    eprint!("{message}");
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("Failed to read confirmation")?;
    Ok(is_affirmative(&input))
}

fn is_affirmative(input: &str) -> bool {
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_affirmative_accepts_y_and_yes_case_insensitively() {
        for s in ["y", "Y", "yes", "YES", "Yes", "  yes  "] {
            assert!(is_affirmative(s), "expected {s:?} to be affirmative");
        }
    }

    #[test]
    fn is_affirmative_rejects_everything_else() {
        for s in ["", "n", "no", "sure", "yep"] {
            assert!(!is_affirmative(s), "expected {s:?} to be rejected");
        }
    }
}
