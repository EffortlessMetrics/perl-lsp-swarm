//! Deterministic AST structural schema identity and freshness-gated inventory.
//!
//! This module is **#8429 / #8155 S3 emit**. It derives one versioned fingerprint
//! and one NodeKind inventory from the compiled structural registry. It does not
//! change AST structure, parser behavior, or downstream consumer policy.
//!
//! Fingerprint inputs are only behavior-bearing structural facts:
//! variant identity and declaration order, field identity/order/cardinality,
//! recovery and source-boundary tags, and declared grammar-name inputs.
//! Host paths, timestamps, map insertion order, Debug formatting, consumer
//! mode, and compatibility policy are excluded.

use super::{GrammarNameSpec, KindBody, KindStructuralRow, NODE_KIND_STRUCTURAL_REGISTRY};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt;

/// Vocabulary version for [`AstStructuralSchemaIdentity`].
pub const AST_STRUCTURAL_SCHEMA_IDENTITY_VERSION: u32 = 1;

/// Digest algorithm token admitted by this vocabulary.
pub const AST_STRUCTURAL_SCHEMA_DIGEST_ALGORITHM: &str = "sha256-v1";

/// Domain separator so this digest cannot collide with other SHA-256 uses.
const IDENTITY_DOMAIN: &[u8] = b"perl-ast:structural-schema-identity:v1\0";

const STATUS_HEADER: &str = "perl-ast-nodekind-status.v1";
const VARIANTS_MARK: &str = "---variants---";
const NOTES_MARK: &str = "---notes---";
const WIRE_PREFIX: &str = "ast-schema.v";
const SHA256_HEX_LEN: usize = 64;

/// Versioned structural schema identity derived from a registry subject.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AstStructuralSchemaIdentity {
    /// Vocabulary version encoded on the wire.
    pub version: u32,
    /// Admitted digest algorithm token.
    pub algorithm: &'static str,
    digest: [u8; 32],
}

impl fmt::Debug for AstStructuralSchemaIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AstStructuralSchemaIdentity")
            .field("version", &self.version)
            .field("algorithm", &self.algorithm)
            .field("wire", &self.wire())
            .finish()
    }
}

impl AstStructuralSchemaIdentity {
    /// Lowercase hex digest body (64 characters).
    #[must_use]
    pub fn digest_hex(&self) -> String {
        hex_encode(&self.digest)
    }

    /// Wire form `ast-schema.v{version}-{algorithm}:{hex}`.
    #[must_use]
    pub fn wire(&self) -> String {
        format!("{WIRE_PREFIX}{}-{}:{}", self.version, self.algorithm, self.digest_hex())
    }
}

/// Failure while parsing a schema-identity wire string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaIdentityError {
    /// Wire string did not match `ast-schema.v{version}-{algorithm}:{hex}`.
    MalformedWire {
        /// Rejected wire string.
        wire: String,
    },
    /// Vocabulary version is not admitted.
    UnknownVersion {
        /// Parsed version.
        version: u32,
    },
    /// Digest algorithm is not admitted.
    UnknownAlgorithm {
        /// Parsed algorithm token.
        algorithm: String,
    },
    /// Digest body was not 64 lowercase hex digits.
    MalformedDigest {
        /// Rejected digest body.
        digest: String,
    },
}

impl fmt::Display for SchemaIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedWire { wire } => write!(f, "malformed AST schema identity wire: {wire}"),
            Self::UnknownVersion { version } => {
                write!(f, "unknown AST schema identity version {version}")
            }
            Self::UnknownAlgorithm { algorithm } => {
                write!(f, "unknown AST schema digest algorithm {algorithm}")
            }
            Self::MalformedDigest { digest } => {
                write!(f, "malformed AST schema digest body: {digest}")
            }
        }
    }
}

/// One variant row in a freshness-gated NodeKind inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariantInventoryRow<'a> {
    /// Stable [`crate::NodeKind::kind_name`].
    pub kind_name: &'a str,
    /// Leaf versus child-bearing body.
    pub body: KindBody,
    /// Recovery/synthetic tag.
    pub recovery: bool,
    /// Recorded source-boundary tag.
    pub source_boundary: bool,
    /// Static grammar name or runtime-derived inputs.
    pub grammar: GrammarNameSpec<'a>,
    /// Child fields in canonical first-emission order.
    pub children: &'a [super::ChildFieldSpec],
}

/// Compiled NodeKind inventory derived from one structural registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeKindInventory<'a> {
    /// Structural identity of `variants`.
    pub identity: AstStructuralSchemaIdentity,
    /// Number of registry rows / variants.
    pub variant_count: usize,
    /// Unique child-field identities named by the registry.
    pub field_count: usize,
    /// Number of recovery-tagged rows.
    pub recovery_count: usize,
    /// Per-variant child/cardinality summaries in declaration order.
    pub variants: Vec<VariantInventoryRow<'a>>,
}

/// One observable structural change between two admitted registry subjects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaChange {
    /// A variant exists only in the later subject.
    AddedVariant {
        /// Added kind name.
        kind_name: String,
    },
    /// A variant exists only in the earlier subject.
    RemovedVariant {
        /// Removed kind name.
        kind_name: String,
    },
    /// The same variants appear in a different declaration order.
    ReorderedVariants {
        /// Earlier declaration order.
        expected: Vec<String>,
        /// Later declaration order.
        actual: Vec<String>,
    },
    /// A child field was added to an existing variant.
    AddedField {
        /// Variant that gained the field.
        kind_name: String,
        /// Added field name.
        field: String,
    },
    /// A child field was removed from an existing variant.
    RemovedField {
        /// Variant that lost the field.
        kind_name: String,
        /// Removed field name.
        field: String,
    },
    /// Present fields match as a set but canonical order changed.
    ReorderedFields {
        /// Variant under comparison.
        kind_name: String,
        /// Earlier field order.
        expected: Vec<String>,
        /// Later field order.
        actual: Vec<String>,
    },
    /// A named field changed cardinality.
    CardinalityChanged {
        /// Variant under comparison.
        kind_name: String,
        /// Field whose cardinality changed.
        field: String,
        /// Earlier cardinality token.
        from: String,
        /// Later cardinality token.
        to: String,
    },
    /// Leaf versus child-bearing tag changed.
    BodyChanged {
        /// Variant under comparison.
        kind_name: String,
        /// Earlier body token.
        from: String,
        /// Later body token.
        to: String,
    },
    /// Recovery tag changed.
    RecoveryChanged {
        /// Variant under comparison.
        kind_name: String,
        /// Earlier tag.
        from: bool,
        /// Later tag.
        to: bool,
    },
    /// Source-boundary tag changed.
    SourceBoundaryChanged {
        /// Variant under comparison.
        kind_name: String,
        /// Earlier tag.
        from: bool,
        /// Later tag.
        to: bool,
    },
    /// Static grammar name or declared runtime inputs changed.
    GrammarChanged {
        /// Variant under comparison.
        kind_name: String,
        /// Earlier grammar encoding.
        from: String,
        /// Later grammar encoding.
        to: String,
    },
}

/// Changed variant/field set between two admitted schema subjects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaDiff {
    /// Identity of the earlier subject.
    pub from: AstStructuralSchemaIdentity,
    /// Identity of the later subject.
    pub to: AstStructuralSchemaIdentity,
    /// Observable structural changes in a stable order.
    pub changes: Vec<SchemaChange>,
}

/// Why a checked NodeKind status document is not current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusFreshnessError {
    /// Document is historical and has no current status vocabulary.
    HistoricalUnversioned,
    /// Current identity, counts, or variant summaries drifted.
    StaleCheckedOutput {
        /// Why the checked document is not current.
        detail: String,
    },
    /// Identity wire in the document failed closed.
    Identity {
        /// Parse failure.
        source: SchemaIdentityError,
    },
}

impl fmt::Display for StatusFreshnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HistoricalUnversioned => {
                write!(f, "historical NodeKind status cannot satisfy current freshness")
            }
            Self::StaleCheckedOutput { detail } => {
                write!(f, "stale checked NodeKind status: {detail}")
            }
            Self::Identity { source } => write!(f, "{source}"),
        }
    }
}

/// Canonical structural subject for an admitted registry. Slice order is identity.
#[must_use]
pub fn canonical_structural_subject(registry: &[KindStructuralRow<'_>]) -> String {
    let mut out = String::new();
    out.push_str("perl-ast.structural-schema.v1\n");
    out.push_str("algorithm=");
    out.push_str(AST_STRUCTURAL_SCHEMA_DIGEST_ALGORITHM);
    out.push('\n');
    out.push_str("row_count=");
    out.push_str(&registry.len().to_string());
    out.push('\n');
    for row in registry {
        out.push_str("row\n");
        out.push_str("kind=");
        out.push_str(row.kind_name);
        out.push('\n');
        out.push_str("body=");
        out.push_str(body_token(row.body));
        out.push('\n');
        out.push_str("recovery=");
        out.push_str(if row.recovery { "1" } else { "0" });
        out.push('\n');
        out.push_str("source_boundary=");
        out.push_str(if row.source_boundary { "1" } else { "0" });
        out.push('\n');
        match row.grammar {
            GrammarNameSpec::Static(name) => {
                out.push_str("grammar=static\n");
                out.push_str("name=");
                out.push_str(name);
                out.push('\n');
            }
            GrammarNameSpec::RuntimeDerived { inputs } => {
                out.push_str("grammar=runtime\n");
                out.push_str("input_count=");
                out.push_str(&inputs.len().to_string());
                out.push('\n');
                for input in inputs {
                    out.push_str("input=");
                    out.push_str(input);
                    out.push('\n');
                }
            }
        }
        out.push_str("field_count=");
        out.push_str(&row.children.len().to_string());
        out.push('\n');
        for child in row.children {
            out.push_str("field=");
            out.push_str(child.field.name());
            out.push('\n');
            out.push_str("cardinality=");
            out.push_str(child.cardinality.token());
            out.push('\n');
        }
    }
    out
}

/// Fingerprint one admitted registry subject.
#[must_use]
pub fn fingerprint_registry(registry: &[KindStructuralRow<'_>]) -> AstStructuralSchemaIdentity {
    let subject = canonical_structural_subject(registry);
    let mut hasher = Sha256::new();
    hasher.update(IDENTITY_DOMAIN);
    hasher.update(subject.as_bytes());
    AstStructuralSchemaIdentity {
        version: AST_STRUCTURAL_SCHEMA_IDENTITY_VERSION,
        algorithm: AST_STRUCTURAL_SCHEMA_DIGEST_ALGORITHM,
        digest: hasher.finalize().into(),
    }
}

/// Current compiled structural schema identity.
#[must_use]
pub fn current_ast_structural_schema_identity() -> AstStructuralSchemaIdentity {
    fingerprint_registry(NODE_KIND_STRUCTURAL_REGISTRY)
}

/// Parse a schema-identity wire string. Unknown versions and algorithms fail closed.
pub fn parse_schema_identity(
    wire: &str,
) -> Result<AstStructuralSchemaIdentity, SchemaIdentityError> {
    let rest = wire
        .strip_prefix(WIRE_PREFIX)
        .ok_or_else(|| SchemaIdentityError::MalformedWire { wire: wire.to_string() })?;
    let (version_str, rest) = rest
        .split_once('-')
        .ok_or_else(|| SchemaIdentityError::MalformedWire { wire: wire.to_string() })?;
    let version = version_str
        .parse::<u32>()
        .map_err(|_| SchemaIdentityError::MalformedWire { wire: wire.to_string() })?;
    if version != AST_STRUCTURAL_SCHEMA_IDENTITY_VERSION {
        return Err(SchemaIdentityError::UnknownVersion { version });
    }
    let (algorithm, digest) = rest
        .split_once(':')
        .ok_or_else(|| SchemaIdentityError::MalformedWire { wire: wire.to_string() })?;
    if algorithm != AST_STRUCTURAL_SCHEMA_DIGEST_ALGORITHM {
        return Err(SchemaIdentityError::UnknownAlgorithm { algorithm: algorithm.to_string() });
    }
    let bytes = decode_sha256_hex(digest)?;
    Ok(AstStructuralSchemaIdentity {
        version: AST_STRUCTURAL_SCHEMA_IDENTITY_VERSION,
        algorithm: AST_STRUCTURAL_SCHEMA_DIGEST_ALGORITHM,
        digest: bytes,
    })
}

/// Build an inventory from an admitted registry subject.
#[must_use]
pub fn inventory_from_registry<'a>(registry: &'a [KindStructuralRow<'a>]) -> NodeKindInventory<'a> {
    let variants: Vec<VariantInventoryRow<'a>> = registry
        .iter()
        .map(|row| VariantInventoryRow {
            kind_name: row.kind_name,
            body: row.body,
            recovery: row.recovery,
            source_boundary: row.source_boundary,
            grammar: row.grammar,
            children: row.children,
        })
        .collect();
    let recovery_count = variants.iter().filter(|row| row.recovery).count();
    let field_count = unique_field_count(registry);
    NodeKindInventory {
        identity: fingerprint_registry(registry),
        variant_count: variants.len(),
        field_count,
        recovery_count,
        variants,
    }
}

/// Inventory of the compiled production registry.
#[must_use]
pub fn current_nodekind_inventory() -> NodeKindInventory<'static> {
    inventory_from_registry(NODE_KIND_STRUCTURAL_REGISTRY)
}

/// Observable structural delta between two admitted registry subjects.
#[must_use]
pub fn diff_structural_registries(
    from: &[KindStructuralRow<'_>],
    to: &[KindStructuralRow<'_>],
) -> SchemaDiff {
    let mut changes = Vec::new();
    let expected: Vec<String> = from.iter().map(|row| row.kind_name.to_string()).collect();
    let actual: Vec<String> = to.iter().map(|row| row.kind_name.to_string()).collect();
    let from_set: BTreeMap<&str, &KindStructuralRow<'_>> =
        from.iter().map(|row| (row.kind_name, row)).collect();
    let to_set: BTreeMap<&str, &KindStructuralRow<'_>> =
        to.iter().map(|row| (row.kind_name, row)).collect();

    for name in from_set.keys() {
        if !to_set.contains_key(name) {
            changes.push(SchemaChange::RemovedVariant { kind_name: (*name).to_string() });
        }
    }
    for name in to_set.keys() {
        if !from_set.contains_key(name) {
            changes.push(SchemaChange::AddedVariant { kind_name: (*name).to_string() });
        }
    }
    if from_set.keys().eq(to_set.keys()) && expected != actual {
        changes.push(SchemaChange::ReorderedVariants { expected, actual });
    }

    for (name, before) in &from_set {
        let Some(after) = to_set.get(name) else {
            continue;
        };
        diff_row(before, after, &mut changes);
    }

    SchemaDiff { from: fingerprint_registry(from), to: fingerprint_registry(to), changes }
}

/// Render a machine-readable checked status document.
///
/// `notes` are retained generated prose. They are not fingerprint inputs and
/// cannot satisfy or fail structural freshness.
#[must_use]
pub fn render_checked_status_report(inventory: &NodeKindInventory<'_>, notes: &str) -> String {
    let mut out = String::new();
    out.push_str(STATUS_HEADER);
    out.push('\n');
    out.push_str("identity=");
    out.push_str(&inventory.identity.wire());
    out.push('\n');
    out.push_str("variant_count=");
    out.push_str(&inventory.variant_count.to_string());
    out.push('\n');
    out.push_str("field_count=");
    out.push_str(&inventory.field_count.to_string());
    out.push('\n');
    out.push_str("recovery_count=");
    out.push_str(&inventory.recovery_count.to_string());
    out.push('\n');
    out.push_str(VARIANTS_MARK);
    out.push('\n');
    for row in &inventory.variants {
        out.push_str(row.kind_name);
        out.push('\t');
        out.push_str(body_token(row.body));
        out.push('\t');
        out.push_str(if row.recovery { "recovery" } else { "source" });
        out.push('\t');
        out.push_str(if row.source_boundary { "boundary" } else { "interior" });
        out.push('\t');
        out.push_str(&grammar_token(row.grammar));
        out.push('\t');
        if row.children.is_empty() {
            out.push('-');
        } else {
            for (i, child) in row.children.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(child.field.name());
                out.push(':');
                out.push_str(child.cardinality.token());
            }
        }
        out.push('\n');
    }
    out.push_str(NOTES_MARK);
    out.push('\n');
    out.push_str(notes);
    if !notes.is_empty() && !notes.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Check a generated status document against the compiled inventory.
///
/// Historical unversioned documents remain readable as ordinary text but cannot
/// satisfy current freshness. Semantic notes may differ; identity, counts, and
/// variant summaries may not.
pub fn check_status_freshness(
    report: &str,
    current: &NodeKindInventory<'_>,
) -> Result<(), StatusFreshnessError> {
    let Some(rest) = report.strip_prefix(STATUS_HEADER) else {
        return Err(StatusFreshnessError::HistoricalUnversioned);
    };
    if !rest.starts_with('\n') {
        return Err(StatusFreshnessError::HistoricalUnversioned);
    }

    let structural = structural_prefix(report);
    let expected_report = render_checked_status_report(current, "");
    let expected = structural_prefix(&expected_report);
    if structural != expected {
        if let Some(identity_line) = report.lines().find_map(|line| line.strip_prefix("identity="))
            && let Err(source) = parse_schema_identity(identity_line)
        {
            return Err(StatusFreshnessError::Identity { source });
        }
        return Err(StatusFreshnessError::StaleCheckedOutput {
            detail: "identity, counts, or variant summaries drifted from the compiled registry"
                .to_string(),
        });
    }
    Ok(())
}

fn structural_prefix(report: &str) -> &str {
    match report.split_once(NOTES_MARK) {
        Some((prefix, _)) => prefix,
        None => report,
    }
}

fn unique_field_count(registry: &[KindStructuralRow<'_>]) -> usize {
    let mut seen = Vec::new();
    for row in registry {
        for child in row.children {
            if !seen.iter().any(|name: &&str| *name == child.field.name()) {
                seen.push(child.field.name());
            }
        }
    }
    seen.len()
}

fn diff_row(
    before: &KindStructuralRow<'_>,
    after: &KindStructuralRow<'_>,
    changes: &mut Vec<SchemaChange>,
) {
    let kind_name = before.kind_name.to_string();
    if before.body != after.body {
        changes.push(SchemaChange::BodyChanged {
            kind_name: kind_name.clone(),
            from: body_token(before.body).to_string(),
            to: body_token(after.body).to_string(),
        });
    }
    if before.recovery != after.recovery {
        changes.push(SchemaChange::RecoveryChanged {
            kind_name: kind_name.clone(),
            from: before.recovery,
            to: after.recovery,
        });
    }
    if before.source_boundary != after.source_boundary {
        changes.push(SchemaChange::SourceBoundaryChanged {
            kind_name: kind_name.clone(),
            from: before.source_boundary,
            to: after.source_boundary,
        });
    }
    let before_grammar = grammar_token(before.grammar);
    let after_grammar = grammar_token(after.grammar);
    if before_grammar != after_grammar {
        changes.push(SchemaChange::GrammarChanged {
            kind_name: kind_name.clone(),
            from: before_grammar,
            to: after_grammar,
        });
    }

    let before_fields: Vec<&str> = before.children.iter().map(|child| child.field.name()).collect();
    let after_fields: Vec<&str> = after.children.iter().map(|child| child.field.name()).collect();
    let before_set: BTreeMap<&str, &str> = before
        .children
        .iter()
        .map(|child| (child.field.name(), child.cardinality.token()))
        .collect();
    let after_set: BTreeMap<&str, &str> = after
        .children
        .iter()
        .map(|child| (child.field.name(), child.cardinality.token()))
        .collect();

    for field in before_set.keys() {
        if !after_set.contains_key(field) {
            changes.push(SchemaChange::RemovedField {
                kind_name: kind_name.clone(),
                field: (*field).to_string(),
            });
        }
    }
    for field in after_set.keys() {
        if !before_set.contains_key(field) {
            changes.push(SchemaChange::AddedField {
                kind_name: kind_name.clone(),
                field: (*field).to_string(),
            });
        }
    }
    if before_fields != after_fields {
        let before_only: BTreeMap<&str, ()> =
            before_fields.iter().map(|name| (*name, ())).collect();
        let after_only: BTreeMap<&str, ()> = after_fields.iter().map(|name| (*name, ())).collect();
        if before_only.keys().eq(after_only.keys()) {
            changes.push(SchemaChange::ReorderedFields {
                kind_name: kind_name.clone(),
                expected: before_fields.iter().map(|name| (*name).to_string()).collect(),
                actual: after_fields.iter().map(|name| (*name).to_string()).collect(),
            });
        }
    }
    for (field, from) in &before_set {
        if let Some(to) = after_set.get(field)
            && from != to
        {
            changes.push(SchemaChange::CardinalityChanged {
                kind_name: kind_name.clone(),
                field: (*field).to_string(),
                from: (*from).to_string(),
                to: (*to).to_string(),
            });
        }
    }
}

fn body_token(body: KindBody) -> &'static str {
    match body {
        KindBody::Leaf => "leaf",
        KindBody::ChildBearing => "child-bearing",
    }
}

fn grammar_token(grammar: GrammarNameSpec<'_>) -> String {
    match grammar {
        GrammarNameSpec::Static(name) => {
            let mut out = String::from("static:");
            out.push_str(name);
            out
        }
        GrammarNameSpec::RuntimeDerived { inputs } => {
            let mut out = String::from("runtime:");
            out.push_str(&inputs.len().to_string());
            for input in inputs {
                out.push(':');
                out.push_str(input);
            }
            out
        }
    }
}

fn hex_encode(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(SHA256_HEX_LEN);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn decode_sha256_hex(hex: &str) -> Result<[u8; 32], SchemaIdentityError> {
    if hex.len() != SHA256_HEX_LEN {
        return Err(SchemaIdentityError::MalformedDigest { digest: hex.to_string() });
    }
    let bytes = hex.as_bytes();
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        let Some(hi) = hex_nibble(bytes[i * 2]) else {
            return Err(SchemaIdentityError::MalformedDigest { digest: hex.to_string() });
        };
        let Some(lo) = hex_nibble(bytes[i * 2 + 1]) else {
            return Err(SchemaIdentityError::MalformedDigest { digest: hex.to_string() });
        };
        *slot = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}
