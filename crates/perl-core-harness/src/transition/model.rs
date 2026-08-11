use perl_core_harness_types::{
    CompileBaseline, CompileBaselineV2, ObservedSemanticBoundary, RunFailure, RunFileResult,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub enum AcceptedBaseline {
    V1(CompileBaseline),
    V2(Box<CompileBaselineV2>),
}

impl AcceptedBaseline {
    pub fn file_results(&self) -> &[RunFileResult] {
        match self {
            Self::V1(value) => &value.file_results,
            Self::V2(value) => &value.file_results,
        }
    }

    pub fn failures(&self) -> &[RunFailure] {
        match self {
            Self::V1(value) => &value.expected_failures,
            Self::V2(value) => &value.expected_failures,
        }
    }

    pub fn semantic_boundaries(&self) -> Option<&[ObservedSemanticBoundary]> {
        match self {
            Self::V1(value) => value.semantic_boundaries.as_deref(),
            Self::V2(value) => Some(&value.semantic_boundaries),
        }
    }

    pub fn buckets(&self) -> &BTreeMap<String, usize> {
        match self {
            Self::V1(value) => &value.buckets,
            Self::V2(value) => &value.buckets,
        }
    }

    pub fn state(&self) -> TransitionRunState {
        match self {
            Self::V1(value) => TransitionRunState {
                files_total: value.files_total,
                files_passed: value.files_passed,
                files_failed: value.files_failed,
                tap_assertions_total: value.tap_assertions_total,
                tap_assertions_passed: value.tap_assertions_passed,
            },
            Self::V2(value) => TransitionRunState {
                files_total: value.files_total,
                files_passed: value.files_passed,
                files_failed: value.files_failed,
                tap_assertions_total: value.tap_assertions_total,
                tap_assertions_passed: value.tap_assertions_passed,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionRunState {
    pub files_total: usize,
    pub files_passed: usize,
    pub files_failed: usize,
    pub tap_assertions_total: usize,
    pub tap_assertions_passed: usize,
}
