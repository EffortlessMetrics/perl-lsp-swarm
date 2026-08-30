//! Structural containment for pull-diagnostic result identity (#7480).
//!
//! #7480 replaced content-only `md5(content)` result IDs on every pull
//! transport with a complete evaluation-and-projection subject composer.
//! These checks pin the structural recurrence guards:
//!
//! 1. No production pull-diagnostics source mints result IDs via `md5` or any
//!    other content-only digest helper.
//! 2. The only place `perl-lsp-rs` still depends on `md5` is recorded as an
//!    unrelated owned use (module-resolution/execute-command cache keys), not
//!    diagnostic report identity.

use std::path::PathBuf;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn crate_src_path(relative: &str) -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest).join("src").join(relative)
}

fn read_source(relative: &str) -> TestResult<String> {
    let path = crate_src_path(relative);
    std::fs::read_to_string(&path)
        .map_err(|error| format!("reading {}: {error}", path.display()).into())
}

fn source_between<'a>(source: &'a str, start: &str, end: &str) -> TestResult<&'a str> {
    let start_offset =
        source.find(start).ok_or_else(|| format!("missing structural start marker: {start}"))?;
    let tail = source
        .get(start_offset..)
        .ok_or_else(|| format!("invalid structural start offset for: {start}"))?;
    let end_offset = tail
        .find(end)
        .ok_or_else(|| format!("missing structural end marker after {start}: {end}"))?;
    tail.get(..end_offset).ok_or_else(|| format!("invalid structural end offset for: {end}").into())
}

fn require_contains(source: &str, needle: &str, message: &str) -> TestResult<()> {
    if !source.contains(needle) {
        return Err(message.to_string().into());
    }
    Ok(())
}

fn require_absent(source: &str, needle: &str, message: &str) -> TestResult<()> {
    if source.contains(needle) {
        return Err(message.to_string().into());
    }
    Ok(())
}

/// Content-only digest calls are forbidden in the pull-identity surfaces.
///
/// If a future change genuinely needs a new digest authority there, it must go
/// through `report_identity.rs` and the domain-separated `ContentDigest`
/// authority — not an inline `md5`/hash helper.
#[test]
fn pull_identity_surfaces_contain_no_content_only_digest_helpers() -> TestResult<()> {
    let guarded = [
        "features/diagnostics/pull.rs",
        "features/diagnostics/report_identity.rs",
        "runtime/diagnostics.rs",
    ];

    for relative in guarded {
        let source = read_source(relative)?;
        for line in source.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            assert!(
                !trimmed.contains("md5::compute("),
                "{relative} must not mint pull result identity from content-only digests \
                 (#7480); found: {trimmed}"
            );
        }
    }

    Ok(())
}

/// `report_identity.rs` must compose through the repository's domain-separated
/// SHA-256 content-digest authority, never a hand-rolled hash or FNV mixer.
#[test]
fn pull_report_identity_composes_through_the_digest_authority() -> TestResult<()> {
    let source = read_source("features/diagnostics/report_identity.rs")?;

    assert!(
        source.contains("ContentDigest::of_bytes"),
        "result identity must fold through ContentDigest (#7480)"
    );

    for forbidden in ["DefaultHasher", "Hasher::new", "fnv", "md5::"] {
        assert!(
            !source.contains(forbidden),
            "free-form hashing ({forbidden}) must never become identity authority"
        );
    }

    Ok(())
}

/// Critic evaluation may only stage a contribution. Overlap-carrier mutation
/// and native-row projection belong to the report-boundary finalizer, after
/// the accepted snapshot is revalidated (#13304).
#[test]
fn pull_critic_rows_commit_only_in_report_finalizer() -> TestResult<()> {
    let source = read_source("features/diagnostics/pull.rs")?;
    let evaluation =
        source_between(&source, "fn evaluate_policy_critic(", "fn finalize_pending_diagnostics(")?;
    let finalizer = source_between(
        &source,
        "fn finalize_pending_diagnostics(",
        "fn parse_error_to_diagnostic(",
    )?;

    require_absent(
        evaluation,
        "take_critic_overlap_observations",
        "critic evaluation must not drain core overlap carriers before report currentness",
    )?;
    require_absent(
        evaluation,
        "project_findings_with_source",
        "critic evaluation must not append native rows before report currentness",
    )?;
    require_contains(
        finalizer,
        "take_critic_overlap_observations",
        "the report-boundary finalizer must own overlap-carrier mutation",
    )?;
    require_contains(
        finalizer,
        "normalized_finding_to_lsp_diagnostic",
        "the report-boundary finalizer must own native-row projection",
    )
}

/// A matching previous ID is only reusable after a final accepted-snapshot
/// currentness read immediately guarding the `Unchanged` branch (#13304).
#[test]
fn unchanged_fast_paths_revalidate_the_final_snapshot() -> TestResult<()> {
    let source = read_source("features/diagnostics/pull.rs")?;
    let document = source_between(
        &source,
        "pub fn get_document_diagnostics_with_context(",
        "pub fn get_workspace_diagnostics(",
    )?;
    let workspace = source_between(
        &source,
        "pub fn get_workspace_diagnostics_with_context(",
        "pub fn get_workspace_diagnostics_partial_with_context(",
    )?;

    require_contains(
        document,
        "if let Some(prior) = unchanged_prior\n            && context.accepted_state_currentness.holds()",
        "document Unchanged must be guarded by final accepted-snapshot currentness",
    )?;
    require_contains(
        workspace,
        ".filter(|_| document_context.accepted_state_currentness.holds())",
        "workspace Unchanged must be guarded by final accepted-snapshot currentness",
    )
}

/// Live workspace diagnostics must capture one context per document and use
/// its sealed accepted snapshot for both evaluation and result identity. Raw
/// selector observations must never be recaptured as identity authority
/// (#13304).
#[test]
fn workspace_identity_uses_the_evaluation_snapshot() -> TestResult<()> {
    let source = read_source("runtime/diagnostics.rs")?;
    let workspace = source_between(
        &source,
        "pub(super) fn handle_workspace_diagnostic(",
        "fn capture_accepted_critic(",
    )?;
    let transaction = source_between(
        &source,
        "fn begin_workspace_critic_transaction(",
        "fn finalize_workspace_critic_transaction(",
    )?;

    require_contains(
        workspace,
        "let identity_context =\n                    PullDiagnosticsOrchestrator::new().build_context(self, uri_str);",
        "workspace diagnostics must capture one pull context per document",
    )?;
    require_contains(
        transaction,
        "identity_context.accepted_critic_snapshot.clone()",
        "workspace transaction must evaluate from its owned context's sealed snapshot",
    )?;
    require_contains(
        transaction,
        "compose_report_identity(",
        "workspace transaction must compose its candidate identity from the same owned context",
    )?;
    require_contains(
        workspace,
        "self.begin_workspace_critic_transaction(",
        "workspace handler must begin one sealed per-document transaction",
    )?;
    require_contains(
        workspace,
        "self.finalize_workspace_critic_transaction(",
        "workspace handler must finalize through the sealed transaction",
    )?;
    require_absent(
        workspace,
        "identity_perlcritic_",
        "workspace result identity must not hoist raw Critic selectors",
    )?;
    require_absent(
        workspace,
        "accepted_critic_snapshot = AcceptedCriticSnapshot::capture",
        "workspace result identity must not freshly recapture Critic configuration",
    )?;
    require_contains(
        workspace,
        "critic_identity.matches_previous(prior)",
        "live workspace Unchanged must revalidate the final accepted snapshot",
    )
}
