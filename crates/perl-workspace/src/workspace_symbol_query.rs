//! One versioned workspace-symbol query profile and one typed per-key match
//! operation (#10794).
//!
//! This module is the single canonical owner of workspace-symbol query policy
//! above [`perl_symbol`]: raw query → compile once →
//! [`WorkspaceSymbolQueryProfile`] →
//! [`match_searchable_key`] → `None |` [`WorkspaceSymbolMatchEvidence`].
//!
//! # Structural guarantees
//!
//! - A non-match is represented only by [`None`]; no numeric fallback tier can
//!   stand for both a subsequence match and a non-match.
//! - Match evidence is search affinity only. It is not entity identity,
//!   resolution authority, edit authority, source ownership, or final
//!   presentation rank.
//! - The evidence comparator is total and deterministic.
//!
//! # Explicit normalization limits
//!
//! Case folding is exactly [`str::to_lowercase`]. This module adds no NFC,
//! NFKC, accent folding, transliteration, locale-sensitive collation,
//! grapheme segmentation, package canonicalization, sigil/qualification,
//! identifier-boundary, acronym, or multiword behavior (Q02/Q03 own those).

use std::cmp::Ordering;

/// Version of the compiled workspace-symbol query profile contract.
pub const WORKSPACE_SYMBOL_QUERY_PROFILE_VERSION: u32 = 1;

/// Identifier of the admitted normalization/matching policy captured by the
/// profile digest.
pub const WORKSPACE_SYMBOL_QUERY_POLICY_ID: &str =
    "ws-symbol-query/exact-prefix-substring-subsequence.v1";

/// Current short-query threshold: queries shorter than this many *folded*
/// `char`s are ineligible for the loose tiers (substring/subsequence).
///
/// The value originates in `perl-symbol::MIN_LOOSE_MATCH_QUERY_CHARS`
/// (#5335/#5407). The policy itself now lives here; retiring the public
/// lower-crate constant is #9268's cut.
const MIN_LOOSE_MATCH_QUERY_CHARS: usize = 2;

/// Role a caller assigns to a searchable key supplied to
/// [`match_searchable_key`].
///
/// Roles are retained as caller-supplied identity context only; this PR
/// defines no role-specific semantic preferences beyond current behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkspaceSymbolSearchKeyRole {
    /// Bare (unqualified) symbol name key.
    BareName,
    /// Package-qualified name key such as `Package::run`.
    QualifiedName,
    /// Compatibility alias such as legacy `Package'run`.
    CompatibilityAlias,
    /// Generated/framework projection key anchored to a source declaration.
    GeneratedFrameworkProjection,
    /// Any reviewed key role not covered above.
    Other,
}

/// One versioned, provider-neutral compiled workspace-symbol query.
///
/// Compiled once per logical request; every matching path of that request
/// consumes the same instance and the same [`WorkspaceSymbolQueryProfile::digest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSymbolQueryProfile {
    version: u32,
    policy_id: &'static str,
    raw_query: String,
    trimmed_query: String,
    folded_query: String,
    folded_char_count: usize,
    browse: bool,
    loose_tier_eligible: bool,
    digest: u64,
}

impl WorkspaceSymbolQueryProfile {
    /// Compiles one profile from the raw query text.
    ///
    /// Compilation is pure: repeated compilation of equal input yields
    /// identical fields and an identical digest.
    #[must_use]
    pub fn compile(raw_query: &str) -> Self {
        let trimmed_query = raw_query.trim();
        // Current reviewed folded form: plain `to_lowercase`. Length gates are
        // measured on the folded form because lowercasing can lengthen a
        // one-`char` input ('İ' U+0130 folds to "i\u{307}").
        let folded_query = trimmed_query.to_lowercase();
        let folded_char_count = folded_query.chars().count();
        let browse = trimmed_query.is_empty();
        let loose_tier_eligible = folded_char_count >= MIN_LOOSE_MATCH_QUERY_CHARS;
        let digest = derive_digest(WORKSPACE_SYMBOL_QUERY_POLICY_ID, &folded_query);
        Self {
            version: WORKSPACE_SYMBOL_QUERY_PROFILE_VERSION,
            policy_id: WORKSPACE_SYMBOL_QUERY_POLICY_ID,
            raw_query: raw_query.to_string(),
            trimmed_query: trimmed_query.to_string(),
            folded_query,
            folded_char_count,
            browse,
            loose_tier_eligible,
            digest,
        }
    }

    /// Profile/schema version.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Normalization-policy identifier captured by the digest.
    #[must_use]
    pub const fn policy_id(&self) -> &'static str {
        self.policy_id
    }

    /// Raw query exactly as received.
    #[must_use]
    pub fn raw_query(&self) -> &str {
        &self.raw_query
    }

    /// Trimmed query.
    #[must_use]
    pub fn trimmed_query(&self) -> &str {
        &self.trimmed_query
    }

    /// Current reviewed folded form (`to_lowercase` of the trimmed query).
    #[must_use]
    pub fn folded_query(&self) -> &str {
        &self.folded_query
    }

    /// `char` count of the folded form.
    #[must_use]
    pub const fn folded_char_count(&self) -> usize {
        self.folded_char_count
    }

    /// Empty/whitespace browse disposition: the query admits everything.
    #[must_use]
    pub const fn is_browse(&self) -> bool {
        self.browse
    }

    /// Whether the loose tiers (substring/subsequence) are eligible for this
    /// query under the current short-query threshold.
    #[must_use]
    pub const fn loose_tier_eligible(&self) -> bool {
        self.loose_tier_eligible
    }

    /// Semantic digest/fingerprint of every admitted query-policy
    /// proposition (version, policy id, folded bytes, dispositions).
    ///
    /// The digest changes whenever any admitted proposition changes, so stale
    /// accelerated evidence can be detected structurally.
    #[must_use]
    pub const fn digest(&self) -> u64 {
        self.digest
    }
}

/// Deterministic FNV-1a64 digest over the admitted policy propositions.
///
/// Process-stable: no randomized hashing participates.
const fn derive_digest(policy_id: &str, folded_query: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let version_bytes = WORKSPACE_SYMBOL_QUERY_PROFILE_VERSION.to_le_bytes();
    let mut i = 0;
    while i < version_bytes.len() {
        hash = (hash ^ (version_bytes[i] as u64)).wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    hash = (hash ^ 0xff).wrapping_mul(0x0000_0100_0000_01b3);
    let policy_bytes = policy_id.as_bytes();
    i = 0;
    while i < policy_bytes.len() {
        hash = (hash ^ (policy_bytes[i] as u64)).wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    hash = (hash ^ 0xff).wrapping_mul(0x0000_0100_0000_01b3);
    let folded_bytes = folded_query.as_bytes();
    i = 0;
    while i < folded_bytes.len() {
        hash = (hash ^ (folded_bytes[i] as u64)).wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    hash
}

/// Tier of an admitted match, ordered best-first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorkspaceSymbolMatchTier {
    /// Folded key equals the folded query.
    Exact,
    /// Folded key starts with the folded query.
    Prefix,
    /// Folded key contains the folded query.
    Substring,
    /// Folded query appears in the folded key in order (fuzzy).
    Subsequence,
}

/// Typed per-searchable-key match evidence returned by
/// [`match_searchable_key`].
///
/// Search affinity only: never entity identity, resolution authority, edit
/// authority, source ownership, or final presentation rank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSymbolMatchEvidence {
    profile_version: u32,
    profile_digest: u64,
    searchable_key: String,
    key_role: WorkspaceSymbolSearchKeyRole,
    tier: WorkspaceSymbolMatchTier,
    /// Char indices into the folded key at which folded-query chars matched.
    /// Contiguous for exact/prefix/substring; gapped for subsequence.
    matched_positions: Vec<u32>,
}

impl WorkspaceSymbolMatchEvidence {
    /// Profile version the evidence was produced under.
    #[must_use]
    pub const fn profile_version(&self) -> u32 {
        self.profile_version
    }

    /// Profile digest the evidence was produced under.
    #[must_use]
    pub const fn profile_digest(&self) -> u64 {
        self.profile_digest
    }

    /// The searchable key this evidence was computed against.
    #[must_use]
    pub fn searchable_key(&self) -> &str {
        &self.searchable_key
    }

    /// Caller-supplied key role.
    #[must_use]
    pub const fn key_role(&self) -> WorkspaceSymbolSearchKeyRole {
        self.key_role
    }

    /// Admitted tier.
    #[must_use]
    pub const fn tier(&self) -> WorkspaceSymbolMatchTier {
        self.tier
    }

    /// Matched char-index positions/run/gap evidence within the folded key.
    #[must_use]
    pub fn matched_positions(&self) -> &[u32] {
        &self.matched_positions
    }

    /// Total deterministic intrinsic comparison between two evidence values
    /// from the same profile generation.
    ///
    /// Ordering (best first): tier, then shorter raw key, then raw
    /// lexicographic key order. Total because it compares a lexicographic
    /// tuple of total keys.
    ///
    /// Non-matches do not participate: they are [`None`] at the API boundary
    /// and never sort as matched rows.
    #[must_use]
    pub fn compare(&self, other: &Self) -> Ordering {
        self.tier
            .cmp(&other.tier)
            .then_with(|| self.searchable_key.len().cmp(&other.searchable_key.len()))
            .then_with(|| self.searchable_key.cmp(&other.searchable_key))
    }
}

/// Outcome of validating precomputed candidate evidence against the request's
/// compiled profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateEvidenceValidation {
    /// Evidence was produced by this exact profile generation.
    Current,
    /// Evidence was produced by a different profile/policy generation and
    /// cannot prove any tier — exactness included.
    ProfileMismatch {
        /// Version recorded by the stale evidence.
        evidence_version: u32,
        /// Digest recorded by the stale evidence.
        evidence_digest: u64,
    },
}

/// Validates accelerated/precomputed candidate evidence against the request
/// profile.
///
/// Mismatched evidence is a typed refusal. It must never be downgraded to a
/// weaker tier nor treated as a complete/proven-superset answer (#10794;
/// #10645 consumes this).
#[must_use]
pub fn validate_candidate_evidence(
    profile: &WorkspaceSymbolQueryProfile,
    evidence: &WorkspaceSymbolMatchEvidence,
) -> CandidateEvidenceValidation {
    if evidence.profile_version == profile.version && evidence.profile_digest == profile.digest() {
        CandidateEvidenceValidation::Current
    } else {
        CandidateEvidenceValidation::ProfileMismatch {
            evidence_version: evidence.profile_version,
            evidence_digest: evidence.profile_digest,
        }
    }
}

/// The one per-searchable-key admission operation.
///
/// Returns the typed evidence for an admitted key, or [`None`] — the only
/// non-match representation.
///
/// Membership replicates the current reviewed behavior exactly:
/// browse queries admit every key (presented at the prefix slot, which is how
/// every current consumer ranks them); otherwise exact/prefix always run and
/// substring/subsequence require loose-tier eligibility measured on the
/// folded query (`char` count ≥ 2, where 'İ' folds to two chars).
#[must_use]
pub fn match_searchable_key(
    profile: &WorkspaceSymbolQueryProfile,
    searchable_key: &str,
    key_role: WorkspaceSymbolSearchKeyRole,
) -> Option<WorkspaceSymbolMatchEvidence> {
    let make = |tier, positions| WorkspaceSymbolMatchEvidence {
        profile_version: profile.version,
        profile_digest: profile.digest(),
        searchable_key: searchable_key.to_string(),
        key_role,
        tier,
        matched_positions: positions,
    };

    let key_folded = searchable_key.to_lowercase();

    // Browse disposition: admit everything. Every current consumer ranks
    // browse rows through the prefix slot (`starts_with("")`), with an empty
    // key taking the exact slot via equality; reproduce that exactly.
    if profile.is_browse() {
        return Some(if key_folded.is_empty() {
            make(WorkspaceSymbolMatchTier::Exact, Vec::new())
        } else {
            make(WorkspaceSymbolMatchTier::Prefix, Vec::new())
        });
    }

    let query_chars: Vec<char> = profile.folded_query.chars().collect();

    if key_folded == profile.folded_query {
        let positions = (0..u32::try_from(query_chars.len()).unwrap_or(u32::MAX)).collect();
        return Some(make(WorkspaceSymbolMatchTier::Exact, positions));
    }

    if key_folded.starts_with(&profile.folded_query) {
        let positions = (0..u32::try_from(query_chars.len()).unwrap_or(u32::MAX)).collect();
        return Some(make(WorkspaceSymbolMatchTier::Prefix, positions));
    }

    if !profile.loose_tier_eligible {
        return None;
    }

    let key_chars: Vec<char> = key_folded.chars().collect();
    if let Some(start) = find_substring_position(&key_chars, &query_chars) {
        let positions: Vec<u32> = (start..start + query_chars.len() as u32).collect();
        return Some(make(WorkspaceSymbolMatchTier::Substring, positions));
    }

    subsequence_positions(&key_chars, &query_chars)
        .map(|positions| make(WorkspaceSymbolMatchTier::Subsequence, positions))
}

fn find_substring_position(key_chars: &[char], query_chars: &[char]) -> Option<u32> {
    if query_chars.is_empty() || query_chars.len() > key_chars.len() {
        return None;
    }
    (0..=key_chars.len() - query_chars.len())
        .find(|&start| key_chars[start..start + query_chars.len()] == *query_chars)
        .and_then(|start| u32::try_from(start).ok())
}

/// Returns query-char positions inside the key when the folded query is a
/// subsequence of the folded key; `None` otherwise (the only non-match).
fn subsequence_positions(key_chars: &[char], query_chars: &[char]) -> Option<Vec<u32>> {
    let mut positions = Vec::with_capacity(query_chars.len());
    let mut cursor = 0usize;
    for &target in query_chars {
        let found = key_chars[cursor..].iter().position(|&c| c == target)?;
        let absolute = cursor + found;
        positions.push(u32::try_from(absolute).ok()?);
        cursor = absolute + 1;
    }
    Some(positions)
}

/// Bounded work counters for one logical request.
///
/// Absent instrumentation is reported as `not_proven` (`None`), never zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSymbolQueryWorkReceipt {
    /// Number of profiles compiled for the logical request (target: 1).
    pub profiles_compiled_per_request: Option<u64>,
    /// Searchable keys examined.
    pub keys_examined: Option<u64>,
    /// Matches admitted per tier.
    pub matches_by_tier: Option<[u64; 4]>,
    /// Keys rejected as non-matches.
    pub nonmatches: Option<u64>,
    /// Whether full and accelerated paths consumed equal profile digests.
    /// `None` when the comparison was not instrumented.
    pub full_vs_accelerated_profile_equal: Option<bool>,
    /// Accelerated candidates refused for profile mismatch.
    pub profile_mismatch_rejections: Option<u64>,
}

impl WorkspaceSymbolQueryWorkReceipt {
    /// Receipt with every counter unset (`not_proven`).
    #[must_use]
    pub const fn unproven() -> Self {
        Self {
            profiles_compiled_per_request: None,
            keys_examined: None,
            matches_by_tier: None,
            nonmatches: None,
            full_vs_accelerated_profile_equal: None,
            profile_mismatch_rejections: None,
        }
    }
}

/// Accumulates work counters while a request matches keys.
#[derive(Debug)]
pub struct WorkspaceSymbolQueryWork {
    profile_digest: u64,
    keys_examined: u64,
    matches_by_tier: [u64; 4],
    nonmatches: u64,
}

impl WorkspaceSymbolQueryWork {
    /// Starts counting for one logical request.
    #[must_use]
    pub fn start(profile: &WorkspaceSymbolQueryProfile) -> Self {
        Self {
            profile_digest: profile.digest(),
            keys_examined: 0,
            matches_by_tier: [0; 4],
            nonmatches: 0,
        }
    }

    /// Records one admission outcome.
    pub fn record(&mut self, outcome: Option<&WorkspaceSymbolMatchEvidence>) {
        self.keys_examined = self.keys_examined.saturating_add(1);
        match outcome {
            Some(evidence) => {
                debug_assert_eq!(evidence.profile_digest(), self.profile_digest);
                self.matches_by_tier[tier_slot(evidence.tier())] =
                    self.matches_by_tier[tier_slot(evidence.tier())].saturating_add(1);
            }
            None => self.nonmatches = self.nonmatches.saturating_add(1),
        }
    }

    /// Records one profile-mismatch refusal.
    pub fn record_profile_mismatch(&mut self) {
        self.nonmatches = self.nonmatches.saturating_add(1);
    }

    /// Snapshot receipt; unset fields stay `not_proven`.
    #[must_use]
    pub fn receipt(&self, profiles_compiled_per_request: u64) -> WorkspaceSymbolQueryWorkReceipt {
        WorkspaceSymbolQueryWorkReceipt {
            profiles_compiled_per_request: Some(profiles_compiled_per_request),
            keys_examined: Some(self.keys_examined),
            matches_by_tier: Some(self.matches_by_tier),
            nonmatches: Some(self.nonmatches),
            full_vs_accelerated_profile_equal: None,
            profile_mismatch_rejections: None,
        }
    }
}

const fn tier_slot(tier: WorkspaceSymbolMatchTier) -> usize {
    match tier {
        WorkspaceSymbolMatchTier::Exact => 0,
        WorkspaceSymbolMatchTier::Prefix => 1,
        WorkspaceSymbolMatchTier::Substring => 2,
        WorkspaceSymbolMatchTier::Subsequence => 3,
    }
}

/// Legacy aggregation rank used by the live index path
/// (`exact 3 > prefix/substring 2 > subsequence 1`), expressed as an explicit
/// projection of the typed tier.
///
/// This is not a matcher: admission stays owned by
/// [`match_searchable_key`]. Unifying the two aggregation orders is
/// #10645/#10642 scope; until then both are pinned by fixtures.
#[must_use]
pub const fn legacy_index_match_rank(tier: WorkspaceSymbolMatchTier) -> u8 {
    match tier {
        WorkspaceSymbolMatchTier::Exact => 3,
        WorkspaceSymbolMatchTier::Prefix | WorkspaceSymbolMatchTier::Substring => 2,
        WorkspaceSymbolMatchTier::Subsequence => 1,
    }
}

/// Reviewed tie-breaker order for equal-intrinsic evidence (#10645): lower
/// ordinal wins. This is presentation-role preference only — it never promotes
/// a weaker tier, and key text/tier/positions are already compared by
/// [`WorkspaceSymbolMatchEvidence::compare`] before this ordinal participates.
#[must_use]
pub const fn role_tiebreak_order(role: WorkspaceSymbolSearchKeyRole) -> u8 {
    match role {
        WorkspaceSymbolSearchKeyRole::BareName => 0,
        WorkspaceSymbolSearchKeyRole::QualifiedName => 1,
        WorkspaceSymbolSearchKeyRole::CompatibilityAlias => 2,
        WorkspaceSymbolSearchKeyRole::GeneratedFrameworkProjection => 3,
        WorkspaceSymbolSearchKeyRole::Other => 4,
    }
}

/// Outcome of offering one evidence value to a [`BestRowMatchAccumulator`].
///
/// Exposed so callers can turn accumulation into bounded work evidence without
/// re-deriving comparison outcomes (and therefore without a second comparator).
/// "Stronger" always means the shared best-first ordering ranks it smaller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BestRowMatchUpdate {
    /// First admitted evidence for this row.
    FirstMatch,
    /// Stronger intrinsic evidence replaced the incumbent.
    ReplacedWeaker,
    /// Equal intrinsic evidence; the reviewed role ordinal chose the arrival.
    TieResolvedByRole,
    /// Weaker-than-incumbent arrival retained only as bounded runner-up work.
    KeptIncumbent,
    /// Equal intrinsic evidence with no better role ordinal: idempotent
    /// duplicate/equal loser ignored in favor of the incumbent.
    KeptEqualDuplicate,
    /// Evidence from another profile generation: typed refusal, never mixed
    /// into the row and never downgraded to insertion-order fallback.
    RefusedProfileMismatch,
}

/// One row's best-match accumulator over its admitted searchable keys.
///
/// The single aggregation authority for #10645: it consumes
/// [`WorkspaceSymbolMatchEvidence::compare`] plus [`role_tiebreak_order`] and
/// nothing else. It defines no tiers, no query logic, and no per-profile-version
/// branching, so later reviewed profile versions (#10806/#10827) need no new
/// aggregator. Retains at most winning + one runner-up evidence as bounded
/// alternative/work evidence.
#[derive(Debug)]
pub struct BestRowMatchAccumulator {
    profile_version: u32,
    profile_digest: u64,
    winner: Option<WorkspaceSymbolMatchEvidence>,
    runner_up: Option<WorkspaceSymbolMatchEvidence>,
    considered_matches: u64,
}

impl BestRowMatchAccumulator {
    /// Binds the accumulator to one compiled profile generation. Evidence from
    /// any other generation is refused for the lifetime of this accumulator.
    #[must_use]
    pub fn for_profile(profile: &WorkspaceSymbolQueryProfile) -> Self {
        Self {
            profile_version: profile.version(),
            profile_digest: profile.digest(),
            winner: None,
            runner_up: None,
            considered_matches: 0,
        }
    }

    /// Offers one admitted match evidence to the row. The evidence is cloned
    /// only when retained (winner or bounded runner-up), so examining many
    /// keys per row stays cheap.
    pub fn consider(&mut self, evidence: &WorkspaceSymbolMatchEvidence) -> BestRowMatchUpdate {
        if evidence.profile_version != self.profile_version
            || evidence.profile_digest != self.profile_digest
        {
            return BestRowMatchUpdate::RefusedProfileMismatch;
        }

        let Some(current) = self.winner.as_ref() else {
            self.considered_matches = self.considered_matches.saturating_add(1);
            self.winner = Some(evidence.clone());
            return BestRowMatchUpdate::FirstMatch;
        };

        let update = match evidence.compare(current) {
            // The shared comparator orders best-first: a smaller ordering is
            // the stronger match.
            Ordering::Less => {
                self.runner_up = self.winner.take();
                self.winner = Some(evidence.clone());
                BestRowMatchUpdate::ReplacedWeaker
            }
            Ordering::Greater => {
                let becomes_runner_up = self
                    .runner_up
                    .as_ref()
                    .is_none_or(|runner| evidence.compare(runner) == Ordering::Less);
                if becomes_runner_up {
                    self.runner_up = Some(evidence.clone());
                }
                BestRowMatchUpdate::KeptIncumbent
            }
            Ordering::Equal => {
                if role_tiebreak_order(evidence.key_role())
                    < role_tiebreak_order(current.key_role())
                {
                    self.runner_up = self.winner.take();
                    self.winner = Some(evidence.clone());
                    BestRowMatchUpdate::TieResolvedByRole
                } else {
                    BestRowMatchUpdate::KeptEqualDuplicate
                }
            }
        };

        if !matches!(update, BestRowMatchUpdate::RefusedProfileMismatch) {
            self.considered_matches = self.considered_matches.saturating_add(1);
        }
        update
    }

    /// Number of admitted matches considered for this row (refusals excluded).
    ///
    /// This is the row's distinct matching-key count: every arrival from an
    /// admitted key counts, including arrivals that only became runner-up
    /// work evidence.
    #[must_use]
    pub const fn considered_match_count(&self) -> u64 {
        self.considered_matches
    }

    /// The strongest admitted evidence for this row, if any key matched.
    #[must_use]
    pub const fn winning_evidence(&self) -> Option<&WorkspaceSymbolMatchEvidence> {
        self.winner.as_ref()
    }

    /// Bounded runner-up work evidence (at most one), weaker than the winner.
    #[must_use]
    pub const fn runner_up_evidence(&self) -> Option<&WorkspaceSymbolMatchEvidence> {
        self.runner_up.as_ref()
    }

    /// Consumes the accumulator into the transport-neutral per-row result
    /// handed to the final composer consumer. `None` when no key matched:
    /// such rows are absent, never materialized with fallback evidence.
    #[must_use]
    pub fn into_best_match(self) -> Option<BestWorkspaceSymbolRowMatch> {
        let winner = self.winner?;
        Some(BestWorkspaceSymbolRowMatch {
            profile_version: winner.profile_version,
            profile_digest: winner.profile_digest,
            winning_evidence: winner,
            runner_up_evidence: self.runner_up,
        })
    }
}

/// Transport-neutral per-row result of evaluating every admitted searchable
/// key of one canonical retained workspace-symbol row under one compiled
/// profile (#10645).
///
/// Carries winning key identity/role/tier/positions plus the originating
/// profile version/digest so the downstream cross-row composer can consume the
/// handoff without reconstructing any query logic or LSP types. The row
/// payload/identity itself stays caller-owned: this type is identity-agnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BestWorkspaceSymbolRowMatch {
    profile_version: u32,
    profile_digest: u64,
    winning_evidence: WorkspaceSymbolMatchEvidence,
    runner_up_evidence: Option<WorkspaceSymbolMatchEvidence>,
}

impl BestWorkspaceSymbolRowMatch {
    /// Profile/schema version the winner was produced under.
    #[must_use]
    pub const fn profile_version(&self) -> u32 {
        self.profile_version
    }

    /// Profile digest the winner was produced under.
    #[must_use]
    pub const fn profile_digest(&self) -> u64 {
        self.profile_digest
    }

    /// Winning per-key match evidence (key text, role, tier, positions).
    #[must_use]
    pub const fn winning_evidence(&self) -> &WorkspaceSymbolMatchEvidence {
        &self.winning_evidence
    }

    /// Bounded runner-up work evidence, weaker than the winner.
    #[must_use]
    pub const fn runner_up_evidence(&self) -> Option<&WorkspaceSymbolMatchEvidence> {
        self.runner_up_evidence.as_ref()
    }
}

/// Selects the best match across one row's searchable keys under one profile.
///
/// Convenience fold of [`match_searchable_key`] +
/// [`BestRowMatchAccumulator`] for callers that hold a small explicit key set
/// (for example generated/framework projection keys). Rows whose every key
/// returns `None` yield `None` and stay absent.
#[must_use]
pub fn select_best_row_match(
    profile: &WorkspaceSymbolQueryProfile,
    keys: &[(&str, WorkspaceSymbolSearchKeyRole)],
) -> Option<BestWorkspaceSymbolRowMatch> {
    let mut accumulator = BestRowMatchAccumulator::for_profile(profile);
    for (key, role) in keys {
        if let Some(evidence) = match_searchable_key(profile, key, *role) {
            accumulator.consider(&evidence);
        }
    }
    accumulator.into_best_match()
}

/// Bounded work counters for per-row best-key aggregation (#10645).
///
/// Absent instrumentation is `not_proven` (`None`), never zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSymbolBestKeyReceipt {
    /// Searchable keys examined by the request.
    pub searchable_keys_examined: Option<u64>,
    /// Distinct searchable keys whose admission returned evidence.
    pub matching_keys: Option<u64>,
    /// Canonical retained rows with at least one matching key.
    pub canonical_rows_matched: Option<u64>,
    /// Rows reached through more than one matching key.
    pub rows_with_multiple_matching_keys: Option<u64>,
    /// Times stronger (or role-resolved equal) evidence replaced an incumbent.
    pub better_alias_replacements: Option<u64>,
    /// Equal-evidence encounters resolved by the reviewed tie-breaker or kept
    /// idempotently.
    pub equal_evidence_ties: Option<u64>,
    /// Evidence refused because it came from another profile generation.
    pub profile_mismatch_refusals: Option<u64>,
    /// Rows materialized after per-row selection (cap applied afterwards).
    pub rows_materialized: Option<u64>,
    /// Geometry-based dedup attempts on the canonical path: structurally zero
    /// because aggregation never consults `(uri, byte)` geometry.
    pub geometry_dedup_attempts: Option<u64>,
    /// Stale key contributions rejected during lifecycle replacement. Not yet
    /// instrumented here (`not_proven`); lifecycle retirement is proven by the
    /// index parity/update/remove suites until #10641 supplies row ownership.
    pub stale_key_contributions_rejected: Option<u64>,
}

impl WorkspaceSymbolBestKeyReceipt {
    /// Receipt with every counter unset (`not_proven`).
    #[must_use]
    pub const fn unproven() -> Self {
        Self {
            searchable_keys_examined: None,
            matching_keys: None,
            canonical_rows_matched: None,
            rows_with_multiple_matching_keys: None,
            better_alias_replacements: None,
            equal_evidence_ties: None,
            profile_mismatch_refusals: None,
            rows_materialized: None,
            geometry_dedup_attempts: None,
            stale_key_contributions_rejected: None,
        }
    }
}

/// Accumulates [`WorkspaceSymbolBestKeyReceipt`] counters while one request
/// aggregates rows.
#[derive(Debug, Default)]
pub struct WorkspaceSymbolBestKeyReceiptBuilder {
    keys_examined: u64,
    matching_keys: u64,
    canonical_rows_matched: u64,
    rows_with_multiple_matching_keys: u64,
    better_alias_replacements: u64,
    equal_evidence_ties: u64,
    profile_mismatch_refusals: u64,
    rows_materialized: u64,
}

impl WorkspaceSymbolBestKeyReceiptBuilder {
    /// Records one searchable-key examination (matched or not).
    pub const fn record_key_examined(&mut self) {
        self.keys_examined = self.keys_examined.saturating_add(1);
    }

    /// Records one admission that returned evidence.
    pub const fn record_matching_key(&mut self) {
        self.matching_keys = self.matching_keys.saturating_add(1);
    }

    /// Records one aggregated row and how many distinct keys contributed.
    pub const fn record_row(&mut self, contributing_keys_excluding_first: u64) {
        self.canonical_rows_matched = self.canonical_rows_matched.saturating_add(1);
        if contributing_keys_excluding_first > 0 {
            self.rows_with_multiple_matching_keys =
                self.rows_with_multiple_matching_keys.saturating_add(1);
        }
    }

    /// Records one accumulation outcome.
    pub const fn record_update(&mut self, update: BestRowMatchUpdate) {
        match update {
            BestRowMatchUpdate::FirstMatch | BestRowMatchUpdate::KeptIncumbent => {}
            BestRowMatchUpdate::ReplacedWeaker | BestRowMatchUpdate::TieResolvedByRole => {
                self.better_alias_replacements = self.better_alias_replacements.saturating_add(1);
            }
            BestRowMatchUpdate::KeptEqualDuplicate => {
                self.equal_evidence_ties = self.equal_evidence_ties.saturating_add(1);
            }
            BestRowMatchUpdate::RefusedProfileMismatch => {
                self.profile_mismatch_refusals = self.profile_mismatch_refusals.saturating_add(1);
            }
        }
    }

    /// Records `count` materialized rows (after any caller-side cap).
    pub const fn record_rows_materialized(&mut self, count: u64) {
        self.rows_materialized = self.rows_materialized.saturating_add(count);
    }

    /// Snapshot receipt. Geometry dedup attempts are reported as a structural
    /// zero on this path; lifecycle rejection stays `not_proven`.
    #[must_use]
    pub fn finish(&self) -> WorkspaceSymbolBestKeyReceipt {
        WorkspaceSymbolBestKeyReceipt {
            searchable_keys_examined: Some(self.keys_examined),
            matching_keys: Some(self.matching_keys),
            canonical_rows_matched: Some(self.canonical_rows_matched),
            rows_with_multiple_matching_keys: Some(self.rows_with_multiple_matching_keys),
            better_alias_replacements: Some(self.better_alias_replacements),
            equal_evidence_ties: Some(self.equal_evidence_ties),
            profile_mismatch_refusals: Some(self.profile_mismatch_refusals),
            rows_materialized: Some(self.rows_materialized),
            geometry_dedup_attempts: Some(0),
            stale_key_contributions_rejected: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CandidateEvidenceValidation, WORKSPACE_SYMBOL_QUERY_PROFILE_VERSION,
        WorkspaceSymbolMatchTier as Tier, WorkspaceSymbolQueryProfile,
        WorkspaceSymbolSearchKeyRole as Role, derive_digest, legacy_index_match_rank,
        match_searchable_key, validate_candidate_evidence,
    };
    use proptest::prelude::*;

    fn tier_of(query: &str, key: &str) -> Option<Tier> {
        let profile = WorkspaceSymbolQueryProfile::compile(query);
        match_searchable_key(&profile, key, Role::BareName).map(|e| e.tier())
    }

    /// WS-QP-001: empty query browse admits everything.
    #[test]
    fn ws_qp_001_empty_query_browse_admits_everything() {
        let profile = WorkspaceSymbolQueryProfile::compile("");
        assert!(profile.is_browse());
        assert!(!profile.loose_tier_eligible());
        for key in ["run", "Package::run", "x"] {
            assert!(match_searchable_key(&profile, key, Role::BareName).is_some());
        }
    }

    /// WS-QP-002: whitespace-only query behaves as browse after trim.
    #[test]
    fn ws_qp_002_whitespace_only_query_is_browse() {
        let profile = WorkspaceSymbolQueryProfile::compile("   \t ");
        assert!(profile.is_browse());
        assert!(match_searchable_key(&profile, "anything", Role::BareName).is_some());
        assert_eq!(profile.trimmed_query(), "");
        assert_eq!(profile.raw_query(), "   \t ");
    }

    /// WS-QP-003/004/005: one-character exact and prefix survive; substring
    /// is rejected by the loose gate.
    #[test]
    fn ws_qp_003_004_005_one_char_query_exact_prefix_only() {
        assert_eq!(tier_of("a", "a"), Some(Tier::Exact));
        assert_eq!(tier_of("a", "alpha"), Some(Tier::Prefix));
        assert_eq!(tier_of("l", "alpha"), None);
        assert_eq!(tier_of("z", "az"), None);
    }

    /// WS-QP-006/007: two-char substring and multi-char subsequence are
    /// admitted once loose-eligible.
    #[test]
    fn ws_qp_006_007_loose_tiers_admit_substring_and_subsequence() {
        assert_eq!(tier_of("ph", "alpha"), Some(Tier::Substring));
        assert_eq!(tier_of("fb", "foobar"), Some(Tier::Subsequence));
        assert_eq!(tier_of("nrm", "normalize_me"), Some(Tier::Subsequence));
    }

    /// WS-QP-008: all four tiers are case-insensitive under folding only.
    #[test]
    fn ws_qp_008_case_insensitive_all_tiers() {
        assert_eq!(tier_of("RUN", "run"), Some(Tier::Exact));
        assert_eq!(tier_of("ru", "RunFast"), Some(Tier::Prefix));
        assert_eq!(tier_of("PH", "alPha"), Some(Tier::Substring));
        assert_eq!(tier_of("FB", "FooBar"), Some(Tier::Subsequence));
    }

    /// WS-QP-009: 'İ' (U+0130) lowercases to two chars, keeping the loose
    /// tiers under the current policy.
    #[test]
    fn ws_qp_009_dotted_capital_i_expansion_keeps_loose_tiers() {
        let profile = WorkspaceSymbolQueryProfile::compile("\u{130}");
        assert_eq!(profile.folded_char_count(), 2);
        assert!(profile.loose_tier_eligible());
        assert!(
            match_searchable_key(&profile, "x_i\u{307}_y", Role::BareName).is_some(),
            "folded two-char query keeps substring admission"
        );
    }

    /// WS-QP-010: explicit combining-mark/non-normalization boundary.
    #[test]
    fn ws_qp_010_no_unicode_normalization_combining_mark_boundary() {
        // e + combining acute is NOT folded to é and never equals it.
        let decomposed = "e\u{301}";
        assert_eq!(tier_of("\u{e9}", decomposed), None);
        assert_eq!(tier_of(decomposed, "\u{e9}"), None);
        // NFC/NFKC/accent folding/transliteration/collation stay out.
        assert_eq!(tier_of("cafe\u{301}", "caf\u{e9}"), None);
    }

    /// WS-QP-011: a non-match returns `None`, structurally distinct from any
    /// tier. Mutation M1 (fallback/no-match conflation) fails this case.
    #[test]
    fn ws_qp_011_non_match_is_none_not_a_subsequence_evidence() {
        assert_eq!(tier_of("zq", "alpha"), None);
        assert_eq!(tier_of("xy", "foobar"), None);
        let profile = WorkspaceSymbolQueryProfile::compile("zq");
        let evidence = match_searchable_key(&profile, "alpha", Role::BareName);
        assert!(evidence.is_none(), "no numeric fallback tier may appear");
    }

    /// WS-QP-012: the digest changes when an admitted policy proposition
    /// changes (version, policy id, folded bytes).
    #[test]
    fn ws_qp_012_digest_changes_on_policy_change() {
        let base = derive_digest(super::WORKSPACE_SYMBOL_QUERY_POLICY_ID, "run");
        assert_ne!(
            base,
            derive_digest("ws-symbol-query/exact-prefix-substring-subsequence.v2", "run")
        );
        assert_ne!(base, derive_digest(super::WORKSPACE_SYMBOL_QUERY_POLICY_ID, "runs"));
        // Equal propositions compile to identical bytes/digest.
        let left = WorkspaceSymbolQueryProfile::compile("  Run ");
        let right = WorkspaceSymbolQueryProfile::compile(" Run ");
        assert_eq!(left.digest(), right.digest());
        assert_eq!(left.trimmed_query(), right.trimmed_query());
        assert_eq!(left.folded_query(), right.folded_query());
        assert_eq!(left.loose_tier_eligible(), right.loose_tier_eligible());
        let profile = WorkspaceSymbolQueryProfile::compile("run");
        assert_eq!(profile.version(), WORKSPACE_SYMBOL_QUERY_PROFILE_VERSION);
    }

    /// WS-QP-013: stale/profile-mismatched candidate evidence cannot prove
    /// exactness — validation refuses with a typed mismatch.
    #[test]
    fn ws_qp_013_profile_mismatch_cannot_prove_exactness() {
        let profile = WorkspaceSymbolQueryProfile::compile("run");
        let stale = match_searchable_key(
            &WorkspaceSymbolQueryProfile::compile("ru"),
            "run",
            Role::BareName,
        )
        .expect("control evidence exists");
        match validate_candidate_evidence(&profile, &stale) {
            CandidateEvidenceValidation::ProfileMismatch { .. } => {}
            CandidateEvidenceValidation::Current => {
                panic!("stale evidence must be refused, never accepted as exact")
            }
        }
        let fresh = match_searchable_key(&profile, "run", Role::BareName).expect("admitted");
        assert_eq!(
            validate_candidate_evidence(&profile, &fresh),
            CandidateEvidenceValidation::Current
        );
    }

    /// WS-QP-014: full and accelerated paths of one logical request consume
    /// one digest — the profile compiles once and is passed by reference.
    #[test]
    fn ws_qp_014_full_and_accelerated_paths_share_one_compiled_digest() {
        let profile = WorkspaceSymbolQueryProfile::compile("Proc");
        let full = match_searchable_key(&profile, "process_data", Role::QualifiedName);
        let accelerated = match_searchable_key(&profile, "process_data", Role::BareName);
        assert_eq!(
            full.as_ref().map(|e| e.profile_digest()),
            accelerated.as_ref().map(|e| e.profile_digest())
        );
        assert_eq!(
            full.expect("admitted").compare(&accelerated.expect("admitted")),
            std::cmp::Ordering::Equal
        );
    }

    /// Evidence carries profile identity, key identity, caller-supplied role,
    /// tier, and deterministic position/run/gap payload.
    #[test]
    fn evidence_carries_profile_identity_role_and_positions() {
        let profile = WorkspaceSymbolQueryProfile::compile("log");
        let evidence = match_searchable_key(&profile, "get_log", Role::QualifiedName)
            .expect("substring match");
        assert_eq!(evidence.tier(), Tier::Substring);
        assert_eq!(evidence.profile_version(), 1);
        assert_eq!(evidence.profile_digest(), profile.digest());
        assert_eq!(evidence.searchable_key(), "get_log");
        assert_eq!(evidence.key_role(), Role::QualifiedName);
        assert_eq!(evidence.matched_positions(), &[4, 5, 6]);

        let fuzzy = match_searchable_key(&profile, "lxogap", Role::BareName).expect("subsequence");
        assert_eq!(fuzzy.tier(), Tier::Subsequence);
        assert_eq!(fuzzy.matched_positions(), &[0, 2, 3]);
    }

    /// The comparator is antisymmetric and transitive over generated finite
    /// inputs, and a non-match never sorts as a matched row.
    #[test]
    fn comparator_is_total_deterministic_and_nonmatch_free() {
        let profile = WorkspaceSymbolQueryProfile::compile("ab");
        let prefix = match_searchable_key(&profile, "abc", Role::BareName).unwrap();
        let substring = match_searchable_key(&profile, "zzab", Role::BareName).unwrap();
        let subseq = match_searchable_key(&profile, "aqb", Role::BareName).unwrap();
        assert_eq!(prefix.compare(&substring), std::cmp::Ordering::Less);
        assert_eq!(substring.compare(&prefix), std::cmp::Ordering::Greater);
        assert_eq!(substring.compare(&subseq), std::cmp::Ordering::Less);
        // Legacy index rank projects the merged prefix/substring band.
        assert_eq!(legacy_index_match_rank(Tier::Exact), 3);
        assert_eq!(legacy_index_match_rank(Tier::Prefix), 2);
        assert_eq!(legacy_index_match_rank(Tier::Substring), 2);
        assert_eq!(legacy_index_match_rank(Tier::Subsequence), 1);
    }

    /// Legacy provider parity: tier asc → raw len asc → raw lexicographic.
    #[test]
    fn comparator_matches_legacy_provider_order() {
        let profile = WorkspaceSymbolQueryProfile::compile("foo");
        let exact = match_searchable_key(&profile, "foo", Role::BareName).unwrap();
        let short_prefix = match_searchable_key(&profile, "foobar", Role::BareName).unwrap();
        let long_prefix = match_searchable_key(&profile, "foobarqux", Role::BareName).unwrap();
        assert_eq!(exact.compare(&short_prefix), std::cmp::Ordering::Less);
        assert_eq!(short_prefix.compare(&long_prefix), std::cmp::Ordering::Less);
    }

    /// Feeds `keys` to a fresh accumulator under `profile` in the given order.
    fn accumulate(
        profile: &WorkspaceSymbolQueryProfile,
        keys: &[(&str, Role)],
    ) -> super::BestRowMatchAccumulator {
        let mut accumulator = super::BestRowMatchAccumulator::for_profile(profile);
        for (key, role) in keys {
            if let Some(evidence) = match_searchable_key(profile, key, *role) {
                accumulator.consider(&evidence);
            }
        }
        accumulator
    }

    fn winning_of(
        profile: &WorkspaceSymbolQueryProfile,
        keys: &[(&str, Role)],
    ) -> Option<(String, Tier, Role)> {
        let best = accumulate(profile, keys).into_best_match()?;
        let evidence = best.winning_evidence();
        Some((evidence.searchable_key().to_string(), evidence.tier(), evidence.key_role()))
    }

    /// WS-BEST-001: bare exact replaces qualified substring for one row,
    /// regardless of key examination order.
    #[test]
    fn ws_best_001_bare_exact_beats_qualified_substring_under_both_orders() {
        let profile = WorkspaceSymbolQueryProfile::compile("run");
        let forward = [("Package::run", Role::QualifiedName), ("run", Role::BareName)];
        let reversed = [("run", Role::BareName), ("Package::run", Role::QualifiedName)];
        for order in [&forward, &reversed] {
            let (key, tier, role) = winning_of(&profile, order).expect("row matched");
            assert_eq!(key, "run");
            assert_eq!(tier, Tier::Exact);
            assert_eq!(role, Role::BareName);
        }
    }

    /// WS-BEST-002: qualified exact replaces weaker bare evidence. A
    /// qualified key is exact only when the query itself carries the
    /// qualification (`P::run`); the bare `run` key then still admits as a
    /// weaker substring and must lose.
    #[test]
    fn ws_best_002_qualified_exact_beats_weaker_bare_match() {
        let profile = WorkspaceSymbolQueryProfile::compile("P::run");
        let keys = [("run", Role::BareName), ("P::run", Role::QualifiedName)];
        let (key, tier, role) = winning_of(&profile, &keys).expect("row matched");
        assert_eq!(key, "P::run");
        assert_eq!(tier, Tier::Exact);
        assert_eq!(role, Role::QualifiedName);

        // Both examination orders agree.
        let reversed = [keys[1], keys[0]];
        let (key, tier, _) = winning_of(&profile, &reversed).expect("row matched");
        assert_eq!((key.as_str(), tier), ("P::run", Tier::Exact));
    }

    /// WS-BEST-003: legacy separator alias competes deterministically; the
    /// equal-tier winner is chosen by the total comparator, never by order.
    #[test]
    fn ws_best_003_legacy_separator_alias_tie_is_deterministic() {
        let profile = WorkspaceSymbolQueryProfile::compile("run");
        let keys = [
            ("Package'run", Role::CompatibilityAlias),
            ("Package::run", Role::QualifiedName),
            ("Package::run", Role::QualifiedName),
        ];
        // Both orders must pick the same winner (`'` sorts before `:`).
        let forward = [keys[0], keys[1]];
        let reversed = [keys[1], keys[0]];
        let expected = ("Package'run".to_string(), Tier::Substring);
        let (key, tier, _) = winning_of(&profile, &forward).expect("row matched");
        assert_eq!((key, tier), expected);
        let (key, tier, _) = winning_of(&profile, &reversed).expect("row matched");
        assert_eq!((key, tier), expected);

        // Equal-comparing duplicate arrivals are idempotent.
        let accumulator = accumulate(&profile, &keys);
        assert_eq!(accumulator.considered_match_count(), 3);
        let best = accumulator.into_best_match().expect("row matched");
        assert_eq!(best.winning_evidence().searchable_key(), "Package'run");
    }

    /// WS-BEST-004: browse admits every key at one row-level disposition;
    /// the retained evidence is independent of alias count/order.
    #[test]
    fn ws_best_004_browse_winner_is_alias_count_and_order_independent() {
        let profile = WorkspaceSymbolQueryProfile::compile("");
        let few = [("run", Role::BareName)];
        let many = [
            ("longest_alias_spelling", Role::Other),
            ("middle_alias", Role::CompatibilityAlias),
            ("a", Role::BareName),
            ("run", Role::BareName),
        ];
        for (keys, expected_key) in [(&few[..], "run"), (&many[..], "a")] {
            let (key, tier, _) = winning_of(&profile, keys).expect("browse matches every key");
            assert_eq!(tier, Tier::Prefix);
            assert_eq!(key, expected_key, "shortest admitted key wins by total comparator");
        }
    }

    /// WS-BEST-008/014: evidence from another profile generation is refused
    /// as a typed outcome and never mixed into the row.
    #[test]
    fn ws_best_008_profile_mismatch_evidence_is_refused_not_mixed() {
        let profile = WorkspaceSymbolQueryProfile::compile("run");
        let stale = match_searchable_key(
            &WorkspaceSymbolQueryProfile::compile("ru"),
            "run",
            Role::BareName,
        )
        .expect("control evidence exists");

        let mut accumulator = super::BestRowMatchAccumulator::for_profile(&profile);
        assert_eq!(accumulator.consider(&stale), super::BestRowMatchUpdate::RefusedProfileMismatch);
        assert!(
            accumulator.into_best_match().is_none(),
            "refused evidence must not materialize a row"
        );

        // A current first match followed by stale evidence keeps the winner.
        let mut accumulator = super::BestRowMatchAccumulator::for_profile(&profile);
        accumulator.consider(
            &match_searchable_key(&profile, "P::run", Role::QualifiedName).expect("admitted"),
        );
        assert_eq!(
            accumulator.consider(
                &match_searchable_key(
                    &WorkspaceSymbolQueryProfile::compile("un"),
                    "run",
                    Role::BareName
                )
                .expect("control evidence exists")
            ),
            super::BestRowMatchUpdate::RefusedProfileMismatch
        );
        let best = accumulator.into_best_match().expect("winner survives refusal");
        assert_eq!(best.winning_evidence().searchable_key(), "P::run");
        assert_eq!(best.profile_digest(), profile.digest());
    }

    /// A row whose every key returns `None` stays absent: no fallback
    /// evidence may materialize it (negative control M8).
    #[test]
    fn ws_best_all_keys_nonmatching_row_is_absent() {
        let profile = WorkspaceSymbolQueryProfile::compile("zq");
        let keys = [("alpha", Role::BareName), ("Package::alpha", Role::QualifiedName)];
        assert!(winning_of(&profile, &keys).is_none());
        assert!(super::select_best_row_match(&profile, &keys).is_none());
    }

    /// The public convenience selector agrees with manual accumulation.
    #[test]
    fn ws_best_selector_matches_manual_accumulation() {
        let profile = WorkspaceSymbolQueryProfile::compile("run");
        let keys = [("Package::run", Role::QualifiedName), ("run", Role::BareName)];
        let selected = super::select_best_row_match(&profile, &keys).expect("matched");
        let manual = accumulate(&profile, &keys).into_best_match().expect("matched");
        assert_eq!(selected, manual);
        assert_eq!(
            selected.runner_up_evidence().map(super::WorkspaceSymbolMatchEvidence::searchable_key),
            Some("Package::run")
        );
    }

    proptest! {
        /// Repeated compilation is deterministic; membership/order-relevant
        /// fields depend only on the trimmed/folded query.
        #[test]
        fn compilation_is_pure(raw in ".*{0,24}") {
            let a = WorkspaceSymbolQueryProfile::compile(&raw);
            let b = WorkspaceSymbolQueryProfile::compile(&raw);
            assert_eq!(a, b);
            assert_eq!(a.digest(), b.digest());
            // Digest/normalization depend only on the trimmed/folded form.
            let trimmed_form = WorkspaceSymbolQueryProfile::compile(raw.trim());
            assert_eq!(a.digest(), trimmed_form.digest());
            assert_eq!(a.folded_query(), trimmed_form.folded_query());
        }

        /// Admission is a function: same profile + key ⇒ same outcome, and
        /// outcomes are either None or deterministic evidence.
        #[test]
        fn match_is_total_and_deterministic(key in "[a-zA-Z_]{0,20}", query in "[a-zA-Z_]{0,12}") {
            let profile = WorkspaceSymbolQueryProfile::compile(&query);
            let first = match_searchable_key(&profile, &key, Role::BareName);
            let second = match_searchable_key(&profile, &key, Role::Other);
            match (first, second) {
                (Some(a), Some(b)) => {
                    assert_eq!(a.tier(), b.tier());
                    assert_eq!(a.matched_positions(), b.matched_positions());
                    assert_eq!(a.compare(&b), std::cmp::Ordering::Equal);
                }
                (None, None) => {}
                _ => panic!("admission must be a deterministic function"),
            }
        }

        /// Antisymmetry of the evidence comparator.
        #[test]
        fn comparator_antisymmetric(a in "[a-z]{0,10}", b in "[a-z]{0,10}", q in "[a-z]{0,8}") {
            let profile = WorkspaceSymbolQueryProfile::compile(&q);
            if let (Some(ea), Some(eb)) = (
                match_searchable_key(&profile, &a, Role::BareName),
                match_searchable_key(&profile, &b, Role::BareName),
            ) {
                prop_assert_eq!(ea.compare(&eb), eb.compare(&ea).reverse());
            }
        }

        /// WS-BEST-005: the retained winner is invariant under key-examination
        /// permutation — first-key/last-key order cannot choose it.
        #[test]
        fn ws_best_prop_winner_invariant_under_key_permutation(
            keys in proptest::collection::vec("[a-zA-Z:']{1,12}", 1..8),
            query in "[a-zA-Z]{0,6}",
        ) {
            let profile = WorkspaceSymbolQueryProfile::compile(&query);
            let owned: std::collections::HashMap<String, ()> =
                keys.iter().map(|k| (k.clone(), ())).collect();
            let unique: Vec<(&str, Role)> =
                owned.keys().map(|k| (k.as_str(), Role::Other)).collect();
            let forward: Vec<(&str, Role)> = unique.clone();
            let reversed: Vec<(&str, Role)> = unique.iter().rev().copied().collect();
            // Rotation is always a bijection, so this stays a real permutation.
            let rotated: Vec<(&str, Role)> = (0..unique.len())
                .map(|i| unique[(i + unique.len() / 2) % unique.len()])
                .collect();
            let expected = super::select_best_row_match(&profile, &forward);
            prop_assert_eq!(expected.clone(), super::select_best_row_match(&profile, &reversed));
            prop_assert_eq!(expected, super::select_best_row_match(&profile, &rotated));
        }
    }
}
