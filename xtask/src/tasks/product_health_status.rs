//! Deterministic generic assembly of independent product-health rails
//! (`product_health_status.v1`, issue #12360).
//!
//! This module consumes the landed dependency-neutral contracts from
//! [`crate::tasks::product_health_rail_contract`] exactly as they exist
//! (`product_health_rail.v1`, `product_health_rail_adapter.v1`,
//! `product_health_rail_registry.v1`, #12359 / PR #12370).  It adds no
//! schema of its own to those contracts: schema evolution belongs to that
//! surface.  The assembler owns generic rail assembly only:
//!
//! * one checked registry plus repository-local source packets are adapted
//!   into independent named rails;
//! * exact currentness and source conflicts are resolved by declared
//!   identity and declared succession only — never by timestamps, file
//!   names, recency, or green preference;
//! * every declared rail stays present with one explicit typed
//!   currentness/result state even when its source is unavailable, stale,
//!   malformed, conflicting, or unsupported (fail closed, never synthetic
//!   green);
//! * the immutable machine object `product_health_status.v1` is emitted
//!   deterministically, and read-only `build`/`check`/`show`/`diff`
//!   commands project it.
//!
//! Source packets resolve from the `sources/` directory next to the
//! registry file.  Only JSON packets are read: authored Markdown, issue,
//! PR, workflow, or check state is structurally invisible to assembly.  A
//! registry currentness relation this generic policy does not implement
//! fails closed as `not_proven` with the typed finding
//! `unsupported_currentness_relation` under state `source_unavailable`; it
//! is never silently reinterpreted.
//!
//! Non-goals owned elsewhere: no source-specific adapter, no new
//! measurement, no Markdown rendering, no release/support/publication
//! authority (structurally constant false/empty here), no GitHub or
//! network access, no proof execution.

use crate::tasks::product_health_rail_contract::{
    Adapter, Applicability, Rail, RailResult, Registry, canonical_json, validate_registry,
};
use clap::Subcommand;
use color_eyre::eyre::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest as Sha256Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Schema identity of the assembled machine status object.
pub const STATUS_SCHEMA: &str = "product_health_status.v1";

/// Generator identity (deterministic constant; no host/timestamp input).
const GENERATOR: &str = "xtask::product_health_status";

/// Generic assembly policy identity implemented by this module.
const ASSEMBLY_POLICY: &str = "generic-exact.v1";

/// Closed generic currentness vocabulary (issue #12360).
pub const CURRENTNESS_STATES: &[&str] = &[
    "current_exact",
    "no_current_source",
    "historical_only",
    "stale_only",
    "invalid_only",
    "conflicting_current_sources",
    "source_subject_missing",
    "source_subject_mismatch",
    "adapter_unavailable",
    "source_unavailable",
];

/// Registry currentness relations this generic assembler implements.  Any
/// other declared relation fails closed as `not_proven` with a typed
/// finding; it is never silently reinterpreted.
const SUPPORTED_RELATION_PREFIX: &str = "exact:";

/// Bounded-detail privacy bounds.  Values beyond the bound are a typed
/// privacy failure, never copied into status output.
const DETAIL_KEY_BOUND: usize = 64;
const DETAIL_VALUE_BOUND: usize = 512;
/// Packet identity fields are bounded so no unbounded source input can
/// reach status output through ids, subjects, or finding details.
const PACKET_IDENTITY_BOUND: usize = 256;

/// Result vocabulary a source packet may carry.  Assembly-level states
/// (`no_current_source`, `conflicting_current_sources`) describe source
/// *relations* and may not be asserted by a single source packet.
const META_SOURCE_RESULTS: &[RailResult] =
    &[RailResult::NoCurrentSource, RailResult::ConflictingCurrentSources];

const SATISFYING_RESULTS: &[RailResult] =
    &[RailResult::Pass, RailResult::PassWithDeclaredLimitations];

/// Result vocabulary a single current source packet can express; the
/// assembly-level relation states are decided by assembly, never asserted
/// by a source.
const SOURCE_EXPRESSIBLE_RESULTS: &[RailResult] = &[
    RailResult::Pass,
    RailResult::PassWithDeclaredLimitations,
    RailResult::Failed,
    RailResult::NotProven,
    RailResult::Stale,
    RailResult::Invalid,
    RailResult::Unsupported,
    RailResult::NotApplicable,
];

/// Deterministic state-to-result relation for every non-current state.
/// `check` enforces it so an edited snapshot cannot pair a typed
/// non-current state with a green result (synthetic green).
fn expected_result_for_state(state: &str) -> Option<RailResult> {
    match state {
        "no_current_source" | "historical_only" => Some(RailResult::NoCurrentSource),
        "stale_only" => Some(RailResult::Stale),
        "invalid_only" | "source_subject_mismatch" => Some(RailResult::Invalid),
        "conflicting_current_sources" => Some(RailResult::ConflictingCurrentSources),
        "source_subject_missing" | "source_unavailable" => Some(RailResult::NotProven),
        "adapter_unavailable" => Some(RailResult::Unsupported),
        _ => None, // current_exact: coherent with its source result instead
    }
}

// ---------------------------------------------------------------------------
// Source packet envelope (generic, owned by this module)
// ---------------------------------------------------------------------------

/// Currentness marker a source packet declares about itself.  Supersession
/// is declared only through `supersedes`; issue closure, PR merges,
/// artifact deletion, or rerun requests are never supersession.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PacketState {
    Current,
    Historical,
    Stale,
}

impl PacketState {
    fn as_str(self) -> &'static str {
        match self {
            PacketState::Current => "current",
            PacketState::Historical => "historical",
            PacketState::Stale => "stale",
        }
    }
}

/// Generic repository-local source packet envelope.  Unknown fields are
/// denied: a packet shape outside this envelope is malformed evidence, not
/// generically decoded input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourcePacket {
    pub schema: String,
    pub packet_id: String,
    /// Routing selector; must equal the selected adapter's subject selector.
    pub selector: String,
    /// The rail this packet answers.
    pub rail_id: String,
    /// Exact subject identity of the measured source.
    pub subject: String,
    pub source_result: RailResult,
    pub state: PacketState,
    /// Declared exact succession (`Some(other_packet_id)`).
    #[serde(default)]
    pub supersedes: Option<String>,
    /// Stable bounded content digest of the source payload.
    pub digest: String,
    #[serde(default)]
    pub limitations: Vec<String>,
    #[serde(default)]
    pub nonclaims: Vec<String>,
    #[serde(default)]
    pub detail: BTreeMap<String, String>,
}

/// A parsed source packet with its canonical bytes and load file
/// reference.  Part of the in-process assembly surface so future
/// source-specific adapters and the unified health view can feed packets
/// without re-implementing loading.
#[derive(Debug, Clone)]
pub struct LoadedPacket {
    /// Repository-local file reference (bounded name).
    pub file: String,
    /// The parsed packet.
    pub packet: SourcePacket,
    /// Canonical serialization used for byte-identity comparisons.
    pub canonical: String,
    /// Typed semantic/privacy validation failure, if any.
    pub invalid_reason: Option<String>,
}

/// A packet file that could not be parsed at all (bad JSON or unknown
/// fields).  It can never be attributed to a rail, so it stays a global
/// typed finding; rails that find no other evidence still fail closed.
#[derive(Debug, Clone)]
pub struct UnparseablePacket {
    /// Repository-local file reference (bounded name).
    pub file: String,
    /// Bounded parse-failure reason.
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Status object
// ---------------------------------------------------------------------------

/// Bounded typed finding.  `subject` is a rail id, packet id, or file ref.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Finding {
    pub code: String,
    pub subject: String,
    pub detail: String,
}

/// Immutable content-addressed reference to a non-current packet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryRef {
    pub packet_id: String,
    pub digest: String,
    pub state: String,
    pub superseded_by: Option<String>,
}

/// One assembled rail.  The landed `product_health_rail.v1` shape is
/// flattened in unchanged — every status rail is a valid landed rail plus
/// the assembly projection — so the #12359 validators and future
/// source-specific adapters consume it without a parallel vocabulary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusRail {
    #[serde(flatten)]
    pub rail: Rail,
    pub currentness_state: String,
    pub source_result: Option<RailResult>,
    pub adapter_id: Option<String>,
    pub packet_id: Option<String>,
    pub owner: String,
    pub wake_event: Option<String>,
    pub blockers: Vec<String>,
    pub max_permitted_wording: String,
    pub history: Vec<HistoryRef>,
    /// Structurally unauthorized: constant `false` from assembly; `check`
    /// refuses any status where this is not false.
    pub support_authorized: bool,
    /// Structurally unauthorized: constant `false` from assembly.
    pub release_authorized: bool,
    /// Structurally unauthorized: constant empty from assembly.
    pub published_channels: Vec<String>,
}

/// Required rail that does not satisfy its proposition, with its exact
/// typed state retained.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequiredNonSatisfying {
    pub rail_id: String,
    pub currentness_state: String,
    pub result: String,
}

/// Exact descriptive sets over the exact named denominator of declared
/// rails.  No average, score, percentage, traffic light, majority, or
/// global verdict exists in this object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Rollup {
    pub required_satisfied: Vec<String>,
    pub required_nonsatisfying: Vec<RequiredNonSatisfying>,
    pub optional_conditional_not_selected: Vec<String>,
    pub source_adapters_unavailable: Vec<String>,
    pub current_source_conflicts: Vec<String>,
}

/// The immutable deterministic machine status object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Status {
    pub schema: String,
    pub generator: String,
    pub assembly_policy: String,
    pub registry_schema: String,
    pub registry_digest: String,
    pub adapters: Vec<Adapter>,
    pub rails: Vec<StatusRail>,
    pub findings: Vec<Finding>,
    pub history: Vec<HistoryRef>,
    pub rollup: Rollup,
    pub semantic_digest: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(71);
    out.push_str("sha256:");
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn rail_result_name(result: &RailResult) -> String {
    serde_json::to_value(result)
        .map(|value| value.as_str().map(str::to_owned).unwrap_or_default())
        .unwrap_or_default()
}

fn applicability_name(applicability: &Applicability) -> String {
    serde_json::to_value(applicability)
        .map(|value| value.as_str().map(str::to_owned).unwrap_or_default())
        .unwrap_or_default()
}

fn digest_shape_ok(digest: &str) -> bool {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Bound an arbitrary packet identity before it can reach a finding
/// detail: unvalidated identities never enter status output at full size.
fn bounded_identity(text: &str) -> String {
    if text.chars().count() > PACKET_IDENTITY_BOUND {
        let mut bounded: String = text.chars().take(PACKET_IDENTITY_BOUND).collect();
        bounded.push_str("…[bounded]");
        bounded
    } else {
        text.to_owned()
    }
}

fn detail_privacy_reason(packet: &SourcePacket) -> Option<String> {
    for (key, value) in &packet.detail {
        if key.len() > DETAIL_KEY_BOUND {
            return Some(format!("detail key exceeds {DETAIL_KEY_BOUND} bytes"));
        }
        if value.len() > DETAIL_VALUE_BOUND {
            return Some(format!("detail value for `{key}` exceeds {DETAIL_VALUE_BOUND} bytes"));
        }
    }
    if !digest_shape_ok(&packet.digest) {
        return Some("digest is not sha256:<64 lowercase hex>".to_owned());
    }
    if packet.schema.is_empty()
        || packet.packet_id.is_empty()
        || packet.selector.is_empty()
        || packet.rail_id.is_empty()
        || packet.subject.is_empty()
    {
        return Some("packet identity field is empty".to_owned());
    }
    for field in
        [&packet.schema, &packet.packet_id, &packet.selector, &packet.rail_id, &packet.subject]
    {
        if field.len() > PACKET_IDENTITY_BOUND {
            return Some(format!("packet identity field exceeds {PACKET_IDENTITY_BOUND} bytes"));
        }
    }
    if META_SOURCE_RESULTS.contains(&packet.source_result) {
        return Some(format!(
            "source_result {} is an assembly-level state a packet may not assert",
            rail_result_name(&packet.source_result)
        ));
    }
    if packet.supersedes.as_deref() == Some(packet.packet_id.as_str()) {
        return Some("packet supersedes itself".to_owned());
    }
    None
}

fn sorted_dedup(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}

fn max_permitted_wording(result: &RailResult, claim_ceiling: &str) -> String {
    match result {
        RailResult::Pass => claim_ceiling.to_owned(),
        RailResult::PassWithDeclaredLimitations => {
            format!("{claim_ceiling} — only with all declared limitations")
        }
        other => format!("no claim permitted ({})", rail_result_name(other)),
    }
}

/// Successor map for one rail's packets: `target_id -> successor_id` where
/// the successor declares `supersedes: target_id`.  History references
/// record the *incoming* edge (who superseded this packet), never the
/// packet's own outgoing declaration.
type Successors = BTreeMap<String, String>;

fn successors_of(packets: &[&LoadedPacket]) -> Successors {
    let mut successors = BTreeMap::new();
    for packet in packets {
        if let Some(target) = &packet.packet.supersedes {
            successors.insert(target.clone(), packet.packet.packet_id.clone());
        }
    }
    successors
}

fn history_refs(
    packets: &[&LoadedPacket],
    successors: &Successors,
    superseded_by_override: Option<&str>,
) -> Vec<HistoryRef> {
    let mut history = packets
        .iter()
        .map(|p| HistoryRef {
            packet_id: p.packet.packet_id.clone(),
            digest: p.packet.digest.clone(),
            state: p.packet.state.as_str().to_owned(),
            superseded_by: superseded_by_override
                .map(str::to_owned)
                .or_else(|| successors.get(&p.packet.packet_id).cloned()),
        })
        .collect::<Vec<_>>();
    history.sort_by(|a, b| a.packet_id.cmp(&b.packet_id).then(a.digest.cmp(&b.digest)));
    history
}

// ---------------------------------------------------------------------------
// Packet loading
// ---------------------------------------------------------------------------

/// Load every `*.json` packet under `dir` (deterministic file order).
/// Only JSON packets are read: authored Markdown, issue text, or any other
/// prose is structurally invisible to assembly.
fn load_packets(dir: &Path) -> Result<(Vec<LoadedPacket>, Vec<UnparseablePacket>)> {
    let mut loaded = Vec::new();
    let mut unparseable = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((loaded, unparseable));
        }
        Err(error) => {
            return Err(color_eyre::eyre::eyre!(
                "cannot read source packet directory {}: {error}",
                dir.display()
            ));
        }
    };
    // Strict enumeration: an unreadable directory entry or an unstattable
    // path is a typed instrument failure, never a silently absent packet
    // that could turn a conflict into an unearned pass.
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| {
            format!("cannot enumerate source packet directory {}", dir.display())
        })?;
        let path = entry.path();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let metadata = fs::metadata(&path)
            .with_context(|| format!("cannot stat source packet candidate {name}"))?;
        if metadata.is_file() && path.extension().is_some_and(|ext| ext == "json") {
            files.push(path);
        }
    }
    files.sort();
    for path in files {
        let file =
            path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes = fs::read(&path).with_context(|| format!("cannot read source packet {file}"))?;
        match serde_json::from_slice::<SourcePacket>(&bytes) {
            Ok(packet) => {
                let canonical = serde_json::to_string(&packet)
                    .with_context(|| format!("cannot canonicalize source packet {file}"))?;
                let invalid_reason = detail_privacy_reason(&packet);
                loaded.push(LoadedPacket { file, packet, canonical, invalid_reason });
            }
            Err(error) => {
                unparseable.push(UnparseablePacket { file, reason: format!("{error}") });
            }
        }
    }
    Ok((loaded, unparseable))
}

// ---------------------------------------------------------------------------
// Assembly
// ---------------------------------------------------------------------------

/// Resolve adapters accepting `source_schema`.  Zero or more-than-one both
/// fail closed: overlapping current adapters without a declared migration
/// are invalid, never resolved by registration order.
fn select_adapter<'a>(registry: &'a Registry, source_schema: &str) -> Vec<&'a Adapter> {
    registry
        .adapters
        .iter()
        .filter(|adapter| adapter.accepted_source_schemas.iter().any(|s| s == source_schema))
        .collect()
}

struct RailAssembly {
    rail: StatusRail,
    findings: Vec<Finding>,
    /// Packet files consumed while resolving this rail.
    consumed_files: BTreeSet<String>,
}

fn base_rail(declared: &Rail) -> Rail {
    Rail {
        schema: declared.schema.clone(),
        rail_id: declared.rail_id.clone(),
        area: declared.area.clone(),
        proposition: declared.proposition.clone(),
        source_schema: declared.source_schema.clone(),
        source_digest: declared.source_digest.clone(),
        subject: declared.subject.clone(),
        currentness: declared.currentness.clone(),
        result: declared.result.clone(),
        applicability: declared.applicability.clone(),
        limitations: declared.limitations.clone(),
        nonclaims: declared.nonclaims.clone(),
        claim_ceiling: declared.claim_ceiling.clone(),
        source_detail: declared.source_detail.clone(),
    }
}

/// Emit a rail whose source did not resolve to exactly one current
/// packet.  The declared rail stays present with its typed state; retained
/// valid packets remain visible as immutable history.
fn failed_state_rail(
    declared: &Rail,
    state: &str,
    result: RailResult,
    adapter_id: Option<String>,
    findings: &mut Vec<Finding>,
    retained: &[&LoadedPacket],
) -> StatusRail {
    let mut rail = base_rail(declared);
    rail.result = result.clone();
    let history = history_refs(retained, &successors_of(retained), None);
    let blockers: Vec<String> =
        findings.iter().map(|f| f.code.clone()).collect::<BTreeSet<_>>().into_iter().collect();
    findings.push(Finding {
        code: format!("rail_state_{state}"),
        subject: declared.rail_id.clone(),
        detail: format!(
            "generic currentness state {state} with result {}",
            rail_result_name(&result)
        ),
    });
    StatusRail {
        rail,
        currentness_state: state.to_owned(),
        source_result: None,
        adapter_id,
        packet_id: None,
        owner: String::new(),
        wake_event: None,
        blockers,
        max_permitted_wording: max_permitted_wording(&result, &declared.claim_ceiling),
        history,
        support_authorized: false,
        release_authorized: false,
        published_channels: Vec::new(),
    }
}

/// Outcome of resolving the current source set of one rail.
enum CurrentResolution<'a> {
    /// Unresolved dual current authority (or changed bytes under one
    /// identity): conflict truth with a typed detail line.
    Conflict { detail: String, findings: Vec<Finding> },
    /// Exactly one current source, possibly after declared succession.
    Exact { packet: &'a LoadedPacket, demoted: Vec<&'a LoadedPacket>, findings: Vec<Finding> },
}

/// Resolve the current source set of one rail: byte-identical duplicates
/// deduplicate with every file reference retained; changed bytes under one
/// packet identity are invalid conflict; dual current authority stays
/// conflict unless an exact declared succession resolves exactly two
/// currents.  File order, names, and recency never participate.
fn resolve_current_source<'a>(
    rail_id: &str,
    current: &'a [&'a LoadedPacket],
) -> CurrentResolution<'a> {
    let mut findings = Vec::new();
    let mut deduped: Vec<&LoadedPacket> = Vec::new();
    for packet in current {
        if let Some(existing) = deduped.iter().find(|kept| kept.canonical == packet.canonical) {
            findings.push(Finding {
                code: "duplicate_packet_ref".to_owned(),
                subject: rail_id.to_owned(),
                detail: format!(
                    "files {} and {} hold byte-identical packet {}",
                    existing.file, packet.file, packet.packet.packet_id
                ),
            });
        } else {
            deduped.push(packet);
        }
    }

    let mut identity_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for packet in &deduped {
        *identity_counts.entry(packet.packet.packet_id.as_str()).or_insert(0) += 1;
    }
    let conflict_detail = |count: usize| {
        format!("{count} distinct current packets for one rail remain unresolved dual authority")
    };

    if identity_counts.values().any(|count| *count > 1) {
        findings.push(Finding {
            code: "changed_byte_identity".to_owned(),
            subject: rail_id.to_owned(),
            detail: "the same packet id appears with different canonical bytes".to_owned(),
        });
        return CurrentResolution::Conflict { detail: conflict_detail(deduped.len()), findings };
    }

    if deduped.len() == 1 {
        return CurrentResolution::Exact { packet: deduped[0], demoted: Vec::new(), findings };
    }

    if deduped.len() == 2 {
        let (a, b) = (deduped[0], deduped[1]);
        let a_supersedes_b = a.packet.supersedes.as_deref() == Some(b.packet.packet_id.as_str());
        let b_supersedes_a = b.packet.supersedes.as_deref() == Some(a.packet.packet_id.as_str());
        if a_supersedes_b != b_supersedes_a {
            let (successor, demoted) = if a_supersedes_b { (a, b) } else { (b, a) };
            findings.push(Finding {
                code: "succession_resolved_conflict".to_owned(),
                subject: rail_id.to_owned(),
                detail: format!(
                    "packet {} supersedes current packet {} by declared succession",
                    successor.packet.packet_id, demoted.packet.packet_id
                ),
            });
            return CurrentResolution::Exact {
                packet: successor,
                demoted: vec![demoted],
                findings,
            };
        }
    }

    CurrentResolution::Conflict { detail: conflict_detail(deduped.len()), findings }
}

/// Assemble one declared rail from the exact registered adapter and its
/// repository-local packets.  Never searches for alternative sources,
/// never drops the rail, never reads non-packet input as truth.
fn assemble_rail(declared: &Rail, registry: &Registry, packets: &[LoadedPacket]) -> RailAssembly {
    let mut findings = Vec::new();
    let mut consumed_files = BTreeSet::new();

    let adapters = select_adapter(registry, &declared.source_schema);
    let adapter = match adapters.as_slice() {
        [adapter] => adapter,
        [] => {
            findings.push(Finding {
                code: "adapter_unavailable".to_owned(),
                subject: declared.rail_id.clone(),
                detail: format!(
                    "no registered adapter accepts source schema {}",
                    declared.source_schema
                ),
            });
            let rail = failed_state_rail(
                declared,
                "adapter_unavailable",
                RailResult::Unsupported,
                None,
                &mut findings,
                &[],
            );
            return RailAssembly { rail, findings, consumed_files };
        }
        many => {
            let ids = many.iter().map(|a| a.adapter_id.as_str()).collect::<Vec<_>>().join(", ");
            findings.push(Finding {
                code: "adapter_ambiguous".to_owned(),
                subject: declared.rail_id.clone(),
                detail: format!(
                    "overlapping current adapters [{ids}] accept source schema {} without a declared migration",
                    declared.source_schema
                ),
            });
            let rail = failed_state_rail(
                declared,
                "adapter_unavailable",
                RailResult::Unsupported,
                None,
                &mut findings,
                &[],
            );
            return RailAssembly { rail, findings, consumed_files };
        }
    };

    if !declared.currentness.starts_with(SUPPORTED_RELATION_PREFIX) {
        findings.push(Finding {
            code: "unsupported_currentness_relation".to_owned(),
            subject: declared.rail_id.clone(),
            detail: format!(
                "declared currentness relation `{}` is not implemented by assembly policy {ASSEMBLY_POLICY}",
                declared.currentness
            ),
        });
        let rail = failed_state_rail(
            declared,
            "source_unavailable",
            RailResult::NotProven,
            Some(adapter.adapter_id.clone()),
            &mut findings,
            &[],
        );
        return RailAssembly { rail, findings, consumed_files };
    }

    // Family: packets routed through this adapter (selector + accepted schema).
    let family: Vec<&LoadedPacket> = packets
        .iter()
        .filter(|p| {
            p.packet.selector == adapter.subject_selector
                && adapter.accepted_source_schemas.iter().any(|s| s == &p.packet.schema)
        })
        .collect();
    if family.is_empty() {
        findings.push(Finding {
            code: "source_unavailable".to_owned(),
            subject: declared.rail_id.clone(),
            detail: format!(
                "no repository-local packet resolves for adapter {} (selector `{}`)",
                adapter.adapter_id, adapter.subject_selector
            ),
        });
        let rail = failed_state_rail(
            declared,
            "source_unavailable",
            RailResult::NotProven,
            Some(adapter.adapter_id.clone()),
            &mut findings,
            &[],
        );
        return RailAssembly { rail, findings, consumed_files };
    }
    // Rail-attributed packets: same rail identity.  A packet for another
    // rail is never this rail's source, however attractive its result.
    // Only rail-attributed files count as consumed; family packets that no
    // declared rail claims stay visible through global findings.
    let rail_packets: Vec<&LoadedPacket> =
        family.into_iter().filter(|p| p.packet.rail_id == declared.rail_id).collect();
    if rail_packets.is_empty() {
        findings.push(Finding {
            code: "source_subject_missing".to_owned(),
            subject: declared.rail_id.clone(),
            detail: format!(
                "source family resolves but no packet declares rail {}",
                declared.rail_id
            ),
        });
        let rail = failed_state_rail(
            declared,
            "source_subject_missing",
            RailResult::NotProven,
            Some(adapter.adapter_id.clone()),
            &mut findings,
            &[],
        );
        return RailAssembly { rail, findings, consumed_files };
    }
    for packet in &rail_packets {
        consumed_files.insert(packet.file.clone());
    }

    // Exact subject matches; same-name different-subject packets are
    // another subject, never a substitute.
    let (subject_packets, mismatches): (Vec<&LoadedPacket>, Vec<&LoadedPacket>) =
        rail_packets.into_iter().partition(|p| p.packet.subject == declared.subject);
    for mismatch in &mismatches {
        findings.push(Finding {
            code: "source_subject_mismatch".to_owned(),
            subject: declared.rail_id.clone(),
            detail: format!(
                "packet {} claims rail {} but declares subject `{}` (rail subject `{}`)",
                bounded_identity(&mismatch.packet.packet_id),
                declared.rail_id,
                bounded_identity(&mismatch.packet.subject),
                bounded_identity(&declared.subject)
            ),
        });
    }
    if subject_packets.is_empty() {
        let rail = failed_state_rail(
            declared,
            "source_subject_mismatch",
            RailResult::Invalid,
            Some(adapter.adapter_id.clone()),
            &mut findings,
            &[],
        );
        return RailAssembly { rail, findings, consumed_files };
    }

    // Semantic/privacy validation of the rail's own packets.
    let mut valid: Vec<&LoadedPacket> = Vec::new();
    for packet in subject_packets {
        if let Some(reason) = &packet.invalid_reason {
            findings.push(Finding {
                code: "invalid_packet".to_owned(),
                subject: declared.rail_id.clone(),
                detail: format!("packet {}: {reason}", packet.packet.packet_id),
            });
        } else {
            valid.push(packet);
        }
    }
    if valid.is_empty() {
        let rail = failed_state_rail(
            declared,
            "invalid_only",
            RailResult::Invalid,
            Some(adapter.adapter_id.clone()),
            &mut findings,
            &[],
        );
        return RailAssembly { rail, findings, consumed_files };
    }

    // Partition by declared self-state.
    let current: Vec<&LoadedPacket> =
        valid.iter().copied().filter(|p| p.packet.state == PacketState::Current).collect();
    let historical: Vec<&LoadedPacket> =
        valid.iter().copied().filter(|p| p.packet.state == PacketState::Historical).collect();
    let stale: Vec<&LoadedPacket> =
        valid.iter().copied().filter(|p| p.packet.state == PacketState::Stale).collect();

    if current.is_empty() {
        let (state, result) = if !stale.is_empty() && historical.is_empty() {
            ("stale_only", RailResult::Stale)
        } else if !historical.is_empty() && stale.is_empty() {
            ("historical_only", RailResult::NoCurrentSource)
        } else {
            ("no_current_source", RailResult::NoCurrentSource)
        };
        let rail = failed_state_rail(
            declared,
            state,
            result,
            Some(adapter.adapter_id.clone()),
            &mut findings,
            &valid,
        );
        return RailAssembly { rail, findings, consumed_files };
    }

    let (packet, demoted) = match resolve_current_source(&declared.rail_id, &current) {
        CurrentResolution::Exact { packet, demoted, findings: extra } => {
            findings.extend(extra);
            (packet, demoted)
        }
        CurrentResolution::Conflict { detail, findings: extra } => {
            findings.extend(extra);
            findings.push(Finding {
                code: "conflicting_current_sources".to_owned(),
                subject: declared.rail_id.clone(),
                detail,
            });
            let rail = failed_state_rail(
                declared,
                "conflicting_current_sources",
                RailResult::ConflictingCurrentSources,
                Some(adapter.adapter_id.clone()),
                &mut findings,
                &valid,
            );
            return RailAssembly { rail, findings, consumed_files };
        }
    };

    // current_exact: exactly one current source after dedup and any
    // declared succession.  Adaptation preserves or narrows, never
    // strengthens: the registry-declared result is not evidence.
    let source_result = packet.packet.source_result.clone();

    let mut limitations = declared.limitations.clone();
    limitations.extend(packet.packet.limitations.iter().cloned());
    sorted_dedup(&mut limitations);
    let mut nonclaims = declared.nonclaims.clone();
    nonclaims.extend(packet.packet.nonclaims.iter().cloned());
    sorted_dedup(&mut nonclaims);

    let result = match &source_result {
        RailResult::Pass if !limitations.is_empty() => {
            findings.push(Finding {
                code: "result_narrowed_by_source_limitations".to_owned(),
                subject: declared.rail_id.clone(),
                detail:
                    "declared or source limitations narrow a plain pass; adaptation may only narrow"
                        .to_owned(),
            });
            RailResult::PassWithDeclaredLimitations
        }
        RailResult::PassWithDeclaredLimitations if limitations.is_empty() => {
            findings.push(Finding {
                code: "limited_pass_without_limitation".to_owned(),
                subject: declared.rail_id.clone(),
                detail: "source reports a limited pass with no declared limitation; adaptation is invalid"
                    .to_owned(),
            });
            RailResult::Invalid
        }
        other => other.clone(),
    };

    let mut rail = base_rail(declared);
    rail.result = result.clone();
    rail.limitations = limitations;
    rail.nonclaims = nonclaims;
    rail.source_digest = packet.packet.digest.clone();

    let mut source_detail = declared.source_detail.clone();
    source_detail.insert("packet_file".to_owned(), packet.file.clone());
    for (key, value) in &packet.packet.detail {
        source_detail.insert(key.clone(), value.clone());
    }
    rail.source_detail = source_detail;

    if declared.result != result {
        findings.push(Finding {
            code: "declared_result_differs".to_owned(),
            subject: declared.rail_id.clone(),
            detail: format!(
                "registry declared {} but assembled evidence yields {}; registry declaration is not evidence",
                rail_result_name(&declared.result),
                rail_result_name(&result)
            ),
        });
    }

    let owner = rail.source_detail.get("owner").cloned().unwrap_or_default();
    let wake_event = rail.source_detail.get("wake_event").cloned();

    let successors = successors_of(&valid);
    let mut history = history_refs(&historical, &successors, None);
    history.extend(history_refs(&stale, &successors, None));
    history.extend(history_refs(&demoted, &successors, Some(packet.packet.packet_id.as_str())));
    history.sort_by(|a, b| a.packet_id.cmp(&b.packet_id).then(a.digest.cmp(&b.digest)));

    let blockers: Vec<String> =
        findings.iter().map(|f| f.code.clone()).collect::<BTreeSet<_>>().into_iter().collect();

    let assembled = StatusRail {
        rail,
        currentness_state: "current_exact".to_owned(),
        source_result: Some(source_result),
        adapter_id: Some(adapter.adapter_id.clone()),
        packet_id: Some(packet.packet.packet_id.clone()),
        owner,
        wake_event,
        blockers,
        max_permitted_wording: max_permitted_wording(&result, &declared.claim_ceiling),
        history,
        support_authorized: false,
        release_authorized: false,
        published_channels: Vec::new(),
    };
    RailAssembly { rail: assembled, findings, consumed_files }
}

fn rollup_from_rails(rails: &[StatusRail]) -> Rollup {
    let mut rollup = Rollup::default();
    for rail in rails {
        match rail.rail.applicability {
            Applicability::Required => {
                if SATISFYING_RESULTS.contains(&rail.rail.result) {
                    rollup.required_satisfied.push(rail.rail.rail_id.clone());
                } else {
                    rollup.required_nonsatisfying.push(RequiredNonSatisfying {
                        rail_id: rail.rail.rail_id.clone(),
                        currentness_state: rail.currentness_state.clone(),
                        result: rail_result_name(&rail.rail.result),
                    });
                }
            }
            Applicability::Conditional | Applicability::Optional => {
                if rail.currentness_state != "current_exact" {
                    rollup.optional_conditional_not_selected.push(rail.rail.rail_id.clone());
                }
            }
            Applicability::NotApplicable => {}
        }
        if rail.currentness_state == "adapter_unavailable" {
            rollup.source_adapters_unavailable.push(rail.rail.rail_id.clone());
        }
        if rail.currentness_state == "conflicting_current_sources" {
            rollup.current_source_conflicts.push(rail.rail.rail_id.clone());
        }
    }
    rollup
}

/// Assemble the immutable machine status from a validated registry and
/// loaded repository-local packets.  Deterministic: independent of registry
/// registration order, packet file order, and completion order.
pub fn assemble(
    registry: &Registry,
    packets: &[LoadedPacket],
    unparseable: &[UnparseablePacket],
) -> Result<Status> {
    validate_registry(registry)?;
    let registry_digest = sha256_hex(canonical_json(registry)?.as_bytes());

    let mut rails = Vec::new();
    let mut findings = Vec::new();
    let mut consumed_files: BTreeSet<String> = BTreeSet::new();
    let mut declared_rails = registry.rails.clone();
    declared_rails.sort_by(|a, b| a.rail_id.cmp(&b.rail_id));
    for declared in &declared_rails {
        let assembly = assemble_rail(declared, registry, packets);
        consumed_files.extend(assembly.consumed_files);
        findings.extend(assembly.findings);
        rails.push(assembly.rail);
    }

    // Global packet findings for files no rail consumed and files that
    // never parsed: typed evidence-integrity failures, never silent skips.
    let accepted_schemas: BTreeSet<&str> = registry
        .adapters
        .iter()
        .flat_map(|a| a.accepted_source_schemas.iter().map(String::as_str))
        .collect();
    for packet in packets {
        if let Some(reason) =
            packet.invalid_reason.as_ref().filter(|_| !consumed_files.contains(&packet.file))
        {
            findings.push(Finding {
                code: "invalid_packet".to_owned(),
                subject: packet.file.clone(),
                detail: format!(
                    "unrouted packet {} ({}) was not consumed: {reason}",
                    bounded_identity(&packet.packet.packet_id),
                    bounded_identity(&packet.packet.schema)
                ),
            });
        }
        if !accepted_schemas.contains(packet.packet.schema.as_str()) {
            findings.push(Finding {
                code: "unknown_source_schema".to_owned(),
                subject: packet.file.clone(),
                detail: format!(
                    "packet {} schema {} is accepted by no registered adapter; never decoded",
                    bounded_identity(&packet.packet.packet_id),
                    bounded_identity(&packet.packet.schema)
                ),
            });
        } else if !consumed_files.contains(&packet.file)
            && !declared_rails.iter().any(|rail| rail.rail_id == packet.packet.rail_id)
        {
            // A routed-shape packet that no declared rail claims never
            // disappears silently.
            findings.push(Finding {
                code: "undeclared_rail_packet".to_owned(),
                subject: packet.file.clone(),
                detail: format!(
                    "packet {} answers undeclared rail {}; no rail consumed it",
                    bounded_identity(&packet.packet.packet_id),
                    bounded_identity(&packet.packet.rail_id)
                ),
            });
        }
    }
    for bad in unparseable {
        findings.push(Finding {
            code: "packet_unparseable".to_owned(),
            subject: bad.file.clone(),
            detail: format!("packet file is not a valid source packet envelope: {}", bad.reason),
        });
    }

    // Dangling declared succession is a typed integrity finding.
    let packet_ids: BTreeSet<&str> = packets.iter().map(|p| p.packet.packet_id.as_str()).collect();
    for packet in packets {
        if let Some(target) =
            packet.packet.supersedes.as_deref().filter(|t| !packet_ids.contains(t))
        {
            findings.push(Finding {
                code: "dangling_supersession".to_owned(),
                subject: packet.packet.packet_id.clone(),
                detail: format!(
                    "declared succession target {target} has no repository-local packet"
                ),
            });
        }
    }

    let mut history = Vec::new();
    for rail in &rails {
        history.extend(rail.history.iter().cloned());
    }
    history.sort_by(|a, b| a.packet_id.cmp(&b.packet_id).then(a.digest.cmp(&b.digest)));
    history.dedup();

    findings.sort_by(|a, b| {
        a.code.cmp(&b.code).then(a.subject.cmp(&b.subject)).then(a.detail.cmp(&b.detail))
    });
    findings.dedup();

    let rollup = rollup_from_rails(&rails);

    // Recorded adapters are normalized exactly like the landed
    // `canonical_json`: sorted by id with sorted, deduplicated accepted
    // source schemas, so equivalent registries yield identical status bytes.
    let mut adapters = registry.adapters.clone();
    adapters.sort_by(|a, b| a.adapter_id.cmp(&b.adapter_id));
    for adapter in &mut adapters {
        adapter.accepted_source_schemas.sort();
        adapter.accepted_source_schemas.dedup();
    }

    let status = Status {
        schema: STATUS_SCHEMA.to_owned(),
        generator: GENERATOR.to_owned(),
        assembly_policy: ASSEMBLY_POLICY.to_owned(),
        registry_schema: registry.schema.clone(),
        registry_digest,
        adapters,
        rails,
        findings,
        history,
        rollup,
        semantic_digest: String::new(),
    };
    let digest = semantic_digest_of(&status)?;
    Ok(Status { semantic_digest: digest, ..status })
}

/// Content digest over the canonical semantic projection (digest field
/// zeroed).  Independent of ordering because every collection is canonical.
fn semantic_digest_of(status: &Status) -> Result<String> {
    let projection = Status { semantic_digest: String::new(), ..status.clone() };
    let bytes =
        serde_json::to_string(&projection).with_context(|| "cannot serialize status projection")?;
    Ok(sha256_hex(bytes.as_bytes()))
}

/// Canonical file bytes of a status snapshot (pretty JSON, newline
/// terminated; deterministic for equal semantic content).
fn status_bytes(status: &Status) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(status).with_context(|| "cannot serialize status")?;
    bytes.push(b'\n');
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Status validation (`check`, and the load path of `show`/`diff`)
// ---------------------------------------------------------------------------

const STATUS_KEYS: &[&str] = &[
    "schema",
    "generator",
    "assembly_policy",
    "registry_schema",
    "registry_digest",
    "adapters",
    "rails",
    "findings",
    "history",
    "rollup",
    "semantic_digest",
];

const STATUS_RAIL_KEYS: &[&str] = &[
    // landed product_health_rail.v1 (flattened, unchanged)
    "schema",
    "rail_id",
    "area",
    "proposition",
    "source_schema",
    "source_digest",
    "subject",
    "currentness",
    "result",
    "applicability",
    "limitations",
    "nonclaims",
    "claim_ceiling",
    "source_detail",
    // assembly projection
    "currentness_state",
    "source_result",
    "adapter_id",
    "packet_id",
    "owner",
    "wake_event",
    "blockers",
    "max_permitted_wording",
    "history",
    "support_authorized",
    "release_authorized",
    "published_channels",
];

fn validate_object_keys(value: &serde_json::Value, expected: &[&str], what: &str) -> Result<()> {
    let Some(object) = value.as_object() else {
        bail!("{what} is not a JSON object");
    };
    let mut actual: BTreeSet<&str> = object.keys().map(String::as_str).collect();
    for key in expected {
        if !actual.remove(key) {
            bail!("{what} is missing key `{key}`");
        }
    }
    if let Some(extra) = actual.iter().next() {
        bail!("{what} has unknown key `{extra}`; the status shape is closed");
    }
    Ok(())
}

/// Full fail-closed validation of one status document, including the
/// #12359 round-trip: every assembled rail, together with the recorded
/// adapters, must satisfy the landed `validate_registry` contract.
pub fn validate_status(status: &Status, raw: &serde_json::Value) -> Result<()> {
    ensure!(status.schema == STATUS_SCHEMA, "unsupported status schema `{}`", status.schema);
    ensure!(
        status.generator == GENERATOR,
        "status generator `{}` is not recognized",
        status.generator
    );
    validate_object_keys(raw, STATUS_KEYS, "status")?;
    let Some(rails_raw) = raw.get("rails").and_then(|r| r.as_array()) else {
        bail!("status rails must be an array");
    };
    for rail in rails_raw {
        validate_object_keys(rail, STATUS_RAIL_KEYS, "status rail")?;
    }
    // The closed shape extends to every nested object: unknown keys inside
    // findings, history, rollup sets, or adapters are unauthenticated
    // bytes, not silently dropped extras.  Validation walks the raw
    // document, since re-serialized parsed values cannot carry them.
    let nested_arrays: [(&str, &[&str]); 4] = [
        ("findings", &["code", "subject", "detail"]),
        ("history", &["packet_id", "digest", "state", "superseded_by"]),
        (
            "adapters",
            &[
                "schema",
                "adapter_id",
                "source_family",
                "accepted_source_schemas",
                "validator_id",
                "subject_selector",
                "currentness_authority",
            ],
        ),
        ("required_nonsatisfying_projection", &["rail_id", "currentness_state", "result"]),
    ];
    for (key, keys) in nested_arrays {
        let source = match key {
            "required_nonsatisfying_projection" => {
                raw.get("rollup").and_then(|r| r.get("required_nonsatisfying"))
            }
            _ => raw.get(key),
        };
        let Some(entries) = source.and_then(|v| v.as_array()) else {
            bail!("status {key} must be an array");
        };
        for entry in entries {
            validate_object_keys(entry, keys, &format!("status {key} entry"))?;
        }
    }
    let Some(rollup_raw) = raw.get("rollup") else {
        bail!("status rollup must be an object");
    };
    validate_object_keys(
        rollup_raw,
        &[
            "required_satisfied",
            "required_nonsatisfying",
            "optional_conditional_not_selected",
            "source_adapters_unavailable",
            "current_source_conflicts",
        ],
        "status rollup",
    )?;

    ensure!(digest_shape_ok(&status.registry_digest), "registry digest is not sha256:<64 hex>");
    ensure!(digest_shape_ok(&status.semantic_digest), "semantic digest is not sha256:<64 hex>");

    let mut ids = BTreeSet::new();
    for rail in &status.rails {
        ensure!(
            ids.insert(rail.rail.rail_id.as_str()),
            "duplicate rail id {} in status",
            rail.rail.rail_id
        );
        ensure!(
            CURRENTNESS_STATES.contains(&rail.currentness_state.as_str()),
            "rail {} has unknown currentness state `{}`",
            rail.rail.rail_id,
            rail.currentness_state
        );
        // State/result coherence: a typed non-current state can never carry
        // a green result or a current source identity, and a current_exact
        // rail must carry its source identity and a source-expressible
        // result that obeys the limitation laws.
        if rail.currentness_state == "current_exact" {
            let Some(source_result) = &rail.source_result else {
                bail!(
                    "rail {} is current_exact without a current source result",
                    rail.rail.rail_id
                );
            };
            ensure!(
                rail.packet_id.is_some() && rail.adapter_id.is_some(),
                "rail {} is current_exact without packet/adapter identity",
                rail.rail.rail_id
            );
            ensure!(
                SOURCE_EXPRESSIBLE_RESULTS.contains(source_result),
                "rail {} carries a non-source result {:?}",
                rail.rail.rail_id,
                source_result
            );
            match rail.rail.result {
                RailResult::Pass => ensure!(
                    rail.rail.limitations.is_empty(),
                    "rail {} is pass with declared limitations; use pass_with_declared_limitations",
                    rail.rail.rail_id
                ),
                RailResult::PassWithDeclaredLimitations => ensure!(
                    !rail.rail.limitations.is_empty(),
                    "rail {} is a limited pass without a declared limitation",
                    rail.rail.rail_id
                ),
                _ => {}
            }
        } else {
            ensure!(
                rail.source_result.is_none() && rail.packet_id.is_none(),
                "rail {} in state {} carries current source identity",
                rail.rail.rail_id,
                rail.currentness_state
            );
            if let Some(expected) = expected_result_for_state(&rail.currentness_state) {
                ensure!(
                    rail.rail.result == expected,
                    "rail {} state {} requires result {}, found {}",
                    rail.rail.rail_id,
                    rail.currentness_state,
                    rail_result_name(&expected),
                    rail_result_name(&rail.rail.result)
                );
            }
        }
        ensure!(
            !rail.support_authorized,
            "rail {} asserts support_authorized; assembly structurally denies it",
            rail.rail.rail_id
        );
        ensure!(
            !rail.release_authorized,
            "rail {} asserts release_authorized; assembly structurally denies it",
            rail.rail.rail_id
        );
        ensure!(
            rail.published_channels.is_empty(),
            "rail {} asserts published channels; assembly structurally denies them",
            rail.rail.rail_id
        );
        for history in &rail.history {
            ensure!(
                digest_shape_ok(&history.digest),
                "rail {} history entry {} has invalid digest",
                rail.rail.rail_id,
                history.packet_id
            );
        }
    }
    let ordered: Vec<&str> = status.rails.iter().map(|r| r.rail.rail_id.as_str()).collect();
    ensure!(
        ordered.windows(2).all(|w| w[0] < w[1]),
        "status rails are not in canonical rail_id order"
    );

    // #12359 round-trip: assembled rails + recorded adapters must satisfy
    // the landed registry validator unchanged.
    let rebuilt = Registry {
        schema: "product_health_rail_registry.v1".to_owned(),
        adapters: status.adapters.clone(),
        rails: status.rails.iter().map(|r| r.rail.clone()).collect(),
    };
    validate_registry(&rebuilt).with_context(
        || "assembled rails do not satisfy the landed product_health_rail_registry.v1 contract",
    )?;

    let recomputed_rollup = rollup_from_rails(&status.rails);
    ensure!(
        recomputed_rollup == status.rollup,
        "rollup does not match the rails; descriptive sets must be exact projections"
    );

    let recomputed_digest = semantic_digest_of(status)?;
    ensure!(
        recomputed_digest == status.semantic_digest,
        "semantic digest does not match status content (rewritten or tampered snapshot)"
    );

    let mut probe = status.findings.clone();
    probe.sort_by(|a, b| {
        a.code.cmp(&b.code).then(a.subject.cmp(&b.subject)).then(a.detail.cmp(&b.detail))
    });
    ensure!(probe == status.findings, "findings are not in canonical order");
    Ok(())
}

fn load_status(path: &Path) -> Result<Status> {
    let bytes = fs::read(path).with_context(|| format!("cannot read status {}", path.display()))?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("status {} is not valid JSON", path.display()))?;
    let status: Status = serde_json::from_value(raw.clone())
        .with_context(|| format!("status {} does not match {STATUS_SCHEMA}", path.display()))?;
    validate_status(&status, &raw)
        .with_context(|| format!("status {} fails {STATUS_SCHEMA} validation", path.display()))?;
    Ok(status)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Subcommand)]
pub enum ProductHealthCommand {
    /// Assemble `product_health_status.v1` from one checked registry and
    /// its repository-local source packets.  Packets resolve from the
    /// `sources/` directory next to the registry file.  The output is an
    /// immutable snapshot: an existing different file is refused.
    Build {
        /// Path to the checked `product_health_rail_registry.v1` document.
        #[arg(long)]
        registry: PathBuf,
        /// Output path for the assembled status snapshot.
        #[arg(long)]
        output: PathBuf,
    },
    /// Fail-closed validation of one status snapshot (closed shape, landed
    /// registry round-trip, authorization constants, rollup and digest
    /// recomputation).
    Check {
        /// Path to a `product_health_status.v1` snapshot.
        status: PathBuf,
    },
    /// Print one rail or the whole status (plain text by default, machine
    /// JSON with `--format json`).  No Markdown is generated.
    Show {
        /// Path to a `product_health_status.v1` snapshot.
        status: PathBuf,
        /// Restrict output to one rail id.
        #[arg(long)]
        rail: Option<String>,
        /// Output format: `text` (default) or `json`.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Compare two status snapshots.  Prints a deterministic machine diff
    /// and exits 1 when they differ semantically (like `diff --exit-code`).
    Diff {
        /// Earlier status snapshot.
        before: PathBuf,
        /// Later status snapshot.
        after: PathBuf,
    },
}

pub fn run(command: ProductHealthCommand) -> Result<()> {
    match command {
        ProductHealthCommand::Build { registry, output } => build_command(&registry, &output),
        ProductHealthCommand::Check { status } => check_command(&status),
        ProductHealthCommand::Show { status, rail, format } => {
            show_command(&status, rail.as_deref(), &format)
        }
        ProductHealthCommand::Diff { before, after } => diff_command(&before, &after),
    }
}

fn build_command(registry_path: &Path, output: &Path) -> Result<()> {
    let registry_bytes = fs::read(registry_path)
        .with_context(|| format!("cannot read registry {}", registry_path.display()))?;
    let registry: Registry = serde_json::from_slice(&registry_bytes).with_context(|| {
        format!("registry {} does not match the landed registry contract", registry_path.display())
    })?;
    validate_registry(&registry)
        .with_context(|| format!("registry {} fails validation", registry_path.display()))?;

    let sources_dir = registry_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.join("sources"))
        .unwrap_or_else(|| PathBuf::from("sources"));
    let (packets, unparseable) = load_packets(&sources_dir)
        .with_context(|| format!("cannot load source packets from {}", sources_dir.display()))?;

    let status = assemble(&registry, &packets, &unparseable)?;
    let bytes = status_bytes(&status)?;

    if output.exists() {
        let existing = fs::read(output)
            .with_context(|| format!("cannot read existing output {}", output.display()))?;
        ensure!(
            existing == bytes,
            "output {} already holds a different immutable snapshot; status snapshots are write-once",
            output.display()
        );
        println!(
            "product-health build: {} is already the current immutable snapshot ({} rails, {} findings)",
            output.display(),
            status.rails.len(),
            status.findings.len()
        );
        return Ok(());
    }
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create output directory {}", parent.display()))?;
    }
    fs::write(output, &bytes)
        .with_context(|| format!("cannot write status {}", output.display()))?;
    println!(
        "product-health build: wrote {} ({} rails, {} findings, semantic digest {})",
        output.display(),
        status.rails.len(),
        status.findings.len(),
        status.semantic_digest
    );
    Ok(())
}

fn check_command(status_path: &Path) -> Result<()> {
    let status = load_status(status_path)?;
    println!(
        "{STATUS_SCHEMA} check passed: {} rails, {} findings, {} history refs, semantic digest {}",
        status.rails.len(),
        status.findings.len(),
        status.history.len(),
        status.semantic_digest
    );
    Ok(())
}

fn show_command(status_path: &Path, rail_id: Option<&str>, format: &str) -> Result<()> {
    let status = load_status(status_path)?;
    let rails: Vec<&StatusRail> = match rail_id {
        Some(id) => {
            let rail = status.rails.iter().find(|r| r.rail.rail_id == id).ok_or_else(|| {
                color_eyre::eyre::eyre!("rail `{id}` is not declared in {}", status_path.display())
            })?;
            vec![rail]
        }
        None => status.rails.iter().collect(),
    };
    match format {
        "json" => {
            let value = if rail_id.is_some() {
                serde_json::to_value(&rails).with_context(|| "cannot serialize rails")?
            } else {
                serde_json::to_value(&status).with_context(|| "cannot serialize status")?
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&value).with_context(|| "cannot render JSON")?
            );
        }
        "text" => {
            for rail in rails {
                let inner = &rail.rail;
                println!("rail {} [{}]", inner.rail_id, inner.area);
                println!("  proposition: {}", inner.proposition);
                println!("  subject: {}", inner.subject);
                println!("  applicability: {}", applicability_name(&inner.applicability));
                println!("  currentness: {}", rail.currentness_state);
                println!(
                    "  source: adapter {} / packet {} / {}",
                    rail.adapter_id.as_deref().unwrap_or("(none)"),
                    rail.packet_id.as_deref().unwrap_or("(none)"),
                    rail.source_result
                        .as_ref()
                        .map(rail_result_name)
                        .unwrap_or_else(|| "(no current source)".to_owned())
                );
                println!("  result: {}", rail_result_name(&inner.result));
                println!(
                    "  limitations: {}",
                    if inner.limitations.is_empty() {
                        "(none)".to_owned()
                    } else {
                        inner.limitations.join("; ")
                    }
                );
                println!(
                    "  nonclaims: {}",
                    if inner.nonclaims.is_empty() {
                        "(none)".to_owned()
                    } else {
                        inner.nonclaims.join("; ")
                    }
                );
                println!("  claim ceiling: {}", inner.claim_ceiling);
                println!("  max permitted wording: {}", rail.max_permitted_wording);
                println!(
                    "  blockers: {}",
                    if rail.blockers.is_empty() {
                        "(none)".to_owned()
                    } else {
                        rail.blockers.join("; ")
                    }
                );
                println!(
                    "  history: {}",
                    if rail.history.is_empty() {
                        "(none)".to_owned()
                    } else {
                        rail.history
                            .iter()
                            .map(|h| format!("{} [{}]", h.packet_id, h.state))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                );
                println!(
                    "  authorization: support_authorized=false release_authorized=false published_channels=[]"
                );
            }
        }
        other => bail!("unknown show format `{other}` (expected text or json)"),
    }
    Ok(())
}

#[derive(Serialize)]
struct DiffEntry {
    rail_id: String,
    field: String,
    before: String,
    after: String,
}

#[derive(Serialize)]
struct DiffReport {
    identical: bool,
    before_digest: String,
    after_digest: String,
    rails_added: Vec<String>,
    rails_removed: Vec<String>,
    changes: Vec<DiffEntry>,
    findings_added: Vec<String>,
    findings_removed: Vec<String>,
}

fn findings_of(status: &Status) -> BTreeSet<String> {
    status.findings.iter().map(|f| format!("{}:{}:{}", f.code, f.subject, f.detail)).collect()
}

/// Deterministic semantic diff of two validated status snapshots.  Pure
/// projection: it mutates nothing and orders nothing by input position.
fn compute_diff(before: &Status, after: &Status) -> DiffReport {
    let before_by_id: BTreeMap<&str, &StatusRail> =
        before.rails.iter().map(|r| (r.rail.rail_id.as_str(), r)).collect();
    let after_by_id: BTreeMap<&str, &StatusRail> =
        after.rails.iter().map(|r| (r.rail.rail_id.as_str(), r)).collect();

    let mut changes = Vec::new();
    for (id, before_rail) in &before_by_id {
        if let Some(after_rail) = after_by_id.get(*id) {
            let pairs: [(&str, String, String); 4] = [
                (
                    "result",
                    rail_result_name(&before_rail.rail.result),
                    rail_result_name(&after_rail.rail.result),
                ),
                (
                    "currentness_state",
                    before_rail.currentness_state.clone(),
                    after_rail.currentness_state.clone(),
                ),
                (
                    "source_digest",
                    before_rail.rail.source_digest.clone(),
                    after_rail.rail.source_digest.clone(),
                ),
                (
                    "packet_id",
                    before_rail.packet_id.clone().unwrap_or_default(),
                    after_rail.packet_id.clone().unwrap_or_default(),
                ),
            ];
            let mut specific_change = false;
            for (field, was, now) in pairs {
                if was != now {
                    specific_change = true;
                    changes.push(DiffEntry {
                        rail_id: (*id).to_owned(),
                        field: field.to_owned(),
                        before: was,
                        after: now,
                    });
                }
            }
            // Any other semantic movement (wording, limitations, history,
            // detail, adapters) still reports one bounded entry so a
            // non-identical diff never exits without a reason.
            if !specific_change && before_rail != after_rail {
                changes.push(DiffEntry {
                    rail_id: (*id).to_owned(),
                    field: "rail_semantics".to_owned(),
                    before: sha256_hex(
                        serde_json::to_string(before_rail).unwrap_or_default().as_bytes(),
                    ),
                    after: sha256_hex(
                        serde_json::to_string(after_rail).unwrap_or_default().as_bytes(),
                    ),
                });
            }
        }
    }

    let before_findings = findings_of(before);
    let after_findings = findings_of(after);
    let identical = before.semantic_digest == after.semantic_digest;
    let rails_added: Vec<String> = after_by_id
        .keys()
        .filter(|id| !before_by_id.contains_key(*id))
        .map(|id| (*id).to_owned())
        .collect();
    let rails_removed: Vec<String> = before_by_id
        .keys()
        .filter(|id| !after_by_id.contains_key(*id))
        .map(|id| (*id).to_owned())
        .collect();
    let findings_added: Vec<String> =
        after_findings.difference(&before_findings).cloned().collect();
    let findings_removed: Vec<String> =
        before_findings.difference(&after_findings).cloned().collect();
    if !identical && changes.is_empty() && rails_added.is_empty() && rails_removed.is_empty() {
        // Status-level movement outside the rail projection (registry
        // identity, adapters) still names itself.
        changes.push(DiffEntry {
            rail_id: "(status)".to_owned(),
            field: "status_semantics".to_owned(),
            before: before.registry_digest.clone(),
            after: after.registry_digest.clone(),
        });
    }
    DiffReport {
        identical,
        before_digest: before.semantic_digest.clone(),
        after_digest: after.semantic_digest.clone(),
        rails_added,
        rails_removed,
        changes,
        findings_added,
        findings_removed,
    }
}

fn diff_command(before_path: &Path, after_path: &Path) -> Result<()> {
    let before = load_status(before_path)?;
    let after = load_status(after_path)?;
    let report = compute_diff(&before, &after);
    println!("{}", serde_json::to_string_pretty(&report).with_context(|| "cannot render diff")?);
    if !report.identical {
        std::process::exit(1);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Committed fixture seam (test golden; no CLI surface)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub const FIXTURE_DIR: &str = "fixtures/product_health_status";

/// Assemble the committed fixture registry exactly as `build` does.
#[cfg(test)]
pub fn assemble_fixture(root: &Path) -> Result<Status> {
    let registry_path = root.join(FIXTURE_DIR).join("registry.json");
    let registry_bytes = fs::read(&registry_path)
        .with_context(|| format!("cannot read fixture registry {}", registry_path.display()))?;
    let registry: Registry = serde_json::from_slice(&registry_bytes)
        .with_context(|| "fixture registry does not match the landed contract")?;
    validate_registry(&registry)?;
    let (packets, unparseable) = load_packets(&root.join(FIXTURE_DIR).join("sources"))?;
    assemble(&registry, &packets, &unparseable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root() -> PathBuf {
        crate::utils::project_root().unwrap()
    }

    fn tempdir() -> PathBuf {
        tempfile::tempdir().unwrap().keep()
    }

    fn sha256_of(text: &str) -> String {
        sha256_hex(text.as_bytes())
    }

    fn packet(
        packet_id: &str,
        rail_id: &str,
        subject: &str,
        result: RailResult,
        state: PacketState,
    ) -> SourcePacket {
        SourcePacket {
            schema: "fixture.v1".to_owned(),
            packet_id: packet_id.to_owned(),
            selector: "fixture.subject".to_owned(),
            rail_id: rail_id.to_owned(),
            subject: subject.to_owned(),
            source_result: result,
            state,
            supersedes: None,
            digest: sha256_of(packet_id),
            limitations: Vec::new(),
            nonclaims: Vec::new(),
            detail: BTreeMap::new(),
        }
    }

    fn loaded(packet: SourcePacket) -> LoadedPacket {
        let canonical = serde_json::to_string(&packet).unwrap();
        let invalid_reason = detail_privacy_reason(&packet);
        LoadedPacket {
            file: format!("{}.json", packet.packet_id),
            packet,
            canonical,
            invalid_reason,
        }
    }

    fn fixture_registry_one_rail(currentness: &str, declared_result: RailResult) -> Registry {
        Registry {
            schema: "product_health_rail_registry.v1".to_owned(),
            adapters: vec![Adapter {
                schema: "product_health_rail_adapter.v1".to_owned(),
                adapter_id: "fixture.adapter".to_owned(),
                source_family: "fixture".to_owned(),
                accepted_source_schemas: vec!["fixture.v1".to_owned()],
                validator_id: "fixture.validator.v1".to_owned(),
                subject_selector: "fixture.subject".to_owned(),
                currentness_authority: "declared-succession".to_owned(),
            }],
            rails: vec![Rail {
                schema: "product_health_rail.v1".to_owned(),
                rail_id: "fixture.parser".to_owned(),
                area: "parser".to_owned(),
                proposition: "fixture parser contract holds".to_owned(),
                source_schema: "fixture.v1".to_owned(),
                source_digest: sha256_of("declared"),
                subject: "fixture-subject".to_owned(),
                currentness: currentness.to_owned(),
                result: declared_result,
                applicability: Applicability::Required,
                limitations: Vec::new(),
                nonclaims: vec!["does not establish release authority".to_owned()],
                claim_ceiling: "fixture parser proposition only".to_owned(),
                source_detail: BTreeMap::new(),
            }],
        }
    }

    // -- determinism / golden -------------------------------------------------

    #[test]
    fn product_health_status_fixture_assembly_is_deterministic_and_matches_golden() {
        let status = assemble_fixture(&root()).unwrap();
        let again = assemble_fixture(&root()).unwrap();
        assert_eq!(status_bytes(&status).unwrap(), status_bytes(&again).unwrap());
        let golden =
            fs::read(root().join(FIXTURE_DIR).join("expected").join("status.json")).unwrap();
        assert_eq!(
            status_bytes(&status).unwrap(),
            golden,
            "committed golden status must equal freshly assembled bytes"
        );
    }

    #[test]
    fn product_health_status_registration_and_file_order_never_change_bytes() {
        let mut registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        registry.adapters.push(Adapter {
            schema: "product_health_rail_adapter.v1".to_owned(),
            adapter_id: "fixture.other.adapter".to_owned(),
            source_family: "other".to_owned(),
            accepted_source_schemas: vec!["other.v1".to_owned()],
            validator_id: "other.validator.v1".to_owned(),
            subject_selector: "other.subject".to_owned(),
            currentness_authority: "declared-succession".to_owned(),
        });
        registry.rails.push(Rail {
            schema: "product_health_rail.v1".to_owned(),
            rail_id: "fixture.other".to_owned(),
            area: "other".to_owned(),
            proposition: "other fixture proposition".to_owned(),
            source_schema: "other.v1".to_owned(),
            source_digest: sha256_of("declared-other"),
            subject: "other-subject".to_owned(),
            currentness: "exact:other".to_owned(),
            result: RailResult::NotProven,
            applicability: Applicability::Optional,
            limitations: Vec::new(),
            nonclaims: vec![],
            claim_ceiling: "other proposition only".to_owned(),
            source_detail: BTreeMap::new(),
        });
        let packets = vec![loaded(packet(
            "p1",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        ))];
        let first = assemble(&registry, &packets, &[]).unwrap();
        let mut reversed = registry.clone();
        reversed.rails.reverse();
        reversed.adapters.reverse();
        let mut shuffled = packets.clone();
        shuffled.reverse();
        let second = assemble(&reversed, &shuffled, &[]).unwrap();
        assert_eq!(first.semantic_digest, second.semantic_digest);
    }

    // -- fail-closed rail retention -------------------------------------------

    #[test]
    fn product_health_status_every_declared_rail_stays_present_with_a_typed_state() {
        let status = assemble_fixture(&root()).unwrap();
        let fixture = fs::read_to_string(root().join(FIXTURE_DIR).join("registry.json")).unwrap();
        let registry: Registry = serde_json::from_str(&fixture).unwrap();
        let declared: BTreeSet<&str> = registry.rails.iter().map(|r| r.rail_id.as_str()).collect();
        let assembled: BTreeSet<&str> =
            status.rails.iter().map(|r| r.rail.rail_id.as_str()).collect();
        assert_eq!(declared, assembled, "every declared rail must remain present");
        for rail in &status.rails {
            assert!(CURRENTNESS_STATES.contains(&rail.currentness_state.as_str()));
        }
    }

    #[test]
    fn product_health_status_no_convenient_source_search_and_no_cross_rail_substitution() {
        let mut registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        registry.rails[0].rail_id = "fixture.a".to_owned();
        let mut rail_b = registry.rails[0].clone();
        rail_b.rail_id = "fixture.b".to_owned();
        rail_b.subject = "subject-b".to_owned();
        rail_b.currentness = "exact:fixture".to_owned();
        registry.rails.push(rail_b);
        let packets = vec![
            loaded(packet(
                "pa",
                "fixture.a",
                "fixture-subject",
                RailResult::Failed,
                PacketState::Current,
            )),
            loaded(packet("pb", "fixture.b", "subject-b", RailResult::Pass, PacketState::Current)),
        ];
        let status = assemble(&registry, &packets, &[]).unwrap();
        let a = status.rails.iter().find(|r| r.rail.rail_id == "fixture.a").unwrap();
        let b = status.rails.iter().find(|r| r.rail.rail_id == "fixture.b").unwrap();
        assert_eq!(a.rail.result, RailResult::Failed);
        assert_eq!(a.currentness_state, "current_exact");
        assert_eq!(b.rail.result, RailResult::Pass);
        assert!(status.rollup.required_nonsatisfying.iter().any(|n| n.rail_id == "fixture.a"));
    }

    #[test]
    fn product_health_status_prose_and_non_packet_files_are_never_truth() {
        let dir = tempdir();
        let sources = dir.join("sources");
        fs::create_dir_all(&sources).unwrap();
        fs::write(sources.join("convenient.md"), "fixture.parser: PASS (authored status document)")
            .unwrap();
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let (packets, unparseable) = load_packets(&sources).unwrap();
        assert!(packets.is_empty());
        assert!(unparseable.is_empty());
        let status = assemble(&registry, &packets, &unparseable).unwrap();
        let rail = &status.rails[0];
        assert_eq!(rail.currentness_state, "source_unavailable");
        assert_eq!(rail.rail.result, RailResult::NotProven);
    }

    #[test]
    fn product_health_status_registry_validation_is_never_bypassed() {
        let mut registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        registry.schema = "product_health_rail_registry.v2".to_owned();
        assert!(assemble(&registry, &[], &[]).is_err());
        let mut duplicate = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        duplicate.rails.push(duplicate.rails[0].clone());
        assert!(assemble(&duplicate, &[], &[]).is_err());
    }

    #[test]
    fn product_health_status_unsupported_currentness_relation_fails_closed() {
        let registry = fixture_registry_one_rail("newest-wins:by-timestamp", RailResult::Pass);
        let passing = packet(
            "p",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        let status = assemble(&registry, &[loaded(passing)], &[]).unwrap();
        let rail = &status.rails[0];
        assert_eq!(rail.currentness_state, "source_unavailable");
        assert_eq!(rail.rail.result, RailResult::NotProven);
        assert!(status.findings.iter().any(|f| f.code == "unsupported_currentness_relation"));
    }

    #[test]
    fn product_health_status_unknown_packet_schema_is_never_decoded_into_a_rail() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let mut alien = packet(
            "alien",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        alien.schema = "fixture.v99".to_owned();
        let status = assemble(&registry, &[loaded(alien)], &[]).unwrap();
        assert_eq!(status.rails[0].currentness_state, "source_unavailable");
        assert!(
            status
                .findings
                .iter()
                .any(|f| f.code == "unknown_source_schema" && f.subject == "alien.json")
        );
        assert_ne!(status.rails[0].rail.result, RailResult::Pass);
    }

    #[test]
    fn product_health_status_neither_newest_nor_green_wins_currentness() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let newest_pass = packet(
            "zzz-newest-pass",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        let older_red = packet(
            "aaa-older-red",
            "fixture.parser",
            "fixture-subject",
            RailResult::Failed,
            PacketState::Current,
        );
        let status = assemble(&registry, &[loaded(newest_pass), loaded(older_red)], &[]).unwrap();
        let rail = &status.rails[0];
        assert_eq!(rail.currentness_state, "conflicting_current_sources");
        assert_eq!(rail.rail.result, RailResult::ConflictingCurrentSources);
    }

    #[test]
    fn product_health_status_same_name_wrong_subject_is_not_this_rails_source() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let wrong_stage = packet(
            "p",
            "fixture.parser",
            "fixture-subject@other-stage",
            RailResult::Pass,
            PacketState::Current,
        );
        let status = assemble(&registry, &[loaded(wrong_stage)], &[]).unwrap();
        let rail = &status.rails[0];
        assert_eq!(rail.currentness_state, "source_subject_mismatch");
        assert_eq!(rail.rail.result, RailResult::Invalid);
        assert!(status.findings.iter().any(|f| f.code == "source_subject_mismatch"));
    }

    #[test]
    fn product_health_status_no_source_backfills_another_rails_dimensions() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let mut rich = packet(
            "rich",
            "fixture.other-rail",
            "other-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        rich.detail.insert("owner".to_owned(), "someone".to_owned());
        rich.limitations.push("bounded by harness".to_owned());
        let bare = packet(
            "bare",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        let status = assemble(&registry, &[loaded(rich), loaded(bare)], &[]).unwrap();
        let rail = &status.rails[0];
        assert_eq!(rail.rail.result, RailResult::Pass);
        assert!(rail.rail.limitations.is_empty(), "another source's limitations must not backfill");
        assert_eq!(rail.owner, "", "another source's owner must not backfill");
    }

    #[test]
    fn product_health_status_historical_pass_cannot_mask_current_red() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::Pass);
        let old_pass = packet(
            "old",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Historical,
        );
        let current_red = packet(
            "now",
            "fixture.parser",
            "fixture-subject",
            RailResult::Failed,
            PacketState::Current,
        );
        let status = assemble(&registry, &[loaded(old_pass), loaded(current_red)], &[]).unwrap();
        let rail = &status.rails[0];
        assert_eq!(rail.currentness_state, "current_exact");
        assert_eq!(rail.rail.result, RailResult::Failed);
        assert_eq!(rail.source_result, Some(RailResult::Failed));
        assert_eq!(rail.history.len(), 1, "historical truth stays visible as history");
        assert_eq!(rail.history[0].state, "historical");
        assert!(
            status.findings.iter().any(|f| f.code == "declared_result_differs"),
            "declared pass must be exposed as non-evidence"
        );
    }

    #[test]
    fn product_health_status_order_never_resolves_a_dual_current_conflict() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let a = packet(
            "a",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        let b = packet(
            "b",
            "fixture.parser",
            "fixture-subject",
            RailResult::Failed,
            PacketState::Current,
        );
        let first = assemble(&registry, &[loaded(a.clone()), loaded(b.clone())], &[]).unwrap();
        let second = assemble(&registry, &[loaded(b), loaded(a)], &[]).unwrap();
        assert_eq!(first.rails[0].currentness_state, "conflicting_current_sources");
        assert_eq!(first.semantic_digest, second.semantic_digest);
    }

    #[test]
    fn product_health_status_declared_succession_resolves_exactly_two_currents() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let mut successor = packet(
            "successor",
            "fixture.parser",
            "fixture-subject",
            RailResult::Failed,
            PacketState::Current,
        );
        successor.supersedes = Some("predecessor".to_owned());
        let predecessor = packet(
            "predecessor",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        let status = assemble(&registry, &[loaded(successor), loaded(predecessor)], &[]).unwrap();
        let rail = &status.rails[0];
        assert_eq!(rail.currentness_state, "current_exact");
        assert_eq!(rail.rail.result, RailResult::Failed);
        assert_eq!(rail.packet_id.as_deref(), Some("successor"));
        assert!(rail.history.iter().any(
            |h| h.packet_id == "predecessor" && h.superseded_by.as_deref() == Some("successor")
        ));
    }

    #[test]
    fn product_health_status_changed_bytes_under_one_identity_are_conflict() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let one = packet(
            "same-id",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        let mut two = one.clone();
        two.source_result = RailResult::Failed;
        let status = assemble(&registry, &[loaded(one), loaded(two)], &[]).unwrap();
        let rail = &status.rails[0];
        assert_eq!(rail.currentness_state, "conflicting_current_sources");
        assert!(status.findings.iter().any(|f| f.code == "changed_byte_identity"));
    }

    #[test]
    fn product_health_status_byte_identical_duplicates_dedupe_retaining_references() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let one = packet(
            "same",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        let two = one.clone();
        let mut first = loaded(one);
        first.file = "a.json".to_owned();
        let mut second = loaded(two);
        second.file = "b.json".to_owned();
        let status = assemble(&registry, &[first, second], &[]).unwrap();
        let rail = &status.rails[0];
        assert_eq!(rail.currentness_state, "current_exact");
        assert_eq!(rail.rail.result, RailResult::Pass);
        assert!(status.findings.iter().any(|f| f.code == "duplicate_packet_ref"
            && f.detail.contains("a.json")
            && f.detail.contains("b.json")));
    }

    #[test]
    fn product_health_status_stale_historical_absent_states_are_typed() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let stale =
            packet("s", "fixture.parser", "fixture-subject", RailResult::Pass, PacketState::Stale);
        let status = assemble(&registry, &[loaded(stale)], &[]).unwrap();
        assert_eq!(status.rails[0].currentness_state, "stale_only");
        assert_eq!(status.rails[0].rail.result, RailResult::Stale);

        let historical = packet(
            "h",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Historical,
        );
        let status = assemble(&registry, &[loaded(historical)], &[]).unwrap();
        assert_eq!(status.rails[0].currentness_state, "historical_only");
        assert_eq!(status.rails[0].rail.result, RailResult::NoCurrentSource);

        // A mixed stale and historical set with no current source is the
        // generic no-current state, not a convenience pick from either.
        let mixed_stale =
            packet("ms", "fixture.parser", "fixture-subject", RailResult::Pass, PacketState::Stale);
        let mixed_historical = packet(
            "mh",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Historical,
        );
        let status =
            assemble(&registry, &[loaded(mixed_stale), loaded(mixed_historical)], &[]).unwrap();
        assert_eq!(status.rails[0].currentness_state, "no_current_source");
        assert_eq!(status.rails[0].rail.result, RailResult::NoCurrentSource);

        // No packet answers the declared rail identity.
        let other = packet(
            "o",
            "fixture.something-else",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        let status = assemble(&registry, &[loaded(other)], &[]).unwrap();
        assert_eq!(status.rails[0].currentness_state, "source_subject_missing");
        assert_eq!(status.rails[0].rail.result, RailResult::NotProven);
    }

    #[test]
    fn product_health_status_adapter_ambiguity_fails_closed() {
        let mut registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        registry.adapters.push(Adapter {
            schema: "product_health_rail_adapter.v1".to_owned(),
            adapter_id: "fixture.overlap".to_owned(),
            source_family: "fixture".to_owned(),
            accepted_source_schemas: vec!["fixture.v1".to_owned()],
            validator_id: "fixture.validator.v1".to_owned(),
            subject_selector: "fixture.subject".to_owned(),
            currentness_authority: "declared-succession".to_owned(),
        });
        let passing = packet(
            "p",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        let status = assemble(&registry, &[loaded(passing)], &[]).unwrap();
        let rail = &status.rails[0];
        assert_eq!(rail.currentness_state, "adapter_unavailable");
        assert_eq!(rail.rail.result, RailResult::Unsupported);
        assert!(status.findings.iter().any(|f| f.code == "adapter_ambiguous"));
    }

    #[test]
    fn product_health_status_invalid_only_and_privacy_failures_fail_closed() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let mut leaky = packet(
            "leak",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        leaky.detail.insert("transcript".to_owned(), "x".repeat(DETAIL_VALUE_BOUND + 1));
        let status = assemble(&registry, &[loaded(leaky)], &[]).unwrap();
        let rail = &status.rails[0];
        assert_eq!(rail.currentness_state, "invalid_only");
        assert_eq!(rail.rail.result, RailResult::Invalid);
        let bytes = serde_json::to_string(&status).unwrap();
        assert!(
            !bytes.contains(&"x".repeat(64)),
            "unbounded private value must not leak into status"
        );

        let mut meta = packet(
            "meta",
            "fixture.parser",
            "fixture-subject",
            RailResult::ConflictingCurrentSources,
            PacketState::Current,
        );
        meta.digest = "not-a-digest".to_owned();
        let status = assemble(&registry, &[loaded(meta)], &[]).unwrap();
        assert_eq!(status.rails[0].currentness_state, "invalid_only");
    }

    #[test]
    fn product_health_status_unknown_envelope_fields_are_not_decoded() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let dir = tempdir().join("sources");
        fs::create_dir_all(&dir).unwrap();
        let mut json = serde_json::to_value(packet(
            "p",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        ))
        .unwrap();
        json.as_object_mut().unwrap().insert("score".to_owned(), serde_json::json!(97));
        fs::write(dir.join("p.json"), serde_json::to_string(&json).unwrap()).unwrap();
        let (packets, unparseable) = load_packets(&dir).unwrap();
        assert!(packets.is_empty());
        assert_eq!(unparseable.len(), 1);
        let status = assemble(&registry, &packets, &unparseable).unwrap();
        assert_eq!(status.rails[0].currentness_state, "source_unavailable");
        assert!(status.findings.iter().any(|f| f.code == "packet_unparseable"));
    }

    #[test]
    fn product_health_status_issue_closure_or_merge_marker_is_not_supersession() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let mut closed = packet(
            "old",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        closed.detail.insert("issue_closed".to_owned(), "#12345".to_owned());
        closed.detail.insert("pr_merged".to_owned(), "#67890".to_owned());
        closed.detail.insert("rerun_requested".to_owned(), "yes".to_owned());
        let mut current_red = packet(
            "now",
            "fixture.parser",
            "fixture-subject",
            RailResult::Failed,
            PacketState::Current,
        );
        current_red.detail.insert("pr_open".to_owned(), "#99999".to_owned());
        // Both still current: closure markers must not demote `old`.
        let status = assemble(&registry, &[loaded(closed), loaded(current_red)], &[]).unwrap();
        assert_eq!(status.rails[0].currentness_state, "conflicting_current_sources");
    }

    // -- authorization, rollup, vocabulary -------------------------------------

    #[test]
    fn product_health_status_authorization_is_structurally_false_and_tamper_detected() {
        let status = assemble_fixture(&root()).unwrap();
        for rail in &status.rails {
            assert!(!rail.support_authorized);
            assert!(!rail.release_authorized);
            assert!(rail.published_channels.is_empty());
        }
        let mut tampered = status.clone();
        tampered.rails[0].support_authorized = true;
        let raw = serde_json::to_value(&tampered).unwrap();
        assert!(validate_status(&tampered, &raw).is_err());

        let mut rewritten = status.clone();
        rewritten.rails[0].rail.result = RailResult::Pass;
        let raw = serde_json::to_value(&rewritten).unwrap();
        assert!(
            validate_status(&rewritten, &raw).is_err(),
            "digest recomputation must catch a rewritten snapshot"
        );

        let mut rollup_edited = status.clone();
        rollup_edited.rollup.required_satisfied.push("phantom".to_owned());
        let raw = serde_json::to_value(&rollup_edited).unwrap();
        assert!(validate_status(&rollup_edited, &raw).is_err());

        let mut wrong_schema = status.clone();
        wrong_schema.schema = "product_health_status.v2".to_owned();
        let raw = serde_json::to_value(&wrong_schema).unwrap();
        assert!(validate_status(&wrong_schema, &raw).is_err());
    }

    #[test]
    fn product_health_status_no_scalar_score_or_global_verdict_exists() {
        let status = assemble_fixture(&root()).unwrap();
        let json = serde_json::to_value(&status).unwrap();
        let text = json.to_string();
        for forbidden in
            ["score", "percentage", "percent", "maturity", "traffic_light", "verdict", "readiness"]
        {
            assert!(!text.contains(forbidden), "status must not contain `{forbidden}`");
        }
        let rollup = json.get("rollup").unwrap().as_object().unwrap();
        for key in rollup.keys() {
            assert!(
                rollup.get(key).unwrap().as_array().is_some(),
                "rollup key `{key}` must be an exact named set, not a scalar"
            );
        }
    }

    #[test]
    fn product_health_status_landed_validators_accept_the_assembled_output() {
        let status = assemble_fixture(&root()).unwrap();
        let rebuilt = Registry {
            schema: "product_health_rail_registry.v1".to_owned(),
            adapters: status.adapters.clone(),
            rails: status.rails.iter().map(|r| r.rail.clone()).collect(),
        };
        validate_registry(&rebuilt)
            .expect("assembled rails must round-trip through #12359 validators");
        let raw = serde_json::to_value(&status).unwrap();
        validate_status(&status, &raw).unwrap();
    }

    // -- immutability -----------------------------------------------------------

    #[test]
    fn product_health_status_build_output_is_write_once() {
        let dir = tempdir();
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let sources = dir.join("sources");
        fs::create_dir_all(&sources).unwrap();
        fs::write(
            sources.join("p.json"),
            serde_json::to_string(&packet(
                "p",
                "fixture.parser",
                "fixture-subject",
                RailResult::Pass,
                PacketState::Current,
            ))
            .unwrap(),
        )
        .unwrap();
        let registry_path = dir.join("registry.json");
        fs::write(&registry_path, serde_json::to_string(&registry).unwrap()).unwrap();
        let output = dir.join("status.json");

        build_command(&registry_path, &output).unwrap();
        build_command(&registry_path, &output).unwrap(); // idempotent identical write

        // A different assembly must not overwrite the immutable snapshot.
        fs::write(
            sources.join("p.json"),
            serde_json::to_string(&packet(
                "p",
                "fixture.parser",
                "fixture-subject",
                RailResult::Failed,
                PacketState::Current,
            ))
            .unwrap(),
        )
        .unwrap();
        assert!(build_command(&registry_path, &output).is_err());
        let stored: Status = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(stored.rails[0].rail.result, RailResult::Pass);
    }

    // -- diff / show ------------------------------------------------------------

    #[test]
    fn product_health_status_diff_reports_only_semantic_changes() {
        let before = assemble_fixture(&root()).unwrap();
        let mut after = before.clone();
        let rail = after.rails.iter_mut().find(|r| r.rail.rail_id == "fixture.compiler").unwrap();
        rail.rail.result = RailResult::Pass;
        after.rollup = rollup_from_rails(&after.rails);
        after.semantic_digest = semantic_digest_of(&after).unwrap();

        let identical = compute_diff(&before, &before);
        assert!(identical.identical);
        assert!(identical.changes.is_empty());

        let report = compute_diff(&before, &after);
        assert!(!report.identical);
        assert!(
            report.changes.iter().any(|c| c.rail_id == "fixture.compiler"
                && c.field == "result"
                && c.after == "pass")
        );
        assert!(report.findings_added.is_empty(), "a rail-result mutation alone adds no finding");
        // A later snapshot that drops a finding reports the removal without
        // mutating either immutable input.
        let mut quiet = after.clone();
        let dropped = quiet.findings.pop().unwrap_or(Finding {
            code: "none".to_owned(),
            subject: String::new(),
            detail: String::new(),
        });
        quiet.semantic_digest = semantic_digest_of(&quiet).unwrap();
        let report = compute_diff(&after, &quiet);
        assert!(
            report
                .findings_removed
                .iter()
                .any(|f| f.starts_with(&format!("{}:{}:", dropped.code, dropped.subject)))
        );
    }

    #[test]
    fn product_health_status_show_projects_only_declared_rails() {
        let dir = tempdir();
        let status = assemble_fixture(&root()).unwrap();
        let path = dir.join("status.json");
        fs::write(&path, status_bytes(&status).unwrap()).unwrap();

        show_command(&path, None, "text").unwrap();
        show_command(&path, Some("fixture.parser"), "json").unwrap();
        assert!(show_command(&path, Some("fixture.missing"), "text").is_err());
        assert!(show_command(&path, None, "markdown").is_err());
    }

    // -- review-repair hardening -----------------------------------------------

    #[test]
    fn product_health_status_registry_limitations_narrow_a_plain_pass() {
        // A registry rail with declared limitations plus a plain-pass
        // packet must assemble to a limited pass: a plain pass with
        // retained limitations would fail the landed validator in `check`.
        let mut registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        registry.rails[0].limitations = vec!["bounded to declared harness".to_owned()];
        let plain = packet(
            "p",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        let status = assemble(&registry, &[loaded(plain)], &[]).unwrap();
        let rail = &status.rails[0];
        assert_eq!(rail.rail.result, RailResult::PassWithDeclaredLimitations);
        assert!(!rail.rail.limitations.is_empty());
        let raw = serde_json::to_value(&status).unwrap();
        validate_status(&status, &raw)
            .expect("assembled snapshot must satisfy its own check after narrowing");
    }

    #[test]
    fn product_health_status_check_rejects_incoherent_state_result_pairs() {
        let status = assemble_fixture(&root()).unwrap();

        // Non-current state paired with a green result, digest recomputed.
        let mut tampered = status.clone();
        let rail = tampered.rails.iter_mut().find(|r| r.rail.rail_id == "fixture.dap").unwrap();
        rail.rail.result = RailResult::Pass;
        tampered.rollup = rollup_from_rails(&tampered.rails);
        tampered.semantic_digest = semantic_digest_of(&tampered).unwrap();
        let raw = serde_json::to_value(&tampered).unwrap();
        assert!(validate_status(&tampered, &raw).is_err(), "stale rail cannot turn green");

        // current_exact stripped of its source identity.
        let mut tampered = status.clone();
        let rail = tampered.rails.iter_mut().find(|r| r.rail.rail_id == "fixture.parser").unwrap();
        rail.source_result = None;
        let raw = serde_json::to_value(&tampered).unwrap();
        assert!(validate_status(&tampered, &raw).is_err(), "current rail needs source result");

        // Non-current rail carrying current source identity.
        let mut tampered = status.clone();
        let rail = tampered.rails.iter_mut().find(|r| r.rail.rail_id == "fixture.dap").unwrap();
        rail.source_result = Some(RailResult::Pass);
        let raw = serde_json::to_value(&tampered).unwrap();
        assert!(validate_status(&tampered, &raw).is_err());
    }

    #[test]
    fn product_health_status_mismatch_finding_is_bounded() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let oversized = "s".repeat(PACKET_IDENTITY_BOUND * 4);
        let wrong =
            packet("p", "fixture.parser", &oversized, RailResult::Pass, PacketState::Current);
        let status = assemble(&registry, &[loaded(wrong)], &[]).unwrap();
        let bytes = serde_json::to_string(&status).unwrap();
        assert!(!bytes.contains(&"s".repeat(PACKET_IDENTITY_BOUND * 2)));
        assert!(
            status
                .findings
                .iter()
                .any(|f| f.code == "source_subject_mismatch" && f.detail.contains("[bounded]"))
        );
    }

    #[test]
    fn product_health_status_undeclared_rail_packet_is_visible() {
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let ghost = packet(
            "ghost",
            "fixture.undeclared-rail",
            "any-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        let real = packet(
            "p",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        );
        let status = assemble(&registry, &[loaded(ghost), loaded(real)], &[]).unwrap();
        assert!(
            status.findings.iter().any(|f| f.code == "undeclared_rail_packet"
                && f.detail.contains("fixture.undeclared-rail"))
        );
    }

    #[test]
    fn product_health_status_history_records_incoming_successor() {
        // Historical A superseded by historical B: A records B, never the
        // reverse reading of its own outgoing declaration.
        let registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        let mut a = packet(
            "a",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Historical,
        );
        a.digest = sha256_of("a");
        let mut b = packet(
            "b",
            "fixture.parser",
            "fixture-subject",
            RailResult::Failed,
            PacketState::Historical,
        );
        b.digest = sha256_of("b");
        b.supersedes = Some("a".to_owned());
        let status = assemble(&registry, &[loaded(a), loaded(b)], &[]).unwrap();
        let history = &status.rails[0].history;
        let a_ref = history.iter().find(|h| h.packet_id == "a").unwrap();
        let b_ref = history.iter().find(|h| h.packet_id == "b").unwrap();
        assert_eq!(a_ref.superseded_by.as_deref(), Some("b"));
        assert_eq!(b_ref.superseded_by, None);

        // Committed fixture: compiler-old was superseded by the current
        // compiler-now packet.
        let fixture = assemble_fixture(&root()).unwrap();
        let compiler = fixture.rails.iter().find(|r| r.rail.rail_id == "fixture.compiler").unwrap();
        assert!(
            compiler.history.iter().any(|h| h.packet_id == "compiler-old"
                && h.superseded_by.as_deref() == Some("compiler-now"))
        );
    }

    #[test]
    fn product_health_status_check_rejects_unknown_nested_keys() {
        let status = assemble_fixture(&root()).unwrap();
        let mut raw = serde_json::to_value(&status).unwrap();
        raw["findings"][0]
            .as_object_mut()
            .unwrap()
            .insert("note".to_owned(), serde_json::json!("unauthenticated"));
        let reparsed: Status = serde_json::from_value(raw.clone()).unwrap();
        assert!(
            validate_status(&reparsed, &raw).is_err(),
            "unknown nested keys are unauthenticated bytes"
        );
    }

    #[test]
    fn product_health_status_diff_reports_non_field_semantic_changes() {
        let before = assemble_fixture(&root()).unwrap();
        let mut after = before.clone();
        let rail = after.rails.iter_mut().find(|r| r.rail.rail_id == "fixture.parser").unwrap();
        rail.rail.claim_ceiling = "narrower ceiling".to_owned();
        after.semantic_digest = semantic_digest_of(&after).unwrap();
        let report = compute_diff(&before, &after);
        assert!(!report.identical);
        assert!(
            report
                .changes
                .iter()
                .any(|c| c.rail_id == "fixture.parser" && c.field == "rail_semantics"),
            "a ceiling-only change must still be named"
        );
    }

    #[test]
    fn product_health_status_equivalent_adapter_schema_order_is_identical() {
        let mut registry = fixture_registry_one_rail("exact:fixture", RailResult::NotProven);
        registry.adapters[0].accepted_source_schemas =
            vec!["fixture.v1".to_owned(), "zeta.v1".to_owned(), "zeta.v1".to_owned()];
        let mut equivalent = registry.clone();
        equivalent.adapters[0].accepted_source_schemas =
            vec!["zeta.v1".to_owned(), "fixture.v1".to_owned()];
        let packets = vec![loaded(packet(
            "p",
            "fixture.parser",
            "fixture-subject",
            RailResult::Pass,
            PacketState::Current,
        ))];
        let first = assemble(&registry, &packets, &[]).unwrap();
        let second = assemble(&equivalent, &packets, &[]).unwrap();
        assert_eq!(first.semantic_digest, second.semantic_digest);
        assert_eq!(first.adapters, second.adapters);
    }
}
