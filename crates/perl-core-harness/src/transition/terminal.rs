//! Typed terminal-admission contract for compiler-harness observations (#6884).
//!
//! Terminal process validity is decided before any file/assertion count is
//! considered. One typed decision covers the sanctioned taxonomy:
//! clean_exit, recognized_runner_status, nonzero_exit, signal, timeout,
//! cancelled, spawn_failure, output_truncated, instrument_failure, and
//! cleanup_failure. Only terminally valid and output-complete observations
//! are scoreable; every other state is instrument/authority evidence —
//! not a compiler regression and not success.
//!
//! The producer currently persists only an exit-status identity
//! ([`RunReport::harness_status`]); [`TerminalProcessOutcome::from_harness_status`]
//! adapts that legacy fact. Later #8528 `ProcessResult` evidence maps onto the
//! richer variants directly without redefining this contract.

use perl_core_harness_types::{HarnessMode, HarnessRunner};

/// Typed terminal process outcome for one harness invocation.
///
/// Unknown nonzero exits, signals, timeouts, truncation, and cleanup failures
/// are instrument/authority states: no count, producer pass, or diagnostic can
/// override their invalidity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalProcessOutcome {
    /// The harness reached exit status 0 on its own.
    CleanExit,
    /// Nonzero exit admitted by the exact runner/mode protocol contract.
    RecognizedRunnerStatus {
        /// Observed nonzero status.
        code: i32,
        /// Contract that recognizes the status.
        meaning: String,
    },
    /// Unrecognized nonzero exit with no reviewed runner/mode meaning.
    NonZeroExit {
        /// Observed nonzero status.
        code: i32,
    },
    /// Terminated by a signal instead of reaching an exit status.
    Signal {
        /// Termination signal identity.
        signal: i32,
        /// Signal name when this platform exposes one.
        name: Option<String>,
    },
    /// This tool stopped the invocation at its finite deadline.
    TimedOut,
    /// Cancellation was requested before completion.
    Cancelled,
    /// The harness could not be spawned at all.
    SpawnFailed,
    /// Captured output was truncated, so counts are incomplete even if the
    /// terminal state itself was fine.
    OutputTruncated,
    /// The measurement instrument failed independently of compiler behavior.
    InstrumentFailure,
    /// Post-observation cleanup failed, so observation identity is untrusted.
    CleanupFailure,
}

impl TerminalProcessOutcome {
    /// Classify the legacy persisted process fact under the exact runner/mode
    /// contract.
    ///
    /// `None` means the report carries no terminal identity at all, which is
    /// recorded as [`TerminalProcessOutcome::InstrumentFailure`]: the recording
    /// instrument did not capture a proven completion.
    pub fn from_harness_status(
        status: Option<i32>,
        runner: HarnessRunner,
        mode: HarnessMode,
    ) -> Self {
        match status {
            Some(0) => Self::CleanExit,
            Some(code) => {
                if recognize_nonzero_exit(runner, mode, code) {
                    Self::RecognizedRunnerStatus { code, meaning: recognized_meaning(runner) }
                } else {
                    Self::NonZeroExit { code }
                }
            }
            None => Self::InstrumentFailure,
        }
    }

    /// Stable snake_case label matching the #6884 taxonomy vocabulary.
    pub fn label(&self) -> &'static str {
        match self {
            Self::CleanExit => "clean_exit",
            Self::RecognizedRunnerStatus { .. } => "recognized_runner_status",
            Self::NonZeroExit { .. } => "nonzero_exit",
            Self::Signal { .. } => "signal",
            Self::TimedOut => "timeout",
            Self::Cancelled => "cancelled",
            Self::SpawnFailed => "spawn_failure",
            Self::OutputTruncated => "output_truncated",
            Self::InstrumentFailure => "instrument_failure",
            Self::CleanupFailure => "cleanup_failure",
        }
    }

    /// Whether the observation is terminally valid and output complete, so
    /// file/assertion counts may be scored.
    pub fn is_scoreable(&self) -> bool {
        matches!(self, Self::CleanExit | Self::RecognizedRunnerStatus { .. })
    }

    /// Human-readable summary of why this outcome is not scoreable.
    pub fn not_proven_reason(&self) -> String {
        match self {
            Self::CleanExit | Self::RecognizedRunnerStatus { .. } => {
                "terminal admission succeeded".to_string()
            }
            Self::NonZeroExit { code } => format!(
                "nonzero exit {code} has no reviewed runner/mode meaning; counts cannot override it"
            ),
            Self::Signal { signal, name } => match name {
                Some(name) => format!("terminated by signal {name} ({signal})"),
                None => format!("terminated by unidentified signal {signal}"),
            },
            Self::TimedOut => "invocation exceeded its deadline before completion".to_string(),
            Self::Cancelled => "invocation was cancelled before completion".to_string(),
            Self::SpawnFailed => "harness could not be spawned".to_string(),
            Self::OutputTruncated => {
                "captured output was truncated; file and assertion counts are incomplete"
                    .to_string()
            }
            Self::InstrumentFailure => {
                "measurement instrument failed; no proven compiler observation exists".to_string()
            }
            Self::CleanupFailure => {
                "post-run cleanup failed; observation identity is not trustworthy".to_string()
            }
        }
    }
}

/// Exact supported runner × mode × status nonzero-exit recognition matrix.
///
/// Execute mode deliberately records the upstream scheduler's nonzero exit
/// alongside green `Test` runner records with status `1` (#3451), so that
/// completion state is recognized rather than misclassified as instrument
/// failure by zero-only defensive code. Parse/compile recognize no nonzero
/// status yet, and execute recognizes no other runner or status: a status of
/// `255` with all-green counts stays `not_proven` until an exact contract
/// independently proves that terminal admissible.
fn recognize_nonzero_exit(runner: HarnessRunner, mode: HarnessMode, code: i32) -> bool {
    matches!((runner, mode, code), (HarnessRunner::Test, HarnessMode::Execute, 1))
}

fn recognized_meaning(runner: HarnessRunner) -> String {
    format!(
        "upstream {} scheduler nonzero exit alongside complete runner records in execute mode (#3451)",
        runner.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(status: Option<i32>, mode: HarnessMode) -> TerminalProcessOutcome {
        TerminalProcessOutcome::from_harness_status(status, HarnessRunner::Test, mode)
    }

    #[test]
    fn zero_status_is_clean_and_scoreable_in_every_mode() {
        for mode in [HarnessMode::Parse, HarnessMode::Compile, HarnessMode::Execute] {
            let terminal = outcome(Some(0), mode);
            assert_eq!(terminal, TerminalProcessOutcome::CleanExit);
            assert_eq!(terminal.label(), "clean_exit");
            assert!(terminal.is_scoreable());
        }
    }

    #[test]
    fn historical_255_all_pass_packet_is_not_proven_in_parse_mode() {
        let terminal = outcome(Some(255), HarnessMode::Parse);
        assert_eq!(
            terminal,
            TerminalProcessOutcome::NonZeroExit { code: 255 },
            "the historical all-pass packet must not become valid merely because counts look green"
        );
        assert!(!terminal.is_scoreable());
        assert_eq!(terminal.label(), "nonzero_exit");
        let reason = terminal.not_proven_reason();
        assert!(reason.contains("255"), "reason must carry the observed code: {reason}");
        assert!(reason.contains("counts cannot override"));
    }

    #[test]
    fn compile_mode_recognizes_no_nonzero_status_yet() {
        let terminal = outcome(Some(1), HarnessMode::Compile);
        assert_eq!(terminal, TerminalProcessOutcome::NonZeroExit { code: 1 });
        assert!(!terminal.is_scoreable());
    }

    #[test]
    fn execute_scheduler_nonzero_exit_is_recognized_not_misclassified() {
        // Opposite-direction control: the upstream scheduler's nonzero exit
        // alongside green records is a genuinely recognized completion state
        // (#3451); zero-only defensive code must not call it instrument
        // failure.
        let terminal = outcome(Some(1), HarnessMode::Execute);
        assert_eq!(
            terminal,
            TerminalProcessOutcome::RecognizedRunnerStatus {
                code: 1,
                meaning: recognized_meaning(HarnessRunner::Test)
            }
        );
        assert!(terminal.is_scoreable(), "recognized nonzero must stay scoreable");
        assert_eq!(terminal.label(), "recognized_runner_status");
    }

    #[test]
    fn execute_unproven_status_is_not_recognized() {
        let terminal = outcome(Some(255), HarnessMode::Execute);
        assert_eq!(terminal, TerminalProcessOutcome::NonZeroExit { code: 255 });
        assert!(!terminal.is_scoreable());
    }

    #[test]
    fn execute_unproven_runner_is_not_recognized() {
        let terminal = TerminalProcessOutcome::from_harness_status(
            Some(1),
            HarnessRunner::Harness,
            HarnessMode::Execute,
        );
        assert_eq!(terminal, TerminalProcessOutcome::NonZeroExit { code: 1 });
        assert!(!terminal.is_scoreable());
    }

    #[test]
    fn missing_terminal_identity_is_instrument_failure() {
        let terminal = outcome(None, HarnessMode::Parse);
        assert_eq!(terminal, TerminalProcessOutcome::InstrumentFailure);
        assert!(!terminal.is_scoreable());
        assert_eq!(terminal.label(), "instrument_failure");
    }

    #[test]
    fn every_taxonomy_variant_has_distinct_label_and_scoreability() {
        let outcomes = [
            (TerminalProcessOutcome::CleanExit, "clean_exit", true),
            (
                TerminalProcessOutcome::RecognizedRunnerStatus {
                    code: 2,
                    meaning: "contract".to_string(),
                },
                "recognized_runner_status",
                true,
            ),
            (TerminalProcessOutcome::NonZeroExit { code: 3 }, "nonzero_exit", false),
            (
                TerminalProcessOutcome::Signal { signal: 15, name: Some("SIGTERM".to_string()) },
                "signal",
                false,
            ),
            (TerminalProcessOutcome::TimedOut, "timeout", false),
            (TerminalProcessOutcome::Cancelled, "cancelled", false),
            (TerminalProcessOutcome::SpawnFailed, "spawn_failure", false),
            (TerminalProcessOutcome::OutputTruncated, "output_truncated", false),
            (TerminalProcessOutcome::InstrumentFailure, "instrument_failure", false),
            (TerminalProcessOutcome::CleanupFailure, "cleanup_failure", false),
        ];
        let mut labels = Vec::new();
        for (outcome, label, scoreable) in outcomes {
            assert_eq!(outcome.label(), label, "label drift for {label}");
            assert_eq!(outcome.is_scoreable(), scoreable, "scoreability drift for {label}");
            if !scoreable {
                let reason = outcome.not_proven_reason();
                assert!(!reason.is_empty(), "not_proven_reason must be populated for {label}");
            }
            labels.push(label);
        }
        let distinct = labels.iter().collect::<std::collections::BTreeSet<_>>();
        assert_eq!(distinct.len(), labels.len(), "taxonomy labels must be distinct");
    }

    #[test]
    fn truncated_output_is_never_scoreable_despite_clean_exit_shape() {
        let terminal = TerminalProcessOutcome::OutputTruncated;
        assert!(!terminal.is_scoreable());
        assert!(terminal.not_proven_reason().contains("incomplete"));
    }
}
