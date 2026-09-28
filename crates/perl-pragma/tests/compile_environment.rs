use perl_pragma::compile_environment::*;
use perl_semantic_facts::semantic_identity::*;
use perl_semantic_facts::{LifecyclePhase, Provenance, SemanticProvenance};
use perl_source_identity::*;
use std::io::Read;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn require(value: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    if value { Ok(()) } else { Err(message.into()) }
}
fn origin() -> Result<Origin, Box<dyn std::error::Error>> {
    Ok(Origin {
        id: "source-effect".into(),
        provenance: SemanticProvenance::Known(Provenance::ExactAst),
        anchor: SemanticSourceAnchor::new(SemanticAnchorRole::ContextMarker, "pragma-digest", 0)?,
    })
}
fn exact<T>(value: T) -> Result<Facet<T>, Box<dyn std::error::Error>> {
    Ok(Facet {
        outcome: Outcome::Exact,
        value: Some(value),
        reason: None,
        origins: vec![origin()?],
    })
}
fn unavailable<T>() -> Facet<T> {
    Facet {
        outcome: Outcome::Unavailable,
        value: None,
        reason: Some("port not supplied".into()),
        origins: vec![],
    }
}
fn fixture() -> Result<StateDraft, Box<dyn std::error::Error>> {
    let project = ProjectId::from_canonical_name("fixture");
    let root = WorkspaceRootId::from_project_and_root_key(&project, "root");
    let file = LogicalSourceId::from_root_and_path(&root, "lib/Foo.pm");
    let source = SourceIdentityEnvelope::for_workspace_file(
        project,
        root,
        file.clone(),
        Some(ContentRevision::new(file, ContentDigest::of_bytes(b"source"))),
        SourceGeneration::Known("generation-a".into()),
    );
    let subject = SemanticSubjectGeneration::new(
        "root-instance:document-instance",
        "generation-a",
        "parser-a",
        "parser-config",
        SemanticSemanticProfileIdentity::new("profile", "profile-digest")?,
    )?;
    let scope = SemanticScopeIdentity::new(
        subject.clone(),
        SemanticScopeKind::File,
        None,
        None,
        origin()?.anchor,
        None,
        SemanticScopeRecovery::Exact,
    )?;
    Ok(StateDraft {
        schema_version: 1,
        binding: Binding { source, semantic: subject, compiler_generation: "compiler-a".into() },
        scope,
        package: None,
        phase: LifecyclePhase::Begin,
        profile: unavailable(),
        version: exact(VersionDeclaration::Absent)?,
        requirements: exact(vec![])?,
        strict_vars: exact(false)?,
        strict_subs: exact(false)?,
        strict_refs: exact(false)?,
        warnings: unavailable(),
        warning_names: vec![],
        features: exact(vec![])?,
        builtins: exact(vec![])?,
        encoding: exact(EncodingState { utf8: false, encoding: None })?,
        locale: exact(LocaleState { enabled: false, categories: vec![] })?,
        boundaries: vec![],
    })
}
fn named(name: &str) -> Result<NamedFact, Box<dyn std::error::Error>> {
    Ok(NamedFact { name: name.into(), known: false, origin: origin()? })
}
fn transition() -> Result<TransitionDraft, Box<dyn std::error::Error>> {
    let state = CompileEnvironmentState::admit(fixture()?)?;
    Ok(TransitionDraft {
        schema_version: 1,
        id: "transition-1".into(),
        effect: unavailable(),
        binding: state.draft().binding.clone(),
        scope: state.draft().scope.clone(),
        package: None,
        phase: LifecyclePhase::Begin,
        order: SemanticSourceOrderIdentity::new(0, "source-order")?,
        byte_anchor: 3,
        kind: TransitionKind::ScopeRestore,
        before: state.digest()?,
        after: state.digest()?,
        delta: vec![Delta::StrictVars(exact(false)?)],
        affects: vec![FactClass::Strict],
        origins: vec![origin()?],
        boundaries: vec![],
    })
}
#[test]
fn strict_requires_all_three_categories() -> TestResult {
    for bits in 0u8..8 {
        let strict =
            StrictCategories { vars: bits & 1 != 0, subs: bits & 2 != 0, refs: bits & 4 != 0 };
        require(
            strict.full_strict() == (bits == 7),
            &format!("incorrect full strict for {bits:03b}"),
        )?;
    }
    Ok(())
}
#[test]
fn qualified_projection_preserves_unrelated_legacy_and_signatures_feature() -> TestResult {
    let mut draft = fixture()?;
    draft.features = exact(vec![named("signatures")?])?;
    let state = CompileEnvironmentState::admit(draft.clone())?;
    require(!state.strict()?.full_strict(), "signatures bridged strict")?;
    let mut legacy = perl_pragma::PragmaState {
        signatures_strict: true,
        warnings: true,
        features: vec!["signatures"],
        encoding: Some("custom".into()),
        ..Default::default()
    };
    let before = legacy.clone();
    state.project_strict(&mut legacy)?;
    let mut expected = before.clone();
    expected.strict_vars = false;
    expected.strict_subs = false;
    expected.strict_refs = false;
    expected.signatures_strict = false;
    require(legacy == expected, "projection changed unrelated legacy state")?;
    draft.strict_vars = unavailable();
    let unavailable_state = CompileEnvironmentState::admit(draft)?;
    require(unavailable_state.project_strict(&mut legacy).is_err(), "unknown strict projected")?;
    require(legacy == expected, "refused projection mutated legacy")
}
#[test]
fn all_outcomes_remain_distinct_and_absent_ports_never_become_exact() -> TestResult {
    let mut encoded = std::collections::BTreeSet::new();
    for outcome in [
        Outcome::Exact,
        Outcome::Conditional,
        Outcome::Limited,
        Outcome::Unsupported,
        Outcome::Stale,
        Outcome::Unavailable,
        Outcome::ResourceInstrument,
    ] {
        let mut draft = fixture()?;
        draft.strict_vars.outcome = outcome;
        draft.strict_vars.reason = (outcome != Outcome::Exact).then(|| "qualified".into());
        let state = CompileEnvironmentState::admit(draft)?;
        require(state.strict().is_ok() == (outcome == Outcome::Exact), "value upgraded outcome")?;
        encoded.insert(state.to_json()?);
    }
    require(encoded.len() == 7, "collapsed outcomes")?;
    let mut draft = fixture()?;
    draft.warnings.outcome = Outcome::Exact;
    require(CompileEnvironmentState::admit(draft).is_err(), "absent warnings became exact")?;
    let state = CompileEnvironmentState::admit(fixture()?)?;
    require(state.draft().warnings.exact().is_err(), "unavailable warning policy lost")
}
#[test]
fn canonical_sets_roundtrip_and_identity_dimensions() -> TestResult {
    let mut draft = fixture()?;
    draft.features = exact(vec![named("custom-z")?, named("signatures")?, named("custom-a")?])?;
    draft.warning_names = vec!["z".into(), "a".into()];
    let first = CompileEnvironmentState::admit(draft.clone())?;
    if let Some(features) = &mut draft.features.value {
        features.reverse();
    }
    draft.warning_names.reverse();
    let reordered = CompileEnvironmentState::admit(draft)?;
    require(
        first.to_json()? == reordered.to_json()? && first.digest()? == reordered.digest()?,
        "set order changed identity",
    )?;
    require(
        CompileEnvironmentState::read(first.to_json()?.as_slice())? == first,
        "roundtrip differs",
    )?;
    let base = first.digest()?;
    for slot in ["logical_source_id", "parser_snapshot_id", "parser_configuration_id"] {
        let mut wire = serde_json::to_value(first.draft())?;
        wire["binding"]["semantic"][slot] = "different".into();
        wire["scope"]["subject"][slot] = "different".into();
        let changed = CompileEnvironmentState::read(serde_json::to_vec(&wire)?.as_slice())?;
        require(changed.digest()? != base, "identity dimension omitted")?;
    }
    let mut draft = first.draft().clone();
    draft.binding.compiler_generation = "compiler-b".into();
    require(
        CompileEnvironmentState::admit(draft)?.digest()? != base,
        "compiler generation omitted",
    )?;
    Ok(())
}
#[test]
fn nested_serde_and_cross_subject_corruption_rejected() -> TestResult {
    let original = serde_json::to_value(fixture()?)?;
    let mut cases = Vec::new();
    let mut value = original.clone();
    value["scope"]["parent_fingerprint"] = "illegal-parent".into();
    cases.push(value);
    let mut value = original.clone();
    value["scope"]["kind"] = "LexicalBlock".into();
    cases.push(value);
    let mut value = original.clone();
    value["binding"]["semantic"]["profile"]["profile_id"] = "".into();
    value["scope"]["subject"]["profile"]["profile_id"] = "".into();
    cases.push(value);
    let mut value = original.clone();
    value["scope"]["subject"]["parser_snapshot_id"] = "wrong".into();
    cases.push(value);
    let mut value = original.clone();
    value["binding"]["semantic"]["source_generation"] = "wrong".into();
    cases.push(value);
    let mut value = original.clone();
    value["schema_version"] = 2.into();
    cases.push(value);
    let mut value = original.clone();
    value["binding"]["source"]["schema_version"] = 2.into();
    cases.push(value);
    let mut value = original.clone();
    value["strict_vars"]["origins"][0]["anchor"]["anchor_digest"] = "".into();
    cases.push(value);
    for value in cases {
        require(
            CompileEnvironmentState::read(serde_json::to_vec(&value)?.as_slice()).is_err(),
            "nested invalid wire admitted",
        )?;
    }
    let mut draft = fixture()?;
    let other =
        LogicalSourceId::from_root_and_path(&draft.binding.source.workspace_root_id, "other.pm");
    if let Some(revision) = &mut draft.binding.source.content_revision {
        revision.logical_source_id = other;
    }
    require(CompileEnvironmentState::admit(draft).is_err(), "revision logical ID mismatch admitted")
}
#[test]
fn port_subject_profile_and_compiler_binding_are_load_bearing() -> TestResult {
    let mut draft = fixture()?;
    let port = AuthorityPort {
        role: PortRole::WarningPolicy,
        schema_version: 1,
        contract: "warning-policy.v1".into(),
        subject: draft.binding.semantic.clone(),
        compiler_generation: draft.binding.compiler_generation.clone(),
        payload: ContentDigest::of_bytes(b"policy"),
    };
    draft.warnings = exact(port)?;
    let valid = CompileEnvironmentState::admit(draft)?;
    for slot in ["parser_snapshot_id", "source_generation"] {
        let mut wire = serde_json::to_value(valid.draft())?;
        wire["warnings"]["value"]["subject"][slot] = "other".into();
        require(
            CompileEnvironmentState::read(serde_json::to_vec(&wire)?.as_slice()).is_err(),
            "wrong port subject admitted",
        )?;
    }
    let mut wire = serde_json::to_value(valid.draft())?;
    wire["warnings"]["value"]["subject"]["profile"]["profile_digest"] = "other".into();
    require(
        CompileEnvironmentState::read(serde_json::to_vec(&wire)?.as_slice()).is_err(),
        "wrong port profile admitted",
    )?;
    let mut wire = serde_json::to_value(valid.draft())?;
    wire["warnings"]["value"]["compiler_generation"] = "other".into();
    require(
        CompileEnvironmentState::read(serde_json::to_vec(&wire)?.as_slice()).is_err(),
        "wrong port compiler admitted",
    )
}
struct Spaces {
    remaining: usize,
    consumed: usize,
}
impl Read for Spaces {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let length = self.remaining.min(output.len());
        for byte in output.iter_mut().take(length) {
            *byte = b' ';
        }
        self.remaining -= length;
        self.consumed += length;
        Ok(length)
    }
}
#[test]
fn wire_cap_bounds_whitespace_and_reader_consumption() -> TestResult {
    for (limit, bundle) in [(MAX_SNAPSHOT_BYTES, false), (MAX_BUNDLE_BYTES, true)] {
        let mut reader = Spaces { remaining: limit + 4096, consumed: 0 };
        let result = if bundle {
            TransitionBundle::read(&mut reader).map(|_| ())
        } else {
            CompileEnvironmentState::read(&mut reader).map(|_| ())
        };
        require(
            matches!(result, Err(SchemaError::Limited("wire bytes"))),
            "wire cap did not classify excess",
        )?;
        require(reader.consumed == limit + 1, "reader consumed beyond cap+1")?;
    }
    Ok(())
}
#[test]
fn wire_exact_boundary_depth_and_invalid_version() -> TestResult {
    let state = CompileEnvironmentState::admit(fixture()?)?;
    let mut bytes = state.to_json()?;
    bytes.resize(MAX_SNAPSHOT_BYTES, b' ');
    require(
        CompileEnvironmentState::read(bytes.as_slice())? == state,
        "exact byte limit rejected",
    )?;
    bytes.push(b' ');
    require(
        matches!(CompileEnvironmentState::read(bytes.as_slice()), Err(SchemaError::Limited(_))),
        "limit+1 accepted",
    )?;
    let nested = format!("{}0{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
    require(
        matches!(
            CompileEnvironmentState::read(nested.as_bytes()),
            Err(SchemaError::Limited("JSON depth"))
        ),
        "depth not bounded",
    )
}
#[test]
fn names_counts_and_origins_are_bounded_without_truncation() -> TestResult {
    require(
        MAX_TRANSITIONS == 65_536
            && MAX_REFERENCES == 256
            && MAX_CUSTOM_NAMES == 1024
            && MAX_NAME_BYTES == 256
            && MAX_BUILTINS == 4096
            && MAX_DELTA_ENTRIES == 4096
            && MAX_SNAPSHOT_BYTES == 1_048_576
            && MAX_BUNDLE_BYTES == 16_777_216
            && MAX_DEPTH == 32,
        "production limits drifted",
    )?;
    let mut draft = fixture()?;
    draft.warning_names = (0..MAX_CUSTOM_NAMES).map(|i| format!("custom-{i}")).collect();
    CompileEnvironmentState::admit(draft.clone())?;
    draft.warning_names.push("over-limit".into());
    require(
        matches!(CompileEnvironmentState::admit(draft), Err(SchemaError::Limited(_))),
        "name count truncated",
    )?;
    let mut draft = fixture()?;
    draft.warning_names = vec!["x".repeat(MAX_NAME_BYTES)];
    CompileEnvironmentState::admit(draft.clone())?;
    draft.warning_names = vec!["x".repeat(MAX_NAME_BYTES + 1)];
    require(
        matches!(CompileEnvironmentState::admit(draft), Err(SchemaError::Limited(_))),
        "name length ignored",
    )?;
    let mut draft = fixture()?;
    draft.strict_vars.origins = (0..MAX_REFERENCES)
        .map(|i| {
            let mut value = origin()?;
            value.id = format!("origin-{i}");
            Ok(value)
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    // Other facets reference source-effect, so aggregate count would be 257.
    require(
        matches!(CompileEnvironmentState::admit(draft), Err(SchemaError::Limited(_))),
        "aggregate provenance cap ignored",
    )?;
    Ok(())
}
#[test]
fn ordered_transitions_restore_and_domain_identity() -> TestResult {
    let first = transition()?;
    let mut second = first.clone();
    second.id = "transition-2".into();
    second.order = SemanticSourceOrderIdentity::new(1, "source-order")?;
    let bundle = TransitionBundle::admit(vec![first.clone(), second.clone()])?;
    let read = TransitionBundle::read(bundle.to_json()?.as_slice())?;
    require(read == bundle, "bundle roundtrip changed order")?;
    require(
        TransitionBundle::admit(vec![second, first.clone()]).is_err(),
        "transition sequence sorted instead of rejected",
    )?;
    let mut wrong = first.clone();
    wrong.affects = vec![FactClass::Features];
    require(CompileEnvironmentTransition::admit(wrong).is_err(), "delta impact ignored")?;
    let mut wrong = first.clone();
    wrong.schema_version = 2;
    require(CompileEnvironmentTransition::admit(wrong).is_err(), "transition schema ignored")?;
    let identity = CompileEnvironmentTransition::admit(first.clone())?.digest()?;
    require(identity != first.before, "state and transition domains alias")?;
    let mut changed = first;
    changed.id = "different-effect".into();
    require(
        CompileEnvironmentTransition::admit(changed)?.digest()? != identity,
        "transition ID omitted",
    )
}
#[test]
fn collection_routes_reject_excess_and_preserve_boundary_references() -> TestResult {
    let mut draft = fixture()?;
    let mut origins = vec![origin()?];
    for i in 1..MAX_REFERENCES {
        let mut item = origin()?;
        item.id = format!("origin-{i}");
        origins.push(item);
    }
    draft.strict_vars.origins = origins;
    CompileEnvironmentState::admit(draft.clone())?;
    let mut excess = origin()?;
    excess.id = "excess".into();
    draft.strict_vars.origins.push(excess);
    require(
        matches!(CompileEnvironmentState::admit(draft), Err(SchemaError::Limited(_))),
        "provenance limit+1 accepted",
    )?;
    let mut draft = fixture()?;
    draft.boundaries = (0..MAX_REFERENCES)
        .map(|i| BoundaryReference {
            id: ContentDigest::of_bytes(format!("boundary-{i}").as_bytes()),
            binding: draft.binding.clone(),
            disposition: BoundaryDisposition::Unavailable,
            pending_authority: false,
            affects: vec![FactClass::Warnings],
            authority: unavailable(),
        })
        .collect();
    CompileEnvironmentState::admit(draft.clone())?;
    draft.boundaries.push(BoundaryReference {
        id: ContentDigest::of_bytes(b"excess"),
        binding: draft.binding.clone(),
        disposition: BoundaryDisposition::Unavailable,
        pending_authority: false,
        affects: vec![FactClass::Warnings],
        authority: unavailable(),
    });
    require(
        matches!(CompileEnvironmentState::admit(draft), Err(SchemaError::Limited(_))),
        "boundary limit+1 accepted",
    )?;
    let mut draft = fixture()?;
    draft.features = exact(
        (0..=MAX_CUSTOM_NAMES).map(|i| named(&format!("f-{i}"))).collect::<Result<Vec<_>, _>>()?,
    )?;
    require(
        matches!(CompileEnvironmentState::admit(draft), Err(SchemaError::Limited(_))),
        "feature count ignored",
    )?;
    let mut draft = fixture()?;
    draft.builtins = exact(
        (0..=MAX_BUILTINS).map(|i| named(&format!("b-{i}"))).collect::<Result<Vec<_>, _>>()?,
    )?;
    require(
        matches!(CompileEnvironmentState::admit(draft), Err(SchemaError::Limited(_))),
        "builtin count ignored",
    )?;
    let mut draft = transition()?;
    draft.delta = vec![Delta::StrictVars(exact(false)?); MAX_DELTA_ENTRIES + 1];
    require(
        matches!(CompileEnvironmentTransition::admit(draft), Err(SchemaError::Limited(_))),
        "delta count ignored",
    )
}
#[test]
fn schema_rejects_host_metadata_and_empty_currentness() -> TestResult {
    let mut value = serde_json::to_value(fixture()?)?;
    value["host_path"] = "C:/secret/path".into();
    require(
        CompileEnvironmentState::read(serde_json::to_vec(&value)?.as_slice()).is_err(),
        "host path entered identity",
    )?;
    let mut value = serde_json::to_value(fixture()?)?;
    value["timestamp"] = 123.into();
    require(
        CompileEnvironmentState::read(serde_json::to_vec(&value)?.as_slice()).is_err(),
        "timestamp entered identity",
    )?;
    for generation in [SourceGeneration::Unknown, SourceGeneration::Known(String::new())] {
        let mut draft = fixture()?;
        draft.binding.source.generation = generation;
        require(
            CompileEnvironmentState::admit(draft).is_err(),
            "unknown generation supports exact facet",
        )?;
    }
    Ok(())
}
#[test]
fn bounded_output_refuses_oversized_constructed_snapshot() -> TestResult {
    let mut draft = fixture()?;
    // Unique maximum-size names are valid individually but cannot bypass snapshot cap.
    draft.builtins = exact(
        (0..MAX_BUILTINS)
            .map(|i| named(&format!("{i:04}{}", "x".repeat(MAX_NAME_BYTES - 4))))
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    require(
        matches!(
            CompileEnvironmentState::admit(draft),
            Err(SchemaError::Limited("snapshot bytes"))
        ),
        "output snapshot cap missing",
    )
}
#[test]
fn version_absence_declaration_and_unavailability_roundtrip_distinctly() -> TestResult {
    let mut outputs = std::collections::BTreeSet::new();
    for version in [
        exact(VersionDeclaration::Absent)?,
        exact(VersionDeclaration::Declared("v5.44".into()))?,
        unavailable(),
    ] {
        let mut draft = fixture()?;
        draft.version = version;
        let state = CompileEnvironmentState::admit(draft)?;
        let bytes = state.to_json()?;
        require(
            CompileEnvironmentState::read(bytes.as_slice())? == state,
            "version roundtrip changed qualification",
        )?;
        outputs.insert(bytes);
    }
    require(outputs.len() == 3, "absent/unavailable versions collapsed")?;
    let mut draft = fixture()?;
    draft.version = exact(VersionDeclaration::Declared(String::new()))?;
    require(CompileEnvironmentState::admit(draft).is_err(), "empty declared version admitted")
}
#[test]
fn strict_delta_preserves_independent_qualification_and_origins() -> TestResult {
    let mut draft = transition()?;
    let mut subs = exact(true)?;
    subs.origins.first_mut().ok_or("missing origin")?.id = "subs-effect".into();
    draft.delta = vec![
        Delta::StrictVars(unavailable()),
        Delta::StrictSubs(subs),
        Delta::StrictRefs(unavailable()),
    ];
    let bundle = TransitionBundle::admit(vec![draft])?;
    let restored = TransitionBundle::read(bundle.to_json()?.as_slice())?;
    let record = restored.transitions().first().ok_or("missing transition")?.draft();
    let [Delta::StrictVars(vars), Delta::StrictSubs(subs), Delta::StrictRefs(refs)] =
        record.delta.as_slice()
    else {
        return Err("strict delta identities collapsed".into());
    };
    require(
        vars.exact().is_err() && *subs.exact()? && refs.exact().is_err(),
        "strict outcome collapsed",
    )?;
    require(
        subs.origins.first().map(|origin| origin.id.as_str()) == Some("subs-effect"),
        "category origin lost",
    )
}
#[test]
fn canonical_digest_uses_prescribed_domain_payload() -> TestResult {
    let state = CompileEnvironmentState::admit(fixture()?)?;
    let expected = ContentDigest::of_bytes(&serde_json::to_vec(&(
        "compile_environment_state.v1",
        state.draft(),
    ))?);
    require(state.digest()? == expected, "state domain/payload contract drifted")?;
    let transition = CompileEnvironmentTransition::admit(transition()?)?;
    let expected = ContentDigest::of_bytes(&serde_json::to_vec(&(
        "compile_environment_transition.v1",
        transition.draft(),
    ))?);
    require(transition.digest()? == expected, "transition domain/payload contract drifted")
}
fn port(role: PortRole, binding: &Binding) -> AuthorityPort {
    AuthorityPort {
        role,
        schema_version: 1,
        contract: "opaque-contract.v1".into(),
        subject: binding.semantic.clone(),
        compiler_generation: binding.compiler_generation.clone(),
        payload: ContentDigest::of_bytes(b"opaque"),
    }
}
#[test]
fn authority_roles_cannot_cross_facets_or_transition_destinations() -> TestResult {
    for role in [
        PortRole::WarningPolicy,
        PortRole::LanguageProfile,
        PortRole::DirectiveEffect,
        PortRole::Boundary,
    ] {
        let mut draft = fixture()?;
        draft.warnings = exact(port(role, &draft.binding))?;
        require(
            CompileEnvironmentState::admit(draft.clone()).is_ok()
                == (role == PortRole::WarningPolicy),
            "warning role not enforced",
        )?;
        require(
            CompileEnvironmentState::read(serde_json::to_vec(&draft)?.as_slice()).is_ok()
                == (role == PortRole::WarningPolicy),
            "wire warning role not enforced",
        )?;
        let mut draft = fixture()?;
        draft.profile = exact(port(role, &draft.binding))?;
        require(
            CompileEnvironmentState::admit(draft).is_ok() == (role == PortRole::LanguageProfile),
            "profile role not enforced",
        )?;
        let mut draft = transition()?;
        draft.effect = exact(port(role, &draft.binding))?;
        require(
            CompileEnvironmentTransition::admit(draft.clone()).is_ok()
                == (role == PortRole::DirectiveEffect),
            "effect role not enforced",
        )?;
        require(
            TransitionBundle::read(serde_json::to_vec(&vec![draft])?.as_slice()).is_ok()
                == (role == PortRole::DirectiveEffect),
            "wire effect role not enforced",
        )?;
        let mut draft = fixture()?;
        draft.boundaries = vec![BoundaryReference {
            id: ContentDigest::of_bytes(b"opaque"),
            binding: draft.binding.clone(),
            disposition: BoundaryDisposition::ExactApplied,
            pending_authority: false,
            affects: vec![FactClass::Warnings],
            authority: exact(AuthorityPort {
                contract: "compile_effect_boundary.v1".into(),
                ..port(role, &draft.binding)
            })?,
        }];
        require(
            CompileEnvironmentState::admit(draft).is_ok() == (role == PortRole::Boundary),
            "boundary role not enforced",
        )?;
        let mut draft = transition()?;
        draft.affects = vec![FactClass::Warnings];
        draft.delta = vec![Delta::Warnings(exact(port(role, &draft.binding))?)];
        require(
            CompileEnvironmentTransition::admit(draft).is_ok() == (role == PortRole::WarningPolicy),
            "delta warning role not enforced",
        )?;
        let mut draft = transition()?;
        draft.affects = vec![FactClass::Profile];
        draft.delta = vec![Delta::Profile(exact(port(role, &draft.binding))?)];
        require(
            CompileEnvironmentTransition::admit(draft).is_ok()
                == (role == PortRole::LanguageProfile),
            "delta profile role not enforced",
        )?;
    }
    Ok(())
}
#[test]
fn nested_unknown_wire_fields_rejected_across_shared_and_tagged_records() -> TestResult {
    let mut draft = fixture()?;
    draft.package = Some(SemanticSourceOrderIdentity::new(0, "package-context")?);
    draft.warnings = exact(port(PortRole::WarningPolicy, &draft.binding))?;
    let base = serde_json::to_value(&draft)?;
    for pointer in [
        "/binding/source",
        "/binding/source/content_revision",
        "/binding/semantic",
        "/binding/semantic/profile",
        "/scope",
        "/scope/subject",
        "/scope/anchor",
        "/package",
        "/strict_vars/origins/0/anchor",
        "/warnings/value/subject",
    ] {
        let mut value = base.clone();
        value
            .pointer_mut(pointer)
            .and_then(serde_json::Value::as_object_mut)
            .ok_or("missing fixture object")?
            .insert("future_field".into(), "unsupported".into());
        require(
            CompileEnvironmentState::read(serde_json::to_vec(&value)?.as_slice()).is_err(),
            &format!("unknown nested state field accepted at {pointer}"),
        )?;
    }
    for role in [PortRole::WarningPolicy, PortRole::LanguageProfile] {
        let mut draft = transition()?;
        let authority = exact(port(role, &draft.binding))?;
        draft.delta = vec![if role == PortRole::WarningPolicy {
            Delta::Warnings(authority)
        } else {
            Delta::Profile(authority)
        }];
        draft.affects = vec![FactClass::Warnings, FactClass::Profile];
        let base = serde_json::to_value(vec![draft])?;
        let variant = if role == PortRole::WarningPolicy { "warnings" } else { "profile" };
        for pointer in [
            "/0/order".to_owned(),
            "/0/scope/anchor".to_owned(),
            format!("/0/delta/0/{variant}/value/subject"),
            format!("/0/delta/0/{variant}/value/subject/profile"),
            format!("/0/delta/0/{variant}/origins/0/anchor"),
        ] {
            let mut value = base.clone();
            value
                .pointer_mut(&pointer)
                .and_then(serde_json::Value::as_object_mut)
                .ok_or("missing tagged fixture object")?
                .insert("future_field".into(), true.into());
            require(
                TransitionBundle::read(serde_json::to_vec(&value)?.as_slice()).is_err(),
                &format!("unknown tagged field accepted at {pointer}"),
            )?;
        }
    }
    Ok(())
}
fn forget<T>(facet: &mut Facet<T>) {
    *facet = unavailable();
}
#[test]
fn nonexact_facets_cannot_bypass_source_generation_correspondence() -> TestResult {
    for generation in [
        SourceGeneration::Unknown,
        SourceGeneration::Known(String::new()),
        SourceGeneration::Known(" ".into()),
    ] {
        let mut state = fixture()?;
        state.binding.source.generation = generation.clone();
        forget(&mut state.profile);
        forget(&mut state.version);
        forget(&mut state.requirements);
        forget(&mut state.strict_vars);
        forget(&mut state.strict_subs);
        forget(&mut state.strict_refs);
        forget(&mut state.warnings);
        forget(&mut state.features);
        forget(&mut state.builtins);
        forget(&mut state.encoding);
        forget(&mut state.locale);
        require(
            CompileEnvironmentState::read(serde_json::to_vec(&state)?.as_slice()).is_err(),
            "wire nonexact state accepted unbound source generation",
        )?;
        require(
            CompileEnvironmentState::admit(state).is_err(),
            "nonexact state accepted unbound source generation",
        )?;
        let mut change = transition()?;
        change.binding.source.generation = generation;
        change.delta.clear();
        require(
            TransitionBundle::read(serde_json::to_vec(&vec![change.clone()])?.as_slice()).is_err(),
            "wire nonexact transition accepted unbound source generation",
        )?;
        require(
            CompileEnvironmentTransition::admit(change).is_err(),
            "nonexact transition accepted unbound source generation",
        )?;
    }
    Ok(())
}
#[test]
fn boundary_impacts_must_be_in_transition_aggregate() -> TestResult {
    let mut draft = transition()?;
    draft.delta.clear();
    draft.affects = vec![FactClass::Features];
    draft.boundaries = vec![BoundaryReference {
        id: ContentDigest::of_bytes(b"warning-boundary"),
        binding: draft.binding.clone(),
        disposition: BoundaryDisposition::Unavailable,
        pending_authority: false,
        affects: vec![FactClass::Warnings],
        authority: unavailable(),
    }];
    require(
        TransitionBundle::read(serde_json::to_vec(&vec![draft.clone()])?.as_slice()).is_err(),
        "wire boundary impact omitted from aggregate",
    )?;
    require(
        CompileEnvironmentTransition::admit(draft.clone()).is_err(),
        "boundary impact omitted from aggregate",
    )?;
    draft.affects.push(FactClass::Warnings);
    CompileEnvironmentTransition::admit(draft)?;
    Ok(())
}
#[test]
fn source_order_is_numeric_tuple_not_context_digest_sorting() -> TestResult {
    let mut first = transition()?;
    first.order = SemanticSourceOrderIdentity::new(2, "z-context")?;
    let mut next = first.clone();
    next.id = "next".into();
    next.order = SemanticSourceOrderIdentity::new(3, "a-context")?;
    TransitionBundle::admit(vec![first.clone(), next.clone()])?;
    next.order = SemanticSourceOrderIdentity::new(2, "other-context")?;
    require(
        TransitionBundle::admit(vec![first.clone(), next.clone()]).is_err(),
        "same-offset ordinal tie admitted",
    )?;
    next.byte_anchor += 1;
    next.order = SemanticSourceOrderIdentity::new(0, "later-source-context")?;
    TransitionBundle::admit(vec![first, next])?;
    Ok(())
}
#[test]
fn wire_rejects_duplicate_fields_and_trailing_documents() -> TestResult {
    let state = CompileEnvironmentState::admit(fixture()?)?;
    let text = String::from_utf8(state.to_json()?)?;
    let duplicate =
        text.replacen("\"schema_version\":1", "\"schema_version\":1,\"schema_version\":1", 1);
    require(
        CompileEnvironmentState::read(duplicate.as_bytes()).is_err(),
        "duplicate known field accepted",
    )?;
    require(
        CompileEnvironmentState::read(format!("{text} {{}}").as_bytes()).is_err(),
        "trailing document accepted",
    )?;
    require(
        CompileEnvironmentState::read(format!("{text} \n\t").as_bytes())? == state,
        "trailing whitespace rejected",
    )
}
#[test]
fn old_adjacent_delta_forms_reject_both_wire_orders() -> TestResult {
    let mut accepted = Vec::new();
    for role in [PortRole::WarningPolicy, PortRole::LanguageProfile] {
        let mut draft = transition()?;
        draft.delta.clear();
        draft.affects = vec![FactClass::Warnings, FactClass::Profile];
        let mut facet = serde_json::to_value(exact(port(role, &draft.binding))?)?;
        facet
            .pointer_mut("/value/subject")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or("missing subject")?
            .insert("future_field".into(), true.into());
        let facet = serde_json::to_string(&facet)?;
        let kind = if role == PortRole::WarningPolicy { "warnings" } else { "profile" };
        let tag_first = format!("{{\"facet\":\"{kind}\",\"value\":{facet}}}");
        let value_first = format!("{{\"value\":{facet},\"facet\":\"{kind}\"}}");
        let bundle = serde_json::to_string(&vec![draft])?;
        for (order, delta) in [("tag_first", tag_first), ("value_first", value_first)] {
            let wire = bundle.replacen("\"delta\":[]", &format!("\"delta\":[{delta}]"), 1);
            require(wire != bundle, "delta fixture replacement absent")?;
            if TransitionBundle::read(wire.as_bytes()).is_ok() {
                accepted.push(format!("{kind}:{order}"));
            }
        }
    }
    require(accepted.is_empty(), &format!("unknown tagged fields admitted: {accepted:?}"))
}
#[test]
fn external_delta_rejects_nested_unknowns_without_rejecting_valid_payloads() -> TestResult {
    for role in [PortRole::WarningPolicy, PortRole::LanguageProfile] {
        let mut draft = transition()?;
        let authority = exact(port(role, &draft.binding))?;
        draft.delta = vec![if role == PortRole::WarningPolicy {
            Delta::Warnings(authority)
        } else {
            Delta::Profile(authority)
        }];
        draft.affects = vec![FactClass::Warnings, FactClass::Profile];
        let variant = if role == PortRole::WarningPolicy { "warnings" } else { "profile" };
        let base = serde_json::to_value(vec![draft])?;
        require(
            base.pointer(&format!("/0/delta/0/{variant}")).is_some(),
            "not external delta wire",
        )?;
        TransitionBundle::read(serde_json::to_vec(&base)?.as_slice())?;
        for path in [
            format!("/0/delta/0/{variant}/value/subject"),
            format!("/0/delta/0/{variant}/value/subject/profile"),
            format!("/0/delta/0/{variant}/origins/0/anchor"),
        ] {
            let mut value = base.clone();
            value
                .pointer_mut(&path)
                .and_then(serde_json::Value::as_object_mut)
                .ok_or("missing external payload")?
                .insert("future_field".into(), true.into());
            require(
                TransitionBundle::read(serde_json::to_vec(&value)?.as_slice()).is_err(),
                &format!("external nested unknown accepted: {path}"),
            )?;
        }
    }
    let mut draft = fixture()?;
    draft.version = exact(VersionDeclaration::Declared("v5.44".into()))?;
    let mut value = serde_json::to_value(draft)?;
    value
        .pointer_mut("/version/value")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("missing version")?
        .insert("future_field".into(), true.into());
    require(
        CompileEnvironmentState::read(serde_json::to_vec(&value)?.as_slice()).is_err(),
        "local version enum unknown field admitted",
    )
}
#[test]
fn clean_old_adjacent_delta_forms_are_unsupported_in_both_orders() -> TestResult {
    for role in [PortRole::WarningPolicy, PortRole::LanguageProfile] {
        let mut draft = transition()?;
        draft.delta.clear();
        draft.affects = vec![FactClass::Warnings, FactClass::Profile];
        let facet = serde_json::to_string(&exact(port(role, &draft.binding))?)?;
        let kind = if role == PortRole::WarningPolicy { "warnings" } else { "profile" };
        let bundle = serde_json::to_string(&vec![draft])?;
        for delta in [
            format!("{{\"facet\":\"{kind}\",\"value\":{facet}}}"),
            format!("{{\"value\":{facet},\"facet\":\"{kind}\"}}"),
        ] {
            let wire = bundle.replacen("\"delta\":[]", &format!("\"delta\":[{delta}]"), 1);
            require(wire != bundle, "clean fixture replacement absent")?;
            require(
                TransitionBundle::read(wire.as_bytes()).is_err(),
                "clean adjacent wire shape still supported",
            )?;
        }
    }
    Ok(())
}

fn sized_builtins(
    target: usize,
    empty_size: usize,
) -> Result<Vec<NamedFact>, Box<dyn std::error::Error>> {
    let mut facts = (0..3000).map(|i| named(&format!("{i:04}"))).collect::<Result<Vec<_>, _>>()?;
    let initial = empty_size + serde_json::to_vec(&facts)?.len() - 2;
    let mut remaining = target.checked_sub(initial).ok_or("fixture exceeds target")?;
    for fact in &mut facts {
        let added = remaining.min(MAX_NAME_BYTES - fact.name.len());
        fact.name.push_str(&"x".repeat(added));
        remaining -= added;
    }
    require(remaining == 0, "fixture cannot reach target within valid names")?;
    Ok(facts)
}

#[test]
fn state_payload_cap_excludes_digest_framing() -> TestResult {
    for size in [MAX_SNAPSHOT_BYTES - 1, MAX_SNAPSHOT_BYTES, MAX_SNAPSHOT_BYTES + 1] {
        let mut draft = fixture()?;
        draft.builtins = exact(vec![])?;
        let empty_size = serde_json::to_vec(&draft)?.len();
        draft.builtins = exact(sized_builtins(size, empty_size)?)?;
        let payload = serde_json::to_vec(&draft)?;
        require(payload.len() == size, "state payload not exact requested size")?;
        let result = CompileEnvironmentState::admit(draft.clone());
        if size > MAX_SNAPSHOT_BYTES {
            require(
                matches!(result, Err(SchemaError::Limited("snapshot bytes"))),
                "oversized state admitted",
            )?;
        } else {
            let state = result?;
            require(state.to_json()?.len() == size, "state output size changed")?;
            let expected = ContentDigest::of_bytes(&serde_json::to_vec(&(
                "compile_environment_state.v1",
                state.draft(),
            ))?);
            require(state.digest()? == expected, "state framed digest changed")?;
            require(
                CompileEnvironmentState::read(payload.as_slice())?.digest()? == expected,
                "state wire boundary rejected",
            )?;
        }
    }
    Ok(())
}

#[test]
fn transition_payload_cap_excludes_digest_framing() -> TestResult {
    for size in [MAX_SNAPSHOT_BYTES - 1, MAX_SNAPSHOT_BYTES, MAX_SNAPSHOT_BYTES + 1] {
        let mut draft = transition()?;
        draft.delta = vec![Delta::Builtins(exact(vec![])?)];
        draft.affects = vec![FactClass::Builtins];
        let empty_size = serde_json::to_vec(&draft)?.len();
        draft.delta = vec![Delta::Builtins(exact(sized_builtins(size, empty_size)?)?)];
        require(
            serde_json::to_vec(&draft)?.len() == size,
            "transition payload not exact requested size",
        )?;
        let result = CompileEnvironmentTransition::admit(draft);
        if size > MAX_SNAPSHOT_BYTES {
            require(
                matches!(result, Err(SchemaError::Limited("snapshot bytes"))),
                "oversized transition admitted",
            )?;
        } else {
            let transition = result?;
            let expected = ContentDigest::of_bytes(&serde_json::to_vec(&(
                "compile_environment_transition.v1",
                transition.draft(),
            ))?);
            require(transition.digest()? == expected, "transition framed digest changed")?;
        }
    }
    Ok(())
}

// Shared fixture for the admitted boundary-record and reference contracts.
fn boundary_fixture() -> Result<BoundaryDraft, Box<dyn std::error::Error>> {
    let mut state = fixture()?;
    state.binding.source.content_revision = Some(ContentRevision::new(
        state.binding.source.logical_source_id.clone(),
        ContentDigest::of_bytes(b"use Custom;         use Custom;          "),
    ));
    Ok(BoundaryDraft {
        schema_version: 1,
        binding: state.binding.clone(),
        scope: state.scope,
        package: None,
        phase: LifecyclePhase::Begin,
        source_kind: perl_semantic_facts::CompileEffectSourceKind::UseDirective,
        effect_id: "directive-1".into(),
        anchor: origin()?.anchor,
        start_byte: 0,
        end_byte: 12,
        reason: serde_json::from_str("\"custom_import\"")?,
        disposition: serde_json::from_str("\"exact_applied\"")?,
        effect: exact(port(PortRole::DirectiveEffect, &state.binding))?,
        possible_impacts: vec![ImpactSelector::StrictCategory(StrictCategory::Vars)],
        applied_impacts: vec![ImpactSelector::StrictCategory(StrictCategory::Vars)],
        state_before: None,
        possible_states: vec![],
        origins: vec![origin()?],
        metadata: BoundaryMetadata {
            explanation: "Reviewed custom importer effect".into(),
            limitations: vec![],
            owner: Some("compiler".into()),
            replacement: None,
            trigger_excerpt: Some("use Custom;".into()),
        },
    })
}

#[test]
fn same_reason_different_anchor_changes_instance() -> TestResult {
    let first = boundary_fixture()?;
    let mut second = first.clone();
    second.anchor =
        SemanticSourceAnchor::new(SemanticAnchorRole::ContextMarker, "another-source-anchor", 1)?;
    second.start_byte = 20;
    second.end_byte = 32;
    require(first.reason == second.reason, "reason identity must remain shared")?;
    require(
        BoundaryRecord::admit(first)?.digest()? != BoundaryRecord::admit(second)?.digest()?,
        "reason-only instance identity loses source anchor",
    )
}

#[test]
fn boundary_digest_has_independent_literal_semantic_projection() -> TestResult {
    let record = BoundaryRecord::admit(boundary_fixture()?)?;
    let draft = record.draft();
    // Explicit fields deliberately do not call the production projection helper.
    let projection = serde_json::json!({
        "schema_version": draft.schema_version, "binding": draft.binding,
        "scope": draft.scope, "package": draft.package, "phase": draft.phase,
        "source_kind": draft.source_kind, "effect_id": draft.effect_id,
        "anchor": draft.anchor, "start_byte": draft.start_byte, "end_byte": draft.end_byte,
        "reason": draft.reason, "disposition": draft.disposition, "effect": { "outcome": draft.effect.outcome, "value": draft.effect.value, "origins": draft.effect.origins },
        "possible_impacts": draft.possible_impacts, "applied_impacts": draft.applied_impacts,
        "state_before": draft.state_before, "possible_states": draft.possible_states,
        "origins": draft.origins
    });
    let expected =
        ContentDigest::of_bytes(&serde_json::to_vec(&("compile_effect_boundary.v1", projection))?);
    require(record.digest()? == expected, "boundary domain/projection contract changed")
}

fn state_with_boundary(
    record: &BoundaryRecord,
) -> Result<CompileEnvironmentState, Box<dyn std::error::Error>> {
    let mut state = fixture()?;
    state.binding = record.draft().binding.clone();
    state.scope = record.draft().scope.clone();
    state.boundaries = vec![record.reference()?];
    Ok(CompileEnvironmentState::admit(state)?)
}

#[test]
fn boundary_metadata_never_changes_record_or_referencing_state_identity() -> TestResult {
    let first = BoundaryRecord::admit(boundary_fixture()?)?;
    let mut other = first.draft().clone();
    other.metadata.explanation = "Localized wording, host C:\\other, workflow 456".into();
    other.metadata.owner = Some("other-owner".into());
    other.metadata.replacement = Some("issue-999".into());
    other.metadata.trigger_excerpt = Some("Different presentation excerpt".into());
    let other = BoundaryRecord::admit(other)?;
    require(first.digest()? == other.digest()?, "presentation metadata entered semantic digest")?;
    require(
        state_with_boundary(&first)?.digest()? == state_with_boundary(&other)?.digest()?,
        "metadata leaked through state reference",
    )?;
    let mut semantic = first.draft().clone();
    semantic.effect_id = "directive-2".into();
    let semantic = BoundaryRecord::admit(semantic)?;
    require(first.digest()? != semantic.digest()?, "effect identity omitted")?;
    require(
        state_with_boundary(&first)?.digest()? != state_with_boundary(&semantic)?.digest()?,
        "state reference omitted record identity",
    )
}

#[test]
fn exact_no_effect_empty_application_is_not_unavailable() -> TestResult {
    let mut draft = boundary_fixture()?;
    draft.disposition = serde_json::from_str("\"exact_no_effect\"")?;
    draft.applied_impacts.clear();
    let no_effect = BoundaryRecord::admit(draft.clone())?;
    draft.disposition = serde_json::from_str("\"unavailable\"")?;
    draft.effect = unavailable();
    let unavailable_record = BoundaryRecord::admit(draft.clone())?;
    require(
        no_effect.digest()? != unavailable_record.digest()?,
        "exact empty collapsed into unavailable",
    )?;
    draft.disposition = serde_json::from_str("\"exact_no_effect\"")?;
    require(BoundaryRecord::admit(draft).is_err(), "missing effect promoted to exact no-effect")
}

#[test]
fn boundary_local_strict_never_implies_encoding_or_unbounded_impact() -> TestResult {
    let record = BoundaryRecord::admit(boundary_fixture()?)?;
    let reference = record.reference()?;
    require(
        reference.affects == vec![FactClass::Strict],
        "strict category spread to unrelated classes",
    )?;
    let mut draft = record.draft().clone();
    draft
        .possible_impacts
        .push(ImpactSelector::Unbounded { unknown_extent: UnknownExtentReason::DynamicTarget });
    require(BoundaryRecord::admit(draft).is_err(), "unbounded and narrower declarations coexist")
}

#[test]
fn boundary_metadata_limits_are_utf8_bytes_with_exact_boundaries() -> TestResult {
    let mut draft = boundary_fixture()?;
    draft.metadata.explanation = "x".repeat(4096);
    draft.metadata.trigger_excerpt = Some("x".repeat(4096));
    draft.metadata.owner = Some("x".repeat(256));
    BoundaryRecord::admit(draft.clone())?;
    draft.metadata.explanation.push('x');
    require(
        matches!(BoundaryRecord::admit(draft), Err(SchemaError::Limited(_))),
        "explanation limit silently truncates",
    )?;
    let mut draft = boundary_fixture()?;
    draft.metadata.owner = Some("é".repeat(129));
    require(
        matches!(BoundaryRecord::admit(draft), Err(SchemaError::Limited(_))),
        "owner limit counts characters rather than UTF8 bytes",
    )
}

#[test]
fn boundary_all_dispositions_roundtrip_as_full_records() -> TestResult {
    use BoundaryDisposition::*;
    let mut identities = std::collections::BTreeSet::new();
    for disposition in [
        ExactApplied,
        ExactNoEffect,
        Conditional,
        Limited,
        Unsupported,
        Unavailable,
        Stale,
        ResourceOrInstrumentFailure,
    ] {
        let mut draft = boundary_fixture()?;
        draft.disposition = disposition;
        if disposition == ExactNoEffect {
            draft.applied_impacts.clear();
        }
        if disposition.outcome() != Outcome::Exact {
            draft.effect = Facet {
                outcome: disposition.outcome(),
                value: None,
                reason: Some("explicit qualified outcome".into()),
                origins: vec![],
            };
            draft.applied_impacts.clear();
        }
        let record = BoundaryRecord::admit(draft)?;
        let roundtrip = BoundaryRecord::read(record.to_json()?.as_slice())?;
        require(roundtrip == record, "full disposition roundtrip changed")?;
        require(identities.insert(record.digest()?), "full dispositions share identity")?;
        require(
            record.reference()?.authority.outcome == disposition.outcome(),
            "reference upgraded qualification",
        )?;
    }
    Ok(())
}

#[test]
fn pending_catalog_and_scc_references_never_promote_to_exact() -> TestResult {
    for selector in [
        ImpactSelector::CategorySubtree {
            class: FactClass::Warnings,
            category: "io".into(),
            pending: PendingAuthority::CategoryCatalog,
        },
        ImpactSelector::ModuleClosure {
            module: "Example".into(),
            scc: true,
            pending: PendingAuthority::ModuleGraph,
            classes: vec![FactClass::ModuleGraph],
        },
    ] {
        let mut draft = boundary_fixture()?;
        draft.possible_impacts = vec![selector];
        draft.applied_impacts.clear();
        for disposition in [BoundaryDisposition::ExactNoEffect, BoundaryDisposition::ExactApplied] {
            draft.disposition = disposition;
            if disposition == BoundaryDisposition::ExactApplied {
                draft.applied_impacts = draft.possible_impacts.clone();
            }
            require(
                BoundaryRecord::admit(draft.clone()).is_err(),
                "pending identity supports exact boundary",
            )?;
        }
        draft.disposition = BoundaryDisposition::Conditional;
        draft.applied_impacts.clear();
        let record = BoundaryRecord::admit(draft)?;
        let reference = record.reference()?;
        require(
            reference.pending_authority && reference.authority.outcome == Outcome::Conditional,
            "pending qualification lost",
        )?;
        state_with_boundary(&record)?;
        let mut state = fixture()?;
        state.binding = record.draft().binding.clone();
        state.scope = record.draft().scope.clone();
        let mut upgraded = reference;
        upgraded.disposition = BoundaryDisposition::ExactNoEffect;
        upgraded.authority.outcome = Outcome::Exact;
        upgraded.authority.reason = None;
        state.boundaries = vec![upgraded];
        require(
            CompileEnvironmentState::admit(state.clone()).is_err(),
            "direct pending reference upgraded",
        )?;
        require(
            CompileEnvironmentState::read(serde_json::to_vec(&state)?.as_slice()).is_err(),
            "wire pending reference upgraded",
        )?;
    }
    Ok(())
}

#[test]
fn boundary_reference_checks_full_source_binding_and_declared_qualification() -> TestResult {
    let record = BoundaryRecord::admit(boundary_fixture()?)?;
    for mode in 0..6 {
        let mut state = state_with_boundary(&record)?.draft().clone();
        let reference = state.boundaries.first_mut().ok_or("reference missing")?;
        match mode {
            0 => {
                reference.binding.source.content_revision = Some(ContentRevision::new(
                    reference.binding.source.logical_source_id.clone(),
                    ContentDigest::of_bytes(b"different bytes"),
                ))
            }
            1 => {
                reference.binding.source.workspace_root_id =
                    WorkspaceRootId::from_project_and_root_key(
                        &reference.binding.source.project_id,
                        "another-root",
                    )
            }
            2 => {
                reference.binding.source.generation =
                    SourceGeneration::Known("another-generation".into())
            }
            3 => reference.binding.compiler_generation = "another-compiler".into(),
            4 => reference.authority.outcome = Outcome::Unavailable,
            _ => reference.id = ContentDigest::of_bytes(b"another record"),
        }
        require(
            CompileEnvironmentState::admit(state.clone()).is_err(),
            "reference mismatch accepted directly",
        )?;
        require(
            CompileEnvironmentState::read(serde_json::to_vec(&state)?.as_slice()).is_err(),
            "reference mismatch accepted on wire",
        )?;
    }
    Ok(())
}

#[test]
fn boundary_currentness_is_independent_of_nonexact_disposition() -> TestResult {
    let mut draft = boundary_fixture()?;
    draft.disposition = BoundaryDisposition::Stale;
    draft.applied_impacts.clear();
    draft.effect = unavailable();
    for generation in [
        SourceGeneration::Unknown,
        SourceGeneration::Known(" ".into()),
        SourceGeneration::Known("other".into()),
    ] {
        let mut stale = draft.clone();
        stale.binding.source.generation = generation;
        require(
            BoundaryRecord::admit(stale.clone()).is_err(),
            "nonexact bypasses declared generation correspondence",
        )?;
        require(
            BoundaryRecord::read(serde_json::to_vec(&stale)?.as_slice()).is_err(),
            "wire generation correspondence bypass",
        )?;
    }
    BoundaryRecord::admit(draft)?;
    Ok(())
}

#[test]
fn boundary_closed_wire_rejects_malformed_range_schema_reason_and_nested_fields() -> TestResult {
    let draft = boundary_fixture()?;
    for (pointer, replacement) in [
        ("/schema_version", serde_json::json!(2)),
        ("/reason", serde_json::json!("future_reason")),
        ("/source_kind", serde_json::json!("future_source")),
        ("/start_byte", serde_json::json!(99)),
        ("/scope/parent_fingerprint", serde_json::json!("file-cannot-have-parent")),
    ] {
        let mut value = serde_json::to_value(&draft)?;
        *value.pointer_mut(pointer).ok_or("fixture path missing")? = replacement;
        require(
            BoundaryRecord::read(serde_json::to_vec(&value)?.as_slice()).is_err(),
            "malformed boundary accepted",
        )?;
    }
    for pointer in [
        "/binding/source",
        "/binding/semantic",
        "/scope",
        "/effect/value",
        "/origins/0/anchor",
        "/metadata",
    ] {
        let mut value = serde_json::to_value(&draft)?;
        value
            .pointer_mut(pointer)
            .and_then(serde_json::Value::as_object_mut)
            .ok_or("object path missing")?
            .insert("future_field".into(), serde_json::json!(true));
        require(
            BoundaryRecord::read(serde_json::to_vec(&value)?.as_slice()).is_err(),
            "unknown nested boundary field accepted",
        )?;
    }
    let bytes = serde_json::to_vec(&draft)?;
    let mut trailing = bytes.clone();
    trailing.extend_from_slice(b"{}");
    require(BoundaryRecord::read(trailing.as_slice()).is_err(), "trailing document accepted")?;
    let text = String::from_utf8(bytes)?;
    let duplicate =
        text.replacen("\"schema_version\":1", "\"schema_version\":1,\"schema_version\":1", 1);
    require(BoundaryRecord::read(duplicate.as_bytes()).is_err(), "duplicate field accepted")
}

#[test]
fn boundary_selector_sets_are_canonical_but_source_bundle_order_is_retained() -> TestResult {
    let mut draft = boundary_fixture()?;
    draft
        .possible_impacts
        .push(ImpactSelector::NamedFact { class: FactClass::Features, name: "signatures".into() });
    draft.possible_states = vec![ContentDigest::of_bytes(b"one"), ContentDigest::of_bytes(b"two")];
    let first = BoundaryRecord::admit(draft.clone())?;
    draft.possible_impacts.reverse();
    draft.possible_states.reverse();
    require(
        first.digest()? == BoundaryRecord::admit(draft.clone())?.digest()?,
        "set insertion order changed identity",
    )?;
    draft.effect_id = "second".into();
    let second = BoundaryRecord::admit(draft)?;
    let forward = BoundaryBundle::admit(vec![first.draft().clone(), second.draft().clone()])?;
    let reverse = BoundaryBundle::admit(vec![second.draft().clone(), first.draft().clone()])?;
    require(forward.to_json()? != reverse.to_json()?, "source collection order erased")?;
    require(
        BoundaryBundle::read(forward.to_json()?.as_slice())? == forward,
        "bundle roundtrip changed",
    )
}

fn sized_boundary_payload(size: usize) -> Result<BoundaryDraft, Box<dyn std::error::Error>> {
    let mut draft = boundary_fixture()?;
    draft.metadata.limitations = vec![String::new(); MAX_BOUNDARY_ITEMS];
    let initial = serde_json::to_vec(&draft)?.len();
    let mut remaining = size.checked_sub(initial).ok_or("fixture above target")?;
    for text in &mut draft.metadata.limitations {
        let added = remaining.min(MAX_BOUNDARY_TEXT_BYTES);
        *text = "x".repeat(added);
        remaining -= added;
    }
    require(remaining == 0, "boundary fixture cannot reach payload size")?;
    require(serde_json::to_vec(&draft)?.len() == size, "boundary payload size differs")?;
    Ok(draft)
}

#[test]
fn boundary_actual_payload_limit_is_distinct_from_digest_framing_and_refusal_outcome() -> TestResult
{
    let expected = BoundaryRecord::admit(boundary_fixture()?)?.digest()?;
    for size in [MAX_SNAPSHOT_BYTES - 1, MAX_SNAPSHOT_BYTES, MAX_SNAPSHOT_BYTES + 1] {
        let draft = sized_boundary_payload(size)?;
        let bytes = serde_json::to_vec(&draft)?;
        if size > MAX_SNAPSHOT_BYTES {
            require(
                matches!(BoundaryRecord::admit(draft), Err(SchemaError::Limited(_))),
                "overlimit creates admitted record",
            )?;
            require(
                matches!(BoundaryRecord::read(bytes.as_slice()), Err(SchemaError::Limited(_))),
                "overlimit wire creates admitted record",
            )?;
        } else {
            let record = BoundaryRecord::admit(draft)?;
            require(record.to_json()?.len() == size, "boundary payload output differs")?;
            require(record.digest()? == expected, "metadata/framing changes semantic identity")?;
            BoundaryRecord::read(bytes.as_slice())?;
        }
    }
    let mut resource = boundary_fixture()?;
    resource.disposition = BoundaryDisposition::ResourceOrInstrumentFailure;
    resource.effect = unavailable();
    resource.applied_impacts.clear();
    let resource = BoundaryRecord::admit(resource)?;
    require(
        resource.reference()?.authority.outcome == Outcome::ResourceInstrument,
        "resource refusal relabeled limited",
    )
}

#[test]
fn boundary_collection_and_item_limits_refuse_without_truncation() -> TestResult {
    require(
        MAX_BOUNDARIES == 4096 && MAX_BOUNDARY_ITEMS == 256 && MAX_BOUNDARY_TEXT_BYTES == 4096,
        "production bounds changed",
    )?;
    let mut draft = boundary_fixture()?;
    draft.disposition = BoundaryDisposition::ExactNoEffect;
    draft.applied_impacts.clear();
    draft.possible_impacts = (0..MAX_BOUNDARY_ITEMS)
        .map(|i| ImpactSelector::NamedFact { class: FactClass::Features, name: format!("f{i}") })
        .collect();
    draft.possible_states = (0..MAX_BOUNDARY_ITEMS)
        .map(|i| ContentDigest::of_bytes(format!("state{i}").as_bytes()))
        .collect();
    draft.origins = (0..MAX_BOUNDARY_ITEMS)
        .map(|i| {
            let mut item = origin()?;
            if i != 0 {
                item.id = format!("origin{i}");
            }
            Ok(item)
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    BoundaryRecord::admit(draft.clone())?;
    for dimension in 0..3 {
        let mut excess = draft.clone();
        match dimension {
            0 => excess.possible_impacts.push(ImpactSelector::NamedFact {
                class: FactClass::Features,
                name: "extra".into(),
            }),
            1 => excess.possible_states.push(ContentDigest::of_bytes(b"extra")),
            _ => {
                let mut item = origin()?;
                item.id = "extra".into();
                excess.origins.push(item);
            }
        }
        require(
            matches!(BoundaryRecord::admit(excess), Err(SchemaError::Limited(_))),
            "item limit+1 admitted",
        )?;
    }
    let base = boundary_fixture()?;
    let drafts = (0..MAX_BOUNDARIES)
        .map(|i| {
            let mut draft = base.clone();
            draft.effect_id = format!("effect{i}");
            draft
        })
        .collect::<Vec<_>>();
    let bundle = BoundaryBundle::admit(drafts.clone())?;
    require(bundle.records().len() == MAX_BOUNDARIES, "bundle silently truncated")?;
    let mut excess = drafts;
    excess.push(base);
    require(
        matches!(BoundaryBundle::admit(excess), Err(SchemaError::Limited(_))),
        "bundle count+1 admitted",
    )
}

#[test]
fn boundary_all_local_shapes_and_empty_finite_sets_are_discriminated() -> TestResult {
    let base = boundary_fixture()?;
    let shapes = vec![
        ImpactSelector::StrictCategory(StrictCategory::Subs),
        ImpactSelector::StrictCategory(StrictCategory::Refs),
        ImpactSelector::NamedFact { class: FactClass::Prototypes, name: "f".into() },
        ImpactSelector::SourceSuffix { class: FactClass::Encoding, start_byte: 12 },
        ImpactSelector::LexicalScope {
            class: FactClass::LexicalBindings,
            scope: Box::new(base.scope.clone()),
        },
        ImpactSelector::Package {
            class: FactClass::PackageBindings,
            package: SemanticSourceOrderIdentity::new(0, "package-context")?,
        },
        ImpactSelector::DeclaredFacts(vec![
            FactClass::ImportVisibility,
            FactClass::ExportVisibility,
            FactClass::Constants,
            FactClass::Inheritance,
            FactClass::Methods,
            FactClass::IncludeRoots,
            FactClass::GeneratedMembers,
            FactClass::Types,
            FactClass::CallResolution,
            FactClass::ProviderExactness,
            FactClass::EditAuthorization,
        ]),
    ];
    for shape in shapes {
        let mut draft = base.clone();
        draft.possible_impacts = vec![shape.clone()];
        draft.applied_impacts = vec![shape];
        let record = BoundaryRecord::admit(draft)?;
        require(
            BoundaryRecord::read(record.to_json()?.as_slice())? == record,
            "local shape roundtrip failed",
        )?;
    }
    for shape in [
        ImpactSelector::DeclaredFacts(vec![]),
        ImpactSelector::ModuleClosure {
            module: "Example".into(),
            scc: false,
            pending: PendingAuthority::ModuleGraph,
            classes: vec![],
        },
    ] {
        let mut draft = base.clone();
        draft.disposition = BoundaryDisposition::Conditional;
        draft.possible_impacts = vec![shape];
        draft.applied_impacts.clear();
        require(
            BoundaryRecord::admit(draft).is_err(),
            "empty inner fact set passes nonempty outer selector guard",
        )?;
    }
    let mut draft = base.clone();
    draft.disposition = BoundaryDisposition::Conditional;
    draft.applied_impacts.clear();
    draft.possible_impacts =
        vec![ImpactSelector::Unbounded { unknown_extent: UnknownExtentReason::UnmodeledExecution }];
    let broad = BoundaryRecord::admit(draft)?;
    require(broad.reference()?.affects == vec![FactClass::AllFacts], "unbounded extent lost")?;
    let mut missing_reason = serde_json::to_value(broad.draft())?;
    missing_reason
        .pointer_mut("/possible_impacts/0/unbounded")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("unbounded fixture missing")?
        .remove("unknown_extent");
    require(
        BoundaryRecord::read(serde_json::to_vec(&missing_reason)?.as_slice()).is_err(),
        "unbounded extent without reason accepted",
    )
}

#[test]
fn source_profile_parser_and_compiler_changes_move_record_and_state_subjects() -> TestResult {
    let first = BoundaryRecord::admit(boundary_fixture()?)?;
    for dimension in 0..6 {
        let mut draft = first.draft().clone();
        let old = draft.binding.semantic.clone();
        let profile = if dimension == 3 {
            SemanticSemanticProfileIdentity::new("different", "different-profile-digest")?
        } else {
            old.profile().clone()
        };
        let generation = if dimension == 5 { "next-generation" } else { old.source_generation() };
        draft.binding.semantic = SemanticSubjectGeneration::new(
            old.logical_source_id(),
            generation,
            if dimension == 2 { "next-parser" } else { old.parser_snapshot_id() },
            old.parser_configuration_id(),
            profile,
        )?;
        draft.binding.source.generation = SourceGeneration::Known(generation.into());
        match dimension {
            0 => {
                draft.binding.source.content_revision = Some(ContentRevision::new(
                    draft.binding.source.logical_source_id.clone(),
                    ContentDigest::of_bytes(b"next-source"),
                ))
            }
            1 => {
                draft.binding.source.workspace_root_id = WorkspaceRootId::from_project_and_root_key(
                    &draft.binding.source.project_id,
                    "next-root",
                )
            }
            4 => draft.binding.compiler_generation = "next-compiler".into(),
            _ => {}
        }
        draft.scope = SemanticScopeIdentity::new(
            draft.binding.semantic.clone(),
            SemanticScopeKind::File,
            None,
            None,
            origin()?.anchor,
            None,
            SemanticScopeRecovery::Exact,
        )?;
        draft.effect = exact(port(PortRole::DirectiveEffect, &draft.binding))?;
        let next = BoundaryRecord::admit(draft)?;
        require(
            next.digest()? != first.digest()?,
            "subject dimension omitted from boundary identity",
        )?;
        require(
            state_with_boundary(&next)?.digest()? != state_with_boundary(&first)?.digest()?,
            "subject dimension omitted from state reference identity",
        )?;
    }
    Ok(())
}

#[test]
fn boundary_nested_selector_closed_wire_and_bundle_reader_budgets() -> TestResult {
    let mut draft = boundary_fixture()?;
    draft.possible_impacts = vec![ImpactSelector::LexicalScope {
        class: FactClass::LexicalBindings,
        scope: Box::new(draft.scope.clone()),
    }];
    draft.applied_impacts = draft.possible_impacts.clone();
    let mut value = serde_json::to_value(&draft)?;
    value
        .pointer_mut("/possible_impacts/0/lexical_scope/scope/subject")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("selector subject missing")?
        .insert("future_field".into(), serde_json::json!(1));
    require(
        BoundaryRecord::read(serde_json::to_vec(&value)?.as_slice()).is_err(),
        "external selector hides nested unknown field",
    )?;
    let bundle = BoundaryBundle::admit(vec![draft])?;
    let mut bytes = bundle.to_json()?;
    bytes.resize(MAX_BUNDLE_BYTES, b' ');
    require(
        BoundaryBundle::read(bytes.as_slice())? == bundle,
        "exact bundle input budget rejected",
    )?;
    bytes.push(b' ');
    require(
        matches!(BoundaryBundle::read(bytes.as_slice()), Err(SchemaError::Limited(_))),
        "bundle input cap+1 accepted",
    )?;
    for bundle in [false, true] {
        let limit = if bundle { MAX_BUNDLE_BYTES } else { MAX_SNAPSHOT_BYTES };
        let mut spaces = Spaces { remaining: limit + 1024, consumed: 0 };
        let result = if bundle {
            BoundaryBundle::read(&mut spaces).map(|_| ())
        } else {
            BoundaryRecord::read(&mut spaces).map(|_| ())
        };
        require(
            matches!(result, Err(SchemaError::Limited(_))) && spaces.consumed == limit + 1,
            "boundary reader overconsumed or misclassified",
        )?;
    }
    let deep = format!("{}0{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
    require(
        matches!(BoundaryRecord::read(deep.as_bytes()), Err(SchemaError::Limited("JSON depth"))),
        "depth cap missed",
    )
}

#[test]
fn review_regression_unbounded_reference_cannot_promote_to_exact_in_direct_or_wire_state()
-> TestResult {
    let mut draft = boundary_fixture()?;
    draft.disposition = BoundaryDisposition::Conditional;
    draft.possible_impacts =
        vec![ImpactSelector::Unbounded { unknown_extent: UnknownExtentReason::UnmodeledExecution }];
    draft.applied_impacts.clear();
    let record = BoundaryRecord::admit(draft)?;
    let mut state = state_with_boundary(&record)?.draft().clone();
    let reference = state.boundaries.first_mut().ok_or("reference missing")?;
    require(
        !reference.pending_authority && reference.affects == vec![FactClass::AllFacts],
        "fixture must exercise unbounded rather than pending catalog",
    )?;
    reference.disposition = BoundaryDisposition::ExactApplied;
    reference.authority.outcome = Outcome::Exact;
    reference.authority.reason = None;
    require(
        CompileEnvironmentState::admit(state.clone()).is_err(),
        "unbounded reference promoted directly",
    )?;
    require(
        CompileEnvironmentState::read(serde_json::to_vec(&state)?.as_slice()).is_err(),
        "unbounded reference promoted on wire",
    )
}

#[test]
fn review_regression_nonexact_effect_explanation_is_not_record_or_reference_identity() -> TestResult
{
    let mut draft = boundary_fixture()?;
    draft.disposition = BoundaryDisposition::Conditional;
    draft.effect.outcome = Outcome::Conditional;
    draft.effect.reason = Some("First localized explanation".into());
    draft.applied_impacts.clear();
    let first = BoundaryRecord::admit(draft.clone())?;
    draft.effect.reason = Some("Second localized explanation".into());
    let second = BoundaryRecord::admit(draft)?;
    require(first.to_json()? != second.to_json()?, "presentation change disappeared from wire")?;
    require(first.digest()? == second.digest()?, "effect explanation enters boundary identity")?;
    require(
        state_with_boundary(&first)?.digest()? == state_with_boundary(&second)?.digest()?,
        "effect explanation enters state reference identity",
    )
}

#[test]
fn detailed_reason_fixtures_keep_representative_impacts_and_unimport_source_context() -> TestResult
{
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/compile_boundary_vocabulary.json"))?;
    let rows = fixture
        .get("reasons")
        .and_then(serde_json::Value::as_array)
        .ok_or("reason rows missing")?;
    for row in rows {
        let token =
            row.get("token").and_then(serde_json::Value::as_str).ok_or("reason token missing")?;
        let class: FactClass =
            serde_json::from_value(row.get("fact").ok_or("representative fact missing")?.clone())?;
        let mut draft = boundary_fixture()?;
        draft.reason = serde_json::from_value(serde_json::Value::String(token.into()))?;
        draft.disposition = if token.starts_with("stale_") {
            BoundaryDisposition::Stale
        } else if matches!(token, "resource_exhaustion" | "instrument_failure") {
            BoundaryDisposition::ResourceOrInstrumentFailure
        } else {
            BoundaryDisposition::Conditional
        };
        draft.source_kind = if token.ends_with("unimport") {
            perl_semantic_facts::CompileEffectSourceKind::NoDirective
        } else {
            perl_semantic_facts::CompileEffectSourceKind::UseDirective
        };
        draft.possible_impacts =
            vec![ImpactSelector::NamedFact { class, name: "representative-fixture".into() }];
        draft.applied_impacts.clear();
        let record = BoundaryRecord::admit(draft)?;
        require(record.reference()?.affects == vec![class], "declared fixture class lost")?;
        require(
            BoundaryRecord::read(record.to_json()?.as_slice())? == record,
            "reason/impact fixture does not roundtrip",
        )?;
    }
    // These are independent declared specimens, not a reason-to-single-fact rule.
    Ok(())
}

#[test]
fn review_regression_stale_and_resource_reason_families_cannot_be_reclassified_as_unsupported_or_exact()
-> TestResult {
    for reason in [
        BoundaryReason::StaleSource,
        BoundaryReason::StaleProfile,
        BoundaryReason::StaleDependency,
        BoundaryReason::ResourceExhaustion,
        BoundaryReason::InstrumentFailure,
    ] {
        for disposition in [
            BoundaryDisposition::Unsupported,
            BoundaryDisposition::Limited,
            BoundaryDisposition::ExactApplied,
        ] {
            let mut draft = boundary_fixture()?;
            draft.reason = reason;
            draft.disposition = disposition;
            require(
                BoundaryRecord::admit(draft.clone()).is_err(),
                "stale/resource reason relabeled as a different failure class",
            )?;
            require(
                BoundaryRecord::read(serde_json::to_vec(&draft)?.as_slice()).is_err(),
                "wire relabeled stale/resource class",
            )?;
        }
    }
    Ok(())
}

#[test]
fn boundary_identifier_trigger_and_limitation_limits_are_exact() -> TestResult {
    let mut draft = boundary_fixture()?;
    draft.effect_id = "x".repeat(MAX_NAME_BYTES);
    draft.metadata.trigger_excerpt = Some("x".repeat(MAX_BOUNDARY_TEXT_BYTES));
    draft.metadata.limitations = vec!["x".repeat(MAX_BOUNDARY_TEXT_BYTES)];
    BoundaryRecord::admit(draft.clone())?;
    for dimension in 0..3 {
        let mut excess = draft.clone();
        match dimension {
            0 => excess.effect_id.push('x'),
            1 => excess.metadata.trigger_excerpt.as_mut().ok_or("trigger missing")?.push('x'),
            _ => excess.metadata.limitations.first_mut().ok_or("limitation missing")?.push('x'),
        }
        require(
            matches!(BoundaryRecord::admit(excess), Err(SchemaError::Limited(_))),
            "field limit+1 admitted",
        )?;
    }
    let mut excess = boundary_fixture()?;
    excess.metadata.limitations = vec![String::new(); MAX_BOUNDARY_ITEMS];
    BoundaryRecord::admit(excess.clone())?;
    excess.metadata.limitations.push(String::new());
    require(
        matches!(BoundaryRecord::admit(excess), Err(SchemaError::Limited(_))),
        "limitation count+1 admitted",
    )
}

#[test]
fn boundary_bundle_refuses_duplicate_instances_and_mixed_subjects() -> TestResult {
    let first = boundary_fixture()?;
    require(
        BoundaryBundle::admit(vec![first.clone(), first.clone()]).is_err(),
        "duplicate instance admitted",
    )?;
    let mut metadata_only = first.clone();
    metadata_only.metadata.explanation = "Different prose same instance".into();
    require(
        BoundaryBundle::admit(vec![first.clone(), metadata_only]).is_err(),
        "metadata created a second semantic instance",
    )?;
    let mut other = first.clone();
    other.effect_id = "other".into();
    other.binding.source.content_revision = Some(ContentRevision::new(
        other.binding.source.logical_source_id.clone(),
        ContentDigest::of_bytes(b"other revision"),
    ));
    BoundaryRecord::admit(other.clone())?;
    require(
        BoundaryBundle::admit(vec![first.clone(), other.clone()]).is_err(),
        "mixed subject bundle admitted",
    )?;
    require(
        BoundaryBundle::read(serde_json::to_vec(&vec![first, other])?.as_slice()).is_err(),
        "mixed subject bundle admitted on wire",
    )
}

#[test]
fn review_regression_boundary_reference_requires_canonical_contract() -> TestResult {
    let record = BoundaryRecord::admit(boundary_fixture()?)?;
    let mut state = state_with_boundary(&record)?.draft().clone();
    state
        .boundaries
        .first_mut()
        .ok_or("reference missing")?
        .authority
        .value
        .as_mut()
        .ok_or("port missing")?
        .contract = "other.v1".into();
    require(
        CompileEnvironmentState::admit(state.clone()).is_err(),
        "noncanonical boundary contract accepted directly",
    )?;
    require(
        CompileEnvironmentState::read(serde_json::to_vec(&state)?.as_slice()).is_err(),
        "noncanonical boundary contract accepted on wire",
    )
}
