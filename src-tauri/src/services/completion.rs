//! Pure derivations for the completion flow: render the captured check result as
//! review test output, and augment the risk notes when checks didn't pass. No IO.

use crate::services::check::CheckOutcome;

/// Render the check result as a review's `test_output`. Empty when no command ran.
pub fn format_test_output(command: &str, outcome: &CheckOutcome) -> String {
    if !outcome.ran {
        return String::new();
    }
    let trailer = if outcome.timed_out {
        "[timed out]".to_string()
    } else if let Some(code) = outcome.exit_code {
        format!("[exit {code}]")
    } else {
        "[no exit code]".to_string()
    };
    format!("$ {command}\n{}\n{trailer}", outcome.output.trim_end())
}

/// Append a risk flag when the check timed out or failed. Unchanged when the
/// check passed or never ran.
pub fn augment_risk_notes(mut notes: Vec<String>, outcome: &CheckOutcome) -> Vec<String> {
    if outcome.timed_out {
        notes.push("Checks timed out".to_string());
    } else if outcome.ran && !outcome.passed() {
        notes.push("Checks failed".to_string());
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(ran: bool, exit: Option<i32>, timed_out: bool, output: &str) -> CheckOutcome {
        CheckOutcome {
            ran,
            exit_code: exit,
            timed_out,
            output: output.to_string(),
        }
    }

    #[test]
    fn not_run_yields_empty_output_and_no_flag() {
        let o = CheckOutcome::not_run();
        assert_eq!(format_test_output("pnpm test", &o), "");
        assert!(augment_risk_notes(vec![], &o).is_empty());
    }

    #[test]
    fn passing_output_has_exit_zero_trailer_and_no_flag() {
        let o = outcome(true, Some(0), false, "all good\n");
        let text = format_test_output("pnpm test", &o);
        assert!(text.starts_with("$ pnpm test\n"));
        assert!(text.contains("all good"));
        assert!(text.ends_with("[exit 0]"));
        assert!(augment_risk_notes(vec![], &o).is_empty());
    }

    #[test]
    fn failing_exit_adds_checks_failed_flag() {
        let o = outcome(true, Some(1), false, "boom");
        assert!(format_test_output("x", &o).ends_with("[exit 1]"));
        let notes = augment_risk_notes(vec!["Large change".to_string()], &o);
        assert_eq!(notes, vec!["Large change".to_string(), "Checks failed".to_string()]);
    }

    #[test]
    fn timeout_adds_timed_out_flag_and_trailer() {
        let o = outcome(true, None, true, "partial");
        assert!(format_test_output("x", &o).ends_with("[timed out]"));
        assert_eq!(augment_risk_notes(vec![], &o), vec!["Checks timed out".to_string()]);
    }

    #[test]
    fn spawn_failure_no_exit_code_trailer() {
        let o = outcome(true, None, false, "failed to start check: ...");
        assert!(format_test_output("x", &o).ends_with("[no exit code]"));
        assert_eq!(augment_risk_notes(vec![], &o), vec!["Checks failed".to_string()]);
    }
}
