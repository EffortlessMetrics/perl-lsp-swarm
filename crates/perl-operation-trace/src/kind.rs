//! Closed vocabulary of operation kinds.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The kind of long-lived operation being traced.
///
/// A **closed** vocabulary, deliberately not `#[non_exhaustive]`: an
/// operation kind is part of the wire contract downstream consumers
/// pattern-match on, so a caller-supplied kind string must never silently
/// become a policy authority for a kind nobody reviewed. `#[derive]`'s
/// generated `Deserialize` already rejects any variant name it does not
/// recognize — that is the fail-closed behavior this type relies on, rather
/// than a hand-written parser with a fallback branch.
///
/// This crate does not wire `OperationKind` into [`crate::OperationEvent`]
/// or [`crate::OperationRecorder`] itself: it defines the vocabulary a
/// producer can use to classify an operation as a whole (as opposed to one
/// of its events), leaving *where* that classification is attached — a
/// dedicated field, a registry-declared event field, or a future addition —
/// to the first real consumer. See `CLAUDE.md`'s "Claim boundary" section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OperationKind {
    /// One LSP request/response cycle.
    LspRequest,
    /// Workspace indexing or extraction.
    WorkspaceIndexing,
    /// A test run (single test or suite).
    TestRun,
    /// A supervised child-process execution.
    ProcessExecution,
    /// A DAP debug session.
    DebugSession,
    /// A compiler-facts build.
    CompilerBuild,
    /// Reload of workspace or editor configuration.
    ConfigurationReload,
    /// Rehydration of a persisted snapshot.
    SnapshotHydration,
    /// Production of a RIPR fact packet.
    RiprPacketProduction,
    /// A release-candidate build or verification pass.
    ReleaseCandidate,
}

impl fmt::Display for OperationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::LspRequest => "lsp_request",
            Self::WorkspaceIndexing => "workspace_indexing",
            Self::TestRun => "test_run",
            Self::ProcessExecution => "process_execution",
            Self::DebugSession => "debug_session",
            Self::CompilerBuild => "compiler_build",
            Self::ConfigurationReload => "configuration_reload",
            Self::SnapshotHydration => "snapshot_hydration",
            Self::RiprPacketProduction => "ripr_packet_production",
            Self::ReleaseCandidate => "release_candidate",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const ALL: [OperationKind; 10] = [
        OperationKind::LspRequest,
        OperationKind::WorkspaceIndexing,
        OperationKind::TestRun,
        OperationKind::ProcessExecution,
        OperationKind::DebugSession,
        OperationKind::CompilerBuild,
        OperationKind::ConfigurationReload,
        OperationKind::SnapshotHydration,
        OperationKind::RiprPacketProduction,
        OperationKind::ReleaseCandidate,
    ];

    #[test]
    fn serde_round_trip_is_lossless_for_every_variant() {
        for kind in ALL {
            let json = serde_json::to_string(&kind).expect("serialize");
            let back: OperationKind = serde_json::from_str(&json).expect("valid kind must parse");
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn unknown_kind_fails_closed() {
        for bad in ["\"NotAKind\"", "\"\"", "\"lsp_request\"", "42"] {
            assert!(
                serde_json::from_str::<OperationKind>(bad).is_err(),
                "must reject {bad}, not silently decode a default variant"
            );
        }
    }

    #[test]
    fn display_is_stable_snake_case() {
        assert_eq!(OperationKind::LspRequest.to_string(), "lsp_request");
        assert_eq!(OperationKind::ReleaseCandidate.to_string(), "release_candidate");
    }

    #[test]
    fn every_variant_has_a_distinct_display_string() {
        let mut seen = std::collections::BTreeSet::new();
        for kind in ALL {
            assert!(seen.insert(kind.to_string()), "duplicate display string for {kind:?}");
        }
    }
}
