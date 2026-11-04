//! Transition model, classifier and CLI contracts share one integration harness.
//!
//! Keep suite-local fixtures and assertions independent. The module names retain
//! the old target names as test filters, for example:
//! `cargo test -p perl-core-harness --test transition_contracts transition_cli_classify::`.

#[path = "transition/transition_accepted_baseline_model.rs"]
mod transition_accepted_baseline_model;
#[path = "transition/transition_classify_regression.rs"]
mod transition_classify_regression;
#[path = "transition/transition_cli_classify.rs"]
mod transition_cli_classify;
