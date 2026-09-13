//! Anti-pattern detection for heredoc edge cases.
//!
//! This module provides detection and analysis of problematic Perl patterns
//! that make static parsing difficult or impossible, particularly around heredocs.
//!
//! The [`crate::heredoc_anti_patterns::AntiPatternDetector`] scans Perl source
//! for seven categories of heredoc-related anti-patterns and produces a
//! [`crate::heredoc_anti_patterns::DetectionReport`]. Completeness is carried by
//! [`crate::heredoc_anti_patterns::DetectionStatus`]; an empty diagnostic list
//! is not a complete-clean scan.
//!
//! [`crate::heredoc_anti_patterns::AntiPatternDetector::detect_all`] remains as
//! a diagnostics-only compatibility projection.

mod detectors;
mod model;
mod utils;

pub use detectors::AntiPatternDetector;
pub use model::{
    AntiPattern, DetectionReport, DetectionStatus, DetectorFailureReason, DetectorId,
    DetectorObservation, DetectorState, Diagnostic, HeredocDelimiter, Location, Severity,
};
