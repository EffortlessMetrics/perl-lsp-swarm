use perl_pragma::compile_environment::{warnings::*, *};
use perl_semantic_facts::semantic_identity::*;
use perl_semantic_facts::{Provenance, SemanticProvenance};
use perl_source_identity::*;

type R = Result<(), Box<dyn std::error::Error>>;

#[test]
fn review_regression_prose_is_not_semantic_policy_identity() -> R {
    let mut draft = fixture()?;
    draft.coverage.outcome = Outcome::Limited;
    draft.coverage.reason = Some("first explanation".into());
    let first = WarningPolicy::admit(draft.clone())?;
    draft.coverage.reason = Some("same facts with edited explanation".into());
    let second = WarningPolicy::admit(draft.clone())?;
    require(first.to_json()? != second.to_json()?, "wire did not retain changed explanation")?;
    require(
        first.digest()? == second.digest()?,
        "explanatory prose changed semantic policy identity",
    )?;
    draft.coverage.outcome = Outcome::Unsupported;
    require(
        WarningPolicy::admit(draft)?.digest()? != first.digest()?,
        "typed qualification excluded from semantic identity",
    )
}

#[test]
fn stale_nonwinning_override_never_becomes_exact_and_keeps_evidence() -> R {
    let mut draft = fixture()?;
    draft.coverage.outcome = Outcome::Limited;
    draft.coverage.reason = Some("partial input".into());
    let mut stale = item(1, "all", WarningDisposition::Fatal)?;
    stale.disposition.outcome = Outcome::Stale;
    stale.disposition.reason = Some("older supplied source".into());
    let mut provenance = origin()?;
    provenance.id = "stale-effective-origin".into();
    stale.disposition.origins = vec![provenance];
    draft.overrides = vec![stale, item(2, "numeric", WarningDisposition::Enabled)?];
    let answer = query(&WarningPolicy::admit(draft)?, "numeric")?;
    require(
        answer.disposition() == Some(WarningDisposition::Enabled) && answer.exact().is_err(),
        "later exact winning value promoted stale policy",
    )?;
    require(
        answer.evidence().iter().any(|e| {
            e.qualification.outcome == Outcome::Stale
                && e.origins.iter().any(|o| o.id == "stale-effective-origin")
        }),
        "stale source evidence lost",
    )
}

#[test]
fn review_regression_explanation_retains_nonwinning_and_boundary_evidence() -> R {
    let mut draft = fixture()?;
    let mut baseline = origin()?;
    baseline.id = "baseline-evidence-8655".into();
    draft.baseline.origins = vec![baseline];
    let mut coverage = origin()?;
    coverage.id = "coverage-evidence-8655".into();
    draft.coverage.origins = vec![coverage];
    draft.coverage.outcome = Outcome::Limited;
    draft.coverage.reason = Some("boundary present".into());
    let mut parent = item(1, "all", WarningDisposition::Fatal)?;
    let mut parent_origin = origin()?;
    parent_origin.id = "parent-evidence-8655".into();
    parent.disposition.origins = vec![parent_origin];
    let mut child = item(2, "numeric", WarningDisposition::Enabled)?;
    let mut child_origin = origin()?;
    child_origin.id = "winner-evidence-8655".into();
    child.disposition.origins = vec![child_origin];
    draft.overrides = vec![parent, child];
    let id = ContentDigest::of_bytes(b"boundary-8655");
    let mut boundary_origin = origin()?;
    boundary_origin.id = "boundary-evidence-8655".into();
    draft.boundaries = vec![BoundaryReference {
        id: id.clone(),
        binding: draft.binding.clone(),
        disposition: BoundaryDisposition::Limited,
        pending_authority: true,
        affects: vec![FactClass::Warnings],
        authority: Facet {
            outcome: Outcome::Limited,
            value: Some(AuthorityPort {
                role: PortRole::Boundary,
                schema_version: 1,
                contract: "compile_effect_boundary.v1".into(),
                subject: draft.binding.semantic.clone(),
                compiler_generation: draft.binding.compiler_generation.clone(),
                payload: id,
            }),
            reason: Some("pending category authority".into()),
            origins: vec![boundary_origin],
        },
    }];
    let policy = WarningPolicy::admit(draft)?;
    let answer = query(&policy, "numeric")?;
    for id in [
        "baseline-evidence-8655",
        "coverage-evidence-8655",
        "parent-evidence-8655",
        "winner-evidence-8655",
        "boundary-evidence-8655",
    ] {
        require(
            answer.evidence().iter().any(|e| e.origins.iter().any(|o| o.id == id)),
            "query explanation dropped contributing source evidence",
        )?;
    }
    require(answer.evidence().iter().any(|e|matches!(&e.source,WarningEvidenceSource::Boundary(reference) if reference.id==ContentDigest::of_bytes(b"boundary-8655"))), "boundary semantic reference lost")?;
    require(
        answer.evidence().iter().any(
            |e| matches!(&e.source,WarningEvidenceSource::Override{ordinal:1,name} if name=="all"),
        ),
        "nonwinning ordinal lost",
    )?;
    Ok(())
}
fn require(ok: bool, message: &str) -> R {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn origin() -> Result<Origin, Box<dyn std::error::Error>> {
    Ok(Origin {
        id: "effect".into(),
        provenance: SemanticProvenance::Known(Provenance::ExactAst),
        anchor: SemanticSourceAnchor::new(SemanticAnchorRole::ContextMarker, "source-anchor", 0)?,
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
fn fixture() -> Result<WarningPolicyDraft, Box<dyn std::error::Error>> {
    let project = ProjectId::from_canonical_name("warnings");
    let root = WorkspaceRootId::from_project_and_root_key(&project, "root");
    let file = LogicalSourceId::from_root_and_path(&root, "input.pl");
    let source = SourceIdentityEnvelope::for_workspace_file(
        project,
        root,
        file.clone(),
        Some(ContentRevision::new(file, ContentDigest::of_bytes(b"source"))),
        SourceGeneration::Known("g1".into()),
    );
    let semantic = SemanticSubjectGeneration::new(
        "document-instance",
        "g1",
        "parser",
        "config",
        SemanticSemanticProfileIdentity::new("opaque-not-a-perl-version", "profile-digest")?,
    )?;
    let scope = SemanticScopeIdentity::new(
        semantic.clone(),
        SemanticScopeKind::File,
        None,
        None,
        origin()?.anchor,
        None,
        SemanticScopeRecovery::Exact,
    )?;
    Ok(WarningPolicyDraft {
        schema_version: 1,
        catalog: WarningCatalog::for_profile("5.44.0")?.identity().clone(),
        binding: Binding { source, semantic, compiler_generation: "compiler".into() },
        scope,
        baseline: exact(WarningBaseline::Uniform(WarningDisposition::Disabled))?,
        overrides: vec![],
        coverage: exact(WarningCoverage::Complete)?,
        boundaries: vec![],
    })
}
fn item(
    ordinal: u32,
    name: &str,
    value: WarningDisposition,
) -> Result<WarningOverride, Box<dyn std::error::Error>> {
    Ok(WarningOverride { ordinal, name: name.into(), disposition: exact(value)? })
}
fn query(policy: &WarningPolicy, name: &str) -> Result<WarningAnswer, Box<dyn std::error::Error>> {
    Ok(policy.warning_disposition_by_name(
        name,
        &policy.draft().binding,
        &policy.draft().catalog,
    )?)
}

#[test]
fn complete_catalogs_defaults_noops_and_documented_relationships() -> R {
    for (version, count, retired) in
        [("5.36.3", 80, 0), ("5.38.5", 80, 8), ("5.42.3", 80, 11), ("5.44.0", 81, 13)]
    {
        let c = WarningCatalog::for_profile(version)?;
        require(
            c.categories().len() == count && c.accepted_noops().len() == retired,
            "incomplete catalog denominator",
        )?;
        require(
            c.category_reference("experimental::signature_named_parameters").is_ok()
                == (version == "5.44.0"),
            "named category version drift",
        )?;
        require(
            c.category_reference("experimental::args_array_with_signatures").is_ok(),
            "args-array category conflated",
        )?;
        let pipe = c.resolve_reference(&c.category_reference("pipe")?)?;
        require(
            pipe.parent.as_deref() == Some("io") && pipe.id == "perl.warning/pipe",
            "prefix tree/native offset used as identity",
        )?;
        require(c.category_reference("io::pipe").is_err(), "invented prefix category")?;
        let relations = c
            .categories()
            .iter()
            .find(|row| row.name == "experimental::signatures")
            .map(|row| &row.documented_relationships)
            .or_else(|| {
                c.accepted_noops()
                    .iter()
                    .find(|row| row.name == "experimental::signatures")
                    .map(|row| &row.documented_relationships)
            })
            .ok_or("missing signatures registry/no-op identity")?;
        require(
            relations
                .iter()
                .any(|r| r.related_name == "signatures" && r.qualifier == "historical_warning"),
            "historical signatures relation lost",
        )?;
        let args = c.resolve_reference(
            &c.category_reference("experimental::args_array_with_signatures")?,
        )?;
        require(
            args.documented_relationships
                .iter()
                .any(|r| r.kind == "documented_signature_syntax" && r.related_name == "signatures"),
            "args syntax became fake feature",
        )?;
    }
    for version in ["5.44", "5.44.1", "5.40.0", "5.8.0", "5.45.0"] {
        require(
            matches!(WarningCatalog::for_profile(version), Err(SchemaError::Unavailable)),
            "unsupported version silently selected",
        )?;
    }
    Ok(())
}

#[test]
fn supplied_fatal_is_not_enabled_and_opposites_remain_distinct() -> R {
    for value in
        [WarningDisposition::Disabled, WarningDisposition::Enabled, WarningDisposition::Fatal]
    {
        let mut draft = fixture()?;
        draft.baseline = exact(WarningBaseline::Uniform(value))?;
        let policy = WarningPolicy::admit(draft)?;
        require(
            query(&policy, "numeric")?.exact()? == value,
            "supplied fatal/ordinary/disabled collapsed",
        )?;
    }
    Ok(())
}

#[test]
fn effective_order_parent_child_and_unrelated_category_are_independent() -> R {
    let mut draft = fixture()?;
    draft.overrides = vec![
        item(10, "experimental", WarningDisposition::Fatal)?,
        item(20, "experimental::signature_named_parameters", WarningDisposition::Enabled)?,
    ];
    let policy = WarningPolicy::admit(draft.clone())?;
    require(
        query(&policy, "experimental")?.exact()? == WarningDisposition::Fatal,
        "parent lost fatal",
    )?;
    require(
        query(&policy, "experimental::args_array_with_signatures")?.exact()?
            == WarningDisposition::Fatal,
        "sibling lost inherited fatal",
    )?;
    require(
        query(&policy, "experimental::signature_named_parameters")?.exact()?
            == WarningDisposition::Enabled,
        "later child failed",
    )?;
    require(
        query(&policy, "numeric")?.exact()? == WarningDisposition::Disabled,
        "unrelated override leaked",
    )?;
    draft.overrides = vec![
        item(10, "experimental::signature_named_parameters", WarningDisposition::Enabled)?,
        item(20, "experimental", WarningDisposition::Fatal)?,
    ];
    let reverse = WarningPolicy::admit(draft)?;
    require(
        query(&reverse, "experimental::signature_named_parameters")?.exact()?
            == WarningDisposition::Fatal,
        "deepest-child wrongly beats later parent",
    )?;
    let mut draft = fixture()?;
    draft.overrides = vec![item(3, "io", WarningDisposition::Enabled)?];
    require(
        query(&WarningPolicy::admit(draft)?, "pipe")?.exact()? == WarningDisposition::Enabled,
        "explicit nonprefix parent ignored",
    )
}

#[test]
fn root_query_is_not_uniform_descendants_and_defaults_are_actual_bits() -> R {
    let mut draft = fixture()?;
    draft.baseline = exact(WarningBaseline::CatalogDefaults)?;
    let defaults = WarningPolicy::admit(draft)?;
    require(
        defaults
            .all_warnings_disposition(&defaults.draft().binding, &defaults.draft().catalog)?
            .exact()?
            == WarningDisposition::Disabled,
        "default root is not its own bit",
    )?;
    require(
        query(&defaults, "glob")?.exact()? == WarningDisposition::Enabled,
        "default-on child invented disabled",
    )?;
    require(
        query(&defaults, "deprecated")?.exact()? == WarningDisposition::Enabled,
        "default-on grouping bit lost",
    )?;
    let mut draft = fixture()?;
    draft.baseline = exact(WarningBaseline::Uniform(WarningDisposition::Enabled))?;
    draft.overrides = vec![item(0, "numeric", WarningDisposition::Disabled)?];
    let policy = WarningPolicy::admit(draft)?;
    require(
        policy
            .all_warnings_disposition(&policy.draft().binding, &policy.draft().catalog)?
            .exact()?
            == WarningDisposition::Enabled,
        "root changed by child",
    )?;
    require(
        query(&policy, "numeric")?.exact()? == WarningDisposition::Disabled,
        "root globalboolean overwrote child",
    )
}

#[test]
fn unknown_noop_and_qualifications_survive_wire_without_promotions() -> R {
    let mut draft = fixture()?;
    draft.overrides = vec![item(0, "experimental::signatures", WarningDisposition::Fatal)?];
    let policy = WarningPolicy::admit(draft.clone())?;
    let noop = query(&policy, "experimental::signatures")?;
    require(
        matches!(noop.lookup(), WarningLookup::AcceptedNoOp(_)) && noop.disposition().is_none(),
        "no-op became live or unknown",
    )?;
    require(
        query(&policy, "numeric")?.exact()? == WarningDisposition::Disabled,
        "no-op affected unrelated live category",
    )?;
    let unknown = query(&policy, "my::registered")?;
    require(
        matches!(unknown.lookup(),WarningLookup::Unknown(s) if s=="my::registered")
            && unknown.exact().is_err(),
        "unknown dropped or exact",
    )?;
    draft.overrides = vec![item(1, "my::registered", WarningDisposition::Fatal)?];
    require(WarningPolicy::admit(draft.clone()).is_err(), "unknown exact override admitted")?;
    if let Some(value) = draft.overrides.first_mut() {
        value.disposition.outcome = Outcome::Conditional;
        value.disposition.reason = Some("registration condition".into());
    }
    require(
        WarningPolicy::admit(draft.clone()).is_err(),
        "exact aggregate concealed unknown effect",
    )?;
    draft.coverage.outcome = Outcome::Limited;
    draft.coverage.reason = Some("partial supplied policy".into());
    draft.coverage.value = Some(WarningCoverage::Partial);
    let partial = WarningPolicy::admit(draft)?;
    let reread = WarningPolicy::read(partial.to_json()?.as_slice())?;
    let answer = query(&reread, "numeric")?;
    require(
        answer.exact().is_err()
            && answer.qualifications().iter().any(|q| {
                q.outcome == Outcome::Conditional
                    && q.reason.as_deref() == Some("registration condition")
            }),
        "unknown extent became no-op or lost original qualifier",
    )?;
    require(reread.digest()? == partial.digest()?, "qualified roundtrip changed identity")
}

#[test]
fn references_profiles_full_binding_and_port_remain_separate_authorities() -> R {
    let policy = WarningPolicy::admit(fixture()?)?;
    let c = WarningCatalog::for_profile("5.44.0")?;
    let reference = c.category_reference("numeric")?;
    require(
        policy.warning_disposition(&reference, &policy.draft().binding)?.exact()?
            == WarningDisposition::Disabled,
        "known reference unavailable",
    )?;
    for bad in [
        CategoryReference {
            catalog: WarningCatalog::for_profile("5.42.3")?.identity().clone(),
            id: reference.id.clone(),
        },
        CategoryReference { catalog: c.identity().clone(), id: "perl.warning/custom".into() },
    ] {
        require(c.resolve_reference(&bad).is_err(), "wrong catalog/custom reference admitted")?;
    }
    let mut bad = reference.clone();
    bad.catalog.digest = ContentDigest::of_bytes(b"wrong");
    require(c.resolve_reference(&bad).is_err(), "wrong digest accepted")?;
    let mut expected = policy.draft().binding.clone();
    expected.compiler_generation = "later".into();
    require(
        policy.warning_disposition(&reference, &expected)?.exact().is_err()
            && policy.port(&expected, c.identity()).is_err(),
        "wrong full binding remained exact",
    )?;
    let port = policy.port(&policy.draft().binding, c.identity())?;
    let digest = policy.digest()?;
    require(
        port.outcome == Outcome::Exact
            && port
                .value
                .as_ref()
                .is_some_and(|p| p.role == PortRole::WarningPolicy && p.payload == digest),
        "wrong typed policy port",
    )?;
    let mut draft = policy.draft().clone();
    draft.catalog.digest = ContentDigest::of_bytes(b"wrong");
    require(WarningPolicy::admit(draft).is_err(), "policy accepted unrelated catalog digest")
}

#[test]
fn normalized_order_identity_and_literal_digest_oracle() -> R {
    let mut draft = fixture()?;
    draft.overrides = vec![
        item(20, "numeric", WarningDisposition::Fatal)?,
        item(10, "all", WarningDisposition::Enabled)?,
    ];
    let policy = WarningPolicy::admit(draft.clone())?;
    draft.overrides.reverse();
    let permutation = WarningPolicy::admit(draft.clone())?;
    require(policy.digest()? == permutation.digest()?, "container insertion order changed policy")?;
    let p = policy.draft();
    let projected = serde_json::json!({
        "schema_version": p.schema_version, "catalog": p.catalog, "binding": p.binding, "scope": p.scope,
        "baseline": {"outcome":p.baseline.outcome,"value":p.baseline.value,"origins":p.baseline.origins},
        "coverage": {"outcome":p.coverage.outcome,"value":p.coverage.value,"origins":p.coverage.origins},
        "overrides":p.overrides.iter().map(|o|serde_json::json!({"ordinal":o.ordinal,"name":o.name,
            "disposition":{"outcome":o.disposition.outcome,"value":o.disposition.value,"origins":o.disposition.origins}})).collect::<Vec<_>>(),
        "boundaries":[]
    });
    let expected = ContentDigest::of_bytes(&serde_json::to_vec(&("warning_policy.v1", projected))?);
    require(policy.digest()? == expected, "policy digest domain/projection drift")?;
    draft.catalog = WarningCatalog::for_profile("5.42.3")?.identity().clone();
    require(
        WarningPolicy::admit(draft)?.digest()? != policy.digest()?,
        "catalog identity omitted",
    )?;
    let roundtrip = WarningPolicy::read(policy.to_json()?.as_slice())?;
    require(roundtrip.draft() == policy.draft(), "exact empty coverage payload lost on wire")
}

#[test]
fn duplicate_order_invalid_shared_wire_and_operational_limits_refuse() -> R {
    let mut draft = fixture()?;
    draft.overrides = vec![
        item(0, "numeric", WarningDisposition::Fatal)?,
        item(0, "io", WarningDisposition::Disabled)?,
    ];
    require(WarningPolicy::admit(draft).is_err(), "duplicate ordinals admitted")?;
    let policy = WarningPolicy::admit(fixture()?)?;
    let mut value = serde_json::to_value(policy.draft())?;
    value
        .pointer_mut("/binding/semantic")
        .and_then(|v| v.as_object_mut())
        .ok_or("missing semantic object")?
        .insert("future_field".into(), true.into());
    require(
        WarningPolicy::read(serde_json::to_vec(&value)?.as_slice()).is_err(),
        "shared nested unknown field silently dropped",
    )?;
    let mut draft = fixture()?;
    draft.binding.source.generation = SourceGeneration::Unknown;
    require(WarningPolicy::admit(draft).is_err(), "unknown generation admitted")?;
    let mut draft = fixture()?;
    for ordinal in 0..MAX_OVERRIDES {
        draft.overrides.push(item(
            u32::try_from(ordinal)?,
            "numeric",
            WarningDisposition::Enabled,
        )?);
    }
    require(WarningPolicy::admit(draft.clone()).is_ok(), "exact override bound refused")?;
    draft.overrides.push(item(
        u32::try_from(MAX_OVERRIDES)?,
        "numeric",
        WarningDisposition::Enabled,
    )?);
    require(
        matches!(WarningPolicy::admit(draft), Err(SchemaError::Limited(_))),
        "over-limit overrides truncated/accepted",
    )?;
    let mut bytes = policy.to_json()?;
    bytes.resize(MAX_SNAPSHOT_BYTES, b' ');
    require(WarningPolicy::read(bytes.as_slice()).is_ok(), "exact wire limit refused")?;
    bytes.push(b' ');
    require(
        matches!(WarningPolicy::read(bytes.as_slice()), Err(SchemaError::Limited(_))),
        "over-limit wire accepted",
    )
}

#[test]
fn structured_policy_payload_exact_limit_and_neighbors() -> R {
    let mut draft = fixture()?;
    draft.coverage.outcome = Outcome::Limited;
    draft.coverage.reason = Some("bounded supplied inputs".into());
    for ordinal in 0..32 {
        let mut value = item(ordinal, "numeric", WarningDisposition::Enabled)?;
        value.disposition.outcome = Outcome::Limited;
        value.disposition.reason = Some("x".into());
        draft.overrides.push(value);
    }
    let mut length = serde_json::to_vec(&draft)?.len();
    'fill: for index in 0..255 {
        let mut shared = origin()?;
        shared.id = format!("shared-origin-{index:03}");
        for position in 0..draft.overrides.len() {
            draft
                .overrides
                .get_mut(position)
                .ok_or("missing override")?
                .disposition
                .origins
                .push(shared.clone());
            let next = serde_json::to_vec(&draft)?.len();
            if next >= MAX_SNAPSHOT_BYTES - 1 {
                draft
                    .overrides
                    .get_mut(position)
                    .ok_or("missing override")?
                    .disposition
                    .origins
                    .pop();
                break 'fill;
            }
            length = next;
        }
    }
    let mut remaining =
        (MAX_SNAPSHOT_BYTES - 1).checked_sub(length).ok_or("fixture exceeded target")?;
    for value in &mut draft.overrides {
        let growth = remaining.min(254);
        value.disposition.reason = Some("x".repeat(1 + growth));
        remaining -= growth;
    }
    require(remaining == 0, "fixture cannot reach actual payload boundary")?;
    for size in [MAX_SNAPSHOT_BYTES - 1, MAX_SNAPSHOT_BYTES, MAX_SNAPSHOT_BYTES + 1] {
        let bytes = serde_json::to_vec(&draft)?;
        require(bytes.len() == size, "structured payload size differs from intended boundary")?;
        if size <= MAX_SNAPSHOT_BYTES {
            let policy = WarningPolicy::admit(draft.clone())?;
            require(policy.to_json()?.len() == size, "normalization changed fixture size")?;
            let read = WarningPolicy::read(bytes.as_slice())?;
            require(read.digest()? == policy.digest()?, "bounded roundtrip identity drift")?;
        } else {
            require(
                matches!(WarningPolicy::admit(draft.clone()), Err(SchemaError::Limited(_))),
                "oversize structured draft produced an admitted policy",
            )?;
            require(
                matches!(WarningPolicy::read(bytes.as_slice()), Err(SchemaError::Limited(_))),
                "oversize structured wire produced an admitted policy",
            )?;
        }
        let reason = draft
            .overrides
            .last_mut()
            .ok_or("missing last override")?
            .disposition
            .reason
            .as_mut()
            .ok_or("missing padding reason")?;
        reason.push('x');
    }
    Ok(())
}
