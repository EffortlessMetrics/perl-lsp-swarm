//! Versioned, portable category fingerprints for admitted semantic facts (#8054).
//!
//! Identity (`AnchorId`, `EntityId`, …) remains a stable lookup key. These
//! fingerprints are a separate proposition: the digest of every currently
//! admitted semantically observable field, in a canonical order, using a
//! platform-independent encoding.
//!
//! The digest algorithm matches [`perl_semantic_facts::semantic_identity::SemanticIdentityFingerprint`]:
//! dual-lane FNV-1a with length-prefixed strings and complementary second-lane
//! mixing. Integer fields are mixed as big-endian bytes rather than host
//! endianness or `std` `Hash`/`DefaultHasher` (which is process-local and not
//! a portable semantic fingerprint authority).
//!
//! `Option<u64>` storage on [`crate::workspace::workspace_index::FileFactShard`]
//! is a compatibility adapter: the 128-bit dual-lane digest is projected by
//! XOR-folding the lanes. The typed encoder remains the authority; changing
//! [`FACT_FINGERPRINT_SCHEMA_VERSION`] is the explicit schema transition.
//!
//! Duplicate stable IDs in one category are instrument failure: the category
//! fingerprint is unavailable (`None`) so incremental replacement rebuilds
//! conservatively rather than inventing a payload tie-breaker.

use perl_semantic_facts::{
    AnchorFact, Confidence, EdgeFact, EdgeKind, EntityFact, EntityKind, OccurrenceFact,
    OccurrenceKind, Provenance,
};

/// Fingerprint-schema identifier mixed into every category digest.
///
/// Distinct from [`super::facts::PRODUCER_SCHEMA_VERSION`]: the producer
/// schema says what facts mean; this schema says which projection was encoded.
/// Bump this constant when the encoded field set or domain tags change, even
/// if fixture bytes would otherwise collide.
pub(crate) const FACT_FINGERPRINT_SCHEMA_VERSION: u32 = 1;

/// Domain tag mixed first into every digest (length-prefixed).
const FINGERPRINT_DOMAIN: &str = "fact-fingerprint-v1";

/// 64-bit FNV-1a offset basis (same as `SemanticIdentityFingerprint`).
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// 64-bit FNV-1a prime (same as `SemanticIdentityFingerprint`).
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// Independent second-lane basis (same as `SemanticIdentityFingerprint`).
const FNV_LANE_B_BASIS: u64 = 0x9e37_79b9_7f4a_7c15;

/// Portable dual-lane FNV-1a accumulator.
struct Acc {
    lane_a: u64,
    lane_b: u64,
}

impl Acc {
    fn new(domain: &str) -> Self {
        let mut acc = Self { lane_a: FNV_OFFSET_BASIS, lane_b: FNV_LANE_B_BASIS };
        acc.mix_str(domain);
        acc
    }

    fn mix_byte(&mut self, byte: u8) {
        self.lane_a ^= u64::from(byte);
        self.lane_a = self.lane_a.wrapping_mul(FNV_PRIME);
        self.lane_b ^= u64::from(!byte).rotate_left(17);
        self.lane_b = self.lane_b.wrapping_mul(FNV_PRIME);
    }

    fn mix_str(&mut self, value: &str) {
        let len = u64::try_from(value.len()).unwrap_or(u64::MAX);
        self.mix_u64(len);
        for byte in value.as_bytes() {
            self.mix_byte(*byte);
        }
    }

    fn mix_u64(&mut self, value: u64) {
        for byte in value.to_be_bytes() {
            self.mix_byte(byte);
        }
    }

    fn mix_u32(&mut self, value: u32) {
        for byte in value.to_be_bytes() {
            self.mix_byte(byte);
        }
    }

    fn field_str(&mut self, label: &str, value: &str) {
        self.mix_str(label);
        self.mix_str(value);
    }

    fn field_u64(&mut self, label: &str, value: u64) {
        self.mix_str(label);
        self.mix_u64(value);
    }

    fn field_u32(&mut self, label: &str, value: u32) {
        self.mix_str(label);
        self.mix_u32(value);
    }

    fn field_optional_u64(&mut self, label: &str, value: Option<u64>) {
        self.mix_str(label);
        match value {
            None => self.mix_byte(0),
            Some(inner) => {
                self.mix_byte(1);
                self.mix_u64(inner);
            }
        }
    }

    fn finish_u64(self) -> u64 {
        self.lane_a ^ self.lane_b.rotate_left(32)
    }
}

fn category_header<'a, T>(
    category: &str,
    schema: u32,
    producer_schema: u32,
    items: &'a [T],
    id_of: impl Fn(&T) -> u64,
) -> Option<(Acc, Vec<&'a T>)> {
    let mut indexed: Vec<&T> = items.iter().collect();
    indexed.sort_by_key(|item| id_of(item));
    if indexed.windows(2).any(|pair| id_of(pair[0]) == id_of(pair[1])) {
        return None;
    }
    let count = u64::try_from(indexed.len()).unwrap_or(u64::MAX);
    let mut acc = Acc::new(FINGERPRINT_DOMAIN);
    acc.field_u32("schema", schema);
    acc.field_u32("producer", producer_schema);
    acc.field_str("category", category);
    acc.field_u64("count", count);
    Some((acc, indexed))
}

/// Fingerprint the complete ordered anchor category, or `None` on duplicate IDs.
pub(crate) fn fingerprint_anchors(anchors: &[AnchorFact], producer_schema: u32) -> Option<u64> {
    fingerprint_anchors_with_schema(anchors, FACT_FINGERPRINT_SCHEMA_VERSION, producer_schema)
}

/// Same as [`fingerprint_anchors`] with an explicit fingerprint-schema id.
pub(crate) fn fingerprint_anchors_with_schema(
    anchors: &[AnchorFact],
    schema: u32,
    producer_schema: u32,
) -> Option<u64> {
    let (mut acc, rows) = category_header("anchors", schema, producer_schema, anchors, |a| a.id.0)?;
    for anchor in rows {
        acc.field_str("row", "anchor");
        acc.field_u64("id", anchor.id.0);
        acc.field_u64("file_id", anchor.file_id.0);
        acc.field_u32("span_start", anchor.span_start_byte);
        acc.field_u32("span_end", anchor.span_end_byte);
        acc.field_optional_u64("scope_id", anchor.scope_id.map(|scope| scope.0));
        acc.field_str("provenance", provenance_tag(anchor.provenance));
        acc.field_str("confidence", confidence_tag(anchor.confidence));
    }
    Some(acc.finish_u64())
}

/// Fingerprint the complete ordered entity category, or `None` on duplicate IDs.
pub(crate) fn fingerprint_entities(entities: &[EntityFact], producer_schema: u32) -> Option<u64> {
    fingerprint_entities_with_schema(entities, FACT_FINGERPRINT_SCHEMA_VERSION, producer_schema)
}

/// Same as [`fingerprint_entities`] with an explicit fingerprint-schema id.
pub(crate) fn fingerprint_entities_with_schema(
    entities: &[EntityFact],
    schema: u32,
    producer_schema: u32,
) -> Option<u64> {
    let (mut acc, rows) =
        category_header("entities", schema, producer_schema, entities, |entity| entity.id.0)?;
    for entity in rows {
        acc.field_str("row", "entity");
        acc.field_u64("id", entity.id.0);
        acc.field_str("kind", entity_kind_tag(entity.kind));
        acc.field_str("canonical_name", &entity.canonical_name);
        acc.field_optional_u64("anchor_id", entity.anchor_id.map(|anchor| anchor.0));
        acc.field_optional_u64("scope_id", entity.scope_id.map(|scope| scope.0));
        acc.field_str("provenance", provenance_tag(entity.provenance));
        acc.field_str("confidence", confidence_tag(entity.confidence));
    }
    Some(acc.finish_u64())
}

/// Fingerprint the complete ordered occurrence category, or `None` on duplicate IDs.
pub(crate) fn fingerprint_occurrences(
    occurrences: &[OccurrenceFact],
    producer_schema: u32,
) -> Option<u64> {
    fingerprint_occurrences_with_schema(
        occurrences,
        FACT_FINGERPRINT_SCHEMA_VERSION,
        producer_schema,
    )
}

/// Same as [`fingerprint_occurrences`] with an explicit fingerprint-schema id.
pub(crate) fn fingerprint_occurrences_with_schema(
    occurrences: &[OccurrenceFact],
    schema: u32,
    producer_schema: u32,
) -> Option<u64> {
    let (mut acc, rows) =
        category_header("occurrences", schema, producer_schema, occurrences, |occ| occ.id.0)?;
    for occurrence in rows {
        acc.field_str("row", "occurrence");
        acc.field_u64("id", occurrence.id.0);
        acc.field_str("kind", occurrence_kind_tag(occurrence.kind));
        acc.field_optional_u64("entity_id", occurrence.entity_id.map(|entity| entity.0));
        acc.field_u64("anchor_id", occurrence.anchor_id.0);
        acc.field_optional_u64("scope_id", occurrence.scope_id.map(|scope| scope.0));
        acc.field_str("provenance", provenance_tag(occurrence.provenance));
        acc.field_str("confidence", confidence_tag(occurrence.confidence));
    }
    Some(acc.finish_u64())
}

/// Fingerprint the complete ordered edge category, or `None` on duplicate IDs.
pub(crate) fn fingerprint_edges(edges: &[EdgeFact], producer_schema: u32) -> Option<u64> {
    fingerprint_edges_with_schema(edges, FACT_FINGERPRINT_SCHEMA_VERSION, producer_schema)
}

/// Same as [`fingerprint_edges`] with an explicit fingerprint-schema id.
pub(crate) fn fingerprint_edges_with_schema(
    edges: &[EdgeFact],
    schema: u32,
    producer_schema: u32,
) -> Option<u64> {
    let (mut acc, rows) =
        category_header("edges", schema, producer_schema, edges, |edge| edge.id.0)?;
    for edge in rows {
        acc.field_str("row", "edge");
        acc.field_u64("id", edge.id.0);
        acc.field_str("kind", edge_kind_tag(edge.kind));
        acc.field_u64("from", edge.from_entity_id.0);
        acc.field_u64("to", edge.to_entity_id.0);
        acc.field_optional_u64("via", edge.via_occurrence_id.map(|occ| occ.0));
        acc.field_str("provenance", provenance_tag(edge.provenance));
        acc.field_str("confidence", confidence_tag(edge.confidence));
    }
    Some(acc.finish_u64())
}

fn entity_kind_tag(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Package => "package",
        EntityKind::Class => "class",
        EntityKind::Role => "role",
        EntityKind::Subroutine => "subroutine",
        EntityKind::Method => "method",
        EntityKind::Variable => "variable",
        EntityKind::Constant => "constant",
        EntityKind::Field => "field",
        EntityKind::Label => "label",
        EntityKind::Format => "format",
        EntityKind::Module => "module",
        EntityKind::GeneratedMember => "generated_member",
        EntityKind::ExternalSymbol => "external_symbol",
        EntityKind::Unknown => "unknown",
    }
}

fn occurrence_kind_tag(kind: OccurrenceKind) -> &'static str {
    match kind {
        OccurrenceKind::Definition => "definition",
        OccurrenceKind::Reference => "reference",
        OccurrenceKind::Read => "read",
        OccurrenceKind::Write => "write",
        OccurrenceKind::Call => "call",
        OccurrenceKind::MethodCall => "method_call",
        OccurrenceKind::StaticMethodCall => "static_method_call",
        OccurrenceKind::CoderefReference => "coderef_reference",
        OccurrenceKind::TypeglobReference => "typeglob_reference",
        OccurrenceKind::Import => "import",
        OccurrenceKind::Export => "export",
        OccurrenceKind::Inheritance => "inheritance",
        OccurrenceKind::RoleComposition => "role_composition",
        OccurrenceKind::GeneratedUse => "generated_use",
        OccurrenceKind::DynamicBoundary => "dynamic_boundary",
    }
}

fn edge_kind_tag(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Defines => "defines",
        EdgeKind::References => "references",
        EdgeKind::Reads => "reads",
        EdgeKind::Writes => "writes",
        EdgeKind::Calls => "calls",
        EdgeKind::ImportsModule => "imports_module",
        EdgeKind::ImportsSymbol => "imports_symbol",
        EdgeKind::ExportsSymbol => "exports_symbol",
        EdgeKind::ExportsGroup => "exports_group",
        EdgeKind::Inherits => "inherits",
        EdgeKind::ComposesRole => "composes_role",
        EdgeKind::MemberOf => "member_of",
        EdgeKind::GeneratedFrom => "generated_from",
        EdgeKind::AliasOf => "alias_of",
        EdgeKind::DependsOn => "depends_on",
        EdgeKind::DynamicBoundary => "dynamic_boundary",
    }
}

fn provenance_tag(provenance: Provenance) -> &'static str {
    match provenance {
        Provenance::ExactAst => "exact_ast",
        Provenance::DesugaredAst => "desugared_ast",
        Provenance::SemanticAnalyzer => "semantic_analyzer",
        Provenance::FrameworkSynthesis => "framework_synthesis",
        Provenance::ImportExportInference => "import_export_inference",
        Provenance::PragmaInference => "pragma_inference",
        Provenance::NameHeuristic => "name_heuristic",
        Provenance::SearchFallback => "search_fallback",
        Provenance::DynamicBoundary => "dynamic_boundary",
        Provenance::LiteralRequireImport => "literal_require_import",
    }
}

fn confidence_tag(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::High => "high",
        Confidence::Medium => "medium",
        Confidence::Low => "low",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_semantic_facts::{AnchorId, EdgeId, EntityId, FileId, OccurrenceId, ScopeId};

    const PRODUCER: u32 = 1;

    fn sample_anchor() -> AnchorFact {
        AnchorFact {
            id: AnchorId(1),
            file_id: FileId(2),
            span_start_byte: 3,
            span_end_byte: 4,
            scope_id: Some(ScopeId(5)),
            provenance: Provenance::ExactAst,
            confidence: Confidence::High,
        }
    }

    fn sample_entity() -> EntityFact {
        EntityFact {
            id: EntityId(6),
            kind: EntityKind::Subroutine,
            canonical_name: "Pkg::foo".to_string(),
            anchor_id: Some(AnchorId(1)),
            scope_id: Some(ScopeId(5)),
            provenance: Provenance::ExactAst,
            confidence: Confidence::High,
        }
    }

    fn sample_occurrence() -> OccurrenceFact {
        OccurrenceFact {
            id: OccurrenceId(7),
            kind: OccurrenceKind::Call,
            entity_id: Some(EntityId(6)),
            anchor_id: AnchorId(1),
            scope_id: None,
            provenance: Provenance::ExactAst,
            confidence: Confidence::High,
        }
    }

    fn sample_edge() -> EdgeFact {
        EdgeFact {
            id: EdgeId(8),
            kind: EdgeKind::References,
            from_entity_id: EntityId(6),
            to_entity_id: EntityId(9),
            via_occurrence_id: Some(OccurrenceId(7)),
            provenance: Provenance::ExactAst,
            confidence: Confidence::High,
        }
    }

    struct CategoryFingerprints {
        anchors: Option<u64>,
        entities: Option<u64>,
        occurrences: Option<u64>,
        edges: Option<u64>,
    }

    fn fps(
        anchors: &[AnchorFact],
        entities: &[EntityFact],
        occurrences: &[OccurrenceFact],
        edges: &[EdgeFact],
    ) -> CategoryFingerprints {
        CategoryFingerprints {
            anchors: fingerprint_anchors(anchors, PRODUCER),
            entities: fingerprint_entities(entities, PRODUCER),
            occurrences: fingerprint_occurrences(occurrences, PRODUCER),
            edges: fingerprint_edges(edges, PRODUCER),
        }
    }

    fn baseline() -> CategoryFingerprints {
        fps(&[sample_anchor()], &[sample_entity()], &[sample_occurrence()], &[sample_edge()])
    }

    fn assert_only_anchors_changed(mutated: &CategoryFingerprints, base: &CategoryFingerprints) {
        assert_ne!(mutated.anchors, base.anchors);
        assert_eq!(mutated.entities, base.entities);
        assert_eq!(mutated.occurrences, base.occurrences);
        assert_eq!(mutated.edges, base.edges);
    }

    fn assert_only_entities_changed(mutated: &CategoryFingerprints, base: &CategoryFingerprints) {
        assert_eq!(mutated.anchors, base.anchors);
        assert_ne!(mutated.entities, base.entities);
        assert_eq!(mutated.occurrences, base.occurrences);
        assert_eq!(mutated.edges, base.edges);
    }

    fn assert_only_occurrences_changed(
        mutated: &CategoryFingerprints,
        base: &CategoryFingerprints,
    ) {
        assert_eq!(mutated.anchors, base.anchors);
        assert_eq!(mutated.entities, base.entities);
        assert_ne!(mutated.occurrences, base.occurrences);
        assert_eq!(mutated.edges, base.edges);
    }

    fn assert_only_edges_changed(mutated: &CategoryFingerprints, base: &CategoryFingerprints) {
        assert_eq!(mutated.anchors, base.anchors);
        assert_eq!(mutated.entities, base.entities);
        assert_eq!(mutated.occurrences, base.occurrences);
        assert_ne!(mutated.edges, base.edges);
    }

    #[test]
    fn each_anchor_field_changes_only_anchors_hash() {
        let base = baseline();
        let mutations = [
            {
                let mut a = sample_anchor();
                a.id = AnchorId(99);
                a
            },
            {
                let mut a = sample_anchor();
                a.file_id = FileId(99);
                a
            },
            {
                let mut a = sample_anchor();
                a.span_start_byte = 30;
                a
            },
            {
                let mut a = sample_anchor();
                a.span_end_byte = 40;
                a
            },
            {
                let mut a = sample_anchor();
                a.scope_id = None;
                a
            },
            {
                let mut a = sample_anchor();
                a.provenance = Provenance::SearchFallback;
                a
            },
            {
                let mut a = sample_anchor();
                a.confidence = Confidence::Low;
                a
            },
        ];
        for anchor in mutations {
            assert_only_anchors_changed(
                &fps(&[anchor], &[sample_entity()], &[sample_occurrence()], &[sample_edge()]),
                &base,
            );
        }
    }

    #[test]
    fn each_entity_field_changes_only_entities_hash() {
        let base = baseline();
        let mutations = [
            {
                let mut e = sample_entity();
                e.id = EntityId(99);
                e
            },
            {
                let mut e = sample_entity();
                e.kind = EntityKind::Method;
                e
            },
            {
                let mut e = sample_entity();
                e.canonical_name = "Pkg::bar".to_string();
                e
            },
            {
                let mut e = sample_entity();
                e.anchor_id = None;
                e
            },
            {
                let mut e = sample_entity();
                e.scope_id = Some(ScopeId(50));
                e
            },
            {
                let mut e = sample_entity();
                e.provenance = Provenance::FrameworkSynthesis;
                e
            },
            {
                let mut e = sample_entity();
                e.confidence = Confidence::Medium;
                e
            },
        ];
        for entity in mutations {
            assert_only_entities_changed(
                &fps(&[sample_anchor()], &[entity], &[sample_occurrence()], &[sample_edge()]),
                &base,
            );
        }
    }

    #[test]
    fn each_occurrence_field_changes_only_occurrences_hash() {
        let base = baseline();
        let mutations = [
            {
                let mut o = sample_occurrence();
                o.id = OccurrenceId(99);
                o
            },
            {
                let mut o = sample_occurrence();
                o.kind = OccurrenceKind::Reference;
                o
            },
            {
                let mut o = sample_occurrence();
                o.entity_id = Some(EntityId(99));
                o
            },
            {
                let mut o = sample_occurrence();
                o.entity_id = None;
                o
            },
            {
                let mut o = sample_occurrence();
                o.anchor_id = AnchorId(42);
                o
            },
            {
                let mut o = sample_occurrence();
                o.scope_id = Some(ScopeId(3));
                o
            },
            {
                let mut o = sample_occurrence();
                o.provenance = Provenance::NameHeuristic;
                o
            },
            {
                let mut o = sample_occurrence();
                o.confidence = Confidence::Low;
                o
            },
        ];
        for occurrence in mutations {
            assert_only_occurrences_changed(
                &fps(&[sample_anchor()], &[sample_entity()], &[occurrence], &[sample_edge()]),
                &base,
            );
        }
    }

    #[test]
    fn each_edge_field_changes_only_edges_hash() {
        let base = baseline();
        let mutations = [
            {
                let mut e = sample_edge();
                e.id = EdgeId(99);
                e
            },
            {
                let mut e = sample_edge();
                e.kind = EdgeKind::Calls;
                e
            },
            {
                let mut e = sample_edge();
                e.from_entity_id = EntityId(1);
                e
            },
            {
                let mut e = sample_edge();
                e.to_entity_id = EntityId(2);
                e
            },
            {
                let mut e = sample_edge();
                e.via_occurrence_id = None;
                e
            },
            {
                let mut e = sample_edge();
                e.provenance = Provenance::DynamicBoundary;
                e
            },
            {
                let mut e = sample_edge();
                e.confidence = Confidence::Low;
                e
            },
        ];
        for edge in mutations {
            assert_only_edges_changed(
                &fps(&[sample_anchor()], &[sample_entity()], &[sample_occurrence()], &[edge]),
                &base,
            );
        }
    }

    #[test]
    fn edge_target_change_with_fixed_id_and_kind_changes_edges_hash() {
        // Strongest discriminator from #8054: EdgeId and EdgeKind stay put
        // while `to` moves. The old DefaultHasher projection missed this.
        let mut moved = sample_edge();
        moved.to_entity_id = EntityId(900);
        let base = baseline();
        assert_only_edges_changed(
            &fps(&[sample_anchor()], &[sample_entity()], &[sample_occurrence()], &[moved]),
            &base,
        );
    }

    #[test]
    fn insertion_order_does_not_change_any_category_hash() {
        let a1 = sample_anchor();
        let mut a2 = sample_anchor();
        a2.id = AnchorId(11);
        a2.span_start_byte = 40;
        let e1 = sample_entity();
        let mut e2 = sample_entity();
        e2.id = EntityId(16);
        e2.canonical_name = "Pkg::bar".to_string();
        let o1 = sample_occurrence();
        let mut o2 = sample_occurrence();
        o2.id = OccurrenceId(17);
        o2.kind = OccurrenceKind::Reference;
        let g1 = sample_edge();
        let mut g2 = sample_edge();
        g2.id = EdgeId(18);
        g2.to_entity_id = EntityId(19);

        let forward = fps(
            &[a1.clone(), a2.clone()],
            &[e1.clone(), e2.clone()],
            &[o1.clone(), o2.clone()],
            &[g1.clone(), g2.clone()],
        );
        let reversed = fps(&[a2, a1], &[e2, e1], &[o2, o1], &[g2, g1]);
        assert_eq!(forward.anchors, reversed.anchors);
        assert_eq!(forward.entities, reversed.entities);
        assert_eq!(forward.occurrences, reversed.occurrences);
        assert_eq!(forward.edges, reversed.edges);
    }

    #[test]
    fn duplicate_stable_id_makes_category_fingerprint_unavailable() {
        let a1 = sample_anchor();
        let mut a2 = sample_anchor();
        a2.span_end_byte = 99;
        assert!(fingerprint_anchors(&[a1, a2], PRODUCER).is_none());

        let e1 = sample_entity();
        let mut e2 = sample_entity();
        e2.kind = EntityKind::Method;
        assert!(fingerprint_entities(&[e1, e2], PRODUCER).is_none());

        let o1 = sample_occurrence();
        let mut o2 = sample_occurrence();
        o2.kind = OccurrenceKind::Read;
        assert!(fingerprint_occurrences(&[o1, o2], PRODUCER).is_none());

        let g1 = sample_edge();
        let mut g2 = sample_edge();
        g2.to_entity_id = EntityId(1);
        assert!(fingerprint_edges(&[g1, g2], PRODUCER).is_none());
    }

    #[test]
    fn fingerprint_schema_movement_cannot_reuse_an_old_digest() {
        let anchors = [sample_anchor()];
        let v1 = fingerprint_anchors_with_schema(&anchors, 1, PRODUCER);
        let v2 = fingerprint_anchors_with_schema(&anchors, 2, PRODUCER);
        assert_ne!(v1, v2);
        assert!(v1.is_some() && v2.is_some());
    }

    #[test]
    fn producer_schema_is_part_of_every_category_digest() {
        let base = baseline();
        assert_ne!(fingerprint_anchors(&[sample_anchor()], 2), base.anchors);
        assert_ne!(fingerprint_entities(&[sample_entity()], 2), base.entities);
        assert_ne!(fingerprint_occurrences(&[sample_occurrence()], 2), base.occurrences);
        assert_ne!(fingerprint_edges(&[sample_edge()], 2), base.edges);
    }

    #[test]
    fn empty_categories_have_some_deterministic_fingerprint() {
        let empty_a = fingerprint_anchors(&[], PRODUCER);
        let empty_e = fingerprint_entities(&[], PRODUCER);
        let empty_o = fingerprint_occurrences(&[], PRODUCER);
        let empty_g = fingerprint_edges(&[], PRODUCER);
        assert!(empty_a.is_some());
        assert!(empty_e.is_some());
        assert!(empty_o.is_some());
        assert!(empty_g.is_some());
        assert_ne!(empty_a, empty_e);
        assert_eq!(empty_a, fingerprint_anchors(&[], PRODUCER));
    }

    #[test]
    fn golden_portable_category_fingerprints() {
        // These hex pins prove the encoder is platform-independent: they must
        // remain identical across hosts, endianness, and `std` hasher seeds.
        // Recompute only when FACT_FINGERPRINT_SCHEMA_VERSION or the encoded
        // field set changes.
        let got = baseline();
        assert_eq!(format!("{:016x}", got.anchors.expect("anchors")), "e179da30d2a27094");
        assert_eq!(format!("{:016x}", got.entities.expect("entities")), "1bfbecbae33327d8");
        assert_eq!(format!("{:016x}", got.occurrences.expect("occurrences")), "9e69e8b638c468e4");
        assert_eq!(format!("{:016x}", got.edges.expect("edges")), "97ac4b20002aeac5");
    }
}
