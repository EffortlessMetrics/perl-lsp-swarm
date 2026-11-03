//! CA02A descriptive capability contract. This neither grants authority nor changes settings.
//! IDs resolve only against the canonical catalog; no alternate field registry is accepted.
use super::{ConfigConsumer, FieldAuthority, authority_by_id};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityRole {
    Select,
    Arm,
    Trigger,
    Widen,
    Reduce,
    Project,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExternalEffect {
    Process,
    Network,
    ExternalFilesystemRead,
    WorkspaceWrite,
    CredentialUse,
    ResourceBudget,
    PresentationOnly,
    None,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CompositionRule {
    ComposesWith { field: String },
    RequiresAuthorityFrom { field: String },
    MayOnlyReduce,
    HardProductEnvelope { owner: String },
    ExplicitUserActionRequired,
    AutomaticLifecycleTrigger,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ProofRequirement {
    FirstEffect { owner: String, negative_control_id: String },
    Unsupported { owner: String, reason: String },
    Removed { owner: String, reason: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewedException {
    pub(crate) owner: String,
    pub(crate) review_ref: String,
    pub(crate) reason: String,
}
/// A staged, explicit declaration, not a default classification for unpopulated rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CapabilityDeclaration {
    pub(crate) field_id: String,
    pub(crate) roles: Vec<CapabilityRole>,
    pub(crate) effects: Vec<ExternalEffect>,
    pub(crate) composition: Vec<CompositionRule>,
    pub(crate) proof: Option<ProofRequirement>,
    pub(crate) reduction_exception: Option<ReviewedException>,
}
#[derive(Debug)]
pub(crate) struct CheckedCapability<'a> {
    pub(crate) field: &'static FieldAuthority,
    pub(crate) declaration: &'a CapabilityDeclaration,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapabilityViolationKind {
    CollectionBounds,
    DuplicateDeclaration,
    UnknownField,
    InvalidRoles,
    InvalidEffects,
    DuplicateComposition,
    MissingProof,
    InvalidProof,
    ReductionStrengthens,
    InvalidException,
    MissingEnvelope,
    PlannerEffectMismatch,
    UnknownTarget,
    UnclassifiedAuthorityTarget,
    AuthorityCycle,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CapabilityViolation {
    pub(crate) field_id: String,
    pub(crate) kind: CapabilityViolationKind,
}
const MAX_DECLARATIONS: usize = 256;
const MAX_COMPOSITION: usize = 32;
fn violation(id: &str, kind: CapabilityViolationKind) -> CapabilityViolation {
    CapabilityViolation { field_id: id.to_owned(), kind }
}
fn nonempty(value: &str) -> bool {
    !value.trim().is_empty()
}
fn unique<T: Ord>(values: &[T]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() == values.len()
}
fn valid_proof(proof: &ProofRequirement) -> bool {
    match proof {
        ProofRequirement::FirstEffect { owner, negative_control_id } => {
            nonempty(owner) && nonempty(negative_control_id)
        }
        ProofRequirement::Unsupported { owner, reason }
        | ProofRequirement::Removed { owner, reason } => nonempty(owner) && nonempty(reason),
    }
}
fn validate_row(
    row: &CapabilityDeclaration,
    field: &FieldAuthority,
) -> Result<(), CapabilityViolation> {
    use CapabilityViolationKind as V;
    let fail = |kind| violation(&row.field_id, kind);
    if row.roles.is_empty() || row.roles.len() > 6 || !unique(&row.roles) {
        return Err(fail(V::InvalidRoles));
    }
    if row.effects.is_empty() || row.effects.len() > 8 || !unique(&row.effects) {
        return Err(fail(V::InvalidEffects));
    }
    if row.composition.len() > MAX_COMPOSITION {
        return Err(fail(V::CollectionBounds));
    }
    if !unique(&row.composition) {
        return Err(fail(V::DuplicateComposition));
    }
    if (row.effects.contains(&ExternalEffect::None)
        || row.effects.contains(&ExternalEffect::PresentationOnly))
        && row.effects.len() != 1
    {
        return Err(fail(V::InvalidEffects));
    }
    if row.effects.iter().any(|effect| *effect != ExternalEffect::None) && row.proof.is_none() {
        return Err(fail(V::MissingProof));
    }
    if row.effects.contains(&ExternalEffect::None)
        && matches!(row.proof, Some(ProofRequirement::FirstEffect { .. }))
    {
        return Err(fail(V::InvalidProof));
    }
    if row.proof.as_ref().is_some_and(|proof| !valid_proof(proof)) {
        return Err(fail(V::InvalidProof));
    }
    if let Some(exception) = &row.reduction_exception
        && (!nonempty(&exception.owner)
            || !nonempty(&exception.review_ref)
            || !nonempty(&exception.reason))
    {
        return Err(fail(V::InvalidException));
    }
    if (row.composition.contains(&CompositionRule::MayOnlyReduce)
        || row.roles.contains(&CapabilityRole::Reduce))
        && row.roles.iter().any(|role| {
            matches!(
                role,
                CapabilityRole::Select
                    | CapabilityRole::Arm
                    | CapabilityRole::Trigger
                    | CapabilityRole::Widen
            )
        })
        && row.reduction_exception.is_none()
    {
        return Err(fail(V::ReductionStrengthens));
    }
    if row.effects.contains(&ExternalEffect::ResourceBudget)
        && !row.composition.iter().any(|rule| matches!(rule, CompositionRule::HardProductEnvelope { owner } if nonempty(owner))) {
        return Err(fail(V::MissingEnvelope));
    }
    if row.composition.iter().any(
        |rule| matches!(rule, CompositionRule::HardProductEnvelope { owner } if !nonempty(owner)),
    ) {
        return Err(fail(V::MissingEnvelope));
    }
    if (row.effects.contains(&ExternalEffect::PresentationOnly)
        || row.effects.contains(&ExternalEffect::None))
        && field.consumers.iter().any(|consumer| {
            matches!(
                consumer,
                ConfigConsumer::AiTransport
                    | ConfigConsumer::AiScheduler
                    | ConfigConsumer::BoundedExecution
                    | ConfigConsumer::ResultCaps
                    | ConfigConsumer::LegacyCritic
                    | ConfigConsumer::SaveFormatting
                    | ConfigConsumer::ExternalFormatter
                    | ConfigConsumer::PerlToolchain
                    | ConfigConsumer::ModuleResolver
                    | ConfigConsumer::WorkspaceDiscovery
                    | ConfigConsumer::WorkspaceIndex
                    | ConfigConsumer::DependencyGraph
            )
        })
    {
        return Err(fail(V::PlannerEffectMismatch));
    }
    Ok(())
}
/// Validate a bounded staged population, retaining declaration and canonical row identities.
/// Output order is canonical by field ID; neither source authority nor capability data is rewritten.
pub(crate) fn validate_capabilities(
    rows: &[CapabilityDeclaration],
) -> Result<Vec<CheckedCapability<'_>>, CapabilityViolation> {
    use CapabilityViolationKind as V;
    if rows.len() > MAX_DECLARATIONS {
        return Err(violation("", V::CollectionBounds));
    }
    let mut indexed = BTreeMap::new();
    for row in rows {
        if indexed.insert(row.field_id.as_str(), row).is_some() {
            return Err(violation(&row.field_id, V::DuplicateDeclaration));
        }
    }
    let mut result = Vec::new();
    for (id, row) in &indexed {
        let field = authority_by_id(id).ok_or_else(|| violation(id, V::UnknownField))?;
        validate_row(row, field)?;
        for rule in &row.composition {
            let target = match rule {
                CompositionRule::ComposesWith { field }
                | CompositionRule::RequiresAuthorityFrom { field } => field,
                _ => continue,
            };
            if authority_by_id(target).is_none() {
                return Err(violation(id, V::UnknownTarget));
            }
            if matches!(rule, CompositionRule::RequiresAuthorityFrom { .. })
                && !indexed.contains_key(target.as_str())
            {
                return Err(violation(id, V::UnclassifiedAuthorityTarget));
            }
        }
        result.push(CheckedCapability { field, declaration: row });
    }
    // Iterative reachability is bounded by MAX_DECLARATIONS, avoiding recursion on supplied graphs.
    for id in indexed.keys() {
        let mut pending = vec![*id];
        let mut seen = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(current) {
                continue;
            }
            if let Some(row) = indexed.get(current) {
                for rule in &row.composition {
                    if let CompositionRule::RequiresAuthorityFrom { field } = rule {
                        if field.as_str() == *id {
                            return Err(violation(id, V::AuthorityCycle));
                        }
                        pending.push(field.as_str());
                    }
                }
            }
        }
    }
    Ok(result)
}
#[cfg(test)]
#[path = "capability_tests.rs"]
mod tests;
