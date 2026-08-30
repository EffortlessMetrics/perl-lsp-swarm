//! Production parse-operation context owned by [`super::Parser`].
//!
//! This is the #8757 / #8700 B01 authority: one immutable configuration
//! identity, one live [`BudgetTracker`], one cancellation-probe handle, one
//! operation identity, and one terminal-state accumulator. Token, node, and
//! diagnostic charging remain #8786 (B02). [`crate::parser_context::ParserContext`]
//! is a parallel AST-v2 helper, not this authority (#8700 B04 / #7105).

use crate::error::{BudgetTracker, ParseBudget, ParseError, ParseResult, ParseStopCause};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Immutable identity of the production parser configuration selected for an
/// operation.
///
/// Convenience constructors [`super::Parser::new`],
/// [`super::Parser::new_with_recovery_config`], and
/// [`super::Parser::with_production_config`] select the same documented default
/// through one path. This is not the public configuration API (#7080).
///
/// [`crate::parser_context::ParserContext`] cannot produce this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParserConfigIdentity {
    budget: ParseBudget,
    max_recursion_depth: usize,
    max_block_nesting_depth: usize,
}

impl ParserConfigIdentity {
    /// Documented production default shared by strict and recovery-aware
    /// constructors.
    ///
    /// Recursion still uses the historical production limit (128), not
    /// [`ParseBudget::max_depth`] (256). Changing that limit would change
    /// when `RecursionDepthExhausted` fires.
    pub fn production_default() -> Self {
        Self {
            budget: ParseBudget::default(),
            max_recursion_depth: super::MAX_RECURSION_DEPTH,
            max_block_nesting_depth: super::MAX_BLOCK_NESTING_DEPTH,
        }
    }

    /// Budget identity stored for this operation. Charging sites land in B02.
    pub fn budget(self) -> ParseBudget {
        self.budget
    }

    /// Select an explicit resource budget for this configuration identity.
    ///
    /// Budget policy is part of the configuration identity, so two parsers with
    /// different budgets are different configurations and may legitimately
    /// reach different typed terminals for the same source (#7291). Recursion
    /// and block-nesting limits are unchanged: they remain the historical
    /// production values, not [`ParseBudget`] fields.
    #[must_use]
    pub fn with_budget(self, budget: ParseBudget) -> Self {
        Self { budget, ..self }
    }

    /// Production recursion-depth limit checked by the live context API.
    pub fn max_recursion_depth(self) -> usize {
        self.max_recursion_depth
    }

    /// Structural block-nesting limit. [`super::Parser::block_depth`] remains
    /// syntactic context, not this resource-control authority.
    pub fn max_block_nesting_depth(self) -> usize {
        self.max_block_nesting_depth
    }
}

/// Unique identity of one parser operation on a [`super::Parser`] instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParserOperationId(u64);

impl ParserOperationId {
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// Raw identity value. Distinct operations receive distinct values.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Live operation-local state owned by the production parser.
pub(crate) struct ParserOperationContext {
    config: ParserConfigIdentity,
    tracker: BudgetTracker,
    cancellation: Option<Arc<AtomicBool>>,
    cancellation_check_counter: usize,
    operation_id: ParserOperationId,
    terminal: Option<ParseStopCause>,
}

impl ParserOperationContext {
    pub(crate) fn new(config: ParserConfigIdentity, cancellation: Option<Arc<AtomicBool>>) -> Self {
        Self {
            config,
            tracker: BudgetTracker::new(),
            cancellation,
            cancellation_check_counter: 0,
            operation_id: ParserOperationId::next(),
            terminal: None,
        }
    }

    /// Start a fresh operation: new identity, zeroed tracker/terminal/counter.
    /// Configuration and the cancellation-probe handle are retained.
    pub(crate) fn begin(&mut self) {
        self.operation_id = ParserOperationId::next();
        self.tracker = BudgetTracker::new();
        self.cancellation_check_counter = 0;
        self.terminal = None;
    }

    pub(crate) fn config(&self) -> ParserConfigIdentity {
        self.config
    }

    pub(crate) fn operation_id(&self) -> ParserOperationId {
        self.operation_id
    }

    #[cfg(test)]
    pub(crate) fn tracker(&self) -> &BudgetTracker {
        &self.tracker
    }

    pub(crate) fn take_tracker(&mut self) -> BudgetTracker {
        std::mem::take(&mut self.tracker)
    }

    /// Record the terminal cause for this operation, preserving the first.
    ///
    /// More than one `Ok`-path branch can record a terminal in a single parse:
    /// a refused heredoc collection does not stop statement parsing, so a later
    /// lexer-budget `UnknownRest` could otherwise overwrite the heredoc cause
    /// and leave `stop_cause()` disagreeing with the diagnostic vector. The
    /// first selected cause is the causal one and is immutable for the rest of
    /// the operation; [`ParserOperationContext::begin`] clears it.
    pub(crate) fn record_terminal(&mut self, cause: ParseStopCause) {
        self.terminal.get_or_insert(cause);
    }

    pub(crate) fn take_terminal(&mut self) -> Option<ParseStopCause> {
        self.terminal.take()
    }

    /// Whether this operation has already selected the heredoc-collection budget
    /// as its terminal.
    ///
    /// Deliberately distinct from
    /// [`ParserOperationContext::heredoc_scan_exhausted`], which is true as soon as
    /// charged usage reaches the limit — including before any collection has been
    /// attempted at all, when the configured budget is zero. Heredoc admission must
    /// let that first declaration through so the drain can refuse it and report the
    /// typed terminal; only once that report exists is further admission pointless.
    pub(crate) fn heredoc_budget_terminal_recorded(&self) -> bool {
        matches!(self.terminal, Some(ParseStopCause::HeredocBudgetExhausted { .. }))
    }

    pub(crate) fn is_pre_cancelled(&self) -> bool {
        self.cancellation.as_ref().is_some_and(|flag| flag.load(Ordering::Relaxed))
    }

    pub(crate) fn probe_cancellation(&mut self) -> ParseResult<()> {
        self.cancellation_check_counter = self.cancellation_check_counter.wrapping_add(1);
        if self.cancellation_check_counter & 63 == 0
            && let Some(ref flag) = self.cancellation
            && flag.load(Ordering::Relaxed)
        {
            return Err(ParseError::Cancelled);
        }
        Ok(())
    }

    /// Check the next depth before entering, then record current and maximum
    /// depth once. Does not increment when the next depth would exceed the
    /// production recursion limit.
    pub(crate) fn enter_recursion(&mut self) -> ParseResult<()> {
        let next = self.tracker.current_depth.saturating_add(1);
        if next > self.config.max_recursion_depth {
            return Err(ParseError::RecursionDepthExhausted {
                depth: next,
                max_depth: self.config.max_recursion_depth,
            });
        }
        self.tracker.enter_depth();
        Ok(())
    }

    pub(crate) fn exit_recursion(&mut self) {
        self.tracker.exit_depth();
    }

    /// Whether the deterministic heredoc collection budget is already spent.
    ///
    /// This is the before-work half of the #7291 charge rule: the parser
    /// refuses to begin another heredoc collection once the charged total
    /// reaches the configured limit.
    pub(crate) fn heredoc_scan_exhausted(&self) -> bool {
        self.tracker.heredoc_scan_exhausted(&self.config.budget())
    }

    /// Configured heredoc scan limit and the usage charged so far.
    pub(crate) fn heredoc_scan_state(&self) -> (usize, usize) {
        (self.config.budget().max_heredoc_scan_bytes, self.tracker.heredoc_scan_bytes)
    }

    /// Charge source bytes traversed by heredoc collection (after-work half).
    pub(crate) fn record_heredoc_scan(&mut self, bytes: usize) {
        self.tracker.record_heredoc_scan(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refused heredoc collection does not stop statement parsing, so a later
    /// lexer-budget stop can be recorded in the same operation. The first cause
    /// is the causal one: without this, `stop_cause()` could name a different
    /// limit than the diagnostic vector reports.
    #[test]
    fn first_recorded_terminal_wins_and_begin_clears_it() {
        let mut ctx = ParserOperationContext::new(ParserConfigIdentity::production_default(), None);

        ctx.record_terminal(ParseStopCause::HeredocBudgetExhausted { limit: 4, usage: 9 });
        ctx.record_terminal(ParseStopCause::LexerBudgetExhausted);

        assert_eq!(
            ctx.take_terminal(),
            Some(ParseStopCause::HeredocBudgetExhausted { limit: 4, usage: 9 }),
            "a later terminal must not overwrite the first causal one"
        );

        ctx.record_terminal(ParseStopCause::LexerBudgetExhausted);
        ctx.begin();
        assert_eq!(ctx.take_terminal(), None, "a new operation must start with no terminal");
    }

    #[test]
    fn production_default_identity_is_stable() {
        assert_eq!(
            ParserConfigIdentity::production_default(),
            ParserConfigIdentity::production_default()
        );
        assert_eq!(
            ParserConfigIdentity::production_default().max_recursion_depth(),
            super::super::MAX_RECURSION_DEPTH
        );
    }

    #[test]
    fn enter_records_max_and_exit_unwinds_current() {
        let mut ctx = ParserOperationContext::new(ParserConfigIdentity::production_default(), None);
        ctx.enter_recursion().expect("depth 1");
        ctx.enter_recursion().expect("depth 2");
        assert_eq!(ctx.tracker.current_depth, 2);
        assert_eq!(ctx.tracker.max_depth_reached, 2);
        ctx.exit_recursion();
        assert_eq!(ctx.tracker.current_depth, 1);
        assert_eq!(ctx.tracker.max_depth_reached, 2);
        ctx.exit_recursion();
        assert_eq!(ctx.tracker.current_depth, 0);
        assert_eq!(ctx.tracker.max_depth_reached, 2);
    }

    #[test]
    fn exhaustion_checks_next_depth_without_entering() {
        let mut ctx = ParserOperationContext::new(ParserConfigIdentity::production_default(), None);
        for _ in 0..super::super::MAX_RECURSION_DEPTH {
            ctx.enter_recursion().expect("within limit");
        }
        let err = ctx.enter_recursion().expect_err("limit + 1 must fail");
        assert!(matches!(
            err,
            ParseError::RecursionDepthExhausted { depth, max_depth }
            if depth == super::super::MAX_RECURSION_DEPTH.saturating_add(1)
                && max_depth == super::super::MAX_RECURSION_DEPTH
        ));
        assert_eq!(ctx.tracker.current_depth, super::super::MAX_RECURSION_DEPTH);
        ctx.exit_recursion();
        assert_eq!(ctx.tracker.current_depth, super::super::MAX_RECURSION_DEPTH.saturating_sub(1));
    }

    #[test]
    fn begin_resets_tracker_and_allocates_a_new_operation_id() {
        let mut ctx = ParserOperationContext::new(ParserConfigIdentity::production_default(), None);
        ctx.enter_recursion().expect("depth");
        ctx.record_terminal(ParseStopCause::Cancelled);
        let first_id = ctx.operation_id;
        ctx.begin();
        assert_ne!(ctx.operation_id, first_id);
        assert_eq!(ctx.tracker.current_depth, 0);
        assert_eq!(ctx.tracker.max_depth_reached, 0);
        assert!(ctx.terminal.is_none());
        assert_eq!(ctx.config, ParserConfigIdentity::production_default());
    }

    #[test]
    fn operation_ids_are_unique() {
        let a = ParserOperationId::next();
        let b = ParserOperationId::next();
        assert_ne!(a, b);
    }
}
