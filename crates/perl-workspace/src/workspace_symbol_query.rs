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
    }
}
