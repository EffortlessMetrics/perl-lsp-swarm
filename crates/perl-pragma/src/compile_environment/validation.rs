use super::{
    AuthorityPort, Binding, Boundary, Delta, EncodingState, Facet, FactClass, LocaleState,
    MAX_BUILTINS, MAX_CUSTOM_NAMES, MAX_DELTA_ENTRIES, MAX_NAME_BYTES, MAX_REFERENCES, NamedFact,
    Origin, Outcome, PortRole, SchemaError, SemanticScopeIdentity, SemanticSourceAnchor,
    SemanticSourceOrderIdentity, SemanticSubjectGeneration, StateDraft, TransitionDraft,
    VersionDeclaration,
};
use perl_semantic_facts::semantic_identity::{
    SemanticDeclarationKey, SemanticScopeRecovery, SemanticSemanticProfileIdentity,
};
use std::collections::BTreeMap;

fn invalid(message: &str) -> SchemaError {
    SchemaError::Invalid(message.into())
}
pub(super) fn count(actual: usize, limit: usize, name: &'static str) -> Result<(), SchemaError> {
    if actual > limit { Err(SchemaError::Limited(name)) } else { Ok(()) }
}
fn name(value: &str) -> Result<(), SchemaError> {
    count(value.len(), MAX_NAME_BYTES, "name bytes")?;
    if value.trim().is_empty() {
        return Err(invalid("empty identity/name"));
    }
    Ok(())
}
fn semantic<T>(result: Result<T, impl std::fmt::Display>) -> Result<T, SchemaError> {
    result.map_err(|error| SchemaError::Invalid(error.to_string()))
}
fn subject(value: &SemanticSubjectGeneration) -> Result<(), SchemaError> {
    let profile = value.profile();
    for text in [
        value.logical_source_id(),
        value.source_generation(),
        value.parser_snapshot_id(),
        value.parser_configuration_id(),
        profile.profile_id(),
        profile.profile_digest(),
    ] {
        name(text)?;
    }
    let profile = semantic(SemanticSemanticProfileIdentity::new(
        profile.profile_id(),
        profile.profile_digest(),
    ))?;
    semantic(SemanticSubjectGeneration::new(
        value.logical_source_id(),
        value.source_generation(),
        value.parser_snapshot_id(),
        value.parser_configuration_id(),
        profile,
    ))?;
    Ok(())
}
fn anchor(value: &SemanticSourceAnchor) -> Result<(), SchemaError> {
    name(value.anchor_digest())?;
    semantic(SemanticSourceAnchor::new(
        value.anchor_role(),
        value.anchor_digest(),
        value.sibling_ordinal(),
    ))?;
    Ok(())
}
fn order(value: &SemanticSourceOrderIdentity) -> Result<(), SchemaError> {
    name(value.context_digest())?;
    semantic(SemanticSourceOrderIdentity::new(value.context_ordinal(), value.context_digest()))?;
    Ok(())
}
fn binding(value: &Binding) -> Result<(), SchemaError> {
    subject(&value.semantic)?;
    name(&value.compiler_generation)?;
    if !value.source.schema_version.is_supported() {
        return Err(invalid("source schema version"));
    }
    if let Some(revision) = &value.source.content_revision
        && revision.logical_source_id != value.source.logical_source_id
    {
        return Err(invalid("revision/source identity mismatch"));
    }
    if let Some(generation) = value.source.generation.as_label() {
        name(generation)?;
        if generation != value.semantic.source_generation() {
            return Err(invalid("source generation mismatch"));
        }
    }
    Ok(())
}
fn scope(value: &SemanticScopeIdentity, binding: &Binding) -> Result<(), SchemaError> {
    subject(value.subject())?;
    if value.subject() != &binding.semantic {
        return Err(invalid("scope subject mismatch"));
    }
    anchor(value.anchor())?;
    if let Some(context) = value.package_context() {
        order(context)?;
    }
    if let Some(parent) = value.parent_fingerprint() {
        name(parent)?;
    }
    let declaration = value
        .owning_declaration_key()
        .map(|key| {
            name(key.declaration_form())?;
            name(key.declared_name())?;
            name(key.declaration_digest())?;
            semantic(SemanticDeclarationKey::new(
                key.declaration_form(),
                key.declared_name(),
                key.declaration_digest(),
            ))
        })
        .transpose()?;
    semantic(SemanticScopeIdentity::new(
        value.subject().clone(),
        value.kind(),
        declaration,
        value.parent_fingerprint().map(str::to_owned),
        value.anchor().clone(),
        value.package_context().cloned(),
        value.recovery(),
    ))?;
    semantic(value.validate())?;
    Ok(())
}
struct Context<'a> {
    binding: &'a Binding,
    exact_scope: bool,
    origins: BTreeMap<String, Origin>,
}
impl Context<'_> {
    fn origin(&mut self, value: &Origin) -> Result<(), SchemaError> {
        name(&value.id)?;
        anchor(&value.anchor)?;
        if let Some(previous) = self.origins.get(&value.id) {
            if previous != value {
                return Err(invalid("contradictory provenance reference"));
            }
        } else {
            count(self.origins.len() + 1, MAX_REFERENCES, "provenance references")?;
            self.origins.insert(value.id.clone(), value.clone());
        }
        Ok(())
    }
    fn origins(&mut self, values: &mut [Origin]) -> Result<(), SchemaError> {
        count(values.len(), MAX_REFERENCES, "provenance references")?;
        values.sort_by(|a, b| a.id.cmp(&b.id));
        for pair in values.windows(2) {
            if pair.first().map(|x| &x.id) == pair.get(1).map(|x| &x.id) {
                return Err(invalid("duplicate provenance reference"));
            }
        }
        for value in values {
            self.origin(value)?;
        }
        Ok(())
    }
    fn facet<T>(&mut self, value: &mut Facet<T>) -> Result<(), SchemaError> {
        self.origins(&mut value.origins)?;
        if value.outcome == Outcome::Exact {
            if value.value.is_none()
                || value.reason.is_some()
                || value.origins.is_empty()
                || !value.origins.iter().all(|origin| origin.provenance.is_exact_grade())
                || !self.exact_scope
                || self.binding.source.content_revision.is_none()
                || self.binding.source.generation.as_label().is_none()
            {
                return Err(invalid("exact facet lacks current complete evidence"));
            }
        } else {
            name(value.reason.as_deref().ok_or_else(|| invalid("nonexact facet needs reason"))?)?;
        }
        Ok(())
    }
    fn port(
        &mut self,
        value: &mut Facet<AuthorityPort>,
        role: PortRole,
    ) -> Result<(), SchemaError> {
        self.facet(value)?;
        if let Some(port) = &value.value {
            if port.role != role {
                return Err(invalid("port authority role mismatch"));
            }
            if port.schema_version != 1 {
                return Err(invalid("port schema version"));
            }
            name(&port.contract)?;
            name(&port.compiler_generation)?;
            subject(&port.subject)?;
            if port.subject != self.binding.semantic
                || port.compiler_generation != self.binding.compiler_generation
            {
                return Err(invalid("port subject/profile/generation mismatch"));
            }
        }
        Ok(())
    }
    fn named(
        &mut self,
        value: &mut Facet<Vec<NamedFact>>,
        limit: usize,
    ) -> Result<(), SchemaError> {
        self.facet(value)?;
        if let Some(values) = &mut value.value {
            count(values.len(), limit, "named facts")?;
            values.sort_by(|a, b| a.name.cmp(&b.name));
            for pair in values.windows(2) {
                if pair.first().map(|x| &x.name) == pair.get(1).map(|x| &x.name) {
                    return Err(invalid("duplicate named fact"));
                }
            }
            for item in values {
                name(&item.name)?;
                self.origin(&item.origin)?;
                if value.outcome == Outcome::Exact && !item.origin.provenance.is_exact_grade() {
                    return Err(invalid("exact named fact has unknown origin"));
                }
            }
        }
        Ok(())
    }
    fn boundaries(&mut self, values: &mut [Boundary]) -> Result<(), SchemaError> {
        count(values.len(), MAX_REFERENCES, "boundary references")?;
        values.sort_by(|a, b| a.id.cmp(&b.id));
        for pair in values.windows(2) {
            if pair.first().map(|x| &x.id) == pair.get(1).map(|x| &x.id) {
                return Err(invalid("duplicate boundary"));
            }
        }
        for value in values {
            name(&value.id)?;
            classes(&mut value.affects)?;
            self.port(&mut value.authority, PortRole::Boundary)?;
        }
        Ok(())
    }
    fn version(&mut self, value: &mut Facet<VersionDeclaration>) -> Result<(), SchemaError> {
        self.facet(value)?;
        if let Some(VersionDeclaration::Declared(text)) = &value.value {
            name(text)?;
        }
        Ok(())
    }
    fn strings(&mut self, value: &mut Facet<Vec<String>>) -> Result<(), SchemaError> {
        self.facet(value)?;
        if let Some(values) = &mut value.value {
            strings(values, MAX_CUSTOM_NAMES)?;
        }
        Ok(())
    }
    fn encoding(&mut self, value: &mut Facet<EncodingState>) -> Result<(), SchemaError> {
        self.facet(value)?;
        if let Some(EncodingState { encoding: Some(text), .. }) = &value.value {
            name(text)?;
        }
        Ok(())
    }
    fn locale(&mut self, value: &mut Facet<LocaleState>) -> Result<(), SchemaError> {
        self.facet(value)?;
        if let Some(locale) = &mut value.value {
            strings(&mut locale.categories, MAX_CUSTOM_NAMES)?;
        }
        Ok(())
    }
}
fn strings(values: &mut [String], limit: usize) -> Result<(), SchemaError> {
    count(values.len(), limit, "names")?;
    values.sort();
    for value in values.iter() {
        name(value)?;
    }
    if values.windows(2).any(|pair| pair.first() == pair.get(1)) {
        return Err(invalid("duplicate name"));
    }
    Ok(())
}
fn classes(values: &mut [FactClass]) -> Result<(), SchemaError> {
    if values.is_empty() {
        return Err(invalid("empty affected fact classes"));
    }
    values.sort();
    if values.windows(2).any(|pair| pair.first() == pair.get(1)) {
        return Err(invalid("duplicate affected class"));
    }
    Ok(())
}
pub(super) fn state(value: &mut StateDraft) -> Result<(), SchemaError> {
    if value.schema_version != 1 {
        return Err(invalid("state schema version"));
    }
    binding(&value.binding)?;
    scope(&value.scope, &value.binding)?;
    if let Some(package) = &value.package {
        order(package)?;
    }
    let mut context = Context {
        binding: &value.binding,
        exact_scope: value.scope.recovery() == SemanticScopeRecovery::Exact,
        origins: BTreeMap::new(),
    };
    context.port(&mut value.profile, PortRole::LanguageProfile)?;
    context.version(&mut value.version)?;
    context.strings(&mut value.requirements)?;
    context.facet(&mut value.strict_vars)?;
    context.facet(&mut value.strict_subs)?;
    context.facet(&mut value.strict_refs)?;
    context.port(&mut value.warnings, PortRole::WarningPolicy)?;
    strings(&mut value.warning_names, MAX_CUSTOM_NAMES)?;
    context.named(&mut value.features, MAX_CUSTOM_NAMES)?;
    context.named(&mut value.builtins, MAX_BUILTINS)?;
    context.encoding(&mut value.encoding)?;
    context.locale(&mut value.locale)?;
    context.boundaries(&mut value.boundaries)?;
    Ok(())
}
pub(super) fn transition(value: &mut TransitionDraft) -> Result<(), SchemaError> {
    if value.schema_version != 1 {
        return Err(invalid("transition schema version"));
    }
    name(&value.id)?;
    binding(&value.binding)?;
    scope(&value.scope, &value.binding)?;
    order(&value.order)?;
    if let Some(package) = &value.package {
        order(package)?;
    }
    classes(&mut value.affects)?;
    count(value.delta.len(), MAX_DELTA_ENTRIES, "delta entries")?;
    let mut context = Context {
        binding: &value.binding,
        exact_scope: value.scope.recovery() == SemanticScopeRecovery::Exact,
        origins: BTreeMap::new(),
    };
    context.port(&mut value.effect, PortRole::DirectiveEffect)?;
    context.origins(&mut value.origins)?;
    context.boundaries(&mut value.boundaries)?;
    for delta in &mut value.delta {
        let class = match delta {
            Delta::StrictVars(facet) | Delta::StrictSubs(facet) | Delta::StrictRefs(facet) => {
                context.facet(facet)?;
                FactClass::Strict
            }
            Delta::Warnings(facet) => {
                context.port(facet, PortRole::WarningPolicy)?;
                FactClass::Warnings
            }
            Delta::Profile(facet) => {
                context.port(facet, PortRole::LanguageProfile)?;
                FactClass::Profile
            }
            Delta::Version(facet) => {
                context.version(facet)?;
                FactClass::Versions
            }
            Delta::Requirements(facet) => {
                context.strings(facet)?;
                FactClass::Versions
            }
            Delta::Features(facet) => {
                context.named(facet, MAX_CUSTOM_NAMES)?;
                FactClass::Features
            }
            Delta::Builtins(facet) => {
                context.named(facet, MAX_BUILTINS)?;
                FactClass::Builtins
            }
            Delta::Encoding(facet) => {
                context.encoding(facet)?;
                FactClass::Encoding
            }
            Delta::Locale(facet) => {
                context.locale(facet)?;
                FactClass::Locale
            }
        };
        if !value.affects.contains(&class) {
            return Err(invalid("delta missing affected class"));
        }
    }
    Ok(())
}
#[cfg(test)]
mod limit_tests {
    use super::{
        MAX_BUILTINS, MAX_CUSTOM_NAMES, MAX_DELTA_ENTRIES, MAX_REFERENCES, SchemaError, count,
        invalid,
    };
    use crate::compile_environment::MAX_TRANSITIONS;
    #[test]
    fn declared_collection_boundaries() -> Result<(), SchemaError> {
        for (limit, label) in [
            (MAX_TRANSITIONS, "transitions"),
            (MAX_REFERENCES, "references"),
            (MAX_CUSTOM_NAMES, "custom names"),
            (MAX_BUILTINS, "builtins"),
            (MAX_DELTA_ENTRIES, "deltas"),
        ] {
            count(limit, limit, label)?;
            if count(limit + 1, limit, label) != Err(SchemaError::Limited(label)) {
                return Err(invalid("boundary+1 did not fail"));
            }
        }
        Ok(())
    }
}
