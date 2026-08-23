//! Offline proof for the #8048 pure terminal-sequence final-newline policy.
//!
//! Every historical false-pass path named by the issue is encoded as a
//! discriminating expectation: force-LF into CRLF, collapse under insert-only,
//! trim removing the one final newline, partial CRLF splitting, evidence
//! reporting preserved after conversion, and `str::lines()` trailing-loss
//! returning as terminal authority.

use perl_lsp_perltidy::native::{
    FinalNewlinePolicy, TerminalSequence, apply_terminal_sequence_policy, source_convention,
    trailing_run,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const NEITHER: FinalNewlinePolicy =
    FinalNewlinePolicy { insert_final_newline: false, trim_final_newlines: false };
const INSERT_ONLY: FinalNewlinePolicy =
    FinalNewlinePolicy { insert_final_newline: true, trim_final_newlines: false };
const TRIM_ONLY: FinalNewlinePolicy =
    FinalNewlinePolicy { insert_final_newline: false, trim_final_newlines: true };
const BOTH: FinalNewlinePolicy =
    FinalNewlinePolicy { insert_final_newline: true, trim_final_newlines: true };

fn outcome(source: &str, policy: FinalNewlinePolicy) -> String {
    apply_terminal_sequence_policy(source, policy, None).bytes
}

#[test]
fn neither_policy_preserves_terminal_bytes_exactly() -> TestResult {
    for source in [
        "",
        "x",
        "x\n",
        "x\r\n",
        "x\r",
        "x\n\n",
        "x\r\n\r\n",
        "x\n\r\n",
        "\n",
        "\r\n",
        "\r",
        "x\r\n\n\r",
    ] {
        let result = outcome(source, NEITHER);
        assert_eq!(result, source, "neither policy must preserve {source:?}");
    }
    Ok(())
}

#[test]
fn insert_only_adds_one_sequence_only_when_none_exists() -> TestResult {
    assert_eq!(outcome("", INSERT_ONLY), "\n");
    assert_eq!(outcome("x", INSERT_ONLY), "x\n");

    // Already-terminated documents keep every existing sequence: insert-only
    // must never collapse or normalize them.
    for source in ["x\n", "x\r\n", "x\r", "x\n\n", "x\r\n\r\n", "\n\n\n"] {
        assert_eq!(
            outcome(source, INSERT_ONLY),
            source,
            "insert-only must not alter terminated {source:?}"
        );
    }
    Ok(())
}

#[test]
fn insert_only_preserves_the_source_convention() -> TestResult {
    // The document established CRLF; insertion must not force LF.
    assert_eq!(source_convention("a\r\nb"), Some(TerminalSequence::CrLf));
    assert_eq!(outcome("a\r\nb", INSERT_ONLY), "a\r\nb\r\n");
    assert_eq!(outcome("a\r\nb\r\nc", INSERT_ONLY), "a\r\nb\r\nc\r\n");

    // Bare-CR documents keep their declared convention too.
    assert_eq!(source_convention("a\rb"), Some(TerminalSequence::Cr));
    assert_eq!(outcome("a\rb", INSERT_ONLY), "a\rb\r");

    // Explicit proven configuration may select another convention.
    let explicit =
        apply_terminal_sequence_policy("a\r\nb", INSERT_ONLY, Some(TerminalSequence::Lf));
    assert_eq!(explicit.bytes, "a\r\nb\n");

    // A document with no line endings defaults to LF.
    assert_eq!(source_convention("abc"), None);
    assert_eq!(outcome("abc", INSERT_ONLY), "abc\n");
    Ok(())
}

#[test]
fn trim_only_retains_the_one_final_sequence_and_removes_only_excess() -> TestResult {
    // Exactly one final sequence survives; trimming must never take it.
    assert_eq!(outcome("x\n", TRIM_ONLY), "x\n");
    assert_eq!(outcome("x\r\n", TRIM_ONLY), "x\r\n");
    assert_eq!(outcome("x\r", TRIM_ONLY), "x\r");

    // Only excess sequences before the retained final one are removed.
    assert_eq!(outcome("x\n\n\n", TRIM_ONLY), "x\n");
    assert_eq!(outcome("x\r\n\r\n", TRIM_ONLY), "x\r\n");
    assert_eq!(outcome("x\n\r\n", TRIM_ONLY), "x\r\n");
    assert_eq!(outcome("\n\n", TRIM_ONLY), "\n");

    // Unterminated documents have nothing to trim.
    assert_eq!(outcome("x", TRIM_ONLY), "x");
    assert_eq!(outcome("", TRIM_ONLY), "");
    Ok(())
}

#[test]
fn both_options_produce_exactly_one_accepted_final_sequence() -> TestResult {
    assert_eq!(outcome("", BOTH), "\n");
    assert_eq!(outcome("x", BOTH), "x\n");
    assert_eq!(outcome("x\n", BOTH), "x\n");
    assert_eq!(outcome("x\n\n", BOTH), "x\n");
    assert_eq!(outcome("x\n\n\n", BOTH), "x\n");
    assert_eq!(outcome("x\r\n\r\n", BOTH), "x\r\n");
    assert_eq!(outcome("x\n\r\n", BOTH), "x\r\n");

    for result in [
        outcome("", BOTH),
        outcome("x\n\n\n", BOTH),
        outcome("x\r\n\r\n", BOTH),
        outcome("x\n\r\n", BOTH),
    ] {
        assert_eq!(trailing_run(&result).len(), 1, "{result:?} must end with exactly one sequence");
    }
    Ok(())
}

#[test]
fn trailing_run_scans_complete_sequences_atomically() -> TestResult {
    assert!(trailing_run("").is_empty());
    assert!(trailing_run("x").is_empty());
    assert_eq!(trailing_run("x\n").sequences(), &[TerminalSequence::Lf]);
    assert_eq!(trailing_run("x\r\n").sequences(), &[TerminalSequence::CrLf]);
    assert_eq!(trailing_run("x\r").sequences(), &[TerminalSequence::Cr]);
    assert_eq!(
        trailing_run("x\r\n\n").sequences(),
        &[TerminalSequence::CrLf, TerminalSequence::Lf]
    );
    assert_eq!(
        trailing_run("x\n\r\n").sequences(),
        &[TerminalSequence::Lf, TerminalSequence::CrLf]
    );
    assert_eq!(
        trailing_run("x\r\n\n\r").sequences(),
        &[TerminalSequence::CrLf, TerminalSequence::Lf, TerminalSequence::Cr]
    );
    Ok(())
}

#[test]
fn evidence_binds_to_the_final_returned_bytes_not_an_intermediate() -> TestResult {
    for (source, policy) in
        [("x", INSERT_ONLY), ("x\n\n", TRIM_ONLY), ("x\r\nb", INSERT_ONLY), ("x\r\n\r\n", BOTH)]
    {
        let result = apply_terminal_sequence_policy(source, policy, None);
        // If evidence were computed before the final projection (for example
        // before an insertion), its recorded final run could not match the
        // returned bytes. This assertion turns that timing defect red.
        assert_eq!(
            result.evidence.final_run,
            trailing_run(&result.bytes),
            "evidence for {source:?} must describe the returned bytes"
        );
        assert!(result.evidence.is_consistent());
    }

    let inserted = apply_terminal_sequence_policy("x", INSERT_ONLY, None);
    assert_eq!(inserted.evidence.inserted, Some(TerminalSequence::Lf));
    assert_eq!(inserted.evidence.removed_count, 0);

    let trimmed = apply_terminal_sequence_policy("x\n\n\n", TRIM_ONLY, None);
    assert_eq!(trimmed.evidence.inserted, None);
    assert_eq!(trimmed.evidence.removed_count, 2);
    Ok(())
}

#[test]
fn contradictory_action_and_change_evidence_is_not_consistent() -> TestResult {
    use perl_lsp_perltidy::native::{TerminalChange, TerminalNewlineEvidence};

    let valid = apply_terminal_sequence_policy("x", INSERT_ONLY, None).evidence;
    assert!(valid.is_consistent());

    let contradictory = TerminalNewlineEvidence { change: TerminalChange::Unchanged, ..valid };
    assert!(!contradictory.is_consistent());

    let contradictory_trim = TerminalNewlineEvidence {
        predecessor: trailing_run("x\n\n"),
        final_run: trailing_run("x\n"),
        inserted: None,
        removed_count: 1,
        change: TerminalChange::Unchanged,
    };
    assert!(!contradictory_trim.is_consistent());
    Ok(())
}

#[test]
fn evidence_reports_change_after_partial_crlf_splitting_and_conversion() -> TestResult {
    use perl_lsp_perltidy::native::{TerminalChange, TerminalNewlineEvidence};

    // Historical impostor: strip CR/LF bytes individually, then append LF.
    // For a CRLF document this splits the pair and forces LF; the evidence
    // over complete sequences must refuse to call that preserved.
    for source in ["a\r\n", "a\r\n\r\n", "a\r"] {
        let impostor = format!("{}\n", source.trim_end_matches(['\n', '\r']));
        assert_ne!(impostor, source, "{source:?} must be altered by the impostor");

        let predecessor = trailing_run(source);
        let final_run = trailing_run(&impostor);
        assert_ne!(
            predecessor.sequences(),
            final_run.sequences(),
            "impostor output for {source:?} must show a changed terminal run"
        );

        // Replaying the same transformation through the policy API is
        // impossible without declaring the actions honestly: an insert-only
        // pass leaves terminated input untouched instead of splitting it.
        let policy_result = apply_terminal_sequence_policy(source, INSERT_ONLY, None);
        assert_eq!(policy_result.evidence.change, TerminalChange::Unchanged);
        assert_eq!(policy_result.bytes, source);
        assert!(policy_result.evidence.is_consistent());
        assert!(policy_result.evidence.is_no_change());

        // An honest record of what the impostor did is detectably
        // inconsistent: predecessor minus zero removals plus one inserted LF
        // cannot reconstruct a run that lost its original sequences.
        let dishonest = TerminalNewlineEvidence {
            predecessor,
            final_run,
            inserted: Some(TerminalSequence::Lf),
            removed_count: 0,
            change: TerminalChange::Unchanged,
        };
        assert!(
            !dishonest.is_consistent(),
            "split/conversion evidence for {source:?} must be inconsistent"
        );
    }
    Ok(())
}

#[test]
fn no_byte_delta_is_classified_as_no_change_with_no_actions() -> TestResult {
    use perl_lsp_perltidy::native::TerminalChange;

    for source in ["x", "x\n", "x\r\n\r\n", ""] {
        let result = apply_terminal_sequence_policy(source, NEITHER, None);
        assert_eq!(result.bytes, source);
        assert!(
            matches!(
                result.evidence.change,
                TerminalChange::Unchanged | TerminalChange::LeftUnterminated
            ),
            "identity application for {source:?} classified as {:?}",
            result.evidence.change
        );
        assert!(result.evidence.is_no_change());
        assert_eq!(result.evidence.inserted, None);
        assert_eq!(result.evidence.removed_count, 0);
    }

    // A no-op projection must never look applied: inserted/removed stay empty
    // exactly when bytes are unchanged.
    let unchanged = apply_terminal_sequence_policy("x\n\n", INSERT_ONLY, None);
    assert_eq!(unchanged.bytes, "x\n\n");
    assert_eq!(unchanged.evidence.inserted, None);
    assert_eq!(unchanged.evidence.removed_count, 0);
    Ok(())
}

#[test]
fn str_lines_trailing_loss_cannot_return_as_terminal_authority() -> TestResult {
    // A line iterator loses the final logical line created by each terminal
    // separator, so a lines()-derived whole-document authority miscounts both
    // the terminal run and the true EOF row.
    let source = "a\nb\n\n";
    let lines_based_lines = source.lines().count();
    let lines_based_last_len = source.lines().last().map_or(0, str::len);
    let lines_based_end = (lines_based_lines - 1, lines_based_last_len);

    let atomic_run = trailing_run(source);
    let true_eof_line = source.matches(['\n', '\r']).count();

    assert_eq!(lines_based_end, (2, 0), "lines()-derived end for {source:?}");
    assert_eq!(atomic_run.sequences(), &[TerminalSequence::Lf, TerminalSequence::Lf]);
    assert_eq!(true_eof_line, 3, "true EOF row counts every terminal separator");

    // The policy layer never consults a line iterator: repeated separators
    // remain individually visible, which is what lets trim retain exactly one.
    assert_eq!(outcome(source, TRIM_ONLY), "a\nb\n");
    assert_eq!(outcome(source, INSERT_ONLY), source);
    Ok(())
}
