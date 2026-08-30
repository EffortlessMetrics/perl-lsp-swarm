//! `cargo xtask init-environment` — the initialize-operation phase and owner
//! ledger (#10040, #8123 E01).
//!
//! This slice changes no startup behaviour. It establishes the checked
//! denominator that the later E02-E04 cutovers consume: every operation
//! reachable from `initialize` or its immediate bootstrap paths gets one stable
//! row, exactly one phase disposition, and an explicit statement of what it may
//! influence.
//!
//! The ledger is deliberately split into two halves that must agree:
//!
//! * [`rows`] *declares* each operation — its owner, triggers, blocking
//!   exposure, and phase.
//! * [`census`] *derives* reachability and blocking exposure from current
//!   source.
//!
//! [`ledger_errors`] fails when the two disagree in either direction. A row may
//! not under-declare exposure (that hides blocking work on the critical path)
//! and may not over-declare it (that lets stale prose outlive the code it
//! describes).

pub mod census;
pub mod rows;

use std::collections::{BTreeMap, BTreeSet};

use census::{Census, Exposure};

/// Entry points that begin the initialize denominator.
///
/// `handle_initialize` is the request itself; `complete_initialization` is the
/// deferred half reached from the `initialized` notification; and
/// `auto_initialize_for_compat` is the compatibility trigger for clients that
/// never send `initialized`. Omitting the compatibility trigger would let a
/// whole class of operations escape the census.
/// Each root is an exact `(file, function)` citation. Bare names are not enough:
/// `handle_initialize` also names an unrelated DAP handler, and
/// `auto_initialize_for_compat` genuinely has two definitions, both of which are
/// real compatibility entry points.
pub const CENSUS_ROOTS: &[(&str, &str)] = &[
    ("crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs", "handle_initialize"),
    ("crates/perl-lsp-rs/src/runtime/dispatch/lifecycle.rs", "complete_initialization"),
    ("crates/perl-lsp-rs/src/runtime/dispatch/lifecycle.rs", "auto_initialize_for_compat"),
    ("crates/perl-lsp-rs/src/runtime/dispatch/preflight.rs", "auto_initialize_for_compat"),
];

/// Exactly-one phase disposition for an initialize-reachable operation.
///
/// The controlling issue forbids a `maybe`, `miscellaneous`, `keep_for_now`, or
/// unnamed future-owner bucket, so this enum is closed by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PhaseDisposition {
    /// Required by the protocol before the initialize response is committed.
    ProtocolRequiredBeforeResponse,
    /// Shapes capability or security/trust negotiation before the response.
    CapabilityOrSecurityCriticalBeforeResponse,
    /// Local, process-free configuration read before the response.
    LocalProcessFreeConfigBeforeResponse,
    /// Belongs to post-initialize environment work.
    DeferToPostInitializeEnvironment,
    /// Should run on first use of the dependent feature.
    LazyOnFirstUse,
    /// Should run only on explicit user action.
    UserTriggeredOnly,
    /// Belongs to repository conformance tooling, not the product lifecycle.
    RepositoryConformanceOnly,
    /// Should leave the product lifecycle entirely.
    RemoveFromProductLifecycle,
    /// Already owned elsewhere; no move required.
    ExistingExternalOwnerNoMove,
}

impl PhaseDisposition {
    /// Stable identifier used in rendered output.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ProtocolRequiredBeforeResponse => "protocol_required_before_response",
            Self::CapabilityOrSecurityCriticalBeforeResponse => {
                "capability_or_security_critical_before_response"
            }
            Self::LocalProcessFreeConfigBeforeResponse => {
                "local_process_free_config_before_response"
            }
            Self::DeferToPostInitializeEnvironment => "defer_to_post_initialize_environment",
            Self::LazyOnFirstUse => "lazy_on_first_use",
            Self::UserTriggeredOnly => "user_triggered_only",
            Self::RepositoryConformanceOnly => "repository_conformance_only",
            Self::RemoveFromProductLifecycle => "remove_from_product_lifecycle",
            Self::ExistingExternalOwnerNoMove => "existing_external_owner_no_move",
        }
    }

    /// Whether this disposition asserts the operation belongs before the
    /// initialize response is committed.
    pub const fn is_before_response(self) -> bool {
        matches!(
            self,
            Self::ProtocolRequiredBeforeResponse
                | Self::CapabilityOrSecurityCriticalBeforeResponse
                | Self::LocalProcessFreeConfigBeforeResponse
        )
    }

    /// Whether this disposition implies the operation must eventually move.
    ///
    /// `existing_external_owner_no_move` and `repository_conformance_only` are
    /// terminal: they say the operation is already where it belongs, or belongs
    /// to tooling outside the product lifecycle. Neither owes a migration wave,
    /// so neither should be dragged into an E02-E04 cutover.
    pub const fn implies_movement(self) -> bool {
        matches!(
            self,
            Self::DeferToPostInitializeEnvironment
                | Self::LazyOnFirstUse
                | Self::UserTriggeredOnly
                | Self::RemoveFromProductLifecycle
        )
    }
}

/// Where the operation actually runs on current `main`.
///
/// This is separate from [`PhaseDisposition`], which is the *target*. E01
/// changes no behaviour, so a row may legitimately record "runs before the
/// response today, belongs after it" — provided it names the wave that moves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExecutionPoint {
    /// Executes before the initialize response is committed.
    BeforeResponse,
    /// Executes after the response, on `initialized` or the compat trigger.
    AfterResponse,
    /// Executes only when a dependent feature or user action demands it.
    OnDemand,
}

impl ExecutionPoint {
    /// Stable identifier used in rendered output.
    pub const fn label(self) -> &'static str {
        match self {
            Self::BeforeResponse => "before_response",
            Self::AfterResponse => "after_response",
            Self::OnDemand => "on_demand",
        }
    }
}

/// The later cutover that acts on a row, when the row implies movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MigrationWave {
    /// No movement implied; current point already matches the disposition.
    None,
    /// E02 cutover.
    E02,
    /// E03 cutover.
    E03,
    /// E04 cutover.
    E04,
}

impl MigrationWave {
    /// Stable identifier used in rendered output.
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::E02 => "E02",
            Self::E03 => "E03",
            Self::E04 => "E04",
        }
    }
}

/// What causes an operation to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Trigger {
    /// The `initialize` request.
    Initialize,
    /// The `initialized` notification.
    Initialized,
    /// The compatibility auto-initialize path.
    AutoInitializeCompat,
    /// Later configuration change.
    Reconfiguration,
    /// First use of a dependent feature.
    FirstUse,
}

impl Trigger {
    /// Stable identifier used in rendered output.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::Initialized => "initialized",
            Self::AutoInitializeCompat => "auto_initialize_compat",
            Self::Reconfiguration => "reconfiguration",
            Self::FirstUse => "first_use",
        }
    }
}

/// One initialize-reachable operation.
#[derive(Debug, Clone)]
pub struct InitOperationRow {
    /// Stable row identity. Never reused for a different operation.
    pub operation_id: &'static str,
    /// Repository-relative file owning the entry function.
    pub file: &'static str,
    /// Entry function name, resolvable in the census.
    pub function: &'static str,
    /// The proposition this operation represents.
    pub proposition: &'static str,
    /// Observable side effects, including client-visible publications.
    pub side_effects: &'static [&'static str],
    /// Blocking/ambient exposure this row claims.
    pub declared_exposure: &'static [Exposure],
    /// Current triggers on `main`.
    pub triggers: &'static [Trigger],
    /// Whether the operation is guarded to run at most once per session.
    pub exactly_once: bool,
    /// Where the operation runs today.
    pub current_point: ExecutionPoint,
    /// Where it belongs.
    pub phase: PhaseDisposition,
    /// The wave that reconciles `current_point` with `phase`.
    pub migration_wave: MigrationWave,
    /// Whether the operation can influence the static `InitializeResult`.
    pub affects_static_initialize_result: bool,
    /// The exact final-surface (#9662) input this row maps to, when it claims a
    /// static-surface effect. Empty otherwise.
    pub static_surface_join: &'static str,
    /// Whether it can influence the dynamic-registration plan.
    pub affects_dynamic_registration_plan: bool,
    /// Whether it can influence position/text/security/trust negotiation.
    pub affects_negotiation: bool,
    /// Whether it can influence initial native document/provider semantics.
    pub affects_initial_native_semantics: bool,
    /// Authority that owns this operation today.
    pub current_owner: &'static str,
    /// Authority that should own it.
    pub target_owner: &'static str,
    /// Proof/falsifier family covering this row.
    pub proof_family: &'static str,
    /// Memoization note, or empty when the operation recomputes each time.
    pub memoization: &'static str,
    /// Whether this row's transitive closure accounts for reachable blocking
    /// work during coverage checking.
    ///
    /// Umbrella rows (an entry point such as `handle_initialize`) set this
    /// `false`. Their closure spans the whole initialize path, so counting it
    /// would let one broad row satisfy coverage for everything and the check
    /// would go green having discriminated nothing. Such a row still accounts
    /// for exposure written directly in its own body.
    pub owns_exposure: bool,
}

/// The maintained ledger.
pub fn ledger_rows() -> Vec<InitOperationRow> {
    rows::ledger_rows()
}

/// Validate the ledger against an independently derived census.
///
/// Returned errors are sorted and deduplicated so output is deterministic.
pub fn ledger_errors(rows: &[InitOperationRow], census: &Census) -> Vec<String> {
    ledger_errors_with_roots(rows, census, CENSUS_ROOTS)
}

/// Validate against an explicit root set.
///
/// Falsifier tests drive this with a synthetic codebase so each rule can be
/// shown to fail on a deliberately wrong ledger, rather than only to pass on the
/// real one.
pub fn ledger_errors_with_roots(
    rows: &[InitOperationRow],
    census: &Census,
    roots: &[(&str, &str)],
) -> Vec<String> {
    let mut errors = Vec::new();

    errors.extend(structural_errors(rows));
    errors.extend(citation_errors(rows, census));
    errors.extend(side_effect_errors(rows, census));
    errors.extend(exposure_errors(rows, census));
    errors.extend(phase_errors(rows, census));
    errors.extend(static_surface_errors(rows, census));
    errors.extend(coverage_errors(rows, census, roots));

    errors.sort();
    errors.dedup();
    errors
}

fn structural_errors(rows: &[InitOperationRow]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for row in rows {
        if !seen.insert(row.operation_id) {
            errors.push(format!("duplicate operation_id: {}", row.operation_id));
        }
        if row.operation_id.is_empty() || row.function.is_empty() || row.file.is_empty() {
            errors.push(format!("row {} has an empty identity field", row.operation_id));
        }
        if row.proposition.is_empty() {
            errors.push(format!("row {} has no represented proposition", row.operation_id));
        }
        if row.triggers.is_empty() {
            errors.push(format!("row {} declares no trigger", row.operation_id));
        }
        if row.proof_family.is_empty() {
            errors.push(format!("row {} names no proof family", row.operation_id));
        }
        if row.current_owner.is_empty() || row.target_owner.is_empty() {
            errors.push(format!("row {} leaves an owner unnamed", row.operation_id));
        }
    }
    errors
}

/// Fail when a row's side effects name a protocol method the source never sends.
///
/// `side_effects` was previously free prose that no rule inspected, and a row
/// claiming `perl/workspaceReady` survived review even though the server sends
/// `perl-lsp/index-ready` and no such method exists anywhere. That is exactly
/// the stale-prose failure this module exists to catch, so the field is now
/// derived against the literals present in scanned source.
fn side_effect_errors(rows: &[InitOperationRow], census: &Census) -> Vec<String> {
    let mut errors = Vec::new();
    for row in rows {
        for effect in row.side_effects {
            for token in effect.split_whitespace() {
                let candidate =
                    token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '/');
                if !census::looks_like_protocol_method(candidate) {
                    continue;
                }
                if !census.declares_method(candidate) {
                    errors.push(format!(
                        "row {} claims side effect `{}`, but no scanned source sends `{}`",
                        row.operation_id, effect, candidate
                    ));
                }
            }
        }
    }
    errors
}

fn citation_errors(rows: &[InitOperationRow], census: &Census) -> Vec<String> {
    let mut errors = Vec::new();
    for row in rows {
        if census.resolve(row.file, row.function).is_some() {
            continue;
        }
        let files = census.files_for(row.function);
        if files.is_empty() {
            errors.push(format!(
                "stale citation in row {}: function `{}` is not present in the scanned source",
                row.operation_id, row.function
            ));
        } else {
            errors.push(format!(
                "stale citation in row {}: function `{}` was found in [{}], not {}",
                row.operation_id,
                row.function,
                files.join(", "),
                row.file
            ));
        }
    }
    errors
}

fn exposure_errors(rows: &[InitOperationRow], census: &Census) -> Vec<String> {
    let mut errors = Vec::new();
    for row in rows {
        let Some(index) = census.resolve(row.file, row.function) else {
            continue;
        };
        let derived = census.transitive_exposures(index, census::MAX_DEPTH);
        let declared: BTreeSet<Exposure> = row.declared_exposure.iter().copied().collect();

        for (exposure, witness) in &derived {
            if !declared.contains(exposure) {
                errors.push(format!(
                    "row {} under-declares exposure: source reaches {} but the row does not \
                     declare it",
                    row.operation_id,
                    witness.render()
                ));
            }
        }
        for exposure in &declared {
            if !derived.contains_key(exposure) {
                errors.push(format!(
                    "row {} over-declares exposure `{}`: no path from `{}::{}` reaches it in \
                     current source",
                    row.operation_id,
                    exposure.label(),
                    row.file,
                    row.function
                ));
            }
        }
    }
    errors
}

fn phase_errors(rows: &[InitOperationRow], census: &Census) -> Vec<String> {
    let mut errors = Vec::new();
    for row in rows {
        // A before-response disposition must describe something that actually
        // runs before the response.
        if row.phase.is_before_response() && row.current_point != ExecutionPoint::BeforeResponse {
            errors.push(format!(
                "row {} claims `{}` but currently runs `{}`",
                row.operation_id,
                row.phase.label(),
                row.current_point.label()
            ));
        }

        // A row that belongs elsewhere but still runs before the response must
        // name the wave that moves it. This is what keeps "defer it later" from
        // becoming an unowned intention.
        if row.phase.implies_movement()
            && row.current_point == ExecutionPoint::BeforeResponse
            && row.migration_wave == MigrationWave::None
        {
            errors.push(format!(
                "row {} defers to `{}` but still runs before the response with no migration wave",
                row.operation_id,
                row.phase.label()
            ));
        }

        // Conversely, a terminal disposition must not claim a cutover.
        if !row.phase.implies_movement() && row.migration_wave != MigrationWave::None {
            errors.push(format!(
                "row {} has terminal disposition `{}` but claims wave {}",
                row.operation_id,
                row.phase.label(),
                row.migration_wave.label()
            ));
        }

        // A row whose disposition and current point already agree must not
        // claim a wave; that would schedule work nobody needs to do.
        // `local_process_free_config_before_response` is a load-bearing claim:
        // it asserts the operation is safe on the critical path. Derived process
        // or PATH work refutes it.
        if row.phase == PhaseDisposition::LocalProcessFreeConfigBeforeResponse
            && let Some(index) = census.resolve(row.file, row.function)
        {
            let derived = census.transitive_exposures(index, census::MAX_DEPTH);
            for blocking in [Exposure::ProcessSpawn, Exposure::PathLookup, Exposure::Network] {
                if let Some(witness) = derived.get(&blocking) {
                    errors.push(format!(
                        "row {} claims process-free configuration but reaches {}",
                        row.operation_id,
                        witness.render()
                    ));
                }
            }
        }
    }
    errors
}

fn static_surface_errors(rows: &[InitOperationRow], census: &Census) -> Vec<String> {
    let mut errors = Vec::new();
    for row in rows {
        if row.affects_static_initialize_result && row.static_surface_join.is_empty() {
            errors.push(format!(
                "row {} claims a static InitializeResult effect without a #9662 final-surface join",
                row.operation_id
            ));
        }
        if !row.affects_static_initialize_result && !row.static_surface_join.is_empty() {
            errors.push(format!(
                "row {} names a final-surface join but claims no static InitializeResult effect",
                row.operation_id
            ));
        }

        // Ambient state is never negotiation authority. `EffectiveLspSurface` is
        // pure: it does not probe tools, read PATH, or infer support from what
        // happens to be installed. A row that reaches ambient state and also
        // claims a static-surface effect is asserting exactly the edge #9662
        // removed.
        if row.affects_static_initialize_result
            && let Some(index) = census.resolve(row.file, row.function)
        {
            let derived = census.transitive_exposures(index, census::MAX_DEPTH);
            for ambient in [Exposure::ProcessSpawn, Exposure::PathLookup, Exposure::Network] {
                if let Some(witness) = derived.get(&ambient) {
                    errors.push(format!(
                        "row {} claims a static InitializeResult effect but depends on ambient \
                         state: {}",
                        row.operation_id,
                        witness.render()
                    ));
                }
            }
        }
    }
    errors
}

/// Fail when source reachable from an initialize root carries blocking
/// exposure that no row accounts for.
///
/// This is the rule that makes the ledger a denominator rather than a list. It
/// attributes exposure transitively, so a helper several hops down still has to
/// belong to some row.
fn coverage_errors(
    rows: &[InitOperationRow],
    census: &Census,
    roots: &[(&str, &str)],
) -> Vec<String> {
    let mut errors = Vec::new();

    // A row always accounts for exposure written directly in its own body. Only
    // an exposure-owning row additionally accounts for its closure.
    let mut owned: BTreeSet<usize> = BTreeSet::new();
    let mut owning: Vec<(&InitOperationRow, usize)> = Vec::new();
    for row in rows {
        let Some(index) = census.resolve(row.file, row.function) else {
            continue;
        };
        owned.insert(index);
        if row.owns_exposure {
            owned.extend(census.reachable_from(index, census::MAX_DEPTH).into_keys());
            owning.push((row, index));
        }
    }

    if owning.is_empty() && !rows.is_empty() {
        errors.push("no ledger row owns exposure; coverage checking would be vacuous".to_string());
    }

    // Exposure-owning rows must be pairwise non-nested. If one owning row sits
    // inside another's closure, the outer row silently absorbs the inner one's
    // work and coverage stops discriminating at that seam.
    for (outer, outer_index) in &owning {
        let closure = census.reachable_from(*outer_index, census::MAX_DEPTH);
        for (inner, inner_index) in &owning {
            if outer.operation_id == inner.operation_id {
                continue;
            }
            if closure.contains_key(inner_index) {
                errors.push(format!(
                    "exposure-owning rows {} and {} are nested: `{}` reaches `{}`, so the outer \
                     row absorbs the inner one",
                    outer.operation_id,
                    inner.operation_id,
                    census.qualified(*outer_index),
                    census.qualified(*inner_index)
                ));
            }
        }
    }

    for (file, function) in roots {
        let Some(root) = census.resolve(file, function) else {
            errors.push(format!(
                "census root `{file}::{function}` is not present in the scanned source; the \
                 denominator cannot be established"
            ));
            continue;
        };
        for index in census.reachable_from(root, census::MAX_DEPTH).into_keys() {
            if owned.contains(&index) {
                continue;
            }
            let direct = census.direct_exposures(index);
            if direct.is_empty() {
                continue;
            }
            let kinds: Vec<&str> = direct.iter().map(|exposure| exposure.label()).collect();
            errors.push(format!(
                "unregistered initialize work: `{}` is reachable from `{file}::{function}` and \
                 performs [{}] but no ledger row owns it",
                census.qualified(index),
                kinds.join(", ")
            ));
        }
    }
    errors
}

/// Render the ledger as deterministic JSON.
pub fn render_json(rows: &[InitOperationRow]) -> String {
    let mut sorted: Vec<&InitOperationRow> = rows.iter().collect();
    sorted.sort_by(|left, right| left.operation_id.cmp(right.operation_id));

    let entries: Vec<serde_json::Value> = sorted
        .iter()
        .map(|row| {
            serde_json::json!({
                "operation_id": row.operation_id,
                "file": row.file,
                "function": row.function,
                "proposition": row.proposition,
                "side_effects": row.side_effects,
                "declared_exposure": row
                    .declared_exposure
                    .iter()
                    .map(|exposure| exposure.label())
                    .collect::<Vec<_>>(),
                "triggers": row.triggers.iter().map(|t| t.label()).collect::<Vec<_>>(),
                "exactly_once": row.exactly_once,
                "current_point": row.current_point.label(),
                "phase": row.phase.label(),
                "migration_wave": row.migration_wave.label(),
                "affects_static_initialize_result": row.affects_static_initialize_result,
                "static_surface_join": row.static_surface_join,
                "affects_dynamic_registration_plan": row.affects_dynamic_registration_plan,
                "affects_negotiation": row.affects_negotiation,
                "affects_initial_native_semantics": row.affects_initial_native_semantics,
                "current_owner": row.current_owner,
                "target_owner": row.target_owner,
                "proof_family": row.proof_family,
                "memoization": row.memoization,
            })
        })
        .collect();

    let document = serde_json::json!({
        "schema": "initialize_operation_ledger.v1",
        "controlling_issue": "#10040",
        "census_roots": CENSUS_ROOTS,
        "rows": entries,
    });

    format!("{}\n", serde_json::to_string_pretty(&document).unwrap_or_default())
}

/// Group rows by phase disposition for the human view.
pub fn by_phase(rows: &[InitOperationRow]) -> BTreeMap<&'static str, Vec<&'static str>> {
    let mut grouped: BTreeMap<&'static str, Vec<&'static str>> = BTreeMap::new();
    for row in rows {
        grouped.entry(row.phase.label()).or_default().push(row.operation_id);
    }
    for ids in grouped.values_mut() {
        ids.sort_unstable();
    }
    grouped
}

/// Group rows by migration wave for the cutover view.
pub fn by_wave(rows: &[InitOperationRow]) -> BTreeMap<&'static str, Vec<&'static str>> {
    let mut grouped: BTreeMap<&'static str, Vec<&'static str>> = BTreeMap::new();
    for row in rows {
        grouped.entry(row.migration_wave.label()).or_default().push(row.operation_id);
    }
    for ids in grouped.values_mut() {
        ids.sort_unstable();
    }
    grouped
}
