//! Stale-source disposition for parse diagnostics.
//!
//! A [`StaleSourceAnchor`] pairs a [`ByteSpan`] with a digest of the source
//! text it was minted from. Consumers call [`StaleSourceAnchor::resolve_for_current`]
//! to determine whether the diagnostic is still valid for the source text they
//! currently hold.
//!
//! # Ownership boundary
//!
//! This is the freshness layer of the diagnostic-anchor contract, not a second
//! position resolver: `perl_parser_core`'s `ParseDiagnosticAnchor` (#6941)
//! owns *where* a diagnostic anchors in source text; this type owns *whether*
//! that text is still the text the diagnostic was produced from. The two are
//! complementary and neither reopens the other's contract.
//!
//! # Stale-source disposition
//!
//! ```text
//! anchor ─── ByteSpan + SourceDigest (minted at parse time)
//!                ↓
//!        resolve_for_current(current_source)
//!                ↓
//!        AnchorResolution::Current(span)        ← source unchanged
//!        AnchorResolution::Stale { .. }          ← source changed
//! ```
//!
//! # Once-per-batch freshness
//!
//! When processing many anchors that were all minted from the same parse pass,
//! use [`BatchFreshnessChecker`] to avoid recomputing the current source digest
//! for every anchor:
//!
//! ```
//! use perl_diagnostics::anchor::{BatchFreshnessChecker, StaleSourceAnchor};
//! use perl_diagnostics::ByteSpan;
//!
//! let source = b"package Foo;\n1;\n";
//! let span = ByteSpan::new(0, 7).expect("valid span");
//! let anchor = StaleSourceAnchor::mint(span, source);
//!
//! let mut checker = BatchFreshnessChecker::new();
//! let resolution = checker.check(&anchor, Some(source));
//!
//! // Second call reuses the cached current digest — no second SHA-256 needed.
//! let resolution2 = checker.check(&anchor, Some(source));
//! # let _ = (resolution, resolution2);
//! ```

use sha2::{Digest as _, Sha256};

use crate::ByteSpan;

// ── Domain constant ───────────────────────────────────────────────────────────

/// Domain separator for parser-level source digests.
///
/// Distinct from `perl-lsp:content-digest:v1` (used by `perl-source-identity`'s
/// `ContentDigest`) to avoid cross-domain collisions even when the SHA-256
/// input bytes happen to match.
const ANCHOR_DIGEST_DOMAIN: &[u8] = b"perl-lsp:anchor-source-digest:v1\0";

// ── SourceDigest ──────────────────────────────────────────────────────────────

/// A domain-separated SHA-256 digest of exact source bytes, scoped to the
/// parser anchor domain.
///
/// `SourceDigest` is the freshness key for [`StaleSourceAnchor`]. Two
/// `SourceDigest` values are equal if and only if the source byte slices
/// they were computed from are byte-for-byte identical.
///
/// # Wire format
///
/// `anchor-digest:<64 lowercase hex digits>` — usable for logging and
/// diagnostics. The prefix distinguishes this digest from `ContentDigest`
/// (used by `perl-source-identity`) even though both use SHA-256.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SourceDigest([u8; 32]);

impl SourceDigest {
    /// Compute a domain-separated digest for the given source bytes.
    ///
    /// The same byte slice always produces the same `SourceDigest`, across
    /// processes and machines. An empty slice produces a valid, deterministic
    /// digest.
    #[must_use]
    pub fn of_bytes(source: &[u8]) -> Self {
        let mut h = Sha256::new();
        h.update(ANCHOR_DIGEST_DOMAIN);
        // Length-prefix the source to avoid `["a", "b"]` vs `["ab"]` collisions
        // if a future variant were to concatenate fields.
        let len_be = (source.len() as u64).to_be_bytes();
        h.update(len_be);
        h.update(source);
        Self(h.finalize().into())
    }

    /// Return the raw 32-byte SHA-256 output.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Format as the wire representation: `anchor-digest:<64 lowercase hex>`.
    #[must_use]
    pub fn as_wire(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity("anchor-digest:".len() + 64);
        out.push_str("anchor-digest:");
        for byte in &self.0 {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
        out
    }

    /// Write the wire representation (`anchor-digest:<64 lowercase hex>`,
    /// excluding the `SourceDigest(...)` wrapper) directly to `f`.
    fn write_wire(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("anchor-digest:")?;
        for byte in &self.0 {
            f.write_fmt(format_args!("{byte:02x}"))?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for SourceDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SourceDigest(")?;
        self.write_wire(f)?;
        f.write_str(")")
    }
}

impl std::fmt::Display for SourceDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.write_wire(f)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for SourceDigest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_wire())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for SourceDigest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(deserializer)?;
        let hex = s.strip_prefix("anchor-digest:").ok_or_else(|| {
            serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&s),
                &"a string starting with `anchor-digest:`",
            )
        })?;
        if hex.len() != 64 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&s),
                &"anchor-digest:<64 lowercase hex digits>",
            ));
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let hi = (chunk[0] as char).to_digit(16).unwrap_or(0) as u8;
            let lo = (chunk[1] as char).to_digit(16).unwrap_or(0) as u8;
            bytes[i] = (hi << 4) | lo;
        }
        Ok(Self(bytes))
    }
}

// ── StaleSourceAnchor ─────────────────────────────────────────────────────

/// A diagnostic anchor: a [`ByteSpan`] paired with a digest of the source text
/// it was minted from.
///
/// Emitted alongside a diagnostic to allow downstream consumers to detect
/// whether the source text has changed since the diagnostic was produced,
/// without re-running the parser.
///
/// # Minting
///
/// ```
/// use perl_diagnostics::anchor::StaleSourceAnchor;
/// use perl_diagnostics::ByteSpan;
///
/// let source = b"package Foo;\n1;\n";
/// let span = ByteSpan::new(0, 7).expect("valid span");
/// let anchor = StaleSourceAnchor::mint(span, source);
/// assert_eq!(anchor.span(), span);
/// ```
///
/// # Resolving
///
/// ```
/// use perl_diagnostics::anchor::{AnchorResolution, StaleSourceAnchor};
/// use perl_diagnostics::ByteSpan;
///
/// let source = b"package Foo;\n1;\n";
/// let span = ByteSpan::new(0, 7).expect("valid span");
/// let anchor = StaleSourceAnchor::mint(span, source);
///
/// // Same source → Current.
/// assert!(matches!(anchor.resolve_for_current(source), AnchorResolution::Current(_)));
///
/// // Different source → Stale.
/// let modified = b"package Bar;\n1;\n";
/// assert!(matches!(anchor.resolve_for_current(modified), AnchorResolution::Stale { .. }));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StaleSourceAnchor {
    /// The byte span of the associated diagnostic.
    span: ByteSpan,
    /// Digest of the source text at the time this anchor was minted.
    minted_digest: SourceDigest,
}

impl StaleSourceAnchor {
    /// Mint a new anchor for the given `span` and `source` text.
    ///
    /// The anchor captures both the span and a digest of the exact source bytes.
    /// Use [`resolve_for_current`](Self::resolve_for_current) or
    /// [`BatchFreshnessChecker::check`] to test freshness later.
    #[must_use]
    pub fn mint(span: ByteSpan, source: &[u8]) -> Self {
        Self { span, minted_digest: SourceDigest::of_bytes(source) }
    }

    /// The byte span of the diagnostic this anchor was minted for.
    #[must_use]
    pub fn span(&self) -> ByteSpan {
        self.span
    }

    /// The source digest recorded at mint time.
    ///
    /// This is the digest of the exact source bytes that were present when the
    /// parser produced the associated diagnostic.
    #[must_use]
    pub fn minted_digest(&self) -> &SourceDigest {
        &self.minted_digest
    }

    /// Resolve this anchor against the given `current_source`.
    ///
    /// Returns:
    /// - [`AnchorResolution::Current`] if the source bytes have not changed
    ///   since minting;
    /// - [`AnchorResolution::Stale`] if they have, carrying both the minted and
    ///   current digests for traceability.
    ///
    /// To avoid recomputing the current-source digest for every anchor in a
    /// batch, use [`BatchFreshnessChecker`] instead.
    #[must_use]
    pub fn resolve_for_current(&self, current_source: &[u8]) -> AnchorResolution {
        let current_digest = SourceDigest::of_bytes(current_source);
        if self.minted_digest == current_digest {
            AnchorResolution::Current(self.span)
        } else {
            AnchorResolution::Stale { minted_digest: self.minted_digest.clone(), current_digest }
        }
    }
}

// ── AnchorResolution ──────────────────────────────────────────────────────────

/// The result of resolving a [`StaleSourceAnchor`] against a current source
/// text.
///
/// Produced by [`StaleSourceAnchor::resolve_for_current`] and
/// [`BatchFreshnessChecker::check`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum AnchorResolution {
    /// The source text has not changed since minting; the span is still valid.
    Current(ByteSpan),

    /// The source text has changed since minting; the diagnostic may be stale.
    ///
    /// Both digests are included so consumers can log or trace staleness without
    /// needing to recompute the hashes.
    Stale {
        /// The digest recorded at mint time.
        minted_digest: SourceDigest,
        /// The digest of the current source text.
        current_digest: SourceDigest,
    },

    /// Freshness cannot be established — the current source is unavailable or
    /// the checker was constructed without a source snapshot.
    ///
    /// Consumers must treat `NotProven` as "potentially stale" and not use the
    /// associated diagnostic span.
    NotProven,
}

impl AnchorResolution {
    /// Returns `true` if the anchor is [`Current`](Self::Current).
    #[must_use]
    pub fn is_current(&self) -> bool {
        matches!(self, Self::Current(_))
    }

    /// Returns `true` if the anchor is [`Stale`](Self::Stale).
    #[must_use]
    pub fn is_stale(&self) -> bool {
        matches!(self, Self::Stale { .. })
    }

    /// Returns `true` if the resolution is [`NotProven`](Self::NotProven).
    #[must_use]
    pub fn is_not_proven(&self) -> bool {
        matches!(self, Self::NotProven)
    }

    /// Extract the span from a [`Current`](Self::Current) resolution.
    ///
    /// Returns `None` for `Stale` and `NotProven` variants.
    #[must_use]
    pub fn current_span(&self) -> Option<ByteSpan> {
        match self {
            Self::Current(span) => Some(*span),
            _ => None,
        }
    }
}

// ── BatchFreshnessChecker ─────────────────────────────────────────────────────

/// Once-per-batch freshness checker for [`StaleSourceAnchor`] values.
///
/// Within a single batch (one source snapshot), the current-source digest is
/// computed at most once, regardless of how many anchors are checked. This
/// avoids redundant SHA-256 computations when processing many diagnostics that
/// were all minted from the same parse pass.
///
/// A new `BatchFreshnessChecker` must be constructed for each distinct source
/// snapshot. Reusing a checker across snapshots will return stale cached results.
///
/// # Example
///
/// ```
/// use perl_diagnostics::anchor::{AnchorResolution, BatchFreshnessChecker, StaleSourceAnchor};
/// use perl_diagnostics::ByteSpan;
///
/// let source = b"my $x = 1;";
/// let span1 = ByteSpan::new(0, 2).expect("valid");
/// let span2 = ByteSpan::new(3, 5).expect("valid");
///
/// let anchor1 = StaleSourceAnchor::mint(span1, source);
/// let anchor2 = StaleSourceAnchor::mint(span2, source);
///
/// let mut checker = BatchFreshnessChecker::new();
/// // First call computes the current digest.
/// let r1 = checker.check(&anchor1, Some(source));
/// // Second call reuses the cached digest (no second SHA-256).
/// let r2 = checker.check(&anchor2, Some(source));
///
/// assert!(r1.is_current());
/// assert!(r2.is_current());
/// ```
#[derive(Debug, Default)]
pub struct BatchFreshnessChecker {
    /// Cached digest of the current source snapshot.
    ///
    /// `None` means the digest has not yet been computed for this batch.
    cached_current: Option<SourceDigest>,
}

impl BatchFreshnessChecker {
    /// Construct an empty checker with no cached digest.
    ///
    /// The digest will be computed on the first call to [`check`](Self::check).
    #[must_use]
    pub fn new() -> Self {
        Self { cached_current: None }
    }

    /// Construct a checker pre-loaded with the digest of `source`.
    ///
    /// Use this when you know the current source at construction time and want
    /// to avoid a redundant digest computation on the first call.
    #[must_use]
    pub fn for_source(source: &[u8]) -> Self {
        Self { cached_current: Some(SourceDigest::of_bytes(source)) }
    }

    /// Check `anchor` against the given `current_source`.
    ///
    /// - `None` → [`AnchorResolution::NotProven`] (source unavailable).
    /// - `Some(source)` → computes the current digest on the first call, then
    ///   reuses the cached digest on all subsequent calls within this batch.
    ///
    /// The anchor's [`resolve_for_current`](StaleSourceAnchor::resolve_for_current)
    /// is equivalent to calling this method with `Some(current_source)` on a
    /// fresh checker; use this method to share the digest across multiple anchors.
    pub fn check(
        &mut self,
        anchor: &StaleSourceAnchor,
        current_source: Option<&[u8]>,
    ) -> AnchorResolution {
        let Some(source) = current_source else {
            return AnchorResolution::NotProven;
        };

        // Compute and cache the current digest on the first call.
        let current_digest =
            self.cached_current.get_or_insert_with(|| SourceDigest::of_bytes(source));

        if anchor.minted_digest == *current_digest {
            AnchorResolution::Current(anchor.span)
        } else {
            AnchorResolution::Stale {
                minted_digest: anchor.minted_digest.clone(),
                current_digest: current_digest.clone(),
            }
        }
    }

    /// Returns the cached current digest, if one has been computed.
    ///
    /// `None` before the first call to [`check`](Self::check) with `Some(source)`.
    #[must_use]
    pub fn cached_digest(&self) -> Option<&SourceDigest> {
        self.cached_current.as_ref()
    }

    /// Reset the checker, clearing the cached digest.
    ///
    /// After calling this, the next call to [`check`](Self::check) will
    /// recompute the current digest. Use this when you want to reuse the
    /// checker struct across snapshots instead of constructing a new one.
    pub fn reset(&mut self) {
        self.cached_current = None;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ByteSpan;

    fn span(start: usize, end: usize) -> ByteSpan {
        ByteSpan::new(start, end).expect("test span must be valid")
    }

    // ── SourceDigest ──────────────────────────────────────────────────────────

    #[test]
    fn source_digest_is_deterministic() {
        let a = SourceDigest::of_bytes(b"package Foo;\n1;\n");
        let b = SourceDigest::of_bytes(b"package Foo;\n1;\n");
        assert_eq!(a, b, "same bytes → same digest");
    }

    #[test]
    fn source_digest_distinguishes_content() {
        let a = SourceDigest::of_bytes(b"package Foo;\n");
        let b = SourceDigest::of_bytes(b"package Bar;\n");
        assert_ne!(a, b, "different bytes → different digest");
    }

    #[test]
    fn source_digest_empty_bytes_is_valid() {
        let d = SourceDigest::of_bytes(b"");
        let d2 = SourceDigest::of_bytes(b"");
        assert_eq!(d, d2, "empty bytes → deterministic digest");
    }

    #[test]
    fn source_digest_wire_has_prefix_and_64_hex() {
        let d = SourceDigest::of_bytes(b"test");
        let wire = d.as_wire();
        assert!(wire.starts_with("anchor-digest:"), "wire must start with anchor-digest:");
        let hex = &wire["anchor-digest:".len()..];
        assert_eq!(hex.len(), 64, "wire hex body must be 64 characters");
        assert!(
            hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
            "wire hex body must be lowercase"
        );
    }

    #[test]
    fn source_digest_display_matches_wire() {
        let d = SourceDigest::of_bytes(b"display");
        assert_eq!(format!("{d}"), d.as_wire());
    }

    // ── StaleSourceAnchor ─────────────────────────────────────────────────

    #[test]
    fn anchor_mint_records_span_and_digest() {
        let source = b"my $x = 42;";
        let s = span(0, 2);
        let anchor = StaleSourceAnchor::mint(s, source);
        assert_eq!(anchor.span(), s);
        assert_eq!(anchor.minted_digest(), &SourceDigest::of_bytes(source));
    }

    #[test]
    fn resolve_current_when_source_unchanged() {
        let source = b"package Foo;\n1;\n";
        let anchor = StaleSourceAnchor::mint(span(0, 7), source);
        let resolution = anchor.resolve_for_current(source);
        assert!(
            matches!(resolution, AnchorResolution::Current(sp) if sp == span(0, 7)),
            "unchanged source must resolve to Current"
        );
    }

    #[test]
    fn resolve_stale_when_source_changed() {
        let source_a = b"package Foo;\n1;\n";
        let source_b = b"package Bar;\n1;\n";
        let anchor = StaleSourceAnchor::mint(span(0, 7), source_a);
        let resolution = anchor.resolve_for_current(source_b);
        assert!(
            matches!(resolution, AnchorResolution::Stale { .. }),
            "changed source must resolve to Stale"
        );
    }

    #[test]
    fn resolve_stale_carries_both_digests() {
        let source_a = b"original";
        let source_b = b"modified";
        let anchor = StaleSourceAnchor::mint(span(0, 8), source_a);
        let resolution = anchor.resolve_for_current(source_b);
        if let AnchorResolution::Stale { minted_digest, current_digest } = resolution {
            assert_eq!(minted_digest, SourceDigest::of_bytes(source_a));
            assert_eq!(current_digest, SourceDigest::of_bytes(source_b));
        } else {
            panic!("expected Stale, got {resolution:?}");
        }
    }

    #[test]
    fn anchor_equality_based_on_span_and_digest() {
        let source = b"my $x;";
        let a = StaleSourceAnchor::mint(span(0, 2), source);
        let b = StaleSourceAnchor::mint(span(0, 2), source);
        let c = StaleSourceAnchor::mint(span(3, 5), source);
        assert_eq!(a, b, "same span + same source → equal anchors");
        assert_ne!(a, c, "different span → different anchors");
    }

    // ── AnchorResolution helpers ──────────────────────────────────────────────

    #[test]
    fn resolution_helper_methods() {
        let current = AnchorResolution::Current(span(0, 3));
        assert!(current.is_current());
        assert!(!current.is_stale());
        assert!(!current.is_not_proven());
        assert_eq!(current.current_span(), Some(span(0, 3)));

        let stale = AnchorResolution::Stale {
            minted_digest: SourceDigest::of_bytes(b"a"),
            current_digest: SourceDigest::of_bytes(b"b"),
        };
        assert!(!stale.is_current());
        assert!(stale.is_stale());
        assert!(!stale.is_not_proven());
        assert_eq!(stale.current_span(), None);

        let not_proven = AnchorResolution::NotProven;
        assert!(!not_proven.is_current());
        assert!(!not_proven.is_stale());
        assert!(not_proven.is_not_proven());
        assert_eq!(not_proven.current_span(), None);
    }

    // ── BatchFreshnessChecker ─────────────────────────────────────────────────

    #[test]
    fn batch_checker_new_has_no_cached_digest() {
        let checker = BatchFreshnessChecker::new();
        assert!(checker.cached_digest().is_none(), "new checker must have no cached digest");
    }

    #[test]
    fn batch_checker_none_source_returns_not_proven() {
        let source = b"my $x;";
        let anchor = StaleSourceAnchor::mint(span(0, 2), source);
        let mut checker = BatchFreshnessChecker::new();
        let resolution = checker.check(&anchor, None);
        assert!(
            matches!(resolution, AnchorResolution::NotProven),
            "None source must yield NotProven"
        );
        // Digest must remain uncached after NotProven.
        assert!(
            checker.cached_digest().is_none(),
            "NotProven must not cache a digest (no source available)"
        );
    }

    #[test]
    fn batch_checker_caches_digest_after_first_check() {
        let source = b"package Foo;\n";
        let anchor = StaleSourceAnchor::mint(span(0, 7), source);
        let mut checker = BatchFreshnessChecker::new();
        assert!(checker.cached_digest().is_none());
        let _ = checker.check(&anchor, Some(source));
        assert!(checker.cached_digest().is_some(), "digest must be cached after first check");
        assert_eq!(checker.cached_digest(), Some(&SourceDigest::of_bytes(source)));
    }

    /// Discriminating proof of once-per-batch freshness: after the first `check`
    /// fixes the batch's source snapshot, later calls passing *different* source
    /// bytes must still resolve against the first snapshot. A checker that
    /// recomputed the digest per call would flip this result to `Stale`.
    #[test]
    fn batch_checker_resolves_later_calls_against_first_snapshot() {
        let snapshot = b"snapshot at batch start";
        let other = b"entirely different bytes";
        let anchor = StaleSourceAnchor::mint(span(0, 8), snapshot);
        let mut checker = BatchFreshnessChecker::new();
        let first = checker.check(&anchor, Some(snapshot));
        assert!(first.is_current(), "first call must resolve against its own snapshot");
        let second = checker.check(&anchor, Some(other));
        assert!(
            second.is_current(),
            "later calls must reuse the first snapshot's digest, not recompute from `other`"
        );
        assert_eq!(
            checker.cached_digest(),
            Some(&SourceDigest::of_bytes(snapshot)),
            "cached digest must remain the first snapshot's digest"
        );
    }

    /// Mixed-anchor single batch: anchor₁ minted from the batch's snapshot bytes
    /// resolves `Current`, while anchor₂ minted from *different* bytes resolves
    /// `Stale` against that same first-snapshot cache. Complements
    /// `batch_checker_resolves_later_calls_against_first_snapshot`, which holds
    /// the anchor fixed and varies the passed source.
    #[test]
    fn batch_checker_mixed_anchors_resolve_against_first_snapshot() {
        let snapshot = b"package Snapshot;\n1;\n";
        let foreign = b"package Foreign;\n1;\n";
        let anchor_from_snapshot = StaleSourceAnchor::mint(span(0, 8), snapshot);
        let anchor_from_foreign = StaleSourceAnchor::mint(span(0, 8), foreign);
        let mut checker = BatchFreshnessChecker::new();
        let current = checker.check(&anchor_from_snapshot, Some(snapshot));
        assert!(
            matches!(current, AnchorResolution::Current(sp) if sp == span(0, 8)),
            "anchor minted from the snapshot bytes must resolve Current"
        );
        let stale = checker.check(&anchor_from_foreign, Some(snapshot));
        if let AnchorResolution::Stale { minted_digest, current_digest } = stale {
            assert_eq!(minted_digest, SourceDigest::of_bytes(foreign));
            assert_eq!(current_digest, SourceDigest::of_bytes(snapshot));
        } else {
            panic!("foreign-minted anchor must resolve Stale in the snapshot batch");
        }
    }

    #[test]
    fn batch_checker_returns_current_for_same_source() {
        let source = b"my $x = 1;";
        let a1 = StaleSourceAnchor::mint(span(0, 2), source);
        let a2 = StaleSourceAnchor::mint(span(3, 5), source);
        let mut checker = BatchFreshnessChecker::new();
        let r1 = checker.check(&a1, Some(source));
        let r2 = checker.check(&a2, Some(source));
        assert!(r1.is_current(), "first anchor must be Current");
        assert!(r2.is_current(), "second anchor must be Current (cached digest reused)");
    }

    #[test]
    fn batch_checker_returns_stale_for_different_source() {
        let source_a = b"original source";
        let source_b = b"modified source";
        let anchor = StaleSourceAnchor::mint(span(0, 8), source_a);
        let mut checker = BatchFreshnessChecker::new();
        let resolution = checker.check(&anchor, Some(source_b));
        assert!(
            matches!(resolution, AnchorResolution::Stale { .. }),
            "anchor minted from source_a must be Stale against source_b"
        );
    }

    #[test]
    fn batch_checker_for_source_pre_loads_digest() {
        let source = b"pre-loaded";
        let anchor = StaleSourceAnchor::mint(span(0, 3), source);
        let mut checker = BatchFreshnessChecker::for_source(source);
        assert!(checker.cached_digest().is_some(), "for_source must pre-load the digest");
        let resolution = checker.check(&anchor, Some(source));
        assert!(resolution.is_current(), "pre-loaded source must match mint source");
    }

    #[test]
    fn batch_checker_reset_clears_cache() {
        let source = b"some source";
        let anchor = StaleSourceAnchor::mint(span(0, 4), source);
        let mut checker = BatchFreshnessChecker::new();
        let _ = checker.check(&anchor, Some(source));
        assert!(checker.cached_digest().is_some());
        checker.reset();
        assert!(checker.cached_digest().is_none(), "reset must clear the cached digest");
    }

    #[test]
    fn batch_checker_handles_multiple_anchors_single_batch() {
        let source = b"my $x = 1; my $y = 2;";
        let anchors: Vec<_> =
            (0..5).map(|i| StaleSourceAnchor::mint(span(i, i + 1), source)).collect();
        let mut checker = BatchFreshnessChecker::new();
        for anchor in &anchors {
            let resolution = checker.check(anchor, Some(source));
            assert!(resolution.is_current(), "all anchors from same source must be Current");
        }
    }

    // ── Falsifiers from acceptance.md ─────────────────────────────────────────

    /// Falsifier: anchor from source_a returns Stale against source_b.
    #[test]
    fn falsifier_stale_when_source_differs() {
        let source_a = b"package A;\n";
        let source_b = b"package B;\n";
        let anchor = StaleSourceAnchor::mint(span(0, 7), source_a);
        assert!(
            matches!(anchor.resolve_for_current(source_b), AnchorResolution::Stale { .. }),
            "falsifier: different source must produce Stale"
        );
    }

    /// Falsifier: anchor from source_a returns Current against the same bytes.
    #[test]
    fn falsifier_current_when_source_same() {
        let source = b"package A;\n";
        let anchor = StaleSourceAnchor::mint(span(0, 7), source);
        assert!(
            anchor.resolve_for_current(source).is_current(),
            "falsifier: same source must produce Current"
        );
    }

    /// Falsifier: BatchFreshnessChecker with no source returns NotProven.
    #[test]
    fn falsifier_not_proven_when_no_source() {
        let source = b"any source";
        let anchor = StaleSourceAnchor::mint(span(0, 3), source);
        let mut checker = BatchFreshnessChecker::new();
        assert!(
            checker.check(&anchor, None).is_not_proven(),
            "falsifier: None source must produce NotProven"
        );
    }

    /// Falsifier: two SourceDigest values for the same bytes are equal.
    #[test]
    fn falsifier_same_bytes_equal_digests() {
        let d1 = SourceDigest::of_bytes(b"identical");
        let d2 = SourceDigest::of_bytes(b"identical");
        assert_eq!(d1, d2, "falsifier: same bytes must produce equal SourceDigest");
    }

    /// Falsifier: two SourceDigest values for different bytes are not equal.
    #[test]
    fn falsifier_different_bytes_unequal_digests() {
        let d1 = SourceDigest::of_bytes(b"left");
        let d2 = SourceDigest::of_bytes(b"right");
        assert_ne!(d1, d2, "falsifier: different bytes must produce unequal SourceDigest");
    }

    /// Falsifier: empty source produces a valid deterministic SourceDigest.
    #[test]
    fn falsifier_empty_source_valid_digest() {
        let d1 = SourceDigest::of_bytes(b"");
        let d2 = SourceDigest::of_bytes(b"");
        assert_eq!(d1, d2, "falsifier: empty source must produce valid equal SourceDigest");
    }

    // ── Serde round-trip (feature-gated) ─────────────────────────────────────

    #[cfg(feature = "serde")]
    mod serde_tests {
        use super::*;

        #[test]
        fn source_digest_serde_round_trip() {
            let d = SourceDigest::of_bytes(b"serde test");
            let json = serde_json::to_string(&d).expect("serialize SourceDigest");
            let back: SourceDigest = serde_json::from_str(&json).expect("deserialize SourceDigest");
            assert_eq!(d, back);
        }

        #[test]
        fn source_digest_serde_rejects_bad_prefix() {
            let bad =
                r#""sha256:0000000000000000000000000000000000000000000000000000000000000000""#;
            assert!(
                serde_json::from_str::<SourceDigest>(bad).is_err(),
                "wrong prefix must be rejected"
            );
        }

        #[test]
        fn source_digest_serde_rejects_short_hex() {
            let bad = r#""anchor-digest:abc123""#;
            assert!(
                serde_json::from_str::<SourceDigest>(bad).is_err(),
                "short hex must be rejected"
            );
        }

        #[test]
        fn anchor_serde_round_trip() {
            let source = b"package Foo;\n1;\n";
            let anchor = StaleSourceAnchor::mint(span(0, 7), source);
            let json = serde_json::to_string(&anchor).expect("serialize anchor");
            let back: StaleSourceAnchor = serde_json::from_str(&json).expect("deserialize anchor");
            assert_eq!(anchor, back);
        }

        #[test]
        fn anchor_resolution_current_serde_round_trip() {
            let res = AnchorResolution::Current(span(1, 5));
            let json = serde_json::to_string(&res).expect("serialize");
            let back: AnchorResolution = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(res, back);
        }

        #[test]
        fn anchor_resolution_stale_serde_round_trip() {
            let res = AnchorResolution::Stale {
                minted_digest: SourceDigest::of_bytes(b"old"),
                current_digest: SourceDigest::of_bytes(b"new"),
            };
            let json = serde_json::to_string(&res).expect("serialize");
            let back: AnchorResolution = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(res, back);
        }

        #[test]
        fn anchor_resolution_not_proven_serde_round_trip() {
            let res = AnchorResolution::NotProven;
            let json = serde_json::to_string(&res).expect("serialize");
            let back: AnchorResolution = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(res, back);
        }
    }
}
