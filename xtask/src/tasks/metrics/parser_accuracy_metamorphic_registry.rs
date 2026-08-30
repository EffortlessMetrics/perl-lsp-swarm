//! Fixture-authored registry of exact safe metamorphic points, regions, and
//! terminal applicability dispositions for #13659.
//!
//! The registry replaces admission-by-absence with admission-by-authored-
//! declaration. Every declared case carries a stable `case_id`, the exact
//! source identity and digest it was authored against, a transformation
//! profile, an exact byte anchor (point insertion or region conversion), the
//! required comparison planes, the allowed coordinate/presentation
//! differences, a criticality, an explicit applicability state with reason,
//! and review provenance. Edit plans are generated through the #13657
//! byte-edit substrate only for admitted cases and are validated against the
//! exact authored anchors.
//!
//! Ownership boundaries this module enforces by construction:
//!
//! - no Perl lexing, parsing, AST spans, or parser diagnostics participate in
//!   admission (the only inputs are authored declarations and exact bytes);
//! - no whole-source substring heuristic is consulted in either direction:
//!   boundary marker bytes in a fixture neither admit nor remove any point by
//!   themselves;
//! - every declared case stays in accounting: stale sources, unresolvable
//!   anchors, and unsupported transformations are typed outcomes, never
//!   silently skipped rows.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use perl_lsp_rs_core::hashing::sha256_hex;

use super::parser_accuracy_metamorphic_transform::{
    ByteRange, ContentAddressedSource, ExactEdit, TransformError, ValidatedTransformation,
    apply_exact_edits,
};

/// Schema identity of the registry declaration vocabulary.
pub const REGISTRY_SCHEMA_VERSION: u32 = 1;

/// Review provenance recorded on every case authored by this registry.
pub const REGISTRY_REVIEW_OWNER: &str = "parser-accuracy-metamorphic-oracles";

/// Inserted trailing horizontal whitespace proposition payload.
const TRAILING_TWO_SPACES: &[u8] = b"  ";
/// Inserted blank logical line payload.
const BLANK_LINE_LF: &[u8] = b"\n";
/// Inserted ordinary line comment payload at a statement boundary.
const ORDINARY_LINE_COMMENT: &[u8] = b"# registry comment\n";

// ---------------------------------------------------------------------------
// Authored fixtures
// ---------------------------------------------------------------------------

/// Ordinary LF-terminated two-statement source.
const FIXTURE_LF_ORDINARY: &str = "my $x = 1;\nmy $y = 2;\n";
/// Ordinary CRLF-terminated two-statement source.
const FIXTURE_CRLF_ORDINARY: &str = "my $x = 1;\r\nmy $y = 2;\r\n";
/// Ordinary bare-CR-terminated two-statement source (bare CR is content under
/// the canonical `lf-source-lines/v1` policy, so the fixture is valid input).
const FIXTURE_CR_ORDINARY: &str = "my $x = 1;\rmy $y = 2;\r";
/// Ordinary source without a final newline (EOF anchor).
const FIXTURE_EOF_NO_NEWLINE: &str = "my $x = 1;\nmy $y = 2;";
/// Whole-source LF-only region subject for the LF→CRLF proposition.
const FIXTURE_LF_REGION: &str = "use strict;\nuse warnings;\nmy $x = 1;\n";
/// Whole-source CRLF region subject: the opposite-direction control twin.
const FIXTURE_CRLF_REGION: &str = "use strict;\r\nuse warnings;\r\nmy $x = 1;\r\n";
/// Heredoc-bearing source whose ordinary line 1 stays independently registered
/// while heredoc body and terminator points are dispositioned not-applicable.
const FIXTURE_HEREDOC_MIXED: &str = "my $x = 1;\nmy $text = <<'EOF';\nbody line\nEOF\nmy $y = 2;\n";
/// Multiline `q{...}` payload plus a `q#...#` hash-delimiter quote subject.
const FIXTURE_QUOTE_PAYLOAD: &str =
    "my $a = q{\ninner text\n};\nmy $b = q#hash delimiter#;\nmy $c = 3;\n";
/// Format body, POD block, and `__DATA__` payload boundary subject.
const FIXTURE_FORMAT_POD_DATA: &str = "my $x = 1;\nformat STDOUT =\n @<<<<\n.\n\n=pod\n\nsample pod text\n\n=cut\n\nmy $z = 3;\n__DATA__\ndata payload line\n";
/// BOM-prefixed source whose BOM-preserving conversion is declared unsupported.
const FIXTURE_BOM_ORDINARY: &str = "\u{feff}my $x = 1;\n";
/// Malformed recovery subject: comment insertion is `not_proven` here.
const FIXTURE_MALFORMED: &str = "my $x = ;\nmy $y = {{{;\nsub {\n";

// Anchor prefixes. Each constant is the exact fixture prefix before the anchor
// byte; its byte length is the anchor offset. The unit tests pin every prefix
// against its fixture and pin the byte found at the offset, so a fixture edit
// without an anchor repair fails closed.

/// LF ordinary fixture: end of statement 1 body (before LF).
const LF_ORD_LINE1_END_PREFIX: &str = "my $x = 1;";
/// LF ordinary fixture: start of statement 2 (statement boundary).
const LF_ORD_STMT2_START_PREFIX: &str = "my $x = 1;\n";
/// CRLF ordinary fixture: end of statement 1 body (before CRLF).
const CRLF_ORD_LINE1_END_PREFIX: &str = "my $x = 1;";
/// Bare-CR ordinary fixture: end of statement 1 body (before bare CR).
const CR_ORD_LINE1_END_PREFIX: &str = "my $x = 1;";
/// EOF fixture: exact end of source without a final newline.
const EOF_NO_NEWLINE_END_PREFIX: &str = "my $x = 1;\nmy $y = 2;";
/// Heredoc fixture: end of ordinary line 1 body (admitted contrast point).
const HEREDOC_ORD_LINE1_END_PREFIX: &str = "my $x = 1;";
/// Heredoc fixture: interior of the heredoc body payload.
const HEREDOC_BODY_MID_PREFIX: &str = "my $x = 1;\nmy $text = <<'EOF';\nbody";
/// Heredoc fixture: interior of the indentation-sensitive terminator line.
const HEREDOC_TERM_MID_PREFIX: &str = "my $x = 1;\nmy $text = <<'EOF';\nbody line\nEO";
/// Quote fixture: trailing edge of the multiline `q{...}` payload.
const QUOTE_QBODY_END_PREFIX: &str = "my $a = q{\ninner text";
/// Quote fixture: interior of the `q#...#` payload after the `#` delimiter.
const QUOTE_HASH_DELIM_PREFIX: &str = "my $a = q{\ninner text\n};\nmy $b = q#h";
/// Format/POD/DATA fixture: trailing edge of the format body geometry line.
const FPD_FORMAT_BODY_END_PREFIX: &str = "my $x = 1;\nformat STDOUT =\n @<<<<";
/// Format/POD/DATA fixture: trailing edge of the POD paragraph text.
const FPD_POD_TEXT_END_PREFIX: &str =
    "my $x = 1;\nformat STDOUT =\n @<<<<\n.\n\n=pod\n\nsample pod text";
/// Format/POD/DATA fixture: interior of the `__DATA__` payload line.
const FPD_DATA_PAYLOAD_END_PREFIX: &str = "my $x = 1;\nformat STDOUT =\n @<<<<\n.\n\n=pod\n\nsample pod text\n\n=cut\n\nmy $z = 3;\n__DATA__\ndata payload line";
/// BOM fixture: end of statement 1 body (the conversion is unsupported here).
const BOM_ORD_LINE1_END_PREFIX: &str = "\u{feff}my $x = 1;";
/// Malformed fixture: statement boundary behind the recovery obligation.
const MALFORMED_STMT2_START_PREFIX: &str = "my $x = ;\n";

/// Authored fixture bytes keyed by fixture identifier.
#[derive(Debug)]
pub struct AuthoredFixture {
    fixture_id: &'static str,
    bytes: &'static [u8],
    /// Pinned exact source identity (`sha256:<hex>`) of [`Self::bytes`].
    source_identity: &'static str,
}

const fn authored_fixture(
    fixture_id: &'static str,
    bytes: &'static str,
    source_identity: &'static str,
) -> AuthoredFixture {
    AuthoredFixture { fixture_id, bytes: bytes.as_bytes(), source_identity }
}

/// The authored fixture set. Ids are stable identities, never paths.
const AUTHORED_FIXTURES: &[AuthoredFixture] = &[
    authored_fixture(
        "registry-lf-ordinary",
        FIXTURE_LF_ORDINARY,
        "sha256:2f24b2cacd000d3eb4b0d618bd482e3637ba658ac7f0c313da4de664a6c12712",
    ),
    authored_fixture(
        "registry-crlf-ordinary",
        FIXTURE_CRLF_ORDINARY,
        "sha256:2c6bedf960495851d5cdcf5d4ce94a87708285ccd9dc8b91639608a741c06394",
    ),
    authored_fixture(
        "registry-cr-ordinary",
        FIXTURE_CR_ORDINARY,
        "sha256:9f9f72fccd4d522672cbff2b3a0ec5eafc4b66ca21c6606f93cdc1e7d5bb3191",
    ),
    authored_fixture(
        "registry-eof-no-newline",
        FIXTURE_EOF_NO_NEWLINE,
        "sha256:652abc54310d4677fb7988a0a38a4007016e0892a14724c7c46c4b0f45960e88",
    ),
    authored_fixture(
        "registry-lf-region",
        FIXTURE_LF_REGION,
        "sha256:e5137c3e9477b17be2a241ecf05ccb765d4fe0637ee4997fc6ab8e494b56fd06",
    ),
    authored_fixture(
        "registry-crlf-region",
        FIXTURE_CRLF_REGION,
        "sha256:387c9cc19abe08a9d07b1511246b436c843745b6d018045ff5fbf14a563e483b",
    ),
    authored_fixture(
        "registry-heredoc-mixed",
        FIXTURE_HEREDOC_MIXED,
        "sha256:d220cabfc1acd4e5397f69f0f906e5ea775a4a2b0a5350b43372aff79c731af3",
    ),
    authored_fixture(
        "registry-quote-payload",
        FIXTURE_QUOTE_PAYLOAD,
        "sha256:8e8079940601bcbf0ee982555ed024d23e8987f23d84457959f9cd51ec93feb3",
    ),
    authored_fixture(
        "registry-format-pod-data",
        FIXTURE_FORMAT_POD_DATA,
        "sha256:744ae9a3c81a5a3ff11dc90a4d20e039342957ad513d271b999e6d6698db1f15",
    ),
    authored_fixture(
        "registry-bom-ordinary",
        FIXTURE_BOM_ORDINARY,
        "sha256:ca4524552536aa7d7819ae5043b20ad9dd5f4c6e3c97745b9edf10e2a2bd79c3",
    ),
    authored_fixture(
        "registry-malformed",
        FIXTURE_MALFORMED,
        "sha256:cf34aa555da3972970b61b8f23b4cd49f85b1c35308a6d42efdf13319c2f45bc",
    ),
];

// ---------------------------------------------------------------------------
// Closed vocabularies
// ---------------------------------------------------------------------------

/// Terminal applicability state of one declared case (#13659).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Applicability {
    /// The exact authored proposition is admitted for generation.
    Admitted,
    /// The authored point is inside a declared fail-closed boundary.
    NotApplicable,
    /// The transformation family cannot express the authored proposition.
    UnsupportedTransformation,
    /// Invariance cannot currently be proven for the authored proposition.
    NotProven,
}

impl Applicability {
    /// Stable snake_case identity of the state.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::NotApplicable => "not_applicable",
            Self::UnsupportedTransformation => "unsupported_transformation",
            Self::NotProven => "not_proven",
        }
    }
}

/// Applicability state plus its explicit terminal reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplicabilityDeclaration {
    /// Terminal state.
    pub state: Applicability,
    /// Explicit reason code; never empty for non-admitted states.
    pub reason: &'static str,
}

impl ApplicabilityDeclaration {
    /// Declare an applicability with its terminal reason.
    pub const fn new(state: Applicability, reason: &'static str) -> Self {
        Self { state, reason }
    }
}

/// Required comparison planes for one case (consumed by #13662, not compared
/// here). The list is closed; parser outputs never select it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ComparisonPlane {
    /// Token stream equality.
    TokenStream,
    /// AST structure equality.
    Ast,
    /// Structural invariant equality.
    StructuralInvariants,
    /// Diagnostic content equality.
    Diagnostics,
    /// Recovery family equality.
    Recovery,
    /// Declared semantic fact equality.
    SemanticFacts,
    /// Byte-coordinate payload relation.
    CoordinatePayload,
}

/// Allowed coordinate/presentation differences for one case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllowedDifferences {
    /// Comparison-plane coordinates may move through the #13657 map.
    pub coordinate_map: bool,
    /// Newline presentation may differ when the proposition owns it.
    pub line_ending_presentation: bool,
}

impl AllowedDifferences {
    /// Differences for anchored point insertions: coordinates shift.
    pub const COORDINATE_SHIFT: Self =
        Self { coordinate_map: true, line_ending_presentation: false };
    /// Differences for newline-style region conversions.
    pub const NEWLINE_PRESENTATION: Self =
        Self { coordinate_map: true, line_ending_presentation: true };
}

/// Review criticality of one case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Criticality {
    /// Required registry population; a failure blocks admission accounting.
    Required,
    /// Investigatory case retained for calibration evidence.
    Investigatory,
}

/// Authored anchor of one case: an exact point insertion or an exact region
/// conversion. Offsets are base-source byte coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthoredAnchor {
    /// Insert an exact payload at one exact byte offset.
    Point {
        /// Stable authored point identity.
        anchor_id: &'static str,
        /// Exact base byte offset of the insertion point.
        offset: usize,
        /// Exact inserted bytes.
        payload: &'static [u8],
    },
    /// Convert newline style across one exact byte region.
    Region {
        /// Stable authored region identity.
        region_id: &'static str,
        /// Exact half-open byte region.
        byte_range: ByteRange,
        /// Declared conversion direction.
        conversion: NewlineConversion,
        /// Whether the region is explicitly declared newline-insensitive.
        newline_insensitive: bool,
    },
}

/// Newline conversion direction of a region proposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewlineConversion {
    /// LF → CRLF across the declared region.
    LfToCrLf,
    /// CRLF → LF across the declared region.
    CrLfToLf,
}

/// One declared case (#13659 case identity, all fields authored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseDeclaration {
    /// Stable case identity; independent of registry enumeration order.
    pub case_id: &'static str,
    /// Authored fixture this case is bound to.
    pub fixture_id: &'static str,
    /// Pinned exact source identity the case was authored against.
    pub source_identity: &'static str,
    /// Versioned transformation profile.
    pub profile_id: &'static str,
    /// Authored point or region anchor.
    pub anchor: AuthoredAnchor,
    /// Required comparison planes.
    pub required_planes: &'static [ComparisonPlane],
    /// Allowed coordinate/presentation differences.
    pub allowed_differences: AllowedDifferences,
    /// Review criticality.
    pub criticality: Criticality,
    /// Terminal applicability state and reason.
    pub applicability: ApplicabilityDeclaration,
    /// Review provenance.
    pub owner: &'static str,
    /// Opposite-direction control case, when the proposition admits a payload
    /// region (fail-closed boundary contract of #13659).
    pub opposite_control: Option<&'static str>,
}

/// Construction or evaluation error of the registry itself.
#[derive(Debug)]
pub enum RegistryError {
    /// Two declarations reused one `case_id`.
    DuplicateCaseId {
        /// Duplicated identity.
        case_id: String,
    },
    /// A declaration referenced an unknown fixture identity.
    UnknownFixture {
        /// Owning case.
        case_id: String,
        /// Referenced fixture identity.
        fixture_id: String,
    },
    /// An anchor was structurally invalid for its fixture.
    InvalidAnchor {
        /// Owning case.
        case_id: String,
        /// Failure description.
        detail: String,
    },
    /// A declaration violated a construction invariant.
    InvalidDeclaration {
        /// Owning case.
        case_id: String,
        /// Failure description.
        detail: String,
    },
    /// Evaluation was requested for an unknown fixture.
    UnknownFixtureRef {
        /// Referenced fixture identity.
        fixture_id: String,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateCaseId { case_id } => {
                write!(formatter, "duplicate case id {case_id:?}")
            }
            Self::UnknownFixture { case_id, fixture_id } => {
                write!(formatter, "case {case_id:?} references unknown fixture {fixture_id:?}")
            }
            Self::InvalidAnchor { case_id, detail } => {
                write!(formatter, "case {case_id:?} has an invalid anchor: {detail}")
            }
            Self::InvalidDeclaration { case_id, detail } => {
                write!(formatter, "case {case_id:?} is invalid: {detail}")
            }
            Self::UnknownFixtureRef { fixture_id } => {
                write!(formatter, "evaluation requested for unknown fixture {fixture_id:?}")
            }
        }
    }
}

impl Error for RegistryError {}

/// Typed classification of an admitted case whose edit plan failed through the
/// #13657 substrate. Stale anchors surface as [`Self::WrongExpectedBytes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformationFailureClass {
    /// Expected old bytes at the anchor no longer match the source.
    WrongExpectedBytes,
    /// Anchor exceeded the exact source bounds.
    OutOfBounds,
    /// Anchored ranges overlapped.
    OverlappingEdits,
    /// Insertion shared or entered another edit boundary.
    AmbiguousEditBoundary,
    /// Identity or profile material was rejected.
    IdentityRejected,
    /// The generated plan was empty.
    EmptyEditPlan,
    /// Source or replacement bytes were not valid UTF-8.
    Utf8,
    /// Canonical line geometry rejected the subject or result.
    Geometry,
    /// Byte arithmetic overflowed.
    Overflow,
    /// The edit would not change exact bytes.
    NoOp,
    /// Any other substrate failure.
    Other,
}

impl From<&TransformError> for TransformationFailureClass {
    fn from(error: &TransformError) -> Self {
        match error {
            TransformError::WrongExpectedBytes { .. } => Self::WrongExpectedBytes,
            TransformError::OutOfBounds { .. } => Self::OutOfBounds,
            TransformError::OverlappingEdits { .. } => Self::OverlappingEdits,
            TransformError::AmbiguousEditBoundary { .. } => Self::AmbiguousEditBoundary,
            TransformError::StaleSourceIdentity { .. }
            | TransformError::InvalidProfileId { .. }
            | TransformError::InvalidEditId { .. }
            | TransformError::DuplicateEditId { .. }
            | TransformError::ReversedRange { .. } => Self::IdentityRejected,
            TransformError::EmptyEditPlan => Self::EmptyEditPlan,
            TransformError::InvalidSourceUtf8(_)
            | TransformError::InvalidReplacementUtf8 { .. }
            | TransformError::InvalidFinalUtf8(_) => Self::Utf8,
            TransformError::InvalidSourceGeometry(_) | TransformError::InvalidFinalGeometry(_) => {
                Self::Geometry
            }
            TransformError::ArithmeticOverflow => Self::Overflow,
            TransformError::NoOpEdit { .. } => Self::NoOp,
            TransformError::InteriorUtf8Boundary { .. } => Self::OutOfBounds,
            TransformError::Serialize(_) => Self::Other,
        }
    }
}

/// One typed accounting outcome for one declared case. Every declared case
/// produces exactly one outcome; nothing is skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseOutcome {
    /// Admitted case whose generated plan was validated through #13657.
    Applied {
        /// Owning case identity.
        case_id: String,
        /// Transformation profile.
        profile_id: String,
        /// Fully validated transformation receipt.
        transformation: Box<ValidatedTransformation>,
    },
    /// Admitted case whose anchor or plan failed transformation.
    TransformationFailure {
        /// Owning case identity.
        case_id: String,
        /// Transformation profile.
        profile_id: String,
        /// Typed failure class.
        class: TransformationFailureClass,
        /// Substrate failure description.
        detail: String,
    },
    /// Declared source identity no longer matches exact bytes.
    StaleSource {
        /// Owning case identity.
        case_id: String,
        /// Transformation profile.
        profile_id: String,
        /// Identity the case was authored against.
        claimed: String,
        /// Identity of the exact supplied bytes.
        observed: String,
    },
    /// Non-admitted case retained with its terminal disposition.
    Dispositioned {
        /// Owning case identity.
        case_id: String,
        /// Transformation profile.
        profile_id: String,
        /// Terminal applicability state (never admitted here).
        state: Applicability,
        /// Explicit terminal reason.
        reason: &'static str,
    },
}

impl CaseOutcome {
    /// Owning case identity.
    #[must_use]
    pub fn case_id(&self) -> &str {
        match self {
            Self::Applied { case_id, .. }
            | Self::TransformationFailure { case_id, .. }
            | Self::StaleSource { case_id, .. }
            | Self::Dispositioned { case_id, .. } => case_id,
        }
    }

    /// Transformation profile of the outcome.
    #[must_use]
    pub fn profile_id(&self) -> &str {
        match self {
            Self::Applied { profile_id, .. }
            | Self::TransformationFailure { profile_id, .. }
            | Self::StaleSource { profile_id, .. }
            | Self::Dispositioned { profile_id, .. } => profile_id,
        }
    }
}

/// One registry inconsistency found by the integrity consult.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryInconsistency {
    /// Owning case identity, or `"(construction)"` for registry-level faults.
    pub case_id: String,
    /// Inconsistency description.
    pub detail: String,
}

/// A point-admission consult request against the authored registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointRequest<'a> {
    /// Authored fixture identity.
    pub fixture_id: &'a str,
    /// Transformation profile.
    pub profile_id: &'a str,
    /// Base byte offset of the candidate point.
    pub offset: usize,
}

/// Fail-closed admission decision for one point request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointDecision {
    /// The exact point is covered by one admitted declaration.
    Admitted {
        /// Covering case identity.
        case_id: &'static str,
    },
    /// The point is not admitted; generation must not occur.
    NotRegistered {
        /// Why the registry rejected the point.
        reason: UnregisteredReason,
    },
}

/// Typed rejection reasons for unregistered points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnregisteredReason {
    /// The fixture identity is not authored.
    UnknownFixture,
    /// No declaration exists for that fixture and profile.
    UnknownProfile,
    /// The offset is outside every registered safe point/region.
    OffsetOutsideEveryRegisteredSafePoint,
    /// The offset is past the end of the authored fixture.
    OffsetOutOfBounds,
}

/// Registry of authored metamorphic cases keyed by `case_id`.
///
/// Construction order never affects identity or evaluation output: cases are
/// held in a `BTreeMap` ordered by `case_id`.
#[derive(Debug, Clone)]
pub struct MetamorphicSafeRegistry {
    fixtures: BTreeMap<&'static str, &'static AuthoredFixture>,
    cases: BTreeMap<&'static str, CaseDeclaration>,
}

impl MetamorphicSafeRegistry {
    /// Construct a registry from declarations, validating construction
    /// invariants. Digests and anchors are *not* resolved here; evaluation and
    /// the integrity consult fail closed on drift.
    pub fn from_declarations(cases: Vec<CaseDeclaration>) -> Result<Self, RegistryError> {
        let fixtures: BTreeMap<&'static str, &'static AuthoredFixture> =
            AUTHORED_FIXTURES.iter().map(|fixture| (fixture.fixture_id, fixture)).collect();
        let mut registry = Self { fixtures, cases: BTreeMap::new() };
        for case in cases {
            registry.insert_case(case)?;
        }
        Ok(registry)
    }

    fn insert_case(&mut self, case: CaseDeclaration) -> Result<(), RegistryError> {
        if case.case_id.is_empty() {
            return Err(RegistryError::InvalidDeclaration {
                case_id: String::new(),
                detail: "case id is empty".to_owned(),
            });
        }
        if !self.fixtures.contains_key(case.fixture_id) {
            return Err(RegistryError::UnknownFixture {
                case_id: case.case_id.to_owned(),
                fixture_id: case.fixture_id.to_owned(),
            });
        }
        if case.applicability.reason.is_empty() {
            return Err(RegistryError::InvalidDeclaration {
                case_id: case.case_id.to_owned(),
                detail: "applicability reason is empty".to_owned(),
            });
        }
        if case.required_planes.is_empty() {
            return Err(RegistryError::InvalidDeclaration {
                case_id: case.case_id.to_owned(),
                detail: "required comparison planes are empty".to_owned(),
            });
        }
        if case.applicability.state == Applicability::Admitted {
            self.validate_admitted_anchor(&case)?;
        }
        let case_id = case.case_id.to_owned();
        if self.cases.insert(case.case_id, case).is_some() {
            return Err(RegistryError::DuplicateCaseId { case_id });
        }
        Ok(())
    }

    fn validate_admitted_anchor(&self, case: &CaseDeclaration) -> Result<(), RegistryError> {
        let fixture_len =
            self.fixtures.get(case.fixture_id).map_or(0, |fixture| fixture.bytes.len());
        match case.anchor {
            AuthoredAnchor::Point { anchor_id, offset, payload } => {
                if anchor_id.is_empty() {
                    return Err(RegistryError::InvalidAnchor {
                        case_id: case.case_id.to_owned(),
                        detail: "point anchor id is empty".to_owned(),
                    });
                }
                if payload.is_empty() {
                    return Err(RegistryError::InvalidAnchor {
                        case_id: case.case_id.to_owned(),
                        detail: "point payload is empty".to_owned(),
                    });
                }
                if offset > fixture_len {
                    return Err(RegistryError::InvalidAnchor {
                        case_id: case.case_id.to_owned(),
                        detail: format!(
                            "point offset {offset} exceeds fixture length {fixture_len}"
                        ),
                    });
                }
            }
            AuthoredAnchor::Region { region_id, byte_range, conversion, newline_insensitive } => {
                if region_id.is_empty() {
                    return Err(RegistryError::InvalidAnchor {
                        case_id: case.case_id.to_owned(),
                        detail: "region anchor id is empty".to_owned(),
                    });
                }
                if byte_range.start > byte_range.end || byte_range.end > fixture_len {
                    return Err(RegistryError::InvalidAnchor {
                        case_id: case.case_id.to_owned(),
                        detail: format!(
                            "region {}..{} does not fit fixture length {fixture_len}",
                            byte_range.start, byte_range.end
                        ),
                    });
                }
                if !newline_insensitive {
                    return Err(RegistryError::InvalidAnchor {
                        case_id: case.case_id.to_owned(),
                        detail: "region conversion without a newline-insensitive declaration"
                            .to_owned(),
                    });
                }
                if conversion_sites(
                    fixture_bytes_of(self.fixtures.get(case.fixture_id)),
                    byte_range,
                    conversion,
                )
                .is_empty()
                {
                    return Err(RegistryError::InvalidAnchor {
                        case_id: case.case_id.to_owned(),
                        detail: "region declares no conversion sites".to_owned(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Authored fixture bytes for one fixture identity.
    #[must_use]
    pub fn fixture_bytes(&self, fixture_id: &str) -> Option<&'static [u8]> {
        self.fixtures.get(fixture_id).map(|fixture| fixture.bytes)
    }

    /// Authored fixture identities in fixture-id sorted order.
    #[must_use]
    pub fn fixture_ids(&self) -> Vec<&'static str> {
        self.fixtures.keys().copied().collect()
    }

    /// All case identities in sorted (enumeration-independent) order.
    #[must_use]
    pub fn case_ids(&self) -> Vec<&'static str> {
        self.cases.keys().copied().collect()
    }

    /// Declaration for one case identity.
    #[must_use]
    pub fn declaration(&self, case_id: &str) -> Option<&CaseDeclaration> {
        self.cases.get(case_id)
    }

    /// Declared case count.
    #[must_use]
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// The admission consult: fail-closed decision for one candidate point.
    ///
    /// Only an admitted declaration whose exact anchor covers the request
    /// admits the point. Boundary marker bytes elsewhere in the fixture never
    /// influence the decision in either direction.
    #[must_use]
    pub fn admission(&self, request: &PointRequest<'_>) -> PointDecision {
        let Some(bytes) = self.fixture_bytes(request.fixture_id) else {
            return PointDecision::NotRegistered { reason: UnregisteredReason::UnknownFixture };
        };
        if request.offset > bytes.len() {
            return PointDecision::NotRegistered { reason: UnregisteredReason::OffsetOutOfBounds };
        }
        let mut profile_known = false;
        for case in self.cases.values() {
            if case.fixture_id != request.fixture_id || case.profile_id != request.profile_id {
                continue;
            }
            profile_known = true;
            if case.applicability.state != Applicability::Admitted {
                continue;
            }
            let covers = match case.anchor {
                AuthoredAnchor::Point { offset, .. } => offset == request.offset,
                AuthoredAnchor::Region { byte_range, .. } => {
                    byte_range.start <= request.offset && request.offset < byte_range.end
                }
            };
            if covers {
                return PointDecision::Admitted { case_id: case.case_id };
            }
        }
        PointDecision::NotRegistered {
            reason: if profile_known {
                UnregisteredReason::OffsetOutsideEveryRegisteredSafePoint
            } else {
                UnregisteredReason::UnknownProfile
            },
        }
    }

    /// Generated exact edit plan for one admitted declaration.
    ///
    /// Returns `None` for non-admitted declarations: generation only occurs for
    /// admitted cases, and every generated edit carries the exact expected old
    /// bytes at its authored anchor.
    #[must_use]
    pub fn edit_plan(&self, case_id: &str) -> Option<Vec<ExactEdit>> {
        let case = self.cases.get(case_id)?;
        if case.applicability.state != Applicability::Admitted {
            return None;
        }
        let bytes = self.fixture_bytes(case.fixture_id)?;
        match case.anchor {
            AuthoredAnchor::Point { offset, payload, .. } => Some(vec![ExactEdit::new(
                format!("{}.apply", case.case_id),
                offset,
                offset,
                Vec::new(),
                payload.to_vec(),
            )]),
            AuthoredAnchor::Region { byte_range, conversion, .. } => {
                let edits = conversion_sites(bytes, byte_range, conversion)
                    .into_iter()
                    .enumerate()
                    .map(|(index, offset)| match conversion {
                        NewlineConversion::LfToCrLf => ExactEdit::new(
                            format!("{}.lf-{index}", case.case_id),
                            offset,
                            offset + 1,
                            b"\n".to_vec(),
                            b"\r\n".to_vec(),
                        ),
                        NewlineConversion::CrLfToLf => ExactEdit::new(
                            format!("{}.crlf-{index}", case.case_id),
                            offset,
                            offset + 2,
                            b"\r\n".to_vec(),
                            b"\n".to_vec(),
                        ),
                    })
                    .collect();
                Some(edits)
            }
        }
    }

    /// Evaluate every declared case against the authored fixture bytes.
    ///
    /// The output is deterministic: one outcome per declaration, ordered by
    /// `case_id`, byte-identical across repeated and shuffled constructions.
    #[must_use]
    pub fn evaluate(&self) -> Vec<CaseOutcome> {
        self.cases
            .values()
            .filter_map(|case| {
                let bytes = self.fixture_bytes(case.fixture_id)?;
                Some(self.outcome_for(case, bytes))
            })
            .collect()
    }

    /// Evaluate every declaration bound to one fixture against exact supplied
    /// bytes. A source whose digest no longer matches a pinned declaration
    /// identity fails that declaration closed as [`CaseOutcome::StaleSource`].
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::UnknownFixtureRef`] for an unauthored fixture.
    pub fn evaluate_source(
        &self,
        fixture_id: &str,
        source: &[u8],
    ) -> Result<Vec<CaseOutcome>, RegistryError> {
        if !self.fixtures.contains_key(fixture_id) {
            return Err(RegistryError::UnknownFixtureRef { fixture_id: fixture_id.to_owned() });
        }
        Ok(self
            .cases
            .values()
            .filter(|case| case.fixture_id == fixture_id)
            .map(|case| self.outcome_for(case, source))
            .collect())
    }

    fn outcome_for(&self, case: &CaseDeclaration, source: &[u8]) -> CaseOutcome {
        let profile_id = case.profile_id.to_owned();
        let case_id = case.case_id.to_owned();
        let observed = sha256_hex(source);
        if observed != case.source_identity {
            return CaseOutcome::StaleSource {
                case_id,
                profile_id,
                claimed: case.source_identity.to_owned(),
                observed,
            };
        }
        if case.applicability.state != Applicability::Admitted {
            return CaseOutcome::Dispositioned {
                case_id,
                profile_id,
                state: case.applicability.state,
                reason: case.applicability.reason,
            };
        }
        let Some(edits) = self.edit_plan(case.case_id) else {
            return CaseOutcome::TransformationFailure {
                case_id,
                profile_id,
                class: TransformationFailureClass::EmptyEditPlan,
                detail: "admitted case produced no edit plan".to_owned(),
            };
        };
        let source_subject =
            ContentAddressedSource::from_claimed(case.source_identity.to_owned(), source.to_vec());
        let Ok(subject) = source_subject else {
            return CaseOutcome::TransformationFailure {
                case_id,
                profile_id,
                class: TransformationFailureClass::IdentityRejected,
                detail: "authored source identity could not construct a subject".to_owned(),
            };
        };
        match apply_exact_edits(&subject, case.profile_id, edits) {
            Ok(transformation) => CaseOutcome::Applied {
                case_id,
                profile_id,
                transformation: Box::new(transformation),
            },
            Err(error) => CaseOutcome::TransformationFailure {
                case_id,
                profile_id,
                class: TransformationFailureClass::from(&error),
                detail: error.to_string(),
            },
        }
    }

    /// Integrity consult over the authored surface: pinned digests against
    /// authored bytes, admitted anchors, and admitted outcomes. An empty
    /// report means every authored proposition resolves exactly.
    #[must_use]
    pub fn integrity_report(&self) -> Vec<RegistryInconsistency> {
        let mut report = Vec::new();
        for (fixture_id, fixture) in &self.fixtures {
            let observed = sha256_hex(fixture.bytes);
            if observed != fixture.source_identity {
                report.push(RegistryInconsistency {
                    case_id: format!("(fixture {fixture_id})"),
                    detail: format!(
                        "pinned identity {} does not match exact bytes {observed}",
                        fixture.source_identity
                    ),
                });
            }
        }
        for case in self.cases.values() {
            let Some(bytes) = self.fixture_bytes(case.fixture_id) else {
                report.push(RegistryInconsistency {
                    case_id: case.case_id.to_owned(),
                    detail: format!("unknown fixture {}", case.fixture_id),
                });
                continue;
            };
            match case.anchor {
                AuthoredAnchor::Point { offset, payload, .. } => {
                    if offset > bytes.len() {
                        report.push(RegistryInconsistency {
                            case_id: case.case_id.to_owned(),
                            detail: format!(
                                "point offset {offset} exceeds fixture length {}",
                                bytes.len()
                            ),
                        });
                    }
                    if case.applicability.state == Applicability::Admitted && payload.is_empty() {
                        report.push(RegistryInconsistency {
                            case_id: case.case_id.to_owned(),
                            detail: "admitted point payload is empty".to_owned(),
                        });
                    }
                }
                AuthoredAnchor::Region { byte_range, conversion, .. } => {
                    if byte_range.end > bytes.len() {
                        report.push(RegistryInconsistency {
                            case_id: case.case_id.to_owned(),
                            detail: format!(
                                "region end {} exceeds fixture length {}",
                                byte_range.end,
                                bytes.len()
                            ),
                        });
                    }
                    if conversion_sites(bytes, byte_range, conversion).is_empty() {
                        report.push(RegistryInconsistency {
                            case_id: case.case_id.to_owned(),
                            detail: "region declares no conversion sites".to_owned(),
                        });
                    }
                }
            }
        }
        for outcome in self.evaluate() {
            match outcome {
                CaseOutcome::StaleSource { case_id, claimed, observed, .. } => {
                    report.push(RegistryInconsistency {
                        case_id,
                        detail: format!(
                            "stale source identity: claimed {claimed}, observed {observed}"
                        ),
                    });
                }
                CaseOutcome::TransformationFailure { case_id, class, detail, .. } => {
                    report.push(RegistryInconsistency {
                        case_id,
                        detail: format!(
                            "admitted case failed transformation ({class:?}): {detail}"
                        ),
                    });
                }
                CaseOutcome::Applied { .. } | CaseOutcome::Dispositioned { .. } => {}
            }
        }
        report
    }
}

/// Bytes of an optional authored fixture reference (empty when absent).
fn fixture_bytes_of(fixture: Option<&&'static AuthoredFixture>) -> &'static [u8] {
    fixture.map_or(&[], |authored| authored.bytes)
}

/// Exact conversion-site offsets of one declared conversion inside a region.
fn conversion_sites(bytes: &[u8], range: ByteRange, conversion: NewlineConversion) -> Vec<usize> {
    let mut sites = Vec::new();
    let end = range.end.min(bytes.len());
    let mut offset = range.start.min(end);
    while offset < end {
        let site_len = match conversion {
            NewlineConversion::LfToCrLf => {
                if bytes[offset] == b'\n' {
                    1
                } else {
                    0
                }
            }
            NewlineConversion::CrLfToLf => {
                if bytes[offset] == b'\r' && offset + 1 < end && bytes[offset + 1] == b'\n' {
                    2
                } else {
                    0
                }
            }
        };
        if site_len > 0 {
            sites.push(offset);
            offset += site_len;
        } else {
            offset += 1;
        }
    }
    sites
}

// ---------------------------------------------------------------------------
// Authored registry: the initial required population of #13659
// ---------------------------------------------------------------------------

const fn admitted(
    case_id: &'static str,
    fixture_id: &'static str,
    profile_id: &'static str,
    anchor: AuthoredAnchor,
    allowed: AllowedDifferences,
    opposite_control: Option<&'static str>,
) -> CaseDeclaration {
    CaseDeclaration {
        case_id,
        fixture_id,
        source_identity: fixture_source_identity(fixture_id),
        profile_id,
        anchor,
        required_planes: REQUIRED_PLANES,
        allowed_differences: allowed,
        criticality: Criticality::Required,
        applicability: ApplicabilityDeclaration::new(
            Applicability::Admitted,
            "authored ordinary-code proposition with exact anchor",
        ),
        owner: REGISTRY_REVIEW_OWNER,
        opposite_control,
    }
}

const fn dispositioned(
    case_id: &'static str,
    fixture_id: &'static str,
    profile_id: &'static str,
    anchor: AuthoredAnchor,
    state: Applicability,
    reason: &'static str,
    criticality: Criticality,
) -> CaseDeclaration {
    CaseDeclaration {
        case_id,
        fixture_id,
        source_identity: fixture_source_identity(fixture_id),
        profile_id,
        anchor,
        required_planes: REQUIRED_PLANES,
        allowed_differences: AllowedDifferences::COORDINATE_SHIFT,
        criticality,
        applicability: ApplicabilityDeclaration::new(state, reason),
        owner: REGISTRY_REVIEW_OWNER,
        opposite_control: None,
    }
}

const fn fixture_source_identity(fixture_id: &str) -> &'static str {
    // Fixture identities are literal constants; resolve the pinned identity by
    // linear scan so the declaration tables stay the single authoring point.
    let mut index = 0;
    while index < AUTHORED_FIXTURES.len() {
        if fixture_matches(AUTHORED_FIXTURES[index].fixture_id, fixture_id) {
            return AUTHORED_FIXTURES[index].source_identity;
        }
        index += 1;
    }
    ""
}

const fn fixture_matches(authored: &str, requested: &str) -> bool {
    let authored_bytes = authored.as_bytes();
    let requested_bytes = requested.as_bytes();
    if authored_bytes.len() != requested_bytes.len() {
        return false;
    }
    let mut index = 0;
    while index < authored_bytes.len() {
        if authored_bytes[index] != requested_bytes[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// Required comparison planes shared by the authored population.
const REQUIRED_PLANES: &[ComparisonPlane] = &[
    ComparisonPlane::TokenStream,
    ComparisonPlane::Ast,
    ComparisonPlane::StructuralInvariants,
    ComparisonPlane::Diagnostics,
    ComparisonPlane::Recovery,
    ComparisonPlane::SemanticFacts,
    ComparisonPlane::CoordinatePayload,
];

/// Profiles of the authored initial population.
pub const PROFILE_TRAILING_HW: &str = "trailing-horizontal-whitespace.v1";
/// Blank-line insertion profile.
pub const PROFILE_BLANK_LINE: &str = "blank-line-insertion.v1";
/// Ordinary line-comment insertion profile.
pub const PROFILE_LINE_COMMENT: &str = "line-comment-insertion.v1";
/// LF → CRLF region conversion profile.
pub const PROFILE_LF_TO_CRLF: &str = "newline-style.lf-to-crlf.v1";
/// CRLF → LF region conversion profile (opposite-direction control).
pub const PROFILE_CRLF_TO_LF: &str = "newline-style.crlf-to-lf.v1";

/// The initial required population of #13659: trailing horizontal whitespace
/// before LF, CRLF, bare CR, and at EOF; blank-line insertion; ordinary
/// line-comment insertion; one whole-source LF→CRLF region conversion with its
/// opposite-direction control; the heredoc-fixture ordinary contrast point;
/// and the fail-closed boundary negatives beside quote-like, heredoc, format,
/// POD, `__DATA__`, BOM, and malformed recovery boundaries.
///
/// # Errors
///
/// Returns [`RegistryError`] if the authored declaration tables violate a
/// construction invariant; callers fail closed on the error.
pub fn authored_registry() -> Result<MetamorphicSafeRegistry, RegistryError> {
    MetamorphicSafeRegistry::from_declarations(AUTHORED_CASES.to_vec())
}

/// Integrity consult over the authored surface; empty means fully consistent.
///
/// The parser-accuracy generation consults this before scoring so a drifted
/// oracle registry fails the run closed instead of scoring against
/// unresolvable propositions.
#[must_use]
pub fn authored_registry_inconsistencies() -> Vec<RegistryInconsistency> {
    match authored_registry() {
        Ok(registry) => registry.integrity_report(),
        Err(error) => vec![RegistryInconsistency {
            case_id: "(construction)".to_owned(),
            detail: error.to_string(),
        }],
    }
}

const AUTHORED_CASES: &[CaseDeclaration] = &[
    // Population 1: trailing horizontal whitespace before LF.
    admitted(
        "registry-lf-ordinary.trailing-hw.line-1.v1",
        "registry-lf-ordinary",
        PROFILE_TRAILING_HW,
        AuthoredAnchor::Point {
            anchor_id: "line-1-before-lf",
            offset: LF_ORD_LINE1_END_PREFIX.len(),
            payload: TRAILING_TWO_SPACES,
        },
        AllowedDifferences::COORDINATE_SHIFT,
        None,
    ),
    // Population 2: trailing horizontal whitespace before CRLF.
    admitted(
        "registry-crlf-ordinary.trailing-hw.line-1.v1",
        "registry-crlf-ordinary",
        PROFILE_TRAILING_HW,
        AuthoredAnchor::Point {
            anchor_id: "line-1-before-crlf",
            offset: CRLF_ORD_LINE1_END_PREFIX.len(),
            payload: TRAILING_TWO_SPACES,
        },
        AllowedDifferences::COORDINATE_SHIFT,
        None,
    ),
    // Population 3: trailing horizontal whitespace before bare CR.
    admitted(
        "registry-cr-ordinary.trailing-hw.line-1.v1",
        "registry-cr-ordinary",
        PROFILE_TRAILING_HW,
        AuthoredAnchor::Point {
            anchor_id: "line-1-before-bare-cr",
            offset: CR_ORD_LINE1_END_PREFIX.len(),
            payload: TRAILING_TWO_SPACES,
        },
        AllowedDifferences::COORDINATE_SHIFT,
        None,
    ),
    // Population 4: trailing horizontal whitespace at EOF, no final newline.
    admitted(
        "registry-eof-no-newline.trailing-hw.eof.v1",
        "registry-eof-no-newline",
        PROFILE_TRAILING_HW,
        AuthoredAnchor::Point {
            anchor_id: "eof-insertion",
            offset: EOF_NO_NEWLINE_END_PREFIX.len(),
            payload: TRAILING_TWO_SPACES,
        },
        AllowedDifferences::COORDINATE_SHIFT,
        None,
    ),
    // Population 5: one blank logical line between complete statements.
    admitted(
        "registry-lf-ordinary.blank-line.stmt-1-2.v1",
        "registry-lf-ordinary",
        PROFILE_BLANK_LINE,
        AuthoredAnchor::Point {
            anchor_id: "stmt-1-2-boundary",
            offset: LF_ORD_STMT2_START_PREFIX.len(),
            payload: BLANK_LINE_LF,
        },
        AllowedDifferences::COORDINATE_SHIFT,
        None,
    ),
    // Population 6: one ordinary line comment between complete statements.
    admitted(
        "registry-lf-ordinary.line-comment.stmt-1-2.v1",
        "registry-lf-ordinary",
        PROFILE_LINE_COMMENT,
        AuthoredAnchor::Point {
            anchor_id: "stmt-1-2-boundary",
            offset: LF_ORD_STMT2_START_PREFIX.len(),
            payload: ORDINARY_LINE_COMMENT,
        },
        AllowedDifferences::COORDINATE_SHIFT,
        None,
    ),
    // Population 7: whole-source LF→CRLF with an explicit newline-insensitive
    // region declaration and its opposite-direction control (case below).
    admitted(
        "registry-lf-region.newline-style.lf-to-crlf.v1",
        "registry-lf-region",
        PROFILE_LF_TO_CRLF,
        AuthoredAnchor::Region {
            region_id: "whole-source",
            byte_range: ByteRange::new(0, FIXTURE_LF_REGION.len()),
            conversion: NewlineConversion::LfToCrLf,
            newline_insensitive: true,
        },
        AllowedDifferences::NEWLINE_PRESENTATION,
        Some("registry-crlf-region.newline-style.crlf-to-lf.control.v1"),
    ),
    // Population 7 control: the opposite-direction conversion, proven to
    // return the exact original LF bytes by the applicability proof.
    admitted(
        "registry-crlf-region.newline-style.crlf-to-lf.control.v1",
        "registry-crlf-region",
        PROFILE_CRLF_TO_LF,
        AuthoredAnchor::Region {
            region_id: "whole-source",
            byte_range: ByteRange::new(0, FIXTURE_CRLF_REGION.len()),
            conversion: NewlineConversion::CrLfToLf,
            newline_insensitive: true,
        },
        AllowedDifferences::NEWLINE_PRESENTATION,
        Some("registry-lf-region.newline-style.lf-to-crlf.v1"),
    ),
    // Negative 3 contrast: the heredoc marker does not remove the explicitly
    // registered ordinary point; registration is per authored point.
    admitted(
        "registry-heredoc-mixed.trailing-hw.ordinary-line-1.v1",
        "registry-heredoc-mixed",
        PROFILE_TRAILING_HW,
        AuthoredAnchor::Point {
            anchor_id: "line-1-before-lf",
            offset: HEREDOC_ORD_LINE1_END_PREFIX.len(),
            payload: TRAILING_TWO_SPACES,
        },
        AllowedDifferences::COORDINATE_SHIFT,
        None,
    ),
    // Negative 1: whitespace inside a multiline q{...} payload.
    dispositioned(
        "registry-quote-payload.trailing-hw.q-body.v1",
        "registry-quote-payload",
        PROFILE_TRAILING_HW,
        AuthoredAnchor::Point {
            anchor_id: "q-body-trailing-edge",
            offset: QUOTE_QBODY_END_PREFIX.len(),
            payload: TRAILING_TWO_SPACES,
        },
        Applicability::NotApplicable,
        "multiline-quote-payload-region",
        Criticality::Required,
    ),
    // Negative 2: a `#` quote delimiter is not a comment boundary.
    dispositioned(
        "registry-quote-payload.line-comment.hash-delimiter.v1",
        "registry-quote-payload",
        PROFILE_LINE_COMMENT,
        AuthoredAnchor::Point {
            anchor_id: "inside-hash-delimited-quote",
            offset: QUOTE_HASH_DELIM_PREFIX.len(),
            payload: ORDINARY_LINE_COMMENT,
        },
        Applicability::NotApplicable,
        "quote-delimiter-not-comment-boundary",
        Criticality::Required,
    ),
    // Negative 3: the heredoc body payload itself.
    dispositioned(
        "registry-heredoc-mixed.trailing-hw.heredoc-body.v1",
        "registry-heredoc-mixed",
        PROFILE_TRAILING_HW,
        AuthoredAnchor::Point {
            anchor_id: "heredoc-body-interior",
            offset: HEREDOC_BODY_MID_PREFIX.len(),
            payload: TRAILING_TWO_SPACES,
        },
        Applicability::NotApplicable,
        "heredoc-payload-region",
        Criticality::Required,
    ),
    // Negative 4: the indentation-sensitive heredoc terminator boundary.
    dispositioned(
        "registry-heredoc-mixed.trailing-hw.heredoc-terminator.v1",
        "registry-heredoc-mixed",
        PROFILE_TRAILING_HW,
        AuthoredAnchor::Point {
            anchor_id: "heredoc-terminator-interior",
            offset: HEREDOC_TERM_MID_PREFIX.len(),
            payload: TRAILING_TWO_SPACES,
        },
        Applicability::NotApplicable,
        "heredoc-terminator-indentation-boundary",
        Criticality::Required,
    ),
    // Negative 5: format-body column geometry is not ordinary whitespace.
    dispositioned(
        "registry-format-pod-data.trailing-hw.format-body.v1",
        "registry-format-pod-data",
        PROFILE_TRAILING_HW,
        AuthoredAnchor::Point {
            anchor_id: "format-body-column-line",
            offset: FPD_FORMAT_BODY_END_PREFIX.len(),
            payload: TRAILING_TWO_SPACES,
        },
        Applicability::NotApplicable,
        "format-body-column-geometry",
        Criticality::Required,
    ),
    // Negative 6a: POD payload changed under a code-only proposition.
    dispositioned(
        "registry-format-pod-data.line-comment.pod-block.v1",
        "registry-format-pod-data",
        PROFILE_LINE_COMMENT,
        AuthoredAnchor::Point {
            anchor_id: "pod-paragraph-text",
            offset: FPD_POD_TEXT_END_PREFIX.len(),
            payload: ORDINARY_LINE_COMMENT,
        },
        Applicability::NotApplicable,
        "pod-payload-region",
        Criticality::Required,
    ),
    // Negative 6b: __DATA__ payload changed under a code-only proposition.
    dispositioned(
        "registry-format-pod-data.trailing-hw.data-payload.v1",
        "registry-format-pod-data",
        PROFILE_TRAILING_HW,
        AuthoredAnchor::Point {
            anchor_id: "data-payload-line",
            offset: FPD_DATA_PAYLOAD_END_PREFIX.len(),
            payload: TRAILING_TWO_SPACES,
        },
        Applicability::NotApplicable,
        "data-end-payload-region",
        Criticality::Required,
    ),
    // Negative 7: comment insertion beside a malformed recovery obligation is
    // not proven invariance; the case stays in accounting as not_proven.
    dispositioned(
        "registry-malformed.line-comment.recovery-boundary.v1",
        "registry-malformed",
        PROFILE_LINE_COMMENT,
        AuthoredAnchor::Point {
            anchor_id: "stmt-boundary-after-malformed-line",
            offset: MALFORMED_STMT2_START_PREFIX.len(),
            payload: ORDINARY_LINE_COMMENT,
        },
        Applicability::NotProven,
        "malformed-recovery-obligation",
        Criticality::Investigatory,
    ),
    // Unsupported: BOM-prefixed sources are not declared newline-insensitive,
    // so the family cannot express a BOM-preserving conversion.
    dispositioned(
        "registry-bom-ordinary.newline-style.lf-to-crlf.v1",
        "registry-bom-ordinary",
        PROFILE_LF_TO_CRLF,
        AuthoredAnchor::Point {
            anchor_id: "line-1-before-lf",
            offset: BOM_ORD_LINE1_END_PREFIX.len(),
            payload: TRAILING_TWO_SPACES,
        },
        Applicability::UnsupportedTransformation,
        "bom-prefixed-source-not-declared-newline-insensitive",
        Criticality::Investigatory,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn Error>>;

    fn authored() -> Result<MetamorphicSafeRegistry, Box<dyn Error>> {
        Ok(authored_registry()?)
    }

    const ADMITTED_EXPECTED: usize = 9;
    const DISPOSITIONED_EXPECTED: usize = 9;

    #[test]
    fn authored_registry_construction_is_valid() -> TestResult {
        let registry = authored_registry()?;
        assert_eq!(registry.case_count(), AUTHORED_CASES.len());
        assert!(!registry.case_ids().is_empty());
        assert_eq!(registry.case_ids().len(), registry.case_count());
        Ok(())
    }

    #[test]
    fn authored_pinned_source_identities_match_exact_fixture_bytes() -> TestResult {
        let registry = authored()?;
        for fixture_id in registry.fixture_ids() {
            let bytes = registry
                .fixture_bytes(fixture_id)
                .ok_or_else(|| format!("missing fixture {fixture_id}"))?;
            let declared = AUTHORED_FIXTURES
                .iter()
                .find(|fixture| fixture.fixture_id == fixture_id)
                .ok_or_else(|| format!("undeclared fixture {fixture_id}"))?;
            assert_eq!(sha256_hex(bytes), declared.source_identity, "fixture {fixture_id}");
        }
        Ok(())
    }

    #[test]
    fn authored_anchor_prefixes_resolve_to_expected_anchor_bytes() -> TestResult {
        let anchor_expectations: &[(&str, &str, Option<u8>)] = &[
            (FIXTURE_LF_ORDINARY, LF_ORD_LINE1_END_PREFIX, Some(b'\n')),
            (FIXTURE_LF_ORDINARY, LF_ORD_STMT2_START_PREFIX, Some(b'm')),
            (FIXTURE_CRLF_ORDINARY, CRLF_ORD_LINE1_END_PREFIX, Some(b'\r')),
            (FIXTURE_CR_ORDINARY, CR_ORD_LINE1_END_PREFIX, Some(b'\r')),
            (FIXTURE_EOF_NO_NEWLINE, EOF_NO_NEWLINE_END_PREFIX, None),
            (FIXTURE_HEREDOC_MIXED, HEREDOC_ORD_LINE1_END_PREFIX, Some(b'\n')),
            (FIXTURE_HEREDOC_MIXED, HEREDOC_BODY_MID_PREFIX, Some(b' ')),
            (FIXTURE_HEREDOC_MIXED, HEREDOC_TERM_MID_PREFIX, Some(b'F')),
            (FIXTURE_QUOTE_PAYLOAD, QUOTE_QBODY_END_PREFIX, Some(b'\n')),
            (FIXTURE_QUOTE_PAYLOAD, QUOTE_HASH_DELIM_PREFIX, Some(b'a')),
            (FIXTURE_FORMAT_POD_DATA, FPD_FORMAT_BODY_END_PREFIX, Some(b'\n')),
            (FIXTURE_FORMAT_POD_DATA, FPD_POD_TEXT_END_PREFIX, Some(b'\n')),
            (FIXTURE_FORMAT_POD_DATA, FPD_DATA_PAYLOAD_END_PREFIX, Some(b'\n')),
            (FIXTURE_BOM_ORDINARY, BOM_ORD_LINE1_END_PREFIX, Some(b'\n')),
            (FIXTURE_MALFORMED, MALFORMED_STMT2_START_PREFIX, Some(b'm')),
        ];
        for (fixture, prefix, anchor_byte) in anchor_expectations {
            assert!(
                fixture.starts_with(prefix),
                "prefix {prefix:?} is not a prefix of its fixture"
            );
            let offset = prefix.len();
            assert_eq!(
                fixture.as_bytes().get(offset).copied(),
                *anchor_byte,
                "unexpected anchor byte at offset {offset} of {fixture:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn authored_integrity_report_is_empty_and_outcomes_are_fully_accounted() -> TestResult {
        let registry = authored()?;
        assert!(registry.integrity_report().is_empty());
        let outcomes = registry.evaluate();
        assert_eq!(outcomes.len(), registry.case_count());
        let mut applied = 0;
        let mut dispositioned = 0;
        for outcome in &outcomes {
            match outcome {
                CaseOutcome::Applied { .. } => applied += 1,
                CaseOutcome::Dispositioned { .. } => dispositioned += 1,
                CaseOutcome::StaleSource { case_id, .. }
                | CaseOutcome::TransformationFailure { case_id, .. } => {
                    return Err(format!("unexpected failure outcome for {case_id}").into());
                }
            }
        }
        assert_eq!(applied, ADMITTED_EXPECTED);
        assert_eq!(dispositioned, DISPOSITIONED_EXPECTED);
        Ok(())
    }

    #[test]
    fn dispositioned_cases_carry_explicit_terminal_reasons() -> TestResult {
        let registry = authored()?;
        let expected_reasons: &[(&str, Applicability, &str)] = &[
            (
                "registry-quote-payload.trailing-hw.q-body.v1",
                Applicability::NotApplicable,
                "multiline-quote-payload-region",
            ),
            (
                "registry-quote-payload.line-comment.hash-delimiter.v1",
                Applicability::NotApplicable,
                "quote-delimiter-not-comment-boundary",
            ),
            (
                "registry-heredoc-mixed.trailing-hw.heredoc-body.v1",
                Applicability::NotApplicable,
                "heredoc-payload-region",
            ),
            (
                "registry-heredoc-mixed.trailing-hw.heredoc-terminator.v1",
                Applicability::NotApplicable,
                "heredoc-terminator-indentation-boundary",
            ),
            (
                "registry-format-pod-data.trailing-hw.format-body.v1",
                Applicability::NotApplicable,
                "format-body-column-geometry",
            ),
            (
                "registry-format-pod-data.line-comment.pod-block.v1",
                Applicability::NotApplicable,
                "pod-payload-region",
            ),
            (
                "registry-format-pod-data.trailing-hw.data-payload.v1",
                Applicability::NotApplicable,
                "data-end-payload-region",
            ),
            (
                "registry-malformed.line-comment.recovery-boundary.v1",
                Applicability::NotProven,
                "malformed-recovery-obligation",
            ),
            (
                "registry-bom-ordinary.newline-style.lf-to-crlf.v1",
                Applicability::UnsupportedTransformation,
                "bom-prefixed-source-not-declared-newline-insensitive",
            ),
        ];
        for (case_id, state, reason) in expected_reasons {
            let case = registry
                .declaration(case_id)
                .ok_or_else(|| format!("missing declared case {case_id}"))?;
            assert_eq!(case.applicability.state, *state, "state drift for {case_id}");
            assert_eq!(case.applicability.reason, *reason, "reason drift for {case_id}");
        }
        Ok(())
    }

    #[test]
    fn point_admission_is_fail_closed_outside_registered_regions() -> TestResult {
        let registry = authored()?;

        // Registered and admitted points admit with their covering case.
        assert_eq!(
            registry.admission(&PointRequest {
                fixture_id: "registry-lf-ordinary",
                profile_id: PROFILE_TRAILING_HW,
                offset: LF_ORD_LINE1_END_PREFIX.len(),
            }),
            PointDecision::Admitted { case_id: "registry-lf-ordinary.trailing-hw.line-1.v1" }
        );
        // A region conversion admits anywhere inside the declared region.
        assert_eq!(
            registry.admission(&PointRequest {
                fixture_id: "registry-lf-region",
                profile_id: PROFILE_LF_TO_CRLF,
                offset: 5,
            }),
            PointDecision::Admitted { case_id: "registry-lf-region.newline-style.lf-to-crlf.v1" }
        );

        // Unregistered offsets inside an authored fixture are rejected.
        assert_eq!(
            registry.admission(&PointRequest {
                fixture_id: "registry-lf-ordinary",
                profile_id: PROFILE_TRAILING_HW,
                offset: 5,
            }),
            PointDecision::NotRegistered {
                reason: UnregisteredReason::OffsetOutsideEveryRegisteredSafePoint
            }
        );
        // A declared-but-dispositioned point does not admit.
        assert_eq!(
            registry.admission(&PointRequest {
                fixture_id: "registry-quote-payload",
                profile_id: PROFILE_TRAILING_HW,
                offset: QUOTE_QBODY_END_PREFIX.len(),
            }),
            PointDecision::NotRegistered {
                reason: UnregisteredReason::OffsetOutsideEveryRegisteredSafePoint
            }
        );
        // Marker bytes carry no authority in either direction: the `<<` bytes
        // of the heredoc intro line sit at a point the registry never admits.
        assert_eq!(
            registry.admission(&PointRequest {
                fixture_id: "registry-heredoc-mixed",
                profile_id: PROFILE_TRAILING_HW,
                offset: "my $text = <<".len(),
            }),
            PointDecision::NotRegistered {
                reason: UnregisteredReason::OffsetOutsideEveryRegisteredSafePoint
            }
        );
        // Unknown profiles and fixtures fail closed with typed reasons.
        assert_eq!(
            registry.admission(&PointRequest {
                fixture_id: "registry-lf-ordinary",
                profile_id: "unknown.profile.v9",
                offset: 5,
            }),
            PointDecision::NotRegistered { reason: UnregisteredReason::UnknownProfile }
        );
        assert_eq!(
            registry.admission(&PointRequest {
                fixture_id: "registry-unauthored",
                profile_id: PROFILE_TRAILING_HW,
                offset: 5,
            }),
            PointDecision::NotRegistered { reason: UnregisteredReason::UnknownFixture }
        );
        // Offsets past the authored bytes are rejected before admission.
        assert_eq!(
            registry.admission(&PointRequest {
                fixture_id: "registry-lf-ordinary",
                profile_id: PROFILE_TRAILING_HW,
                offset: FIXTURE_LF_ORDINARY.len() + 1,
            }),
            PointDecision::NotRegistered { reason: UnregisteredReason::OffsetOutOfBounds }
        );
        Ok(())
    }

    #[test]
    fn duplicate_case_ids_are_rejected() -> TestResult {
        let mut cases = AUTHORED_CASES.to_vec();
        let duplicated = AUTHORED_CASES[0].clone();
        cases.push(duplicated);
        let Err(error) = MetamorphicSafeRegistry::from_declarations(cases) else {
            return Err("duplicate case ids must be rejected".into());
        };
        assert!(
            matches!(error, RegistryError::DuplicateCaseId { .. }),
            "unexpected error: {error}"
        );
        Ok(())
    }

    #[test]
    fn declarations_without_terminal_reasons_are_rejected() -> TestResult {
        let mut case = AUTHORED_CASES[0].clone();
        case.applicability = ApplicabilityDeclaration::new(Applicability::Admitted, "");
        let Err(error) = MetamorphicSafeRegistry::from_declarations(vec![case]) else {
            return Err("empty reasons must be rejected".into());
        };
        assert!(
            matches!(error, RegistryError::InvalidDeclaration { .. }),
            "unexpected error: {error}"
        );
        Ok(())
    }

    #[test]
    fn admitted_anchors_beyond_the_fixture_are_rejected_at_construction() -> TestResult {
        let mut case = AUTHORED_CASES[0].clone();
        case.anchor = AuthoredAnchor::Point {
            anchor_id: "beyond-eof",
            offset: FIXTURE_LF_ORDINARY.len() + 1,
            payload: TRAILING_TWO_SPACES,
        };
        let Err(error) = MetamorphicSafeRegistry::from_declarations(vec![case]) else {
            return Err("unresolvable anchors must be rejected".into());
        };
        assert!(matches!(error, RegistryError::InvalidAnchor { .. }), "unexpected error: {error}");
        Ok(())
    }

    #[test]
    fn evaluation_is_deterministic_and_independent_of_construction_order() -> TestResult {
        let registry = authored()?;
        let first = registry.evaluate();
        let second = registry.evaluate();
        assert_eq!(first, second);

        let mut reversed = AUTHORED_CASES.to_vec();
        reversed.reverse();
        let shuffled = MetamorphicSafeRegistry::from_declarations(reversed)?;
        assert_eq!(shuffled.case_ids(), registry.case_ids());
        assert_eq!(shuffled.evaluate(), first);
        Ok(())
    }

    #[test]
    fn edit_plans_exist_only_for_admitted_cases_and_carry_exact_anchors() -> TestResult {
        let registry = authored()?;
        for case in AUTHORED_CASES {
            let plan = registry.edit_plan(case.case_id);
            if case.applicability.state == Applicability::Admitted {
                let edits =
                    plan.ok_or_else(|| format!("admitted case {} lost its plan", case.case_id))?;
                assert!(!edits.is_empty());
                // Every generated edit is anchored: the base range carries
                // exactly the expected old bytes of the authored proposition.
                let bytes = registry
                    .fixture_bytes(case.fixture_id)
                    .ok_or_else(|| format!("missing fixture {}", case.fixture_id))?;
                for edit in &edits {
                    let range = edit.base_range();
                    // Point insertions are zero-width; region conversion edits
                    // sit exactly on the declared conversion-site bytes.
                    match case.anchor {
                        AuthoredAnchor::Point { .. } => {
                            assert!(
                                range.is_empty(),
                                "point edit for {} is not a zero-width insertion",
                                case.case_id
                            );
                        }
                        AuthoredAnchor::Region { conversion, .. } => {
                            let expected: &[u8] = match conversion {
                                NewlineConversion::LfToCrLf => b"\n",
                                NewlineConversion::CrLfToLf => b"\r\n",
                            };
                            let observed = bytes.get(range.start..range.end).ok_or_else(|| {
                                format!("edit range out of bounds for {}", case.case_id)
                            })?;
                            assert_eq!(observed, expected, "anchor drift for {}", case.case_id);
                        }
                    }
                }
            } else {
                assert!(plan.is_none(), "non-admitted case {} generated a plan", case.case_id);
            }
        }
        Ok(())
    }

    #[test]
    fn tampered_source_fails_stale_and_stays_in_accounting() -> TestResult {
        let registry = authored()?;
        let mut tampered = FIXTURE_LF_ORDINARY.as_bytes().to_vec();
        tampered[0] = b'M';
        let outcomes = registry.evaluate_source("registry-lf-ordinary", &tampered)?;
        assert_eq!(outcomes.len(), registry.declaration_count_for("registry-lf-ordinary"));
        for outcome in &outcomes {
            let CaseOutcome::StaleSource { claimed, observed, .. } = outcome else {
                return Err(format!(
                    "tampered source produced a non-stale outcome for {}",
                    outcome.case_id()
                )
                .into());
            };
            assert_eq!(claimed, registry.declared_identity("registry-lf-ordinary"));
            assert_ne!(claimed, observed);
        }
        Ok(())
    }

    #[test]
    fn integrity_report_flags_a_stale_declaration() -> TestResult {
        let mut cases = AUTHORED_CASES.to_vec();
        let mut drifted = AUTHORED_CASES[0].clone();
        drifted.source_identity =
            "sha256:0000000000000000000000000000000000000000000000000000000000000000";
        let original_id = drifted.case_id;
        cases[0] = drifted;
        let registry = MetamorphicSafeRegistry::from_declarations(cases)?;
        let report = registry.integrity_report();
        assert!(
            report.iter().any(|inconsistency| inconsistency.case_id == original_id),
            "stale declaration was not flagged: {report:?}"
        );
        Ok(())
    }

    #[test]
    fn evaluation_source_rejects_unknown_fixtures() -> TestResult {
        let registry = authored()?;
        let Err(error) = registry.evaluate_source("registry-unauthored", b"") else {
            return Err("unknown fixtures must be rejected".into());
        };
        assert!(
            matches!(error, RegistryError::UnknownFixtureRef { .. }),
            "unexpected error: {error}"
        );
        Ok(())
    }

    impl MetamorphicSafeRegistry {
        fn declaration_count_for(&self, fixture_id: &str) -> usize {
            self.cases.values().filter(|case| case.fixture_id == fixture_id).count()
        }

        fn declared_identity(&self, fixture_id: &str) -> &str {
            AUTHORED_FIXTURES
                .iter()
                .find(|fixture| fixture.fixture_id == fixture_id)
                .map_or("", |fixture| fixture.source_identity)
        }
    }
}
