use super::validation::{Context, anchor, binding, classes, count, name, order, scope};
use super::{
    Binding, BoundaryDisposition, BoundaryDraft, BoundaryReason, FactClass, ImpactSelector,
    LifecyclePhase, MAX_BOUNDARY_ITEMS, MAX_BOUNDARY_TEXT_BYTES, MAX_SNAPSHOT_BYTES, Outcome,
    PendingAuthority, PortRole, SchemaError, wire,
};

fn invalid(message: &str) -> SchemaError {
    SchemaError::Invalid(message.into())
}
fn bounded_text(text: &str) -> Result<(), SchemaError> {
    count(text.len(), MAX_BOUNDARY_TEXT_BYTES, "boundary text bytes")
}
fn finite_classes(values: &mut [FactClass]) -> Result<(), SchemaError> {
    count(values.len(), MAX_BOUNDARY_ITEMS, "fact classes")?;
    classes(values)?;
    if values.contains(&FactClass::AllFacts) {
        return Err(invalid("all facts requires unbounded selector"));
    }
    Ok(())
}
fn selectors(values: &mut Vec<ImpactSelector>, subject: &Binding) -> Result<(), SchemaError> {
    count(values.len(), MAX_BOUNDARY_ITEMS, "impact selectors")?;
    let has_broad = values.iter().any(|value| matches!(value, ImpactSelector::Unbounded { .. }));
    if has_broad && values.len() != 1 {
        return Err(invalid("unbounded/narrow impact contradiction"));
    }
    for value in values.iter_mut() {
        match value {
            ImpactSelector::StrictCategory(_) | ImpactSelector::Unbounded { .. } => {}
            ImpactSelector::NamedFact { name: value, .. } => name(value)?,
            ImpactSelector::CategorySubtree { class, category, pending } => {
                name(category)?;
                if *class != FactClass::Warnings || *pending != PendingAuthority::CategoryCatalog {
                    return Err(invalid("category subtree lacks matching pending catalog"));
                }
            }
            ImpactSelector::SourceSuffix { .. } => {}
            ImpactSelector::LexicalScope { scope: value, .. } => scope(value, subject)?,
            ImpactSelector::Package { package, .. } => order(package)?,
            ImpactSelector::ModuleClosure { module, pending, classes, .. } => {
                name(module)?;
                if *pending != PendingAuthority::ModuleGraph {
                    return Err(invalid("module closure lacks pending graph"));
                }
                finite_classes(classes)?;
            }
            ImpactSelector::DeclaredFacts(values) => finite_classes(values)?,
        }
        if !matches!(value, ImpactSelector::Unbounded { .. })
            && value.fact_classes().contains(&FactClass::AllFacts)
        {
            return Err(invalid("all facts requires unbounded selector"));
        }
    }
    let mut keyed = values
        .drain(..)
        .map(|value| {
            let key = wire::encode(&value, MAX_SNAPSHOT_BYTES)?;
            Ok((key, value))
        })
        .collect::<Result<Vec<_>, SchemaError>>()?;
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    if keyed.windows(2).any(|pair| pair.first().map(|x| &x.0) == pair.get(1).map(|x| &x.0)) {
        return Err(invalid("duplicate impact selector"));
    }
    values.extend(keyed.into_iter().map(|(_, value)| value));
    Ok(())
}
pub(super) fn validate(value: &mut BoundaryDraft) -> Result<(), SchemaError> {
    if value.schema_version != 1 {
        return Err(invalid("boundary schema version"));
    }
    binding(&value.binding)?;
    scope(&value.scope, &value.binding)?;
    if let Some(package) = &value.package {
        order(package)?;
    }
    name(&value.effect_id)?;
    anchor(&value.anchor)?;
    if value.start_byte > value.end_byte {
        return Err(invalid("inverted boundary byte range"));
    }
    // This schema has a content digest, not source bytes: it cannot attest byte
    // length or UTF-8 boundaries. A producer must validate those against its snapshot.
    count(
        value.possible_impacts.len().saturating_add(value.applied_impacts.len()),
        MAX_BOUNDARY_ITEMS,
        "impact selectors",
    )?;
    if value.possible_impacts.is_empty() {
        return Err(invalid("missing possible impact declaration"));
    }
    selectors(&mut value.possible_impacts, &value.binding)?;
    selectors(&mut value.applied_impacts, &value.binding)?;
    if value.applied_impacts.iter().any(|impact| !value.possible_impacts.contains(impact)) {
        return Err(invalid("applied impact absent from possible impacts"));
    }
    count(value.possible_states.len(), MAX_BOUNDARY_ITEMS, "possible states")?;
    value.possible_states.sort();
    if value.possible_states.windows(2).any(|pair| pair.first() == pair.get(1)) {
        return Err(invalid("duplicate possible state"));
    }
    let mut context = Context::new(&value.binding, &value.scope);
    context.port(&mut value.effect, PortRole::DirectiveEffect)?;
    context.origins(&mut value.origins)?;
    match value.reason {
        BoundaryReason::StaleSource
        | BoundaryReason::StaleProfile
        | BoundaryReason::StaleDependency
            if value.disposition != BoundaryDisposition::Stale =>
        {
            return Err(invalid("stale reason requires stale disposition"));
        }
        BoundaryReason::ResourceExhaustion | BoundaryReason::InstrumentFailure
            if value.disposition != BoundaryDisposition::ResourceOrInstrumentFailure =>
        {
            return Err(invalid("resource reason requires resource/instrument disposition"));
        }
        _ => {}
    }
    let exact = value.disposition.outcome() == Outcome::Exact;
    if exact
        && (value.effect.outcome != Outcome::Exact
            || value.phase == LifecyclePhase::Unknown
            || value.origins.is_empty()
            || value.origins.iter().any(|origin| !origin.provenance.is_exact_grade())
            || value.possible_impacts.iter().any(ImpactSelector::cannot_be_exact))
    {
        return Err(invalid("exact boundary lacks current resolved authority"));
    }
    match value.disposition {
        BoundaryDisposition::ExactApplied if value.applied_impacts.is_empty() => {
            return Err(invalid("exact-applied has no applied effects"));
        }
        BoundaryDisposition::ExactNoEffect if !value.applied_impacts.is_empty() => {
            return Err(invalid("exact-no-effect has applied effects"));
        }
        _ => {}
    }
    bounded_text(&value.metadata.explanation)?;
    if let Some(text) = &value.metadata.trigger_excerpt {
        bounded_text(text)?;
    }
    if let Some(owner) = &value.metadata.owner {
        name(owner)?;
    }
    if let Some(replacement) = &value.metadata.replacement {
        name(replacement)?;
    }
    count(value.metadata.limitations.len(), MAX_BOUNDARY_ITEMS, "boundary limitations")?;
    for text in &value.metadata.limitations {
        bounded_text(text)?;
    }
    Ok(())
}
