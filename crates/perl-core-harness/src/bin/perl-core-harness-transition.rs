//! Thin CLI for classifying a compile observation against an accepted ratchet.
//!
//! This slice wires argument parsing and one V2 classify I/O path onto the
//! in-lib [`perl_core_harness::transition::classify_transition`] core.
//! Discovery/series identity binding, receipt digests, the `check` command,
//! and V1 baseline migration remain follow-up slices.

#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

use color_eyre::eyre::{Context, Result, bail};
use perl_core_harness::transition::{AcceptedBaseline, Classification, classify_transition};
use perl_core_harness_types::{
    COMPILE_BASELINE_V2_SCHEMA_VERSION, CompatibilityTransition, CompileBaselineV2,
    RUN_REPORT_SCHEMA_VERSION, RunReport,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

include!("perl-core-harness-transition/cli.rs");
include!("perl-core-harness-transition/run.rs");
