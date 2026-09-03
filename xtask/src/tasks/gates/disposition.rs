//! `gate_disposition.v1` — typed gate lifecycle disposition authority (#10176).
//!
//! One read-only resolver that reconciles the repository's *existing* quarantine
//! representations by gate identity and returns exactly one typed lifecycle
//! result for every governed gate in `.ci/gate-policy.yaml`:
//!
//! - the per-gate `quarantine: bool` compatibility bit in the gate policy;
//! - the quarantined-test rows in `.ci/debt-ledger.yaml` (declared by the gate
//!   policy's `flake_policy.debt_ledger_path` as the source of truth for
//!   quarantined items);
//! - the `flake_policy.quarantined_gates` projection (empty by design; a
//!   non-empty entry that disagrees with the ledger is a conflicting source).
//!
//! No second registry is introduced: the resolver only reads the canonical
//! sources above. Dormant-gate inventory, bounded proof runs, and activation
//! batches remain #6261's separate programme.
//!
//! ## Independent axes
//!
//! These facts are kept separate and are never inferred from each other:
//!
//! ```text
//! policy role        required | advisory (mechanically derived from the gate
//!                    row's `required` bit — the same policy authority)
//! lifecycle          active | dormant | quarantined | retired | blocked
//! resolution         current | expired | invalid
//! selector           applicable | not-applicable (#9149's evidence, never an
//!                    input here)
//! ```
//!
//! `not_applicable` is *not* a lifecycle disposition: it stays selector
//! evidence owned by #9149. A lifecycle disposition is equally not a planned
//! outcome or an execution result; the planner seams at the bottom of this
//! module combine lifecycle with selector evidence into outcomes without ever
//! mutating either input.
//!
//! ## Fail-closed rules
//!
//! A quarantined row that lacks an accountable owner (or owner issue), a
//! reason token, or a review horizon resolves to `Invalid`. An expired
//! quarantine resolves to `Expired`. Both are explicit action-required
//! non-success states: they never silently revert to active, become a no-op,
//! or disappear from the governed denominator (#10178 keeps every
//! policy-defined gate in the denominator regardless of lifecycle).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use chrono::{NaiveDate, Utc};
use color_eyre::eyre::{Result, bail, eyre};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{GatePolicy, QuarantinedGate};

/// Authority identity stamped into every explanation.
pub const AUTHORITY_NAME: &str = "gate_disposition.v1";
/// Schema version of this resolver's result contract.
pub const SCHEMA_VERSION: u32 = 1;
/// Gate-policy schema versions this resolver knows how to reconcile.
const SUPPORTED_GATE_POLICY_SCHEMA_VERSIONS: &[u32] = &[1];
/// Debt-ledger schema versions this resolver knows how to reconcile.
const SUPPORTED_LEDGER_SCHEMA_VERSIONS: &[u32] = &[1];
/// Marker recorded when a ledger row carries no name at all.
const UNNAMED_LEDGER_ROW: &str = "(unnamed debt-ledger row)";

/// The only ledger tier that currently carries a reviewed quarantine
/// disposition contract. `disabled` (completely skipped) has no reviewed
/// mapping into the lifecycle vocabulary and fails closed.
const LEDGER_TIER_QUARANTINE: &str = "quarantine";

// ---------------------------------------------------------------------------
// Typed result vocabulary
// ---------------------------------------------------------------------------

/// A gate's lifecycle disposition, independent of policy role and selector
/// applicability.
// `Dormant`, `Retired`, and `Blocked` have no live source representation on
// current main; they are the typed vocabulary #6261's later inventory work
// and #9148's integration contract consume (non-runnable governed rows).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateLifecycle {
    /// Ordinary governed row; mechanically derived active default.
    Active,
    /// Governed but deliberately non-runnable pending activation (#6261).
    Dormant,
    /// Applies to subjects, but a current reviewed policy disposition prevents
    /// execution.
    Quarantined,
    /// Withdrawn; must not remain runnable or be referenced as active.
    Retired,
    /// Blocked pending repair; non-runnable.
    Blocked,
}

impl GateLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            GateLifecycle::Active => "active",
            GateLifecycle::Dormant => "dormant",
            GateLifecycle::Quarantined => "quarantined",
            GateLifecycle::Retired => "retired",
            GateLifecycle::Blocked => "blocked",
        }
    }
}

impl std::fmt::Display for GateLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether a lifecycle row's evidence is currently authoritative.
///
/// `Expired` and `Invalid` are action-required non-success states. They must
/// never be weakened into `Current`, active, a no-op, or product pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispositionResolution {
    Current,
    Expired,
    Invalid,
}

impl DispositionResolution {
    pub fn as_str(self) -> &'static str {
        match self {
            DispositionResolution::Current => "current",
            DispositionResolution::Expired => "expired",
            DispositionResolution::Invalid => "invalid",
        }
    }
}

impl std::fmt::Display for DispositionResolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Repository merge-policy role for the gate row, mechanically derived from
/// the same policy authority (the `required` bit of the gate row). Lifecycle
/// state never moves policy role: a quarantined or retired `required` gate
/// keeps `Required` while being non-runnable.
///
/// The richer #6858 status-context role vocabulary
/// (`required|advisory|informational|local`) belongs to
/// `.ci/policies/required-checks.toml`; inner gate rows only carry the bool,
/// so `Required|Advisory` is the complete honest mechanical projection here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatePolicyRole {
    Required,
    Advisory,
}

impl GatePolicyRole {
    fn from_required(required: bool) -> Self {
        if required { GatePolicyRole::Required } else { GatePolicyRole::Advisory }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            GatePolicyRole::Required => "required",
            GatePolicyRole::Advisory => "advisory",
        }
    }
}

impl std::fmt::Display for GatePolicyRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Structured quarantine evidence a *current* quarantine must carry. Rows
/// whose evidence is incomplete keep lifecycle `Quarantined` but resolve to
/// `Invalid`/`Expired` with a closed error detail instead of silently losing
/// the claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineAuthority {
    /// Accountable owner handle, or the owner issue reference.
    pub owner: String,
    /// Owner tracking issue, when the owner is named by issue rather than
    /// handle.
    pub owner_issue: Option<String>,
    /// Short mechanical reason token (failure pattern when present, otherwise
    /// the ledger notes).
    pub reason_token: String,
    /// Review horizon: the quarantine must be renewed or dispositioned by
    /// this date.
    pub review_after: NaiveDate,
    /// When the quarantine was recorded, when the evidence carries it.
    pub quarantined_at: Option<NaiveDate>,
    /// Failure signature from the ledger, when present.
    pub failure_pattern: Option<String>,
}

/// The source representation a row was reconciled from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispositionSource {
    /// Active default mechanically derived from the gate-policy row (no
    /// contrary evidence in any canonical source).
    GatePolicyDefault,
    /// Quarantine bit corroborated by a debt-ledger quarantine row.
    GatePolicyBitAndLedger,
    /// Non-success row derived from missing, conflicting, or insufficient
    /// evidence.
    FailedReconciliation,
}

impl DispositionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            DispositionSource::GatePolicyDefault => "gate-policy-default",
            DispositionSource::GatePolicyBitAndLedger => "gate-policy+debt-ledger",
            DispositionSource::FailedReconciliation => "failed-reconciliation",
        }
    }
}

/// One closed typed lifecycle result for one governed gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDisposition {
    pub gate_id: String,
    pub policy_role: GatePolicyRole,
    pub lifecycle: GateLifecycle,
    /// Intended execution profile: the gate row's native policy tier.
    pub intended_profile: String,
    /// Present exactly when the lifecycle claim is `Quarantined` and its
    /// evidence is complete.
    pub quarantine: Option<QuarantineAuthority>,
    pub resolution: DispositionResolution,
    /// Closed error detail explaining why a row is not current.
    pub detail: Option<String>,
    pub source: DispositionSource,
}

/// The resolved authority: one row per governed gate, plus fail-closed
/// bookkeeping for quarantine-source rows that cannot be attributed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispositionAuthority {
    pub schema_version: u32,
    /// Sorted by `gate_id`; exactly one row per canonical policy gate.
    pub rows: Vec<GateDisposition>,
    /// Quarantine-source rows that fail closed because they cannot be
    /// attributed at all: nameless debt-ledger rows, and
    /// `quarantined_gates` projection entries naming gates the policy does
    /// not define (that projection is gate-scoped, so an unknown name there
    /// is an unknown-gate failure, not test-level debt).
    pub unknown_ledger_entries: Vec<String>,
    /// Debt-ledger quarantine rows naming something that is not a governed
    /// gate. The ledger's documented format mixes gate-level rows with
    /// ordinary quarantined *tests* (e.g. `lsp::test_completion_timeout`);
    /// such rows assert no gate lifecycle, so they are recorded here as
    /// informational test-level debt and do not invalidate the authority.
    pub test_level_ledger_rows: Vec<String>,
    /// Canonical source identities this authority was reconciled from
    /// (gate-policy path and the declared debt-ledger path). Empty for the
    /// pure `resolve_from` projection, which receives inputs by value.
    pub source_paths: Vec<String>,
    /// Semantic digest over the canonical row content: source-order
    /// independent, movement sensitive.
    pub semantic_digest: String,
}

impl DispositionAuthority {
    /// The typed lookup consumers (#9148 route planning, #9156/#9159
    /// execution and fan-in) use instead of reparsing policy comments,
    /// quarantine booleans, or selected/skipped lists.
    #[allow(dead_code)] // consumer seam: #9148 / PR #10147, #9156, #9159
    pub fn get(&self, gate_id: &str) -> Option<&GateDisposition> {
        self.rows.iter().find(|row| row.gate_id == gate_id)
    }

    /// True only when every governed row is `Current` and no quarantine
    /// source row is unattributable. Ordinary test-level ledger debt does
    /// not invalidate the gate authority. Expired or invalid evidence is
    /// action-required non-success — never silently success.
    #[allow(dead_code)] // consumer seam: #10178 denominator validation, #9156
    pub fn is_current(&self) -> bool {
        self.unknown_ledger_entries.is_empty()
            && self.rows.iter().all(|row| row.resolution == DispositionResolution::Current)
    }

    /// Human-readable status/explain output identifying the exact authority
    /// and why each row is current, expired, or invalid.
    pub fn format_explanation(&self) -> String {
        let mut lines = vec![format!("{AUTHORITY_NAME} schema={}", self.schema_version)];
        if !self.source_paths.is_empty() {
            lines.push(format!("sources: {}", self.source_paths.join(", ")));
        }
        if !self.unknown_ledger_entries.is_empty() {
            lines.push(format!(
                "unknown ledger entries (invalid): {}",
                self.unknown_ledger_entries.join(", ")
            ));
        }
        if !self.test_level_ledger_rows.is_empty() {
            lines.push(format!(
                "test-level ledger rows (no gate lifecycle claim): {}",
                self.test_level_ledger_rows.join(", ")
            ));
        }
        for row in &self.rows {
            let mut line = format!(
                "{} lifecycle={} resolution={} role={} profile={} source={}",
                row.gate_id,
                row.lifecycle,
                row.resolution,
                row.policy_role,
                row.intended_profile,
                row.source.as_str(),
            );
            if let Some(authority) = &row.quarantine {
                line.push_str(&format!(
                    " owner={} reason={} review_after={}",
                    authority.owner,
                    authority.reason_token,
                    authority.review_after.format("%Y-%m-%d")
                ));
            }
            if let Some(detail) = &row.detail {
                line.push_str(&format!(" detail={detail}"));
            }
            line.push_str(&format!(
                " skip_population={}",
                match skip_population_admission(row) {
                    SkipPopulationAdmission::Admissible => "admissible".to_string(),
                    SkipPopulationAdmission::Refused { lifecycle } => {
                        format!("refused({lifecycle})")
                    }
                }
            ));
            lines.push(line);
        }
        lines.push(format!("semantic_digest={}", self.semantic_digest));
        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// Canonical inputs
// ---------------------------------------------------------------------------

/// Quarantine-relevant projection of `.ci/debt-ledger.yaml`. The ledger file
/// stays the single canonical source; this is a richer typed view of it than
/// the debt-report projection (which drops owner/reason/tier).
#[derive(Debug, Clone, Deserialize)]
pub struct QuarantineLedger {
    #[serde(default = "default_ledger_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub flaky_tests: Vec<LedgerQuarantineEntry>,
}

fn default_ledger_schema_version() -> u32 {
    1
}

/// One `flaky_tests` row. Every field is optional at the deserialization
/// layer so that malformed evidence fails closed in the resolver (typed
/// `Invalid`) instead of aborting the parse.
#[derive(Debug, Clone, Deserialize)]
pub struct LedgerQuarantineEntry {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub added: Option<String>,
    #[serde(default)]
    pub issue: Option<String>,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub quarantine_days: Option<i64>,
    #[serde(default)]
    pub expires: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub failure_pattern: Option<String>,
}

/// Load the debt-ledger quarantine projection from the canonical file.
pub fn load_quarantine_ledger(path: &Path) -> Result<QuarantineLedger> {
    let content = fs::read_to_string(path)
        .map_err(|err| eyre!("failed to read debt ledger {}: {err}", path.display()))?;
    serde_yaml_ng::from_str(&content)
        .map_err(|err| eyre!("failed to parse debt ledger {}: {err}", path.display()))
}

/// Resolve the typed disposition authority from the checked-in canonical
/// sources under `root`.
// Canonical entry point for consumers without a policy-path override
// (#10178 denominator validation, #9156 execution, #9159 fan-in); the gates
// CLI itself uses `resolve_with_policy_path` to honor `--gate-policy`.
#[allow(dead_code)]
pub fn resolve(root: &Path) -> Result<DispositionAuthority> {
    resolve_with_policy_path(root, &root.join(".ci/gate-policy.yaml"))
}

/// Like [`resolve`], but reading the gate policy from an explicit path (the
/// runner's `--gate-policy` override) while resolving the declared
/// debt-ledger path against `root`.
pub fn resolve_with_policy_path(root: &Path, policy_path: &Path) -> Result<DispositionAuthority> {
    let policy = super::load_policy_for_inspection(policy_path)?;

    let ledger =
        match policy.flake_policy.as_ref().and_then(|flake| flake.debt_ledger_path.as_deref()) {
            Some(declared) => {
                Some((declared.to_string(), load_quarantine_ledger(&root.join(declared))?))
            }
            // No declared ledger authority: quarantine bits cannot be evidenced,
            // which `resolve_from` reports as typed `Invalid` rows rather than
            // silently dropping the claims.
            None => None,
        };

    check_schema_versions(&policy, ledger.as_ref().map(|(_, ledger)| ledger))?;

    let mut source_paths = vec![policy_path.display().to_string()];
    let ledger = ledger.map(|(declared, ledger)| {
        source_paths.push(declared);
        ledger
    });

    let mut authority = resolve_from(&policy, ledger.as_ref(), Utc::now().date_naive());
    authority.source_paths = source_paths;
    Ok(authority)
}

/// Unsupported source schema versions are refused before interpretation:
/// they are never read under the current contract by accident.
fn check_schema_versions(policy: &GatePolicy, ledger: Option<&QuarantineLedger>) -> Result<()> {
    if !SUPPORTED_GATE_POLICY_SCHEMA_VERSIONS.contains(&policy.schema_version) {
        bail!(
            "{AUTHORITY_NAME}: unsupported gate-policy schema_version {} (supported: \
             {SUPPORTED_GATE_POLICY_SCHEMA_VERSIONS:?})",
            policy.schema_version
        );
    }
    if let Some(ledger) = ledger
        && !SUPPORTED_LEDGER_SCHEMA_VERSIONS.contains(&ledger.schema_version)
    {
        bail!(
            "{AUTHORITY_NAME}: unsupported debt-ledger schema_version {} (supported: \
             {SUPPORTED_LEDGER_SCHEMA_VERSIONS:?})",
            ledger.schema_version
        );
    }
    Ok(())
}

/// Pure reconciliation over the canonical inputs; `today` is injectable so
/// expiry is deterministically testable.
pub fn resolve_from(
    policy: &GatePolicy,
    ledger: Option<&QuarantineLedger>,
    today: NaiveDate,
) -> DispositionAuthority {
    let duplicate_gate_ids = duplicate_names(policy);
    let ledger_by_name = index_ledger(ledger);
    let projection_by_name = index_flake_projection(policy);

    let mut unknown_ledger_entries: BTreeSet<String> = BTreeSet::new();
    let mut test_level_ledger_rows: BTreeSet<String> = BTreeSet::new();
    for name in ledger_by_name.keys() {
        if !policy.gates.iter().any(|gate| gate.name == *name) {
            // The ledger's documented format mixes gate-level quarantine rows
            // with ordinary quarantined *tests* (e.g. `lsp::test_...`). A row
            // naming something that is not a governed gate asserts no gate
            // lifecycle, so it is test-level debt, not an unknown-gate
            // failure: recording it as invalid would let routine test debt
            // invalidate the whole gate authority.
            test_level_ledger_rows.insert(name.clone());
        }
    }
    if let Some(ledger) = ledger {
        for entry in &ledger.flaky_tests {
            if entry.name.as_deref().map(str::trim).filter(|name| !name.is_empty()).is_none() {
                unknown_ledger_entries.insert(UNNAMED_LEDGER_ROW.to_string());
            }
        }
    }
    // The `quarantined_gates` projection is gate-scoped by name and schema:
    // an entry naming something the policy does not define is an unknown-gate
    // failure (control 11), never test-level debt.
    for projected in projection_by_name.values().flatten() {
        if !policy.gates.iter().any(|gate| gate.name == *projected.gate) {
            unknown_ledger_entries.insert(format!(
                "quarantined_gates projection names unknown gate {:?}",
                projected.gate
            ));
        }
    }

    let mut rows: Vec<GateDisposition> = policy
        .gates
        .iter()
        .map(|gate| {
            reconcile_gate(GateInputs {
                gate_id: gate.name.as_str(),
                policy_role: GatePolicyRole::from_required(gate.required),
                native_tier: gate.tier.as_str(),
                quarantine_bit: gate.quarantine,
                ledger_rows: ledger_by_name.get(gate.name.as_str()).map(Vec::as_slice),
                projection: projection_by_name.get(gate.name.as_str()).map(Vec::as_slice),
                duplicate_in_policy: duplicate_gate_ids.contains(gate.name.as_str()),
                today,
            })
        })
        .collect();
    rows.sort_by(|left, right| left.gate_id.cmp(&right.gate_id));

    let semantic_digest = semantic_digest(&rows, &unknown_ledger_entries);

    DispositionAuthority {
        schema_version: SCHEMA_VERSION,
        rows,
        unknown_ledger_entries: unknown_ledger_entries.into_iter().collect(),
        test_level_ledger_rows: test_level_ledger_rows.into_iter().collect(),
        source_paths: Vec::new(),
        semantic_digest,
    }
}

struct GateInputs<'a> {
    gate_id: &'a str,
    policy_role: GatePolicyRole,
    native_tier: &'a str,
    quarantine_bit: bool,
    ledger_rows: Option<&'a [&'a LedgerQuarantineEntry]>,
    projection: Option<&'a [&'a QuarantinedGate]>,
    duplicate_in_policy: bool,
    today: NaiveDate,
}

fn duplicate_names(policy: &GatePolicy) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    for gate in &policy.gates {
        if !seen.insert(gate.name.clone()) {
            duplicates.insert(gate.name.clone());
        }
    }
    duplicates
}

fn index_ledger(
    ledger: Option<&QuarantineLedger>,
) -> BTreeMap<String, Vec<&LedgerQuarantineEntry>> {
    let mut index: BTreeMap<String, Vec<&LedgerQuarantineEntry>> = BTreeMap::new();
    if let Some(ledger) = ledger {
        for entry in &ledger.flaky_tests {
            if let Some(name) = entry.name.as_deref() {
                index.entry(name.to_string()).or_default().push(entry);
            }
        }
    }
    index
}

fn index_flake_projection(policy: &GatePolicy) -> BTreeMap<String, Vec<&QuarantinedGate>> {
    let mut index: BTreeMap<String, Vec<&QuarantinedGate>> = BTreeMap::new();
    if let Some(flake) = policy.flake_policy.as_ref() {
        for projected in &flake.quarantined_gates {
            index.entry(projected.gate.clone()).or_default().push(projected);
        }
    }
    index
}

/// Reconcile one gate row against every canonical quarantine representation.
fn reconcile_gate(inputs: GateInputs<'_>) -> GateDisposition {
    let GateInputs {
        gate_id,
        policy_role,
        native_tier,
        quarantine_bit,
        ledger_rows,
        projection,
        duplicate_in_policy,
        today,
    } = inputs;

    let ledger_rows = ledger_rows.unwrap_or_default();
    let projection = projection.unwrap_or_default();
    let mut failures: Vec<String> = Vec::new();
    if duplicate_in_policy {
        failures.push("duplicate gate rows in gate policy claim conflicting dispositions".into());
    }
    if projection.len() > 1 {
        failures.push(
            "duplicate quarantined_gates projection entries claim competing dispositions".into(),
        );
    }

    // A quarantine claim exists when any canonical source asserts one. The
    // claim keeps lifecycle `Quarantined` even when its evidence is missing,
    // expired, or contradictory — it never silently becomes active.
    let quarantine_claim = quarantine_bit || !ledger_rows.is_empty() || !projection.is_empty();

    match ledger_rows.len() {
        0 => {
            if quarantine_bit {
                failures.push(
                    "quarantine bit has no debt-ledger evidence: no owner, reason, or review \
                     horizon"
                        .into(),
                );
            } else if !projection.is_empty() {
                failures.push(
                    "flake_policy.quarantined_gates projection names this gate without a \
                     debt-ledger row (conflicting sources)"
                        .into(),
                );
            }
        }
        1 => {
            let entry = ledger_rows[0];
            if !quarantine_bit {
                failures.push(
                    "debt-ledger quarantine row conflicts with gate-policy quarantine: false"
                        .into(),
                );
            }
            let tier = entry.tier.as_deref().unwrap_or_default();
            if tier != LEDGER_TIER_QUARANTINE {
                failures.push(format!(
                    "unsupported debt-ledger quarantine tier {tier:?}; expected \
                     {LEDGER_TIER_QUARANTINE:?}"
                ));
            }

            // Evidence axes (controls 1-3): owner, reason token, review
            // horizon. Each missing axis is an independent failure so the
            // closed detail names exactly what must be repaired.
            let owner_issue = non_empty(entry.issue.as_deref());
            let owner = non_empty(entry.owner.as_deref())
                .map(str::to_string)
                .or_else(|| owner_issue.map(|issue| format!("issue {issue}")));
            let reason_token = non_empty(entry.failure_pattern.as_deref())
                .or_else(|| non_empty(entry.notes.as_deref()))
                .map(str::to_string);
            let review_horizon = review_horizon(entry);

            if owner.is_none() {
                failures.push("quarantine lacks an accountable owner or owner issue".into());
            }
            if reason_token.is_none() {
                failures.push("quarantine lacks a reason token".into());
            }
            if review_horizon.is_none() {
                failures.push("quarantine lacks a review horizon (expires or added + days)".into());
            }

            let authority = match (owner, reason_token, review_horizon) {
                (Some(owner), Some(reason_token), Some(review_after)) => {
                    Some(QuarantineAuthority {
                        owner,
                        owner_issue: owner_issue.map(str::to_string),
                        reason_token,
                        review_after,
                        quarantined_at: non_empty(entry.added.as_deref())
                            .and_then(|added| added.parse::<NaiveDate>().ok()),
                        failure_pattern: non_empty(entry.failure_pattern.as_deref())
                            .map(str::to_string),
                    })
                }
                _ => None,
            };

            // The `quarantined_gates` projection is declared a projection of
            // the same ledger evidence: when it is populated, its carried
            // fields must agree with the corroborating row. A disagreement
            // is a conflicting source even when the name matches.
            if let (Some(authority), Some(projected)) = (authority.as_ref(), projection.first()) {
                if let Some(projected_issue) =
                    projected.issue.as_deref().map(str::trim).filter(|issue| !issue.is_empty())
                {
                    let ledger_issue = authority
                        .owner_issue
                        .as_deref()
                        .map(str::trim)
                        .filter(|issue| !issue.is_empty());
                    if ledger_issue.is_some_and(|ledger_issue| ledger_issue != projected_issue) {
                        failures.push(format!(
                            "quarantined_gates projection issue {projected_issue:?} disagrees \
                             with debt-ledger issue {:?}",
                            authority.owner_issue
                        ));
                    }
                }
                let projected_at = projected.quarantined_at.trim().parse::<NaiveDate>().ok();
                if let (Some(projected_at), Some(ledger_at)) =
                    (projected_at, authority.quarantined_at)
                    && projected_at != ledger_at
                {
                    failures.push(format!(
                        "quarantined_gates projection quarantined_at {} disagrees with \
                         debt-ledger added {}",
                        projected_at.format("%Y-%m-%d"),
                        ledger_at.format("%Y-%m-%d")
                    ));
                }
                if authority.reason_token != projected.reason.trim() {
                    failures.push(format!(
                        "quarantined_gates projection reason {:?} disagrees with debt-ledger \
                         reason token {:?}",
                        projected.reason, authority.reason_token
                    ));
                }
            }

            if failures.is_empty()
                && let Some(authority) = authority
            {
                if authority.review_after < today {
                    let expired_on = authority.review_after.format("%Y-%m-%d").to_string();
                    return GateDisposition {
                        gate_id: gate_id.to_string(),
                        policy_role,
                        lifecycle: GateLifecycle::Quarantined,
                        intended_profile: native_tier.to_string(),
                        quarantine: Some(authority),
                        resolution: DispositionResolution::Expired,
                        detail: Some(format!(
                            "quarantine expired on {expired_on}; renewal or disposition required"
                        )),
                        source: DispositionSource::FailedReconciliation,
                    };
                }
                return GateDisposition {
                    gate_id: gate_id.to_string(),
                    policy_role,
                    lifecycle: GateLifecycle::Quarantined,
                    intended_profile: native_tier.to_string(),
                    quarantine: Some(authority),
                    resolution: DispositionResolution::Current,
                    detail: None,
                    source: DispositionSource::GatePolicyBitAndLedger,
                };
            }
        }
        _ => {
            failures
                .push("conflicting duplicate debt-ledger rows claim competing dispositions".into());
        }
    }

    // The ordinary governed row: no canonical source claims any non-active
    // lifecycle, so the mechanically derived active default applies.
    if failures.is_empty() && !quarantine_claim {
        return GateDisposition {
            gate_id: gate_id.to_string(),
            policy_role,
            lifecycle: GateLifecycle::Active,
            intended_profile: native_tier.to_string(),
            quarantine: None,
            resolution: DispositionResolution::Current,
            detail: None,
            source: DispositionSource::GatePolicyDefault,
        };
    }

    // Every non-current path lands here with at least one named failure.
    GateDisposition {
        gate_id: gate_id.to_string(),
        policy_role,
        lifecycle: if quarantine_claim {
            GateLifecycle::Quarantined
        } else {
            GateLifecycle::Active
        },
        intended_profile: native_tier.to_string(),
        quarantine: None,
        resolution: DispositionResolution::Invalid,
        detail: Some(if failures.is_empty() {
            "quarantine evidence incomplete".to_string()
        } else {
            failures.join("; ")
        }),
        source: DispositionSource::FailedReconciliation,
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|trimmed| !trimmed.is_empty())
}

/// Review horizon from `expires`, or `added + quarantine_days`, mirroring the
/// debt-report expiry semantics over the same canonical file.
fn review_horizon(entry: &LedgerQuarantineEntry) -> Option<NaiveDate> {
    if let Some(expires) = non_empty(entry.expires.as_deref())
        && let Ok(date) = expires.parse::<NaiveDate>()
    {
        return Some(date);
    }
    if let (Some(added), Some(days)) = (
        non_empty(entry.added.as_deref()).and_then(|added| added.parse::<NaiveDate>().ok()),
        entry.quarantine_days,
    ) {
        return added.checked_add_signed(chrono::Duration::days(days));
    }
    None
}

/// Semantic digest over the complete canonical row content: rows are sorted
/// by gate id, so reordering the source cannot move it, while any semantic
/// movement — including closed failure details, source identity, and every
/// quarantine evidence field — does.
fn semantic_digest(rows: &[GateDisposition], unknown_ledger_entries: &BTreeSet<String>) -> String {
    let mut hasher = Sha256::new();
    for row in rows {
        hasher.update(row.gate_id.as_bytes());
        hasher.update([0x1f]);
        hasher.update(row.policy_role.as_str().as_bytes());
        hasher.update([0x1f]);
        hasher.update(row.lifecycle.as_str().as_bytes());
        hasher.update([0x1f]);
        hasher.update(row.resolution.as_str().as_bytes());
        hasher.update([0x1f]);
        hasher.update(row.intended_profile.as_bytes());
        hasher.update([0x1f]);
        hasher.update(row.source.as_str().as_bytes());
        hasher.update([0x1f]);
        hash_option(&mut hasher, row.detail.as_deref());
        hasher.update([0x1f]);
        if let Some(authority) = &row.quarantine {
            hasher.update(authority.owner.as_bytes());
            hasher.update([0x1f]);
            hash_option(&mut hasher, authority.owner_issue.as_deref());
            hasher.update([0x1f]);
            hasher.update(authority.reason_token.as_bytes());
            hasher.update([0x1f]);
            hasher.update(authority.review_after.format("%Y-%m-%d").to_string().as_bytes());
            hasher.update([0x1f]);
            hash_option(
                &mut hasher,
                authority.quarantined_at.map(|date| date.format("%Y-%m-%d").to_string()).as_deref(),
            );
            hasher.update([0x1f]);
            hash_option(&mut hasher, authority.failure_pattern.as_deref());
        } else {
            hasher.update([0x0]);
        }
        hasher.update([0x1e]);
    }
    for unknown in unknown_ledger_entries {
        hasher.update(unknown.as_bytes());
        hasher.update([0x1e]);
    }
    // Established repository hex encoding (per-byte `format!("{byte:02x}")`);
    // `Sha256::digest` does not implement `LowerHex` under the current
    // sha2/generic-array pair.
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Presence-tagged option hashing so `None` and `Some("")` never collide.
fn hash_option(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        Some(text) => {
            hasher.update([0x1]);
            hasher.update(text.as_bytes());
        }
        None => hasher.update([0x0]),
    }
}

// ---------------------------------------------------------------------------
// Planner and execution seams (consumed by #9148/#10147, #9156, #9159)
//
// These seams land with this PR as the typed contract the route-plan (#9148),
// execution (#9156), and fan-in (#9159) lanes consume. On current main they
// have no non-test caller yet; the `#[allow(dead_code)]` annotations cite
// each consumer so the annotations retire when the consumers land.
// ---------------------------------------------------------------------------

/// Exact-subject selector applicability evidence. Owned by #9149; never an
/// input to lifecycle resolution and never inferred from lifecycle state.
#[allow(dead_code)] // consumer seam: #9148 / PR #10147 + #9149 selector compiler
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorEvidence {
    /// The gate applies to the exact subject.
    Applicable,
    /// Positive selector evidence that the gate does not apply to the exact
    /// subject — the only evidence that can justify `ScopedNoop`.
    NotApplicableToSubject,
}

/// The planner-facing outcome for one governed gate. An execution `Skip` is
/// deliberately absent: an execution observation establishes neither
/// proposition and can never be evidence for `ScopedNoop` or `Quarantined`.
#[allow(dead_code)] // consumer seam: #9148 route-plan outcome compilation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedOutcome {
    /// Active current work that applies to the subject.
    Run,
    /// Selector-evidence-backed non-applicability to the exact subject — the
    /// sole justification, and only for a current active row.
    ScopedNoop,
    /// Current quarantine authority retained with owner/reason/review
    /// identity; never executed through ordinary admission and never placed
    /// in a skipped population.
    Quarantined,
    /// Dormant, retired, or blocked governed row: non-runnable, but still in
    /// the denominator.
    NonRunnable,
    /// Missing, expired, contradictory, unsupported, or incomplete authority:
    /// explicit non-success.
    ActionRequired,
}

/// Combine a resolved lifecycle disposition with selector evidence. The
/// disposition is immutable here: selector non-applicability can never mutate
/// lifecycle, and lifecycle can never fabricate selector evidence.
#[allow(dead_code)] // consumer seam: #9148 route-plan outcome compilation
pub fn planned_outcome(
    disposition: &GateDisposition,
    selector: SelectorEvidence,
) -> PlannedOutcome {
    match disposition.resolution {
        DispositionResolution::Expired | DispositionResolution::Invalid => {
            PlannedOutcome::ActionRequired
        }
        DispositionResolution::Current => match disposition.lifecycle {
            GateLifecycle::Active => match selector {
                SelectorEvidence::Applicable => PlannedOutcome::Run,
                SelectorEvidence::NotApplicableToSubject => PlannedOutcome::ScopedNoop,
            },
            GateLifecycle::Quarantined => PlannedOutcome::Quarantined,
            GateLifecycle::Dormant | GateLifecycle::Retired | GateLifecycle::Blocked => {
                PlannedOutcome::NonRunnable
            }
        },
    }
}

/// May this gate be placed in a planner's *skipped* population? Only a
/// current active row may (and even then only with #9149 selector evidence).
/// A quarantined gate — current, expired, or invalid — is always refused:
/// it belongs in the quarantined/action-required population with its
/// authority retained, never erased into a skip list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipPopulationAdmission {
    Admissible,
    Refused { lifecycle: GateLifecycle },
}

pub fn skip_population_admission(disposition: &GateDisposition) -> SkipPopulationAdmission {
    match disposition.resolution {
        DispositionResolution::Current => match disposition.lifecycle {
            GateLifecycle::Active => SkipPopulationAdmission::Admissible,
            other => SkipPopulationAdmission::Refused { lifecycle: other },
        },
        DispositionResolution::Expired | DispositionResolution::Invalid => {
            SkipPopulationAdmission::Refused { lifecycle: disposition.lifecycle }
        }
    }
}

/// An execution observation from a gate result. The runner's `"skip"` status
/// (for example, the legacy quarantine skip in `run_single_gate`) establishes
/// neither selector non-applicability nor quarantine.
#[allow(dead_code)] // consumer seam: #9156 execution-result interpretation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionObservation {
    Skip,
}

#[allow(dead_code)] // consumer seam: #9156 execution-result interpretation
impl ExecutionObservation {
    pub fn establishes_scoped_noop(self) -> bool {
        false
    }

    pub fn establishes_quarantine(self) -> bool {
        false
    }
}

/// Interpret a `GateResult.status` string as an execution observation.
/// Returns `Some(Skip)` only for `"skip"`; pass/fail/error/timeout are
/// execution results, not skip observations.
#[allow(dead_code)] // consumer seam: #9156 execution-result interpretation
pub fn interpret_gate_status(status: &str) -> Option<ExecutionObservation> {
    match status {
        "skip" => Some(ExecutionObservation::Skip),
        _ => None,
    }
}

/// Execution admission under the normalized lifecycle contract. Diagnostic
/// verbosity (`--verbose`) is presentation/control only: it cannot turn an
/// expired or invalid quarantine into runnable current work, and a current
/// valid quarantine is never executed through ordinary admission (bounded
/// proof runs are #6261's separate capability).
#[allow(dead_code)] // consumer seam: #9156 normalized execution admission
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionAdmission {
    Execute,
    QuarantineSkip,
    NonRunnable,
    ActionRequired,
}

#[allow(dead_code)] // consumer seam: #9156 normalized execution admission
pub fn execution_admission(disposition: &GateDisposition, verbose: bool) -> ExecutionAdmission {
    let _ = verbose; // deliberately ignored: see the type documentation
    match disposition.resolution {
        DispositionResolution::Expired | DispositionResolution::Invalid => {
            ExecutionAdmission::ActionRequired
        }
        DispositionResolution::Current => match disposition.lifecycle {
            GateLifecycle::Active => ExecutionAdmission::Execute,
            GateLifecycle::Quarantined => ExecutionAdmission::QuarantineSkip,
            GateLifecycle::Dormant | GateLifecycle::Retired | GateLifecycle::Blocked => {
                ExecutionAdmission::NonRunnable
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod gate_disposition_spec {
    use super::*;
    use crate::tasks::gates::{
        FlakePolicy, GateDefinition, GatePlanningConfig, GatePlanningRole, GatePolicy,
        GlobalSettings, QuarantinedGate,
    };
    use std::collections::HashMap;

    const TODAY: NaiveDate = NaiveDate::from_ymd_opt(2026, 8, 24).unwrap();

    fn gate(name: &str, tier: &str, required: bool, quarantine: bool) -> GateDefinition {
        GateDefinition {
            name: name.to_string(),
            tier: tier.to_string(),
            description: name.to_string(),
            required,
            command: "true".to_string(),
            timeout_seconds: 30,
            retry_count: 0,
            budgets: None,
            quarantine,
            tags: Vec::new(),
            artifacts: Vec::new(),
            matrix: None,
            planning: Some(GatePlanningConfig {
                role: GatePlanningRole::Static,
                packages: Vec::new(),
            }),
            short_circuit: false,
        }
    }

    fn policy_with_flake(gates: Vec<GateDefinition>, flake: Option<FlakePolicy>) -> GatePolicy {
        GatePolicy {
            schema_version: 1,
            global: GlobalSettings {
                default_timeout_seconds: 30,
                artifact_retention_days: 0,
                default_retry_count: 0,
                environment: HashMap::new(),
                toolchain: None,
            },
            tiers: HashMap::new(),
            gates,
            flake_policy: flake,
            audit: None,
        }
    }

    fn policy(gates: Vec<GateDefinition>) -> GatePolicy {
        policy_with_flake(gates, None)
    }

    fn ledger(entries: Vec<LedgerQuarantineEntry>) -> Option<QuarantineLedger> {
        Some(QuarantineLedger { schema_version: 1, flaky_tests: entries })
    }

    fn entry(name: &str) -> LedgerQuarantineEntry {
        LedgerQuarantineEntry {
            name: Some(name.to_string()),
            added: Some("2026-08-01".to_string()),
            issue: Some("#10176".to_string()),
            tier: Some("quarantine".to_string()),
            quarantine_days: Some(30),
            expires: Some("2026-09-30".to_string()),
            owner: Some("maintainer".to_string()),
            notes: Some("flaky under load".to_string()),
            failure_pattern: Some("timeout waiting for completion".to_string()),
        }
    }

    fn flake_with_projection(gate: &str) -> FlakePolicy {
        FlakePolicy {
            max_retries: 2,
            auto_quarantine_threshold: 3,
            quarantine_duration_days: 7,
            debt_ledger_path: Some(".ci/debt-ledger.yaml".to_string()),
            quarantined_gates: vec![QuarantinedGate {
                gate: gate.to_string(),
                reason: "projected".to_string(),
                quarantined_at: "2026-08-01".to_string(),
                issue: Some("#10176".to_string()),
            }],
            known_flaky_patterns: Vec::new(),
        }
    }

    // -------------------------------------------------------------------
    // Canonical default and complete quarantine
    // -------------------------------------------------------------------

    #[test]
    fn ordinary_gate_resolves_active_current_from_policy_default() {
        let authority =
            resolve_from(&policy(vec![gate("fmt", "pr_fast", true, false)]), None, TODAY);
        let row = authority.get("fmt").unwrap();
        assert_eq!(row.lifecycle, GateLifecycle::Active);
        assert_eq!(row.resolution, DispositionResolution::Current);
        assert_eq!(row.policy_role, GatePolicyRole::Required);
        assert_eq!(row.source, DispositionSource::GatePolicyDefault);
        assert!(authority.is_current());
    }

    #[test]
    fn evidenced_quarantine_resolves_current_with_owner_reason_and_horizon() {
        let authority = resolve_from(
            &policy(vec![gate("clippy_scoped", "pr_fast", false, true)]),
            ledger(vec![entry("clippy_scoped")]).as_ref(),
            TODAY,
        );
        let row = authority.get("clippy_scoped").unwrap();
        assert_eq!(row.lifecycle, GateLifecycle::Quarantined);
        assert_eq!(row.resolution, DispositionResolution::Current);
        let quarantine = row.quarantine.as_ref().unwrap();
        assert_eq!(quarantine.owner, "maintainer");
        assert_eq!(quarantine.reason_token, "timeout waiting for completion");
        assert_eq!(quarantine.review_after, "2026-09-30".parse().unwrap());
        assert_eq!(quarantine.quarantined_at, Some("2026-08-01".parse().unwrap()));
        assert!(authority.is_current());
    }

    #[test]
    fn named_gate_added_to_policy_is_covered_exactly_once() {
        // #9436 recurrence control: adding a named governed gate must yield
        // exactly one mechanically derived lifecycle row — omitting it from
        // resolution would break the coverage invariant, not default outside
        // the canonical authority.
        let gates = vec![
            gate("existing", "merge_gate", true, false),
            gate("named_new_contract", "merge_gate", false, false),
        ];
        let authority = resolve_from(&policy(gates.clone()), None, TODAY);
        assert_eq!(authority.rows.len(), gates.len());
        let unique: BTreeSet<&str> =
            authority.rows.iter().map(|row| row.gate_id.as_str()).collect();
        assert_eq!(unique.len(), gates.len());
        assert!(authority.get("named_new_contract").is_some());
    }

    // -------------------------------------------------------------------
    // Negative controls 1-4: evidence axes and expiry
    // -------------------------------------------------------------------

    #[test]
    fn quarantine_without_owner_fails_closed() {
        let mut no_owner = entry("orphan");
        no_owner.owner = None;
        no_owner.issue = None;
        let authority = resolve_from(
            &policy(vec![gate("orphan", "nightly", false, true)]),
            ledger(vec![no_owner]).as_ref(),
            TODAY,
        );
        let row = authority.get("orphan").unwrap();
        assert_eq!(row.resolution, DispositionResolution::Invalid);
        assert_eq!(row.lifecycle, GateLifecycle::Quarantined);
        assert!(row.detail.as_deref().unwrap().contains("owner"));
        assert!(!authority.is_current());
    }

    #[test]
    fn quarantine_without_reason_fails_closed() {
        let mut no_reason = entry("silent");
        no_reason.failure_pattern = None;
        no_reason.notes = None;
        let authority = resolve_from(
            &policy(vec![gate("silent", "nightly", false, true)]),
            ledger(vec![no_reason]).as_ref(),
            TODAY,
        );
        let row = authority.get("silent").unwrap();
        assert_eq!(row.resolution, DispositionResolution::Invalid);
        assert!(row.detail.as_deref().unwrap().contains("reason"));
    }

    #[test]
    fn quarantine_without_review_horizon_fails_closed() {
        let mut no_horizon = entry("horizonless");
        no_horizon.expires = None;
        no_horizon.quarantine_days = None;
        let authority = resolve_from(
            &policy(vec![gate("horizonless", "nightly", false, true)]),
            ledger(vec![no_horizon]).as_ref(),
            TODAY,
        );
        let row = authority.get("horizonless").unwrap();
        assert_eq!(row.resolution, DispositionResolution::Invalid);
        assert!(row.detail.as_deref().unwrap().contains("review horizon"));
    }

    #[test]
    fn expired_quarantine_is_action_required_non_success() {
        let mut expired = entry("stale");
        expired.expires = Some("2026-05-26".to_string());
        let authority = resolve_from(
            &policy(vec![gate("stale", "merge_gate", false, true)]),
            ledger(vec![expired]).as_ref(),
            TODAY,
        );
        let row = authority.get("stale").unwrap();
        assert_eq!(row.lifecycle, GateLifecycle::Quarantined);
        assert_eq!(row.resolution, DispositionResolution::Expired);
        assert!(row.detail.as_deref().unwrap().contains("2026-05-26"));
        assert!(!authority.is_current());
        assert_eq!(
            planned_outcome(row, SelectorEvidence::Applicable),
            PlannedOutcome::ActionRequired
        );
    }

    // -------------------------------------------------------------------
    // Controls 5, 8: scoped_noop != quarantined != skip
    // -------------------------------------------------------------------

    #[test]
    fn quarantined_applicable_gate_cannot_compile_as_scoped_noop() {
        let authority = resolve_from(
            &policy(vec![gate("qg", "merge_gate", false, true)]),
            ledger(vec![entry("qg")]).as_ref(),
            TODAY,
        );
        let row = authority.get("qg").unwrap();
        assert_eq!(planned_outcome(row, SelectorEvidence::Applicable), PlannedOutcome::Quarantined);
        // Even positive selector non-applicability cannot erase a current
        // quarantine into a scoped no-op.
        assert_eq!(
            planned_outcome(row, SelectorEvidence::NotApplicableToSubject),
            PlannedOutcome::Quarantined
        );
    }

    #[test]
    fn scoped_noop_requires_both_active_lifecycle_and_selector_evidence() {
        let authority =
            resolve_from(&policy(vec![gate("ag", "pr_fast", true, false)]), None, TODAY);
        let row = authority.get("ag").unwrap();
        assert_eq!(
            planned_outcome(row, SelectorEvidence::NotApplicableToSubject),
            PlannedOutcome::ScopedNoop
        );
        assert_eq!(planned_outcome(row, SelectorEvidence::Applicable), PlannedOutcome::Run);
    }

    #[test]
    fn execution_skip_observation_establishes_neither_proposition() {
        let observation = interpret_gate_status("skip").unwrap();
        assert_eq!(observation, ExecutionObservation::Skip);
        assert!(!observation.establishes_scoped_noop());
        assert!(!observation.establishes_quarantine());
        assert!(interpret_gate_status("pass").is_none());
        assert!(interpret_gate_status("fail").is_none());
        assert!(interpret_gate_status("error").is_none());
    }

    #[test]
    fn selector_non_applicability_cannot_mutate_lifecycle_disposition() {
        let authority = resolve_from(
            &policy(vec![gate("frozen", "merge_gate", false, true)]),
            ledger(vec![entry("frozen")]).as_ref(),
            TODAY,
        );
        let row = authority.get("frozen").unwrap();
        let before = (row.lifecycle, row.resolution);
        let _ = planned_outcome(row, SelectorEvidence::NotApplicableToSubject);
        assert_eq!((row.lifecycle, row.resolution), before);
    }

    // -------------------------------------------------------------------
    // Controls 6, 7, 9: role/lifecycle independence, retired non-runnable
    // -------------------------------------------------------------------

    #[test]
    fn required_role_cannot_promote_dormant_or_blocked_to_active() {
        // Lifecycle and policy role are independent axes: a dormant/blocked
        // row with a required role keeps its lifecycle and stays non-runnable.
        let dormant = GateDisposition {
            gate_id: "dormant_req".to_string(),
            policy_role: GatePolicyRole::Required,
            lifecycle: GateLifecycle::Dormant,
            intended_profile: "merge_gate".to_string(),
            quarantine: None,
            resolution: DispositionResolution::Current,
            detail: None,
            source: DispositionSource::GatePolicyDefault,
        };
        assert_eq!(dormant.policy_role, GatePolicyRole::Required);
        assert_eq!(dormant.lifecycle, GateLifecycle::Dormant);
        assert_eq!(
            planned_outcome(&dormant, SelectorEvidence::Applicable),
            PlannedOutcome::NonRunnable
        );
        assert_eq!(
            skip_population_admission(&dormant),
            SkipPopulationAdmission::Refused { lifecycle: GateLifecycle::Dormant }
        );
    }

    #[test]
    fn retired_gate_cannot_remain_runnable() {
        let retired = GateDisposition {
            gate_id: "gone".to_string(),
            policy_role: GatePolicyRole::Advisory,
            lifecycle: GateLifecycle::Retired,
            intended_profile: "pr_fast".to_string(),
            quarantine: None,
            resolution: DispositionResolution::Current,
            detail: None,
            source: DispositionSource::GatePolicyDefault,
        };
        assert_eq!(execution_admission(&retired, false), ExecutionAdmission::NonRunnable);
        assert_eq!(execution_admission(&retired, true), ExecutionAdmission::NonRunnable);
    }

    #[test]
    fn policy_role_is_consumed_not_moved_by_quarantine() {
        // security_audit shape: advisory (required: false) AND quarantined —
        // quarantine neither promotes nor demotes the role.
        let authority = resolve_from(
            &policy(vec![gate("sec", "merge_gate", false, true)]),
            ledger(vec![entry("sec")]).as_ref(),
            TODAY,
        );
        let row = authority.get("sec").unwrap();
        assert_eq!(row.policy_role, GatePolicyRole::Advisory);
        assert_eq!(row.lifecycle, GateLifecycle::Quarantined);
    }

    // -------------------------------------------------------------------
    // Controls 10, 11: conflicting and unknown sources
    // -------------------------------------------------------------------

    #[test]
    fn ledger_row_without_quarantine_bit_conflicts_and_fails_closed() {
        let authority = resolve_from(
            &policy(vec![gate("drift", "pr_fast", true, false)]),
            ledger(vec![entry("drift")]).as_ref(),
            TODAY,
        );
        let row = authority.get("drift").unwrap();
        assert_eq!(row.resolution, DispositionResolution::Invalid);
        assert!(row.detail.as_deref().unwrap().contains("conflicts"));
    }

    #[test]
    fn flake_projection_without_ledger_row_fails_closed() {
        let gates = vec![gate("projected", "pr_fast", true, false)];
        let authority = resolve_from(
            &policy_with_flake(gates, Some(flake_with_projection("projected"))),
            ledger(Vec::new()).as_ref(),
            TODAY,
        );
        let row = authority.get("projected").unwrap();
        assert_eq!(row.resolution, DispositionResolution::Invalid);
        assert!(row.detail.as_deref().unwrap().contains("conflicting"));
    }

    /// A projection that mirrors the corroborating ledger evidence leaves the
    /// row `Current` — the field reconciliation must not false-fire.
    fn mirroring_projection(gate_id: &str) -> FlakePolicy {
        FlakePolicy {
            quarantined_gates: vec![QuarantinedGate {
                gate: gate_id.to_string(),
                reason: "timeout waiting for completion".to_string(),
                quarantined_at: "2026-08-01".to_string(),
                issue: Some("#10176".to_string()),
            }],
            ..flake_with_projection("unused")
        }
    }

    #[test]
    fn mirroring_projection_keeps_the_quarantine_current() {
        let authority = resolve_from(
            &policy_with_flake(
                vec![gate("mirrored", "pr_fast", false, true)],
                Some(mirroring_projection("mirrored")),
            ),
            ledger(vec![entry("mirrored")]).as_ref(),
            TODAY,
        );
        let row = authority.get("mirrored").unwrap();
        assert_eq!(row.resolution, DispositionResolution::Current);
        assert_eq!(row.lifecycle, GateLifecycle::Quarantined);
    }

    #[test]
    fn projection_field_disagreements_with_ledger_evidence_fail_closed() {
        // Review finding: reducing `quarantined_gates` entries to their name
        // let a projection contradict the corroborating ledger evidence in
        // issue, quarantine date, and reason while still resolving Current.
        for mutated in [
            (
                "issue",
                QuarantinedGate {
                    issue: Some("#99999".to_string()),
                    ..mirroring_projection("drift").quarantined_gates.remove(0)
                },
            ),
            (
                "quarantined_at",
                QuarantinedGate {
                    quarantined_at: "2025-01-01".to_string(),
                    ..mirroring_projection("drift").quarantined_gates.remove(0)
                },
            ),
            (
                "reason",
                QuarantinedGate {
                    reason: "a different story".to_string(),
                    ..mirroring_projection("drift").quarantined_gates.remove(0)
                },
            ),
        ] {
            let flake = FlakePolicy {
                quarantined_gates: vec![mutated.1],
                ..mirroring_projection("unused")
            };
            let authority = resolve_from(
                &policy_with_flake(vec![gate("drift", "pr_fast", false, true)], Some(flake)),
                ledger(vec![entry("drift")]).as_ref(),
                TODAY,
            );
            let row = authority.get("drift").unwrap();
            assert_eq!(
                row.resolution,
                DispositionResolution::Invalid,
                "projection {} disagreement must fail closed",
                mutated.0
            );
            assert!(row.detail.as_deref().unwrap().contains("disagrees"));
        }
    }

    #[test]
    fn duplicate_projection_entries_fail_closed() {
        let flake = FlakePolicy {
            quarantined_gates: vec![
                mirroring_projection("twins").quarantined_gates.remove(0),
                mirroring_projection("twins").quarantined_gates.remove(0),
            ],
            ..mirroring_projection("unused")
        };
        let authority = resolve_from(
            &policy_with_flake(vec![gate("twins", "pr_fast", false, true)], Some(flake)),
            ledger(vec![entry("twins")]).as_ref(),
            TODAY,
        );
        let row = authority.get("twins").unwrap();
        assert_eq!(row.resolution, DispositionResolution::Invalid);
        assert!(row.detail.as_deref().unwrap().contains("duplicate"));
    }

    #[test]
    fn duplicate_policy_rows_and_duplicate_ledger_rows_fail_closed() {
        let duplicated =
            vec![gate("twin", "pr_fast", true, false), gate("twin", "pr_fast", true, false)];
        let authority = resolve_from(&policy(duplicated), ledger(Vec::new()).as_ref(), TODAY);
        assert!(authority.rows.iter().all(|row| row.resolution == DispositionResolution::Invalid));

        let authority = resolve_from(
            &policy(vec![gate("twin_ledger", "pr_fast", true, true)]),
            ledger(vec![entry("twin_ledger"), entry("twin_ledger")]).as_ref(),
            TODAY,
        );
        let row = authority.get("twin_ledger").unwrap();
        assert_eq!(row.resolution, DispositionResolution::Invalid);
        assert!(row.detail.as_deref().unwrap().contains("duplicate"));
    }

    #[test]
    fn ordinary_quarantined_tests_are_test_level_debt_not_unknown_gates() {
        // The debt ledger's documented format mixes gate-level quarantine
        // rows with ordinary quarantined *tests* such as
        // `lsp::test_completion_timeout`. A test row asserts no gate
        // lifecycle: it must be recorded as test-level debt without
        // invalidating the gate authority (review finding: the previous
        // unknown-gate treatment made routine test debt fail `is_current`).
        let authority = resolve_from(
            &policy(vec![gate("known", "pr_fast", true, false)]),
            ledger(vec![entry("lsp::test_completion_timeout")]).as_ref(),
            TODAY,
        );
        assert_eq!(
            authority.test_level_ledger_rows,
            vec!["lsp::test_completion_timeout".to_string()]
        );
        assert!(authority.unknown_ledger_entries.is_empty());
        assert!(authority.is_current());
        assert_eq!(authority.get("known").unwrap().resolution, DispositionResolution::Current);
        assert!(authority.format_explanation().contains("test-level ledger rows"));
    }

    #[test]
    fn unknown_projection_gate_and_unsupported_schema_fail_closed() {
        // The `quarantined_gates` projection is gate-scoped: naming a gate
        // the policy does not define is an unknown-gate failure (control 11).
        let projected_at_unknown = FlakePolicy {
            quarantined_gates: vec![QuarantinedGate {
                gate: "ghost".to_string(),
                reason: "stale projection".to_string(),
                quarantined_at: "2026-08-01".to_string(),
                issue: None,
            }],
            ..flake_with_projection("unused")
        };
        let authority = resolve_from(
            &policy_with_flake(
                vec![gate("known", "pr_fast", true, false)],
                Some(projected_at_unknown),
            ),
            ledger(Vec::new()).as_ref(),
            TODAY,
        );
        assert_eq!(authority.unknown_ledger_entries.len(), 1);
        assert!(authority.unknown_ledger_entries[0].contains("ghost"));
        assert!(!authority.is_current());

        // Unsupported source schema versions are refused before reading.
        let mut unsupported_policy = policy(vec![gate("x", "pr_fast", true, false)]);
        unsupported_policy.schema_version = 2;
        let err = check_schema_versions(&unsupported_policy, None).unwrap_err();
        assert!(err.to_string().contains("unsupported gate-policy schema_version"));

        let mut unsupported_ledger = ledger(Vec::new());
        unsupported_ledger.as_mut().unwrap().schema_version = 7;
        let err =
            check_schema_versions(&policy(Vec::new()), unsupported_ledger.as_ref()).unwrap_err();
        assert!(err.to_string().contains("unsupported debt-ledger schema_version"));
    }

    #[test]
    fn unnamed_ledger_row_fails_closed() {
        let mut unnamed = entry("named");
        unnamed.name = None;
        let authority = resolve_from(
            &policy(vec![gate("named", "pr_fast", true, false)]),
            ledger(vec![unnamed]).as_ref(),
            TODAY,
        );
        assert_eq!(authority.unknown_ledger_entries, vec![UNNAMED_LEDGER_ROW.to_string()]);
        assert!(!authority.is_current());
    }

    // -------------------------------------------------------------------
    // Control 12: digest semantics
    // -------------------------------------------------------------------

    #[test]
    fn source_reordering_preserves_digest_and_semantic_movement_changes_it() {
        let first = resolve_from(
            &policy(vec![
                gate("alpha", "pr_fast", true, false),
                gate("beta", "merge_gate", false, true),
            ]),
            ledger(vec![entry("beta")]).as_ref(),
            TODAY,
        );
        let reordered = resolve_from(
            &policy(vec![
                gate("beta", "merge_gate", false, true),
                gate("alpha", "pr_fast", true, false),
            ]),
            ledger(vec![entry("beta")]).as_ref(),
            TODAY,
        );
        assert_eq!(first.semantic_digest, reordered.semantic_digest);

        let moved = resolve_from(
            &policy(vec![
                gate("alpha", "pr_fast", false, false),
                gate("beta", "merge_gate", false, true),
            ]),
            ledger(vec![entry("beta")]).as_ref(),
            TODAY,
        );
        assert_ne!(first.semantic_digest, moved.semantic_digest);
    }

    #[test]
    fn digest_covers_every_semantic_field_of_a_row() {
        // Review finding: the digest previously omitted `detail`, `source`,
        // `owner_issue`, `quarantined_at`, and `failure_pattern`, so rows
        // that differ only in those fields kept the same identity. Each
        // mutation below must move it.
        let base = resolve_from(
            &policy(vec![gate("q", "pr_fast", false, true)]),
            ledger(vec![entry("q")]).as_ref(),
            TODAY,
        );

        // Different closed failure detail on an invalid row: a quarantine
        // bit without ledger evidence versus a projection without one.
        let bit_without_ledger = resolve_from(
            &policy(vec![gate("q", "pr_fast", true, true)]),
            ledger(Vec::new()).as_ref(),
            TODAY,
        );
        let projected_without_ledger = resolve_from(
            &policy_with_flake(
                vec![gate("q", "pr_fast", true, false)],
                Some(flake_with_projection("q")),
            ),
            ledger(Vec::new()).as_ref(),
            TODAY,
        );
        assert_ne!(
            bit_without_ledger.semantic_digest, projected_without_ledger.semantic_digest,
            "invalid rows with different closed reasons must hash differently"
        );

        // Same gate/lifecycle/resolution, different quarantine evidence
        // fields: owner_issue, quarantined_at, failure_pattern.
        let mut reissued = entry("q");
        reissued.issue = Some("#424242".to_string());
        let mut refiled = entry("q");
        refiled.added = Some("2026-08-15".to_string());
        let mut repatterned = entry("q");
        repatterned.failure_pattern = Some("a different failure signature".to_string());
        for mutated in [reissued, refiled, repatterned] {
            let authority = resolve_from(
                &policy(vec![gate("q", "pr_fast", false, true)]),
                ledger(vec![mutated]).as_ref(),
                TODAY,
            );
            assert_ne!(
                base.semantic_digest, authority.semantic_digest,
                "quarantine evidence movement must move the digest"
            );
        }
    }

    // -------------------------------------------------------------------
    // Integration seam for #9148 / PR #10147
    // -------------------------------------------------------------------

    #[test]
    fn selector_skipped_and_quarantined_gate_keeps_quarantine_authority() {
        // Required integration falsifier: a gate that is BOTH selector-skipped
        // and currently quarantined must keep its quarantine disposition; a
        // planner that skipped the lifecycle lookup would degrade it to
        // scoped_noop and fail this test.
        let authority = resolve_from(
            &policy(vec![gate("seam", "merge_gate", false, true)]),
            ledger(vec![entry("seam")]).as_ref(),
            TODAY,
        );
        let row = authority.get("seam").unwrap();
        assert_eq!(
            planned_outcome(row, SelectorEvidence::NotApplicableToSubject),
            PlannedOutcome::Quarantined
        );
        assert_eq!(
            skip_population_admission(row),
            SkipPopulationAdmission::Refused { lifecycle: GateLifecycle::Quarantined }
        );
        assert!(!row.quarantine.as_ref().unwrap().owner.is_empty());
    }

    #[test]
    fn verbose_cannot_turn_expired_or_invalid_quarantine_into_runnable_work() {
        let mut expired = entry("loud");
        expired.expires = Some("2026-01-01".to_string());
        let authority = resolve_from(
            &policy(vec![gate("loud", "merge_gate", false, true)]),
            ledger(vec![expired]).as_ref(),
            TODAY,
        );
        let row = authority.get("loud").unwrap();
        assert_eq!(execution_admission(row, true), ExecutionAdmission::ActionRequired);
        assert_eq!(execution_admission(row, false), ExecutionAdmission::ActionRequired);

        let mut ownerless = entry("loud2");
        ownerless.owner = None;
        ownerless.issue = None;
        let authority = resolve_from(
            &policy(vec![gate("loud2", "merge_gate", false, true)]),
            ledger(vec![ownerless]).as_ref(),
            TODAY,
        );
        let row = authority.get("loud2").unwrap();
        assert_eq!(execution_admission(row, true), ExecutionAdmission::ActionRequired);
    }

    #[test]
    fn current_quarantine_is_never_executed_through_ordinary_admission() {
        let authority = resolve_from(
            &policy(vec![gate("held", "merge_gate", false, true)]),
            ledger(vec![entry("held")]).as_ref(),
            TODAY,
        );
        let row = authority.get("held").unwrap();
        assert_eq!(execution_admission(row, false), ExecutionAdmission::QuarantineSkip);
        assert_eq!(execution_admission(row, true), ExecutionAdmission::QuarantineSkip);
    }

    #[test]
    fn explain_output_names_authority_and_row_reasons() {
        let mut expired = entry("explained");
        expired.expires = Some("2026-05-26".to_string());
        let authority = resolve_from(
            &policy(vec![
                gate("explained", "merge_gate", false, true),
                gate("plain", "pr_fast", true, false),
            ]),
            ledger(vec![expired]).as_ref(),
            TODAY,
        );
        let text = authority.format_explanation();
        assert!(text.contains("gate_disposition.v1"));
        assert!(text.contains("explained lifecycle=quarantined resolution=expired"));
        assert!(text.contains("plain lifecycle=active resolution=current"));
        assert!(text.contains("semantic_digest="));
    }

    // -------------------------------------------------------------------
    // Checked-in policy reconciliation (current main state)
    // -------------------------------------------------------------------

    #[test]
    fn checked_in_security_audit_resolves_invalid_not_quarantined_or_noop() {
        let root = crate::utils::project_root().unwrap();
        let authority = resolve(&root).unwrap();
        let row =
            authority.get("security_audit").expect("security_audit must remain a governed gate");
        // Source-backed: the ledger evidence is expired (2026-05-26) and
        // ownerless, so this is invalid action-required input — not a valid
        // current quarantine, and never scoped_noop.
        assert_eq!(row.lifecycle, GateLifecycle::Quarantined);
        assert_ne!(row.resolution, DispositionResolution::Current);
        let detail = row.detail.as_deref().unwrap();
        assert!(detail.contains("owner"), "detail must name the missing owner: {detail}");
        assert_eq!(row.policy_role, GatePolicyRole::Advisory);
        assert_eq!(
            planned_outcome(row, SelectorEvidence::NotApplicableToSubject),
            PlannedOutcome::ActionRequired
        );
    }

    #[test]
    fn checked_in_quarantine_bits_without_ledger_rows_resolve_invalid() {
        let root = crate::utils::project_root().unwrap();
        let authority = resolve(&root).unwrap();
        let unevidenced: Vec<&str> = authority
            .rows
            .iter()
            .filter(|row| {
                row.resolution == DispositionResolution::Invalid
                    && row
                        .detail
                        .as_deref()
                        .is_some_and(|detail| detail.contains("no debt-ledger evidence"))
            })
            .map(|row| row.gate_id.as_str())
            .collect();
        // The nightly expensive-gate bits (mutation, fuzz, ...) claim
        // quarantine without ledger evidence — honest fail-closed reporting,
        // surfaced for #6261's later inventory work.
        assert!(
            !unevidenced.is_empty(),
            "current policy carries quarantine bits without ledger rows; expected non-empty"
        );
        assert!(unevidenced.contains(&"mutation"));
        assert!(unevidenced.contains(&"fuzz"));
    }

    #[test]
    fn checked_in_rows_cover_every_governed_gate_exactly_once() {
        let root = crate::utils::project_root().unwrap();
        let policy =
            crate::tasks::gates::load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))
                .unwrap();
        let authority = resolve(&root).unwrap();
        assert_eq!(authority.rows.len(), policy.gates.len());
        for gate in &policy.gates {
            assert!(authority.get(&gate.name).is_some(), "missing row for {}", gate.name);
        }
        // The checked-in authority names its canonical sources.
        assert!(authority.source_paths.iter().any(|path| path.contains("gate-policy.yaml")));
        assert!(authority.source_paths.iter().any(|path| path.contains("debt-ledger.yaml")));
    }
}
