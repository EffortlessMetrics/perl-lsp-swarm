//! Versioned builtin catalogs and immutable supplied policies, not an interpreter.
//! Selected Perl version and opaque consumer profile remain independent identities.
use super::{
    AuthorityPort, Binding, BoundaryReference, Facet, MAX_SNAPSHOT_BYTES, Origin, Outcome,
    PortRole, SchemaError, SemanticScopeIdentity, validation, wire,
};
use perl_source_identity::ContentDigest;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::sync::LazyLock;

/// Operational category bound, not a Perl limit.
pub const MAX_CATEGORIES: usize = 256;
/// Operational ancestor depth bound.
pub const MAX_CATEGORY_DEPTH: usize = 16;
/// Operational total alias bound.
pub const MAX_ALIASES: usize = 256;
/// Operational effective override bound.
pub const MAX_OVERRIDES: usize = 1024;
/// Operational distinct unknown-name bound.
pub const MAX_UNKNOWN_NAMES: usize = 256;
fn invalid(message: &str) -> SchemaError {
    SchemaError::Invalid(message.into())
}

/// Fatality is distinct from ordinary enablement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningDisposition {
    /// Suppressed warning.
    Disabled,
    /// Ordinary warning.
    Enabled,
    /// Warning raised as an exception.
    Fatal,
}
/// Exact catalog identity, independent of document/profile identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogIdentity {
    /// Exact patch version, with no nearest-profile fallback.
    pub profile: String,
    /// Complete domain-separated catalog content identity.
    pub digest: ContentDigest,
}
/// Untrusted reference: resolve against its catalog before treating it as known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryReference {
    /// Exact selected catalog.
    pub catalog: CatalogIdentity,
    /// Stable name-derived ID, never a bit offset.
    pub id: String,
}
/// Raw primary source identity, without newline normalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSource {
    /// Exact upstream tag and path.
    pub url: String,
    /// SHA256 of source bytes.
    pub sha256: String,
}
/// Documentation association only, not feature admission or emission logic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentedRelationship {
    /// Feature-section association or signature syntax context.
    pub kind: String,
    /// Documented owner, not inferred from category spelling.
    pub related_name: String,
    /// Historical/documentation-only or syntax qualification.
    pub qualifier: String,
    /// Exact section locator.
    pub source_section: String,
    /// Version-pinned documentation URL.
    pub source_url: String,
    /// Raw documentation identity.
    pub source_sha256: String,
}
/// Complete builtin category and separate native oracle metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarningCategory {
    /// Stable canonical ID.
    pub id: String,
    /// Canonical source spelling.
    pub name: String,
    /// Explicit parent; separators do not imply ancestry.
    pub parent: Option<String>,
    /// Raw decimal metadata, not an inferred stable-version support range.
    pub introduced_upstream: String,
    /// Literal upstream default declaration, absent on grouping-only nodes.
    pub declared_default: Option<String>,
    /// Only source-established aliases (currently none).
    pub aliases: Vec<String>,
    /// Builtin membership; custom registration is not admitted.
    pub builtin: bool,
    /// Empty means no association established by selected authorities.
    pub documented_relationships: Vec<DocumentedRelationship>,
    /// Native oracle bit position, never the stable identity.
    pub native_offset: usize,
    /// Generated ordinary-warning mask as hexadecimal.
    pub enabled_mask: String,
    /// Generated fatal-warning mask as hexadecimal.
    pub fatal_mask: String,
    /// This category's own bit in the generated default mask.
    pub default_enabled: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetiredTreeMetadata {
    id: String,
    name: String,
    parent: Option<String>,
    introduced_upstream: String,
    declared_default: Option<String>,
    aliases: Vec<String>,
    builtin: bool,
    documented_relationships: Vec<DocumentedRelationship>,
}
/// Accepted zero-mask spelling, not a live category alias.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedNoOp {
    /// Accepted historical name.
    pub name: String,
    tree_metadata: Option<RetiredTreeMetadata>,
    /// Established documentation associations.
    pub documented_relationships: Vec<DocumentedRelationship>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogData {
    schema_version: u32,
    profile: String,
    sources: Vec<CatalogSource>,
    categories: Vec<WarningCategory>,
    accepted_noops: Vec<AcceptedNoOp>,
    default_mask: String,
}
/// Immutable admission of the source-checked embedded catalog.
#[derive(Debug)]
pub struct WarningCatalog {
    data: CatalogData,
    identity: CatalogIdentity,
}
static CATALOGS: LazyLock<Result<Vec<WarningCatalog>, String>> = LazyLock::new(|| {
    let data: Vec<CatalogData> =
        serde_json::from_str(include_str!("../../tests/fixtures/warning_catalogs.json"))
            .map_err(|error| error.to_string())?;
    data.into_iter()
        .map(|data| WarningCatalog::admit(data).map_err(|error| error.to_string()))
        .collect()
});
impl WarningCatalog {
    fn admit(data: CatalogData) -> Result<Self, SchemaError> {
        if data.schema_version != 1 {
            return Err(invalid("warning catalog schema"));
        }
        validation::name(&data.profile)?;
        validation::count(data.categories.len(), MAX_CATEGORIES, "warning categories")?;
        validation::count(data.accepted_noops.len(), MAX_CATEGORIES, "retired warning names")?;
        let mut names = BTreeSet::new();
        let mut ids = BTreeSet::new();
        let mut aliases = 0;
        for row in &data.categories {
            validation::name(&row.name)?;
            validation::name(&row.id)?;
            if row.id != format!("perl.warning/{}", row.name)
                || !row.builtin
                || !names.insert(row.name.clone())
                || !ids.insert(&row.id)
            {
                return Err(invalid("warning category ID/duplicate/builtin"));
            }
            for alias in &row.aliases {
                validation::name(alias)?;
                aliases += 1;
                if !names.insert(alias.clone()) {
                    return Err(invalid("warning alias collision"));
                }
            }
        }
        validation::count(aliases, MAX_ALIASES, "warning aliases")?;
        for retired in &data.accepted_noops {
            validation::name(&retired.name)?;
            if !names.insert(retired.name.clone()) {
                return Err(invalid("retired/live warning collision"));
            }
        }
        let rows: BTreeMap<_, _> =
            data.categories.iter().map(|row| (row.name.as_str(), row)).collect();
        if rows.get("all").is_none_or(|row| row.parent.is_some()) {
            return Err(invalid("missing warning root"));
        }
        for row in &data.categories {
            let mut current = row;
            let mut seen = BTreeSet::new();
            while let Some(parent) = &current.parent {
                if !seen.insert(&current.name) {
                    return Err(invalid("warning category cycle"));
                }
                validation::count(seen.len(), MAX_CATEGORY_DEPTH, "warning category depth")?;
                current = rows
                    .get(parent.as_str())
                    .copied()
                    .ok_or_else(|| invalid("missing warning parent"))?;
            }
            if current.name != "all" {
                return Err(invalid("detached warning category"));
            }
        }
        let digest = ContentDigest::of_bytes(&wire::canonical(
            "warning_catalog.v1",
            &data,
            MAX_SNAPSHOT_BYTES,
        )?);
        Ok(Self { identity: CatalogIdentity { profile: data.profile.clone(), digest }, data })
    }
    /// All profiles except the exact sourced patch versions are unavailable.
    pub fn for_profile(profile: &str) -> Result<&'static Self, SchemaError> {
        CATALOGS
            .as_ref()
            .map_err(|error| SchemaError::Instrument(error.clone()))?
            .iter()
            .find(|catalog| catalog.data.profile == profile)
            .ok_or(SchemaError::Unavailable)
    }
    /// Complete independent catalog identity.
    pub fn identity(&self) -> &CatalogIdentity {
        &self.identity
    }
    /// Complete live category denominator.
    pub fn categories(&self) -> &[WarningCategory] {
        &self.data.categories
    }
    /// Accepted retired spellings, separate from live categories.
    pub fn accepted_noops(&self) -> &[AcceptedNoOp] {
        &self.data.accepted_noops
    }
    /// Version-pinned raw authority identities.
    pub fn sources(&self) -> &[CatalogSource] {
        &self.data.sources
    }
    fn find(&self, name: &str) -> Option<&WarningCategory> {
        self.data
            .categories
            .iter()
            .find(|row| row.name == name || row.aliases.iter().any(|alias| alias == name))
    }
    /// Create a bound reference only for a real live member.
    pub fn category_reference(&self, name: &str) -> Result<CategoryReference, SchemaError> {
        let row = self.find(name).ok_or(SchemaError::Unavailable)?;
        Ok(CategoryReference { catalog: self.identity.clone(), id: row.id.clone() })
    }
    /// Revalidate an untrusted catalog/category reference.
    pub fn resolve_reference(
        &self,
        reference: &CategoryReference,
    ) -> Result<&WarningCategory, SchemaError> {
        if reference.catalog != self.identity {
            return Err(invalid("warning catalog/profile mismatch"));
        }
        self.data
            .categories
            .iter()
            .find(|row| row.id == reference.id)
            .ok_or(SchemaError::Unavailable)
    }
    fn matches(&self, target: &WarningCategory, selected: &WarningCategory) -> bool {
        let mut current = Some(target);
        while let Some(row) = current {
            if row.id == selected.id {
                return true;
            }
            current = row.parent.as_deref().and_then(|parent| self.find(parent));
        }
        false
    }
}
/// Qualified policy baseline; generated defaults are per-category, not uniform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningBaseline {
    /// Use each category's own generated default bit.
    CatalogDefaults,
    /// Explicit already-interpreted uniform state.
    Uniform(WarningDisposition),
}
/// Explicit payload avoids exact-known absence being serialized as null.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningCoverage {
    /// Complete supplied policy.
    Complete,
    /// Producer declares partial coverage.
    Partial,
}
/// Already interpreted effective override, not source directive syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarningOverride {
    /// Unique effective order; vector insertion order is irrelevant.
    pub ordinal: u32,
    /// Live, accepted no-op, or retained unknown spelling.
    pub name: String,
    /// Qualified supplied warning state.
    pub disposition: Facet<WarningDisposition>,
}
/// Untrusted policy draft; catalog and complete consumer binding are independent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarningPolicyDraft {
    /// Wire schema (1).
    pub schema_version: u32,
    /// Selected exact catalog, a producer language-profile declaration.
    pub catalog: CatalogIdentity,
    /// Accepted document/source/parser/profile/compiler identity.
    pub binding: Binding,
    /// Scope of this supplied policy.
    pub scope: SemanticScopeIdentity,
    /// Qualified baseline.
    pub baseline: Facet<WarningBaseline>,
    /// Effective overrides, normalized by ordinal.
    pub overrides: Vec<WarningOverride>,
    /// Supplied aggregate coverage; exact may not conceal qualified inputs.
    pub coverage: Facet<WarningCoverage>,
    /// Existing canonical boundary references; no selector authority invented.
    pub boundaries: Vec<BoundaryReference>,
}
/// Lookup preserves live membership, accepted no-op and custom/unknown distinctly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningLookup {
    /// Live catalog member.
    Known(CategoryReference),
    /// Accepted historical no-effect spelling.
    AcceptedNoOp(String),
    /// Retained spelling without registration authority.
    Unknown(String),
}
/// Original qualification, never ranked by an invented severity ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarningQualification {
    /// Actual supplied outcome.
    pub outcome: Outcome,
    /// Original nonexact reason.
    pub reason: Option<String>,
}
/// Origin of a contributing piece of query evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningEvidenceSource {
    /// Aggregate policy coverage.
    Coverage,
    /// Supplied baseline.
    Baseline,
    /// Applicable or unknown-extent effective override.
    Override {
        /// Producer order.
        ordinal: u32,
        /// Retained supplied spelling.
        name: String,
    },
    /// Full canonical boundary reference, not just an anonymous qualification.
    Boundary(Box<BoundaryReference>),
}
/// Bounded contributing evidence, including overwritten matching overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarningEvidence {
    /// Actual source of this evidence.
    pub source: WarningEvidenceSource,
    /// Original qualification, without severity ranking.
    pub qualification: WarningQualification,
    /// Original provenance references.
    pub origins: Vec<Origin>,
}
/// Immutable explanation and qualified value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarningAnswer {
    /// Catalog, no-op or unknown identity.
    lookup: WarningLookup,
    /// Known state; no-op and unknown have no invented value.
    disposition: Option<WarningDisposition>,
    /// Applicable qualifications preserve distinct causes.
    qualifications: Vec<WarningQualification>,
    /// Provenance of selected baseline or winning override.
    origins: Vec<Origin>,
    /// Winning ordinal; absent means baseline.
    effective_ordinal: Option<u32>,
    /// Contributing evidence, bounded by the admitted policy's counts and bytes.
    evidence: Vec<WarningEvidence>,
}
impl WarningAnswer {
    /// Validated lookup result.
    pub fn lookup(&self) -> &WarningLookup {
        &self.lookup
    }
    /// Known supplied value, still subject to qualifications.
    pub fn disposition(&self) -> Option<WarningDisposition> {
        self.disposition
    }
    /// All retained qualification causes.
    pub fn qualifications(&self) -> &[WarningQualification] {
        &self.qualifications
    }
    /// Winning value's provenance convenience view.
    pub fn origins(&self) -> &[Origin] {
        &self.origins
    }
    /// Winning effective ordinal, if any.
    pub fn effective_ordinal(&self) -> Option<u32> {
        self.effective_ordinal
    }
    /// All applicable contributing evidence.
    pub fn evidence(&self) -> &[WarningEvidence] {
        &self.evidence
    }
    /// Exact category value only; no-op is not silently called disabled.
    pub fn exact(&self) -> Result<WarningDisposition, SchemaError> {
        if !matches!(self.lookup, WarningLookup::Known(_))
            || self.qualifications.is_empty()
            || self.qualifications.iter().any(|q| q.outcome != Outcome::Exact)
        {
            return Err(SchemaError::Unavailable);
        }
        self.disposition.ok_or(SchemaError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_ALIASES, MAX_CATEGORIES, MAX_CATEGORY_DEPTH, WarningAnswer, WarningCatalog,
        WarningDisposition, WarningLookup,
    };
    #[test]
    fn contradictory_internal_answer_cannot_claim_exact() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut forged = WarningAnswer {
            lookup: WarningLookup::Unknown("custom".into()),
            disposition: Some(WarningDisposition::Fatal),
            qualifications: vec![],
            origins: vec![],
            effective_ordinal: None,
            evidence: vec![],
        };
        if forged.exact().is_ok() {
            return Err("unknown empty-evidence answer forged exact fatal".into());
        }
        forged
            .qualifications
            .push(super::WarningQualification { outcome: super::Outcome::Exact, reason: None });
        if forged.exact().is_ok() {
            return Err("unknown answer with exact evidence forged exact fatal".into());
        }
        forged.lookup = WarningLookup::Known(
            WarningCatalog::for_profile("5.44.0")?.category_reference("numeric")?,
        );
        forged.qualifications.clear();
        if forged.exact().is_ok() {
            return Err("known answer without evidence forged exact fatal".into());
        }
        Ok(())
    }
    #[test]
    fn catalog_cycles_aliases_duplicates_and_bounds_are_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let catalog = WarningCatalog::for_profile("5.44.0")?;
        let mut data = catalog.data.clone();
        data.categories.first_mut().ok_or("missing row")?.parent = Some("numeric".into());
        if WarningCatalog::admit(data).is_ok() {
            return Err("root/cycle admitted".into());
        }
        let mut data = catalog.data.clone();
        let duplicate = data.categories.first().ok_or("missing row")?.clone();
        data.categories.push(duplicate);
        if WarningCatalog::admit(data).is_ok() {
            return Err("duplicate category ID admitted".into());
        }
        let mut data = catalog.data.clone();
        data.categories.first_mut().ok_or("missing row")?.aliases = vec!["numeric".into()];
        if WarningCatalog::admit(data).is_ok() {
            return Err("alias/member collision admitted".into());
        }
        let mut data = catalog.data.clone();
        let row = data.categories.first().ok_or("missing row")?.clone();
        data.categories.resize(MAX_CATEGORIES + 1, row);
        if WarningCatalog::admit(data).is_ok() {
            return Err("category cap ignored".into());
        }
        let mut data = catalog.data.clone();
        data.categories.first_mut().ok_or("missing row")?.aliases =
            (0..MAX_ALIASES + 1).map(|i| format!("alias{i}")).collect();
        if WarningCatalog::admit(data).is_ok() {
            return Err("alias cap ignored".into());
        }
        for depth in [MAX_CATEGORY_DEPTH, MAX_CATEGORY_DEPTH + 1] {
            let mut data = catalog.data.clone();
            let template = data
                .categories
                .iter()
                .find(|r| r.name == "numeric")
                .ok_or("missing numeric")?
                .clone();
            for i in 0..depth {
                let mut row = template.clone();
                row.name = format!("depth{i}");
                row.id = format!("perl.warning/{}", row.name);
                row.parent = Some(if i == 0 { "all".into() } else { format!("depth{}", i - 1) });
                data.categories.push(row);
            }
            if WarningCatalog::admit(data).is_ok() != (depth == MAX_CATEGORY_DEPTH) {
                return Err("catalog depth boundary changed".into());
            }
        }
        Ok(())
    }
}
/// Validated immutable policy; does not parse/apply use or no warnings.
#[derive(Debug, Clone)]
pub struct WarningPolicy {
    draft: WarningPolicyDraft,
}
fn qualification<T>(facet: &Facet<T>) -> WarningQualification {
    WarningQualification { outcome: facet.outcome, reason: facet.reason.clone() }
}
fn evidence<T>(source: WarningEvidenceSource, facet: &Facet<T>) -> WarningEvidence {
    WarningEvidence { source, qualification: qualification(facet), origins: facet.origins.clone() }
}
impl WarningPolicy {
    /// Validate identities, evidence, membership, order, coverage and bounds.
    pub fn admit(mut draft: WarningPolicyDraft) -> Result<Self, SchemaError> {
        if draft.schema_version != 1 {
            return Err(invalid("warning policy schema"));
        }
        let catalog = WarningCatalog::for_profile(&draft.catalog.profile)?;
        if draft.catalog != *catalog.identity() {
            return Err(invalid("warning catalog digest"));
        }
        validation::binding(&draft.binding)?;
        validation::scope(&draft.scope, &draft.binding)?;
        validation::count(draft.overrides.len(), MAX_OVERRIDES, "warning overrides")?;
        let mut context = validation::Context::new(&draft.binding, &draft.scope);
        context.facet(&mut draft.baseline)?;
        context.facet(&mut draft.coverage)?;
        context.boundaries(&mut draft.boundaries)?;
        draft.overrides.sort_by_key(|item| item.ordinal);
        let mut last = None;
        let mut unknown = BTreeSet::new();
        for item in &mut draft.overrides {
            if last == Some(item.ordinal) {
                return Err(invalid("duplicate warning effective ordinal"));
            }
            last = Some(item.ordinal);
            validation::name(&item.name)?;
            context.facet(&mut item.disposition)?;
            if catalog.find(&item.name).is_none()
                && !catalog.accepted_noops().iter().any(|row| row.name == item.name)
            {
                unknown.insert(&item.name);
                if item.disposition.outcome == Outcome::Exact {
                    return Err(invalid("unknown warning override cannot be exact"));
                }
            }
        }
        validation::count(unknown.len(), MAX_UNKNOWN_NAMES, "unknown warning names")?;
        if draft.coverage.outcome == Outcome::Exact
            && (draft.coverage.value != Some(WarningCoverage::Complete)
                || draft.baseline.outcome != Outcome::Exact
                || draft.overrides.iter().any(|item| item.disposition.outcome != Outcome::Exact)
                || draft.boundaries.iter().any(|item| item.authority.outcome != Outcome::Exact))
        {
            return Err(invalid("exact warning coverage conceals qualified input"));
        }
        wire::encode(&draft, MAX_SNAPSHOT_BYTES)?;
        Ok(Self { draft })
    }
    /// Bounded strict wire admission, including shared nested fields.
    pub fn read(reader: impl Read) -> Result<Self, SchemaError> {
        Self::admit(wire::read(reader, MAX_SNAPSHOT_BYTES)?)
    }
    /// Canonical normalized policy payload.
    pub fn to_json(&self) -> Result<Vec<u8>, SchemaError> {
        wire::encode(&self.draft, MAX_SNAPSHOT_BYTES)
    }
    /// Domain-separated identity of the policy and both independent bindings.
    pub fn digest(&self) -> Result<ContentDigest, SchemaError> {
        let mut semantic = serde_json::to_value(&self.draft)
            .map_err(|error| SchemaError::Instrument(error.to_string()))?;
        for field in ["baseline", "coverage"] {
            if let Some(object) = semantic.get_mut(field).and_then(serde_json::Value::as_object_mut)
            {
                object.remove("reason");
            }
        }
        for (collection, facet) in [("overrides", "disposition"), ("boundaries", "authority")] {
            if let Some(values) =
                semantic.get_mut(collection).and_then(serde_json::Value::as_array_mut)
            {
                for value in values {
                    if let Some(object) =
                        value.get_mut(facet).and_then(serde_json::Value::as_object_mut)
                    {
                        object.remove("reason");
                    }
                }
            }
        }
        Ok(ContentDigest::of_bytes(&wire::canonical(
            "warning_policy.v1",
            &semantic,
            MAX_SNAPSHOT_BYTES,
        )?))
    }
    /// Validated normalized draft.
    pub fn draft(&self) -> &WarningPolicyDraft {
        &self.draft
    }
    fn current(&self, expected: &Binding, catalog: &CatalogIdentity) -> bool {
        expected == &self.draft.binding && catalog == &self.draft.catalog
    }
    /// Query against the caller's current complete subject and selected catalog.
    pub fn warning_disposition_by_name(
        &self,
        name: &str,
        expected: &Binding,
        identity: &CatalogIdentity,
    ) -> Result<WarningAnswer, SchemaError> {
        validation::name(name)?;
        let catalog = WarningCatalog::for_profile(&self.draft.catalog.profile)?;
        let row = catalog.find(name);
        let lookup = if let Some(row) = row {
            WarningLookup::Known(catalog.category_reference(&row.name)?)
        } else if catalog.accepted_noops().iter().any(|item| item.name == name) {
            WarningLookup::AcceptedNoOp(name.into())
        } else {
            WarningLookup::Unknown(name.into())
        };
        let mut answer = WarningAnswer {
            lookup,
            disposition: None,
            qualifications: vec![qualification(&self.draft.coverage)],
            origins: Vec::new(),
            effective_ordinal: None,
            evidence: vec![evidence(WarningEvidenceSource::Coverage, &self.draft.coverage)],
        };
        if !self.current(expected, identity) {
            answer.qualifications.push(WarningQualification {
                outcome: Outcome::Stale,
                reason: Some("warning policy binding/catalog mismatch".into()),
            });
            return Ok(answer);
        }
        for boundary in &self.draft.boundaries {
            answer.qualifications.push(qualification(&boundary.authority));
            answer.evidence.push(evidence(
                WarningEvidenceSource::Boundary(Box::new(boundary.clone())),
                &boundary.authority,
            ));
        }
        for item in &self.draft.overrides {
            if catalog.find(&item.name).is_none()
                && !catalog.accepted_noops().iter().any(|noop| noop.name == item.name)
            {
                answer.qualifications.push(qualification(&item.disposition));
                answer.evidence.push(evidence(
                    WarningEvidenceSource::Override {
                        ordinal: item.ordinal,
                        name: item.name.clone(),
                    },
                    &item.disposition,
                ));
            }
        }
        let Some(row) = row else {
            if matches!(answer.lookup, WarningLookup::Unknown(_)) {
                answer.qualifications.push(WarningQualification {
                    outcome: Outcome::Unavailable,
                    reason: Some("no admitted warning registration authority".into()),
                });
            }
            return Ok(answer);
        };
        answer.qualifications.push(qualification(&self.draft.baseline));
        answer.evidence.push(evidence(WarningEvidenceSource::Baseline, &self.draft.baseline));
        answer.origins = self.draft.baseline.origins.clone();
        answer.disposition = self.draft.baseline.value.map(|baseline| match baseline {
            WarningBaseline::CatalogDefaults => {
                if row.default_enabled {
                    WarningDisposition::Enabled
                } else {
                    WarningDisposition::Disabled
                }
            }
            WarningBaseline::Uniform(value) => value,
        });
        for item in &self.draft.overrides {
            if let Some(selected) = catalog.find(&item.name)
                && catalog.matches(row, selected)
            {
                answer.disposition = item.disposition.value;
                answer.qualifications.push(qualification(&item.disposition));
                answer.evidence.push(evidence(
                    WarningEvidenceSource::Override {
                        ordinal: item.ordinal,
                        name: item.name.clone(),
                    },
                    &item.disposition,
                ));
                answer.origins = item.disposition.origins.clone();
                answer.effective_ordinal = Some(item.ordinal);
            }
        }
        Ok(answer)
    }
    /// Query a bound category ID; wrong catalog or unknown ID is refused.
    pub fn warning_disposition(
        &self,
        category: &CategoryReference,
        expected: &Binding,
    ) -> Result<WarningAnswer, SchemaError> {
        let catalog = WarningCatalog::for_profile(&self.draft.catalog.profile)?;
        let row = catalog.resolve_reference(category)?;
        self.warning_disposition_by_name(&row.name, expected, &category.catalog)
    }
    /// Root category only, not a uniform-descendant assertion.
    pub fn all_warnings_disposition(
        &self,
        expected: &Binding,
        catalog: &CatalogIdentity,
    ) -> Result<WarningAnswer, SchemaError> {
        self.warning_disposition_by_name("all", expected, catalog)
    }
    /// Includes winning ordinal, origins and all original qualifications.
    pub fn explain_warning(
        &self,
        name: &str,
        expected: &Binding,
        catalog: &CatalogIdentity,
    ) -> Result<WarningAnswer, SchemaError> {
        self.warning_disposition_by_name(name, expected, catalog)
    }
    /// Existing #8520 typed port, validating local consistency, not payload authenticity.
    pub fn port(
        &self,
        expected: &Binding,
        catalog: &CatalogIdentity,
    ) -> Result<Facet<AuthorityPort>, SchemaError> {
        if !self.current(expected, catalog) {
            return Err(invalid("warning policy port binding/catalog mismatch"));
        }
        Ok(Facet {
            outcome: self.draft.coverage.outcome,
            reason: self.draft.coverage.reason.clone(),
            origins: self.draft.coverage.origins.clone(),
            value: Some(AuthorityPort {
                role: PortRole::WarningPolicy,
                schema_version: 1,
                contract: "warning_policy.v1".into(),
                subject: self.draft.binding.semantic.clone(),
                compiler_generation: self.draft.binding.compiler_generation.clone(),
                payload: self.digest()?,
            }),
        })
    }
}
