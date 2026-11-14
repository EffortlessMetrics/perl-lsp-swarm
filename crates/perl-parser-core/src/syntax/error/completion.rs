//! Typed parser-operation completion.
//!
//! [`ParseCompletion`] is the terminal result of one parser operation. It is
//! produced from the operation's [`ParseStopCause`] and structured recovery
//! count, never from diagnostic wording, diagnostic order, or a post-hoc scan
//! of the diagnostic vector.
//!
//! [`ParseOutput`] remains the AST/diagnostic/recovery payload. Completion is
//! the operation's terminal class; it is not a second diagnostics vocabulary.

use super::{ParseCoreDimension, ParseOutput, ParseStopCause};
use perl_ast::{Node, NodeKind};
use thiserror::Error;

/// Coarse terminal class projected from [`ParseCompletion`].
///
/// Independent expected-class oracles (#7008) can name this class without
/// calling a snapshot classifier or scanning diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseCompletionClass {
    /// Parsing completed without recovery, budget exhaustion, or cancellation.
    Clean,
    /// Parsing completed with at least one structured recovery.
    Recovered,
    /// Parsing could not produce an ordinary clean or recovered result.
    Catastrophic,
    /// Parsing stopped through cooperative cancellation.
    Cancelled,
    /// Parsing stopped because a parser resource budget was exhausted.
    BudgetExhausted,
}

/// Stage at which a cancelled or exhausted operation stopped.
///
/// Exact per-stage hooks remain #3615. Constructors accept every stage so
/// later producers can fill them without changing the completion vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseCompletionStage {
    /// The producer did not supply a stage.
    Unspecified,
    /// Stopped before the parser consumed source.
    BeforeParse,
    /// Stopped during ordinary parsing.
    OrdinaryParsing,
    /// Stopped while applying recovery.
    Recovery,
    /// Stopped after a candidate existed but before publication.
    BeforePublication,
}

/// Identity of the cancellation authority that stopped the operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseCancellationIdentity {
    /// Cooperative `Arc<AtomicBool>` probe. Parser-core has no operation-ID
    /// payload on the stop cause yet (#3615).
    CooperativeToken,
}

/// Which budget dimension refused further work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseBudgetExhaustionDimension {
    /// Recursion-depth budget.
    Recursion,
    /// Structural nesting or postfix-chain depth budget.
    NestingOrDepth,
    /// Heredoc scan-byte budget.
    HeredocScanBytes,
    /// An admitted core work dimension (#8786).
    Core(ParseCoreDimension),
    /// Lexer per-token budget that degraded the remainder of the source.
    Lexer,
}

/// Bounded cause of a catastrophic parser/lexer failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseCatastrophicCause {
    /// Parser or lexer failure that cannot satisfy the ordinary recovered contract.
    UnrecoverableParserOrLexerFailure,
    /// Reserved future/unknown typed terminal; consumers must not infer a
    /// more specific class from diagnostics.
    FutureTypedTerminal,
}

/// Optional source range associated with budget exhaustion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParseCompletionRange {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

/// Typed result of one parser operation.
///
/// Diagnostic count, message text, and post-hoc diagnostic mutation cannot
/// choose the variant. Cancellation and budget exhaustion come only from a
/// live [`ParseStopCause`]. Recovery requires a nonzero structured recovery
/// count. Catastrophe is a constructed terminal, not "any blocking diagnostic".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseCompletion {
    /// Completed with a current AST and no structured recovery.
    ///
    /// Advisory diagnostics may still be present.
    CompletedClean,
    /// Completed with a current (possibly partial) tree and recorded recovery.
    CompletedWithRecovery {
        /// Number of structured [`super::ParseError::Recovered`] repairs.
        recovered_count: usize,
    },
    /// Stopped by cooperative cancellation.
    Cancelled {
        /// Stage supplied by the producer, or [`ParseCompletionStage::Unspecified`].
        stage: ParseCompletionStage,
        /// Cancellation authority identity.
        identity: ParseCancellationIdentity,
    },
    /// Stopped because a live tracker denied work.
    BudgetExhausted {
        /// Dimension that refused the next unit of work.
        dimension: ParseBudgetExhaustionDimension,
        /// Configured limit when the producer supplied one.
        limit: Option<usize>,
        /// Usage at refusal when the producer supplied one.
        usage: Option<usize>,
        /// Stage supplied by the producer, or [`ParseCompletionStage::Unspecified`].
        stage: ParseCompletionStage,
        /// Source range when the producer supplied one.
        range: Option<ParseCompletionRange>,
    },
    /// Unrecoverable parser or lexer failure.
    Catastrophic {
        /// Bounded catastrophic cause.
        cause: ParseCatastrophicCause,
        /// Whether the retained AST is an explicitly safe partial of the source.
        safe_partial: bool,
    },
}

/// Why a stored completion disagrees with the native parser output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ParseCompletionValidationError {
    /// Clean completion recorded a structured recovery count.
    #[error("clean completion forbids recovered_count {recovered_count}")]
    CleanWithRecovery {
        /// Observed structured recovery count.
        recovered_count: usize,
    },
    /// Clean completion recorded an early-stop cause.
    #[error("clean completion forbids stop cause {cause:?}")]
    CleanWithStopCause {
        /// Observed stop cause.
        cause: ParseStopCause,
    },
    /// Recovered completion had no structured recovery evidence.
    #[error("recovered completion requires recovered_count > 0")]
    RecoveredWithoutRecovery,
    /// Recovered completion recorded an early-stop cause.
    #[error("recovered completion forbids stop cause {cause:?}")]
    RecoveredWithStopCause {
        /// Observed stop cause.
        cause: ParseStopCause,
    },
    /// Cancelled completion was not backed by a cancelled stop cause.
    #[error("cancelled completion requires ParseStopCause::Cancelled")]
    CancelledWithoutCancelledCause,
    /// Budget-exhausted completion was not backed by a budget stop cause.
    #[error("budget-exhausted completion requires a budget ParseStopCause")]
    BudgetWithoutBudgetCause,
    /// Catastrophic completion was not backed by a catastrophic stop cause.
    #[error("catastrophic completion requires a catastrophic ParseStopCause")]
    CatastrophicWithoutCatastrophicCause,
    /// Catastrophic completion claimed a safe partial AST that is only the
    /// terminal fallback shell.
    #[error("catastrophic safe_partial requires a retained AST that is not the fallback shell")]
    CatastrophicSafePartialMissingAst,
    /// Recovered completion recorded a different count than the output.
    #[error("recovered completion count {recorded} disagrees with output {actual}")]
    RecoveredCountMismatch {
        /// Count stored on the completion.
        recorded: usize,
        /// Structured recovery count on the output.
        actual: usize,
    },
    /// Budget-exhausted completion payload disagrees with the stop cause.
    #[error("budget-exhausted completion payload disagrees with stop cause {cause:?}")]
    BudgetPayloadMismatch {
        /// Dimension stored on the completion.
        recorded_dimension: ParseBudgetExhaustionDimension,
        /// Limit stored on the completion.
        recorded_limit: Option<usize>,
        /// Usage stored on the completion.
        recorded_usage: Option<usize>,
        /// Observed budget stop cause.
        cause: ParseStopCause,
    },
    /// Catastrophic cause disagrees with the stop-cause variant.
    #[error("catastrophic cause {recorded:?} disagrees with stop cause {cause:?}")]
    CatastrophicCauseMismatch {
        /// Cause stored on the completion.
        recorded: ParseCatastrophicCause,
        /// Observed catastrophic stop cause.
        cause: ParseStopCause,
    },
}

impl ParseCompletion {
    /// Project the operation terminal from the live stop cause and structured
    /// recovery count.
    ///
    /// This is the only production constructor. It does not inspect diagnostic
    /// variants or message text.
    #[must_use]
    pub fn from_parser_terminal(
        stop_cause: Option<ParseStopCause>,
        recovered_count: usize,
        ast: &Node,
    ) -> Self {
        match stop_cause {
            Some(ParseStopCause::Cancelled) => Self::Cancelled {
                stage: ParseCompletionStage::Unspecified,
                identity: ParseCancellationIdentity::CooperativeToken,
            },
            Some(cause) if cause.is_budget_exhaustion() => budget_from_cause(cause),
            Some(ParseStopCause::CatastrophicTermination) => Self::Catastrophic {
                cause: ParseCatastrophicCause::UnrecoverableParserOrLexerFailure,
                safe_partial: !is_terminal_fallback_shell(ast),
            },
            Some(ParseStopCause::FutureTypedTerminal) => Self::Catastrophic {
                cause: ParseCatastrophicCause::FutureTypedTerminal,
                safe_partial: false,
            },
            Some(_) => Self::Catastrophic {
                cause: ParseCatastrophicCause::FutureTypedTerminal,
                safe_partial: false,
            },
            None if recovered_count > 0 => Self::CompletedWithRecovery { recovered_count },
            None => Self::CompletedClean,
        }
    }

    /// Construct a cancelled completion with an explicit stage.
    #[must_use]
    pub const fn cancelled(
        stage: ParseCompletionStage,
        identity: ParseCancellationIdentity,
    ) -> Self {
        Self::Cancelled { stage, identity }
    }

    /// Construct a budget-exhausted completion with producer-supplied fields.
    #[must_use]
    pub const fn budget_exhausted(
        dimension: ParseBudgetExhaustionDimension,
        limit: Option<usize>,
        usage: Option<usize>,
        stage: ParseCompletionStage,
        range: Option<ParseCompletionRange>,
    ) -> Self {
        Self::BudgetExhausted { dimension, limit, usage, stage, range }
    }

    /// Construct a catastrophic completion, with or without a safe partial AST.
    #[must_use]
    pub const fn catastrophic(cause: ParseCatastrophicCause, safe_partial: bool) -> Self {
        Self::Catastrophic { cause, safe_partial }
    }

    /// Coarse class independent of payload fields.
    #[must_use]
    pub const fn class(self) -> ParseCompletionClass {
        match self {
            Self::CompletedClean => ParseCompletionClass::Clean,
            Self::CompletedWithRecovery { .. } => ParseCompletionClass::Recovered,
            Self::Cancelled { .. } => ParseCompletionClass::Cancelled,
            Self::BudgetExhausted { .. } => ParseCompletionClass::BudgetExhausted,
            Self::Catastrophic { .. } => ParseCompletionClass::Catastrophic,
        }
    }

    /// Required/forbidden field checks against the native parser output.
    pub fn validate_against_output(
        self,
        output: &ParseOutput,
    ) -> Result<(), ParseCompletionValidationError> {
        match self {
            Self::CompletedClean => {
                if let Some(cause) = output.stop_cause() {
                    return Err(ParseCompletionValidationError::CleanWithStopCause { cause });
                }
                if output.recovered_count > 0 {
                    return Err(ParseCompletionValidationError::CleanWithRecovery {
                        recovered_count: output.recovered_count,
                    });
                }
                Ok(())
            }
            Self::CompletedWithRecovery { recovered_count } => {
                if let Some(cause) = output.stop_cause() {
                    return Err(ParseCompletionValidationError::RecoveredWithStopCause { cause });
                }
                if recovered_count == 0 || output.recovered_count == 0 {
                    return Err(ParseCompletionValidationError::RecoveredWithoutRecovery);
                }
                if recovered_count != output.recovered_count {
                    return Err(ParseCompletionValidationError::RecoveredCountMismatch {
                        recorded: recovered_count,
                        actual: output.recovered_count,
                    });
                }
                Ok(())
            }
            Self::Cancelled { .. } => match output.stop_cause() {
                Some(ParseStopCause::Cancelled) => Ok(()),
                Some(_) | None => {
                    Err(ParseCompletionValidationError::CancelledWithoutCancelledCause)
                }
            },
            Self::BudgetExhausted { dimension, limit, usage, .. } => match output.stop_cause() {
                Some(cause) if cause.is_budget_exhaustion() => match budget_from_cause(cause) {
                    Self::BudgetExhausted {
                        dimension: expected_dimension,
                        limit: expected_limit,
                        usage: expected_usage,
                        ..
                    } if dimension == expected_dimension
                        && limit == expected_limit
                        && usage == expected_usage =>
                    {
                        Ok(())
                    }
                    Self::BudgetExhausted { .. } => {
                        Err(ParseCompletionValidationError::BudgetPayloadMismatch {
                            recorded_dimension: dimension,
                            recorded_limit: limit,
                            recorded_usage: usage,
                            cause,
                        })
                    }
                    _ => Err(ParseCompletionValidationError::BudgetWithoutBudgetCause),
                },
                Some(_) | None => Err(ParseCompletionValidationError::BudgetWithoutBudgetCause),
            },
            Self::Catastrophic { cause, safe_partial } => match output.stop_cause() {
                Some(stop) => match Self::from_parser_terminal(
                    Some(stop),
                    output.recovered_count,
                    &output.ast,
                ) {
                    Self::Catastrophic { cause: expected_cause, .. } => {
                        if cause != expected_cause {
                            Err(ParseCompletionValidationError::CatastrophicCauseMismatch {
                                recorded: cause,
                                cause: stop,
                            })
                        } else if safe_partial && is_terminal_fallback_shell(&output.ast) {
                            Err(ParseCompletionValidationError::CatastrophicSafePartialMissingAst)
                        } else {
                            Ok(())
                        }
                    }
                    _ => Err(ParseCompletionValidationError::CatastrophicWithoutCatastrophicCause),
                },
                None => Err(ParseCompletionValidationError::CatastrophicWithoutCatastrophicCause),
            },
        }
    }
}

fn budget_from_cause(cause: ParseStopCause) -> ParseCompletion {
    match cause {
        ParseStopCause::RecursionBudgetExhausted { limit, usage } => {
            ParseCompletion::BudgetExhausted {
                dimension: ParseBudgetExhaustionDimension::Recursion,
                limit,
                usage,
                stage: ParseCompletionStage::Unspecified,
                range: None,
            }
        }
        ParseStopCause::NestingOrDepthBudgetExhausted { limit, usage } => {
            ParseCompletion::BudgetExhausted {
                dimension: ParseBudgetExhaustionDimension::NestingOrDepth,
                limit: Some(limit),
                usage: Some(usage),
                stage: ParseCompletionStage::Unspecified,
                range: None,
            }
        }
        ParseStopCause::HeredocBudgetExhausted { limit, usage } => {
            ParseCompletion::BudgetExhausted {
                dimension: ParseBudgetExhaustionDimension::HeredocScanBytes,
                limit: Some(limit),
                usage: Some(usage),
                stage: ParseCompletionStage::Unspecified,
                range: None,
            }
        }
        ParseStopCause::CoreBudgetExhausted { dimension, limit, usage } => {
            ParseCompletion::BudgetExhausted {
                dimension: ParseBudgetExhaustionDimension::Core(dimension),
                limit: Some(limit),
                usage: Some(usage),
                stage: ParseCompletionStage::Unspecified,
                range: None,
            }
        }
        ParseStopCause::LexerBudgetExhausted => ParseCompletion::BudgetExhausted {
            dimension: ParseBudgetExhaustionDimension::Lexer,
            limit: None,
            usage: None,
            stage: ParseCompletionStage::Unspecified,
            range: None,
        },
        ParseStopCause::Cancelled
        | ParseStopCause::CatastrophicTermination
        | ParseStopCause::FutureTypedTerminal => ParseCompletion::Catastrophic {
            cause: ParseCatastrophicCause::FutureTypedTerminal,
            safe_partial: false,
        },
    }
}

/// The empty `Program` shell returned after `parse()` `Err` is not an
/// explicitly safe partial of the source.
pub(super) fn is_terminal_fallback_shell(ast: &Node) -> bool {
    matches!(&ast.kind, NodeKind::Program { statements } if statements.is_empty())
        && ast.location.start == 0
        && ast.location.end == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::error::{BudgetTracker, ParseError, RecoveryKind, RecoverySite};
    use perl_ast::SourceLocation;

    fn empty_ast() -> Node {
        Node::new(NodeKind::Program { statements: vec![] }, SourceLocation { start: 0, end: 0 })
    }

    fn nonempty_ast() -> Node {
        Node::new(
            NodeKind::Program {
                statements: vec![Node::new(
                    NodeKind::Number { value: "1".into() },
                    SourceLocation { start: 0, end: 1 },
                )],
            },
            SourceLocation { start: 0, end: 1 },
        )
    }

    #[test]
    fn advisory_diagnostics_do_not_create_recovery() {
        let output = ParseOutput::finish(
            empty_ast(),
            vec![ParseError::Advisory { message: "style".into(), location: 0 }],
            BudgetTracker::new(),
            None,
        );
        assert_eq!(output.completion(), ParseCompletion::CompletedClean);
        assert_eq!(output.completion().class(), ParseCompletionClass::Clean);
        assert!(output.completion().validate_against_output(&output).is_ok());
    }

    #[test]
    fn recovered_count_creates_recovery_without_stop_cause() {
        let output = ParseOutput::finish(
            empty_ast(),
            vec![ParseError::Recovered {
                site: RecoverySite::InfixRhs,
                kind: RecoveryKind::MissingOperand,
                location: 8,
            }],
            BudgetTracker::new(),
            None,
        );
        assert_eq!(
            output.completion(),
            ParseCompletion::CompletedWithRecovery { recovered_count: 1 }
        );
        assert_eq!(output.completion().class(), ParseCompletionClass::Recovered);
    }

    #[test]
    fn cancelled_diagnostic_without_stop_cause_stays_clean() {
        let output = ParseOutput::finish(
            empty_ast(),
            vec![ParseError::Cancelled],
            BudgetTracker::new(),
            None,
        );
        assert_eq!(output.completion(), ParseCompletion::CompletedClean);
        assert_ne!(output.completion().class(), ParseCompletionClass::Cancelled);
    }

    #[test]
    fn recursion_diagnostic_without_stop_cause_is_not_exhaustion() {
        let output = ParseOutput::finish(
            empty_ast(),
            vec![ParseError::RecursionLimit, ParseError::NestingTooDeep { depth: 9, max_depth: 8 }],
            BudgetTracker::new(),
            None,
        );
        assert_eq!(output.completion(), ParseCompletion::CompletedClean);
        assert_ne!(output.completion().class(), ParseCompletionClass::BudgetExhausted);
    }

    #[test]
    fn typed_cancel_wins_over_recovery_diagnostics() {
        let output = ParseOutput::finish(
            empty_ast(),
            vec![
                ParseError::Recovered {
                    site: RecoverySite::Statement,
                    kind: RecoveryKind::InferredSemicolon,
                    location: 0,
                },
                ParseError::syntax("later", 4),
            ],
            BudgetTracker::new(),
            Some(ParseStopCause::Cancelled),
        );
        assert_eq!(
            output.completion(),
            ParseCompletion::cancelled(
                ParseCompletionStage::Unspecified,
                ParseCancellationIdentity::CooperativeToken,
            )
        );
        assert_eq!(output.recovered_count, 1);
    }

    #[test]
    fn typed_budget_wins_over_cancelled_diagnostic_text() {
        let cause = ParseStopCause::CoreBudgetExhausted {
            dimension: ParseCoreDimension::TokensConsumed,
            limit: 4,
            usage: 4,
        };
        let output = ParseOutput::finish(
            empty_ast(),
            vec![ParseError::Cancelled],
            BudgetTracker::new(),
            Some(cause),
        );
        match output.completion() {
            ParseCompletion::BudgetExhausted {
                dimension: ParseBudgetExhaustionDimension::Core(ParseCoreDimension::TokensConsumed),
                limit: Some(4),
                usage: Some(4),
                ..
            } => {}
            other => panic!("expected core budget exhaustion, got {other:?}"),
        }
    }

    #[test]
    fn catastrophic_constructor_distinguishes_safe_partial() {
        let fallback = ParseOutput::finish(
            empty_ast(),
            vec![ParseError::UnexpectedEof],
            BudgetTracker::new(),
            Some(ParseStopCause::CatastrophicTermination),
        );
        assert_eq!(
            fallback.completion(),
            ParseCompletion::catastrophic(
                ParseCatastrophicCause::UnrecoverableParserOrLexerFailure,
                false,
            )
        );

        let partial = ParseOutput::finish(
            nonempty_ast(),
            vec![ParseError::UnexpectedEof],
            BudgetTracker::new(),
            Some(ParseStopCause::CatastrophicTermination),
        );
        assert_eq!(
            partial.completion(),
            ParseCompletion::catastrophic(
                ParseCatastrophicCause::UnrecoverableParserOrLexerFailure,
                true,
            )
        );
        assert!(partial.completion().validate_against_output(&partial).is_ok());
    }

    #[test]
    fn blocking_diagnostic_without_recovery_is_not_recovered() {
        let output = ParseOutput::finish(
            empty_ast(),
            vec![ParseError::UnexpectedEof],
            BudgetTracker::new(),
            None,
        );
        assert_eq!(output.completion(), ParseCompletion::CompletedClean);
        assert_ne!(output.completion().class(), ParseCompletionClass::Recovered);
        assert_ne!(output.completion().class(), ParseCompletionClass::Catastrophic);
    }

    #[test]
    fn field_matrix_rejects_mismatched_required_fields() {
        let clean = ParseOutput::success(empty_ast());
        assert!(ParseCompletion::CompletedClean.validate_against_output(&clean).is_ok());
        assert_eq!(
            ParseCompletion::CompletedWithRecovery { recovered_count: 1 }
                .validate_against_output(&clean),
            Err(ParseCompletionValidationError::RecoveredWithoutRecovery)
        );
        assert_eq!(
            ParseCompletion::cancelled(
                ParseCompletionStage::BeforeParse,
                ParseCancellationIdentity::CooperativeToken,
            )
            .validate_against_output(&clean),
            Err(ParseCompletionValidationError::CancelledWithoutCancelledCause)
        );

        let cancelled = ParseOutput::finish(
            empty_ast(),
            vec![],
            BudgetTracker::new(),
            Some(ParseStopCause::Cancelled),
        );
        assert_eq!(
            ParseCompletion::CompletedClean.validate_against_output(&cancelled),
            Err(ParseCompletionValidationError::CleanWithStopCause {
                cause: ParseStopCause::Cancelled,
            })
        );

        let catastrophic = ParseOutput::finish(
            empty_ast(),
            vec![],
            BudgetTracker::new(),
            Some(ParseStopCause::CatastrophicTermination),
        );
        assert_eq!(
            ParseCompletion::catastrophic(
                ParseCatastrophicCause::UnrecoverableParserOrLexerFailure,
                true,
            )
            .validate_against_output(&catastrophic),
            Err(ParseCompletionValidationError::CatastrophicSafePartialMissingAst)
        );
    }

    #[test]
    fn validate_rejects_mismatched_recovery_count_budget_payload_and_cause() {
        let recovered = ParseOutput::finish(
            empty_ast(),
            vec![
                ParseError::Recovered {
                    site: RecoverySite::InfixRhs,
                    kind: RecoveryKind::MissingOperand,
                    location: 1,
                },
                ParseError::Recovered {
                    site: RecoverySite::Statement,
                    kind: RecoveryKind::InferredSemicolon,
                    location: 4,
                },
            ],
            BudgetTracker::new(),
            None,
        );
        assert_eq!(recovered.recovered_count, 2);
        assert_eq!(
            ParseCompletion::CompletedWithRecovery { recovered_count: 1 }
                .validate_against_output(&recovered),
            Err(ParseCompletionValidationError::RecoveredCountMismatch { recorded: 1, actual: 2 })
        );

        let recursion = ParseOutput::finish(
            empty_ast(),
            vec![],
            BudgetTracker::new(),
            Some(ParseStopCause::RecursionBudgetExhausted { limit: Some(8), usage: Some(9) }),
        );
        assert_eq!(
            ParseCompletion::budget_exhausted(
                ParseBudgetExhaustionDimension::Lexer,
                None,
                None,
                ParseCompletionStage::Unspecified,
                None,
            )
            .validate_against_output(&recursion),
            Err(ParseCompletionValidationError::BudgetPayloadMismatch {
                recorded_dimension: ParseBudgetExhaustionDimension::Lexer,
                recorded_limit: None,
                recorded_usage: None,
                cause: ParseStopCause::RecursionBudgetExhausted { limit: Some(8), usage: Some(9) },
            })
        );
        assert!(recursion.completion().validate_against_output(&recursion).is_ok());

        let catastrophic = ParseOutput::finish(
            empty_ast(),
            vec![],
            BudgetTracker::new(),
            Some(ParseStopCause::CatastrophicTermination),
        );
        assert_eq!(
            ParseCompletion::catastrophic(ParseCatastrophicCause::FutureTypedTerminal, false)
                .validate_against_output(&catastrophic),
            Err(ParseCompletionValidationError::CatastrophicCauseMismatch {
                recorded: ParseCatastrophicCause::FutureTypedTerminal,
                cause: ParseStopCause::CatastrophicTermination,
            })
        );
    }

    #[test]
    fn every_cancellation_stage_is_constructible() {
        for stage in [
            ParseCompletionStage::Unspecified,
            ParseCompletionStage::BeforeParse,
            ParseCompletionStage::OrdinaryParsing,
            ParseCompletionStage::Recovery,
            ParseCompletionStage::BeforePublication,
        ] {
            let completion =
                ParseCompletion::cancelled(stage, ParseCancellationIdentity::CooperativeToken);
            assert_eq!(completion.class(), ParseCompletionClass::Cancelled);
            match completion {
                ParseCompletion::Cancelled { stage: observed, .. } => assert_eq!(observed, stage),
                other => panic!("expected cancelled, got {other:?}"),
            }
        }
    }

    #[test]
    fn independent_budget_dimensions_are_constructible() {
        let dimensions = [
            ParseBudgetExhaustionDimension::Recursion,
            ParseBudgetExhaustionDimension::NestingOrDepth,
            ParseBudgetExhaustionDimension::HeredocScanBytes,
            ParseBudgetExhaustionDimension::Core(ParseCoreDimension::NodesConstructed),
            ParseBudgetExhaustionDimension::Lexer,
        ];
        for dimension in dimensions {
            let completion = ParseCompletion::budget_exhausted(
                dimension,
                Some(1),
                Some(1),
                ParseCompletionStage::OrdinaryParsing,
                Some(ParseCompletionRange { start: 0, end: 4 }),
            );
            assert_eq!(completion.class(), ParseCompletionClass::BudgetExhausted);
        }
    }
}
