//! Fixtures and negative controls for the package topology contract (#8913).

use std::error::Error;

use super::{
    PACKAGE_TOPOLOGY_SCHEMA_TAG, PACKAGE_TOPOLOGY_SCHEMA_VERSION, PackageDeclarationAnchor,
    PackageDeclarationContribution, PackageDeclarationForm, PackageDeclarationId,
    PackageEditAuthority, PackageId, PackageNameAnchor, PackageResolution,
    PackageResolutionRequest, PackageSourceTier, PackageTopologyContractError,
};
use crate::semantic_identity::{
    SemanticDependencyIdentity, SemanticDependencyKind, SemanticSubjectStatus,
};
use crate::{
    AnchorId, BoundaryKind, Confidence, EntityId, EntityKind, FileId, Provenance,
    SemanticConfidence, SemanticProducer, SemanticProvenance, SourceAnchor,
};
use perl_test_must::{must_err_with, must_with};

type FixtureResult = Result<(), Box<dyn Error>>;

fn span(start: u32, end: u32) -> SourceAnchor {
    SourceAnchor::new(Some(AnchorId(1)), FileId(11), start, end)
}

fn package_id(domain: &str, name: &str) -> Result<PackageId, PackageTopologyContractError> {
    PackageId::new("project-app", domain, name)
}

fn request(
    domain: &str,
    name: &str,
) -> Result<PackageResolutionRequest, PackageTopologyContractError> {
    PackageResolutionRequest::new("project-app", domain, name)
}

fn name_anchor(
    name: &str,
    start: u32,
    end: u32,
) -> Result<PackageNameAnchor, PackageTopologyContractError> {
    PackageNameAnchor::new(name, format!("name-digest-{name}"), Some(span(start, end)))
}

fn declaration_anchor(
    digest: &str,
    start: u32,
    end: u32,
) -> Result<PackageDeclarationAnchor, PackageTopologyContractError> {
    PackageDeclarationAnchor::new(digest, Some(span(start, end)))
}

fn declaration_id(
    source: &str,
    generation: &str,
    form: PackageDeclarationForm,
    name: &str,
    ordinal: u32,
) -> Result<PackageDeclarationId, PackageTopologyContractError> {
    PackageDeclarationId::new(
        source,
        "rev-1",
        generation,
        form,
        Some(format!("name-digest-{name}")),
        ordinal,
    )
}

fn contribution(
    package: PackageId,
    declaration: PackageDeclarationId,
    entity: u64,
    kind: EntityKind,
    name: &str,
    completeness: SemanticSubjectStatus,
    limitations: Vec<String>,
) -> Result<PackageDeclarationContribution, PackageTopologyContractError> {
    PackageDeclarationContribution::new(
        package,
        declaration,
        EntityId(entity),
        kind,
        Some(declaration_anchor(&format!("decl-{name}"), 0, 12)?),
        Some(name_anchor(name, 8, 11)?),
        PackageSourceTier::Workspace,
        PackageEditAuthority::Editable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        completeness,
        limitations,
        Vec::new(),
    )
}

fn isolated_foo() -> Result<PackageDeclarationContribution, PackageTopologyContractError> {
    let package = package_id("root-a", "Foo")?;
    let declaration = declaration_id(
        "root-a::lib/Foo.pm",
        "gen-1",
        PackageDeclarationForm::PackageStatement,
        "Foo",
        0,
    )?;
    contribution(
        package,
        declaration,
        7,
        EntityKind::Package,
        "Foo",
        SemanticSubjectStatus::Complete,
        Vec::new(),
    )
}

#[test]
fn isolated_package_declaration_is_identified_by_project_domain_and_name() -> FixtureResult {
    let contrib = isolated_foo()?;
    assert_eq!(contrib.package_id().project_identity(), "project-app");
    assert_eq!(contrib.package_id().resolution_domain(), "root-a");
    assert_eq!(contrib.package_id().qualified_name(), "Foo");
    assert_eq!(contrib.entity_id(), EntityId(7));
    assert_eq!(contrib.kind(), EntityKind::Package);
    assert!(contrib.name_anchor().and_then(PackageNameAnchor::span).is_some());
    Ok(())
}

#[test]
fn reopened_package_in_two_files_shares_package_id() -> FixtureResult {
    let package = package_id("root-a", "Foo")?;
    let first = declaration_id(
        "root-a::lib/Foo.pm",
        "gen-1",
        PackageDeclarationForm::PackageStatement,
        "Foo",
        0,
    )?;
    let second = declaration_id(
        "root-a::lib/Foo/Extra.pm",
        "gen-1",
        PackageDeclarationForm::PackageStatement,
        "Foo",
        0,
    )?;
    let a = contribution(
        package.clone(),
        first,
        7,
        EntityKind::Package,
        "Foo",
        SemanticSubjectStatus::Complete,
        Vec::new(),
    )?;
    let b = contribution(
        package,
        second,
        8,
        EntityKind::Package,
        "Foo",
        SemanticSubjectStatus::Complete,
        Vec::new(),
    )?;
    assert_eq!(a.package_id(), b.package_id());
    assert_ne!(a.declaration_id(), b.declaration_id());
    assert_ne!(a.fingerprint(), b.fingerprint());
    Ok(())
}

#[test]
fn several_packages_in_one_file_keep_distinct_declaration_ids() -> FixtureResult {
    let foo = contribution(
        package_id("root-a", "Foo")?,
        declaration_id(
            "root-a::lib/Bundle.pm",
            "gen-1",
            PackageDeclarationForm::PackageStatement,
            "Foo",
            0,
        )?,
        7,
        EntityKind::Package,
        "Foo",
        SemanticSubjectStatus::Complete,
        Vec::new(),
    )?;
    let bar = contribution(
        package_id("root-a", "Bar")?,
        declaration_id(
            "root-a::lib/Bundle.pm",
            "gen-1",
            PackageDeclarationForm::PackageBlock,
            "Bar",
            1,
        )?,
        9,
        EntityKind::Package,
        "Bar",
        SemanticSubjectStatus::Complete,
        Vec::new(),
    )?;
    assert_ne!(foo.package_id(), bar.package_id());
    assert_eq!(foo.logical_source_id(), bar.logical_source_id());
    assert_ne!(foo.declaration_id(), bar.declaration_id());
    Ok(())
}

#[test]
fn same_qualified_name_in_two_roots_is_two_package_ids() -> FixtureResult {
    let left = package_id("root-a", "Foo")?;
    let right = package_id("root-b", "Foo")?;
    assert_ne!(left, right);
    assert_ne!(left.fingerprint(), right.fingerprint());
    Ok(())
}

#[test]
fn workspace_and_resolved_dependency_same_spelling_remain_distinct() -> FixtureResult {
    let workspace = package_id("root-a", "Foo")?;
    let dependency = PackageId::new("project-app", "cpan:Foo-1.23", "Foo")?;
    let workspace_resolution = PackageResolution::workspace(workspace.clone())?;
    let dependency_resolution = PackageResolution::resolved_dependency(
        dependency.clone(),
        PackageSourceTier::ResolvedDependency,
    )?;
    assert_ne!(workspace, dependency);
    assert_ne!(workspace_resolution, dependency_resolution);
    assert_ne!(workspace_resolution.fingerprint(), dependency_resolution.fingerprint());
    assert_eq!(workspace_resolution.tag(), "workspace");
    assert_eq!(dependency_resolution.tag(), "resolved-dependency");
    Ok(())
}

#[test]
fn main_in_two_source_domains_is_two_package_ids() -> FixtureResult {
    let script = package_id("root-a::script.pl", "main")?;
    let test = package_id("root-a::t/app.t", "main")?;
    assert_eq!(script.qualified_name(), "main");
    assert_eq!(test.qualified_name(), "main");
    assert_ne!(script, test);
    Ok(())
}

#[test]
fn source_identical_later_generation_keeps_package_id_and_moves_declaration() -> FixtureResult {
    let package = package_id("root-a", "Foo")?;
    let first = declaration_id(
        "root-a::lib/Foo.pm",
        "gen-1",
        PackageDeclarationForm::PackageStatement,
        "Foo",
        0,
    )?;
    let later = declaration_id(
        "root-a::lib/Foo.pm",
        "gen-2",
        PackageDeclarationForm::PackageStatement,
        "Foo",
        0,
    )?;
    let a = contribution(
        package.clone(),
        first,
        7,
        EntityKind::Package,
        "Foo",
        SemanticSubjectStatus::Complete,
        Vec::new(),
    )?;
    let b = contribution(
        package,
        later,
        7,
        EntityKind::Package,
        "Foo",
        SemanticSubjectStatus::Complete,
        Vec::new(),
    )?;
    assert_eq!(a.package_id(), b.package_id());
    assert_ne!(a.declaration_id(), b.declaration_id());
    assert_ne!(a.source_generation(), b.source_generation());
    Ok(())
}

#[test]
fn recovered_declaration_can_omit_exact_name_span() -> FixtureResult {
    let package = package_id("root-a", "Foo")?;
    let declaration = PackageDeclarationId::new(
        "root-a::lib/Foo.pm",
        "rev-1",
        "gen-1",
        PackageDeclarationForm::PackageStatement,
        None,
        0,
    )?;
    let contrib = PackageDeclarationContribution::new(
        package,
        declaration,
        EntityId(7),
        EntityKind::Package,
        Some(PackageDeclarationAnchor::new("decl-recovered", None)?),
        None,
        PackageSourceTier::Workspace,
        PackageEditAuthority::Editable,
        SemanticProducer::Parser,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::Medium),
        SemanticSubjectStatus::PartialRecovered,
        vec!["missing exact name span".to_string()],
        Vec::new(),
    )?;
    assert!(contrib.name_anchor().is_none());
    assert_eq!(contrib.completeness(), SemanticSubjectStatus::PartialRecovered);
    Ok(())
}

#[test]
fn complete_declaration_rejects_missing_name_span() -> FixtureResult {
    let package = package_id("root-a", "Foo")?;
    let declaration = PackageDeclarationId::new(
        "root-a::lib/Foo.pm",
        "rev-1",
        "gen-1",
        PackageDeclarationForm::PackageStatement,
        None,
        0,
    )?;
    let err = PackageDeclarationContribution::new(
        package,
        declaration,
        EntityId(7),
        EntityKind::Package,
        Some(declaration_anchor("decl-Foo", 0, 12)?),
        None,
        PackageSourceTier::Workspace,
        PackageEditAuthority::Editable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        Vec::new(),
    );
    let err = must_err_with(err, "complete without a name span must fail");
    assert!(matches!(err, PackageTopologyContractError::ContradictoryStatus(_)));
    Ok(())
}

#[test]
fn name_anchor_spelling_must_match_package_identity() -> FixtureResult {
    let err = PackageDeclarationContribution::new(
        package_id("root-a", "Foo")?,
        declaration_id(
            "root-a::lib/Foo.pm",
            "gen-1",
            PackageDeclarationForm::PackageStatement,
            "Bar",
            0,
        )?,
        EntityId(7),
        EntityKind::Package,
        Some(declaration_anchor("decl-Bar", 0, 12)?),
        Some(name_anchor("Bar", 8, 11)?),
        PackageSourceTier::Workspace,
        PackageEditAuthority::Editable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        Vec::new(),
    );
    let err = must_err_with(err, "Foo package cannot join a Bar name anchor");
    assert!(matches!(err, PackageTopologyContractError::ContradictoryStatus(_)));
    Ok(())
}

fn complete_with_name_span(
    name_span: SourceAnchor,
) -> Result<PackageDeclarationContribution, PackageTopologyContractError> {
    PackageDeclarationContribution::new(
        package_id("root-a", "Foo")?,
        declaration_id(
            "root-a::lib/Foo.pm",
            "gen-1",
            PackageDeclarationForm::PackageStatement,
            "Foo",
            0,
        )?,
        EntityId(7),
        EntityKind::Package,
        Some(declaration_anchor("decl-Foo", 0, 12)?),
        Some(PackageNameAnchor::new("Foo", "name-digest-Foo", Some(name_span))?),
        PackageSourceTier::Workspace,
        PackageEditAuthority::Editable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        Vec::new(),
    )
}

#[test]
fn complete_declaration_rejects_empty_or_foreign_name_span() -> FixtureResult {
    let empty = complete_with_name_span(span(8, 8));
    let empty = must_err_with(empty, "empty name span is not an exact name token");
    assert!(matches!(empty, PackageTopologyContractError::ContradictoryStatus(_)));

    let cross_file =
        complete_with_name_span(SourceAnchor::new(Some(AnchorId(1)), FileId(12), 8, 11));
    let cross_file =
        must_err_with(cross_file, "cross-file name span is not inside the declaration");
    assert!(matches!(cross_file, PackageTopologyContractError::ContradictoryStatus(_)));

    let outside = complete_with_name_span(span(20, 23));
    let outside = must_err_with(outside, "name span past the declaration is not exact");
    assert!(matches!(outside, PackageTopologyContractError::ContradictoryStatus(_)));
    Ok(())
}

#[test]
fn fingerprints_are_deterministic_and_field_boundary_sensitive() -> FixtureResult {
    let a = PackageId::new("ab", "c", "Foo")?;
    let b = PackageId::new("a", "bc", "Foo")?;
    assert_ne!(a.fingerprint(), b.fingerprint());
    assert_eq!(a.fingerprint(), PackageId::new("ab", "c", "Foo")?.fingerprint());
    Ok(())
}

#[test]
fn resolution_states_remain_distinct_and_have_no_external_arm() -> FixtureResult {
    let package = package_id("root-a", "Foo")?;
    let other = package_id("root-b", "Foo")?;
    let asked = request("root-a", "Foo")?;
    let states = [
        PackageResolution::workspace(package.clone())?,
        PackageResolution::resolved_dependency(
            PackageId::new("project-app", "cpan:Foo-1.23", "Foo")?,
            PackageSourceTier::Bundled,
        )?,
        PackageResolution::ambiguous(asked.clone(), vec![package, other])?,
        PackageResolution::missing(asked.clone())?,
        PackageResolution::dynamic(asked.clone(), BoundaryKind::DynamicRequire)?,
        PackageResolution::unsupported(asked.clone(), "resolver-not-admitted")?,
        PackageResolution::stale(asked, "gen-2", "gen-1")?,
    ];
    let mut tags: Vec<&str> = states.iter().map(PackageResolution::tag).collect();
    tags.sort();
    tags.dedup();
    assert_eq!(tags.len(), 7, "each resolution state must have a distinct tag");
    assert!(!tags.contains(&"external"));
    for state in &states {
        match state {
            PackageResolution::Workspace(_)
            | PackageResolution::ResolvedDependency { .. }
            | PackageResolution::Ambiguous { .. }
            | PackageResolution::Missing { .. }
            | PackageResolution::Dynamic { .. }
            | PackageResolution::Unsupported { .. }
            | PackageResolution::Stale { .. } => {}
        }
        assert_ne!(state.tag(), "external");
        if matches!(
            state,
            PackageResolution::Missing { .. }
                | PackageResolution::Dynamic { .. }
                | PackageResolution::Unsupported { .. }
                | PackageResolution::Ambiguous { .. }
                | PackageResolution::Stale { .. }
        ) {
            assert!(state.resolved_package().is_none());
        }
    }
    Ok(())
}

#[test]
fn ambiguous_candidates_are_order_independent() -> FixtureResult {
    let foo_a = package_id("root-a", "Foo")?;
    let foo_b = package_id("root-b", "Foo")?;
    let asked = request("workspace", "Foo")?;
    let left = PackageResolution::ambiguous(asked.clone(), vec![foo_a.clone(), foo_b.clone()])?;
    let right = PackageResolution::ambiguous(asked, vec![foo_b, foo_a])?;
    assert_eq!(left, right);
    assert_eq!(left.fingerprint(), right.fingerprint());
    Ok(())
}

#[test]
fn ambiguous_candidates_must_answer_the_requested_package() -> FixtureResult {
    let asked = request("workspace", "Foo")?;
    let other_name = package_id("root-a", "Bar")?;
    let other_project = PackageId::new("project-other", "root-a", "Foo")?;
    let name_err =
        PackageResolution::ambiguous(asked.clone(), vec![package_id("root-a", "Foo")?, other_name]);
    let name_err = must_err_with(name_err, "Bar is not a Foo candidate");
    assert!(matches!(name_err, PackageTopologyContractError::MalformedResolution(_)));

    let project_err =
        PackageResolution::ambiguous(asked, vec![package_id("root-a", "Foo")?, other_project]);
    let project_err =
        must_err_with(project_err, "another project is not a candidate for this request");
    assert!(matches!(project_err, PackageTopologyContractError::MalformedResolution(_)));
    Ok(())
}

#[test]
fn sentinel_entity_id_cannot_mint_a_declaration() -> FixtureResult {
    let err = PackageDeclarationContribution::new(
        package_id("root-a", "Foo")?,
        declaration_id(
            "root-a::lib/Foo.pm",
            "gen-1",
            PackageDeclarationForm::PackageStatement,
            "Foo",
            0,
        )?,
        EntityId(0),
        EntityKind::Package,
        Some(declaration_anchor("decl-Foo", 0, 12)?),
        Some(name_anchor("Foo", 8, 11)?),
        PackageSourceTier::Workspace,
        PackageEditAuthority::Editable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        Vec::new(),
    );
    let err = must_err_with(err, "EntityId(0) must be rejected");
    assert_eq!(err, PackageTopologyContractError::SentinelEntityId);
    Ok(())
}

#[test]
fn rootless_and_path_only_identities_are_rejected() {
    assert!(matches!(
        PackageId::new("project-app", "", "Foo"),
        Err(PackageTopologyContractError::EmptyIdentityField("PackageId.resolution_domain"))
    ));
    assert!(matches!(
        PackageId::new("", "lib/Foo.pm", "Foo"),
        Err(PackageTopologyContractError::EmptyIdentityField("PackageId.project_identity"))
    ));
    assert!(matches!(
        PackageId::new("project-app", "root-a", ""),
        Err(PackageTopologyContractError::EmptyIdentityField("PackageId.qualified_name"))
    ));
    assert!(matches!(
        PackageId::new("   ", "root-a", "Foo"),
        Err(PackageTopologyContractError::EmptyIdentityField("PackageId.project_identity"))
    ));
}

#[test]
fn resolved_dependency_cannot_claim_workspace_tier() -> FixtureResult {
    let err = PackageResolution::resolved_dependency(
        package_id("cpan:Foo-1.23", "Foo")?,
        PackageSourceTier::Workspace,
    );
    let err = must_err_with(err, "workspace tier is not a dependency tier");
    assert!(matches!(err, PackageTopologyContractError::MalformedResolution(_)));
    Ok(())
}

#[test]
fn editable_authority_is_workspace_only() -> FixtureResult {
    let err = PackageDeclarationContribution::new(
        PackageId::new("project-app", "cpan:Foo-1.23", "Foo")?,
        declaration_id(
            "dep::lib/Foo.pm",
            "gen-1",
            PackageDeclarationForm::PackageStatement,
            "Foo",
            0,
        )?,
        EntityId(4),
        EntityKind::Package,
        Some(declaration_anchor("decl-Foo", 0, 12)?),
        Some(name_anchor("Foo", 8, 11)?),
        PackageSourceTier::ResolvedDependency,
        PackageEditAuthority::Editable,
        SemanticProducer::WorkspaceIndex,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        Vec::new(),
    );
    let err = must_err_with(err, "dependency source cannot be editable");
    assert!(matches!(err, PackageTopologyContractError::ContradictoryStatus(_)));
    Ok(())
}

#[test]
fn class_and_role_forms_match_entity_kind() -> FixtureResult {
    let class = contribution(
        package_id("root-a", "App::Thing")?,
        declaration_id(
            "root-a::lib/App/Thing.pm",
            "gen-1",
            PackageDeclarationForm::Class,
            "App::Thing",
            0,
        )?,
        11,
        EntityKind::Class,
        "App::Thing",
        SemanticSubjectStatus::Complete,
        Vec::new(),
    )?;
    assert_eq!(class.kind(), EntityKind::Class);
    let mismatch = PackageDeclarationContribution::new(
        package_id("root-a", "App::Thing")?,
        declaration_id(
            "root-a::lib/App/Thing.pm",
            "gen-1",
            PackageDeclarationForm::Class,
            "App::Thing",
            0,
        )?,
        EntityId(11),
        EntityKind::Package,
        Some(declaration_anchor("decl-App::Thing", 0, 12)?),
        Some(name_anchor("App::Thing", 8, 18)?),
        PackageSourceTier::Workspace,
        PackageEditAuthority::Editable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        Vec::new(),
    );
    let mismatch = must_err_with(mismatch, "class form cannot declare a package kind");
    assert!(matches!(mismatch, PackageTopologyContractError::KindFormMismatch { .. }));
    Ok(())
}

#[test]
fn json_round_trip_survives_and_forged_sentinel_fails_validate() -> FixtureResult {
    let contrib = isolated_foo()?;
    let encoded = must_with(serde_json::to_string(&contrib), "serialize contribution");
    let decoded: PackageDeclarationContribution =
        must_with(serde_json::from_str(&encoded), "deserialize contribution");
    decoded.validate()?;
    assert_eq!(decoded, contrib);
    assert_eq!(decoded.fingerprint(), contrib.fingerprint());

    let mut forged: serde_json::Value = must_with(serde_json::from_str(&encoded), "json value");
    forged["entity_id"] = serde_json::json!(0);
    let decoded: PackageDeclarationContribution =
        must_with(serde_json::from_value(forged), "forged record still deserializes");
    assert_eq!(decoded.validate(), Err(PackageTopologyContractError::SentinelEntityId));

    let with_dep = PackageDeclarationContribution::new(
        package_id("root-a", "Foo")?,
        declaration_id(
            "root-a::lib/Foo.pm",
            "gen-1",
            PackageDeclarationForm::PackageStatement,
            "Foo",
            0,
        )?,
        EntityId(7),
        EntityKind::Package,
        Some(declaration_anchor("decl-Foo", 0, 12)?),
        Some(name_anchor("Foo", 8, 11)?),
        PackageSourceTier::Workspace,
        PackageEditAuthority::Editable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        vec![SemanticDependencyIdentity::new(SemanticDependencyKind::NamedFact, "fact-1")?],
    )?;
    let encoded = must_with(serde_json::to_string(&with_dep), "serialize contribution with dep");
    let mut forged: serde_json::Value = must_with(serde_json::from_str(&encoded), "dep json value");
    forged["dependencies"][0]["target_fingerprint"] = serde_json::json!(" ");
    let decoded: PackageDeclarationContribution =
        must_with(serde_json::from_value(forged), "blank-target dep still deserializes");
    assert_eq!(
        decoded.validate(),
        Err(PackageTopologyContractError::EmptyIdentityField(
            "PackageDeclarationContribution.dependencies.target_fingerprint",
        ))
    );
    Ok(())
}

#[test]
fn schema_tag_is_stable() {
    assert_eq!(PACKAGE_TOPOLOGY_SCHEMA_VERSION, 1);
    assert_eq!(PACKAGE_TOPOLOGY_SCHEMA_TAG, "package-topology.v1");
}

#[test]
fn fingerprints_include_sorted_dependencies() -> FixtureResult {
    let dep_b = SemanticDependencyIdentity::new(SemanticDependencyKind::PackageState, "pkg-b")?;
    let dep_a = SemanticDependencyIdentity::new(SemanticDependencyKind::Declaration, "decl-a")?;
    let package = package_id("root-a", "Foo")?;
    let declaration = declaration_id(
        "root-a::lib/Foo.pm",
        "gen-1",
        PackageDeclarationForm::PackageStatement,
        "Foo",
        0,
    )?;
    let first = PackageDeclarationContribution::new(
        package.clone(),
        declaration.clone(),
        EntityId(7),
        EntityKind::Package,
        Some(declaration_anchor("decl-Foo", 0, 12)?),
        Some(name_anchor("Foo", 8, 11)?),
        PackageSourceTier::Workspace,
        PackageEditAuthority::Editable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        vec![dep_b.clone(), dep_a.clone()],
    )?;
    let second = PackageDeclarationContribution::new(
        package,
        declaration,
        EntityId(7),
        EntityKind::Package,
        Some(declaration_anchor("decl-Foo", 0, 12)?),
        Some(name_anchor("Foo", 8, 11)?),
        PackageSourceTier::Workspace,
        PackageEditAuthority::Editable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        vec![dep_a, dep_b],
    )?;
    assert_eq!(first, second);
    assert_eq!(first.fingerprint(), second.fingerprint());
    Ok(())
}

/// Architecture fence: the schema carries no LSP, parser, provider, or
/// workspace-graph types, and never constructs `EntityId(0)` in non-test code.
#[test]
fn architecture_fence_forbids_provider_parser_and_graph_ownership() {
    let sources = [
        include_str!("mod.rs"),
        include_str!("identity.rs"),
        include_str!("resolution.rs"),
        include_str!("contribution.rs"),
    ];
    for source in sources {
        for forbidden in [
            "std::time::",
            "std::thread::",
            "tower_lsp",
            "lsp_types",
            "perl_parser",
            "perl_lsp",
            "perl_ast",
            "perl_workspace",
            "perl_semantic_analyzer",
            "PackageGraphIndex",
            "EntityId(0)",
        ] {
            assert!(
                !source.contains(forbidden),
                "the package topology contract must not own {forbidden}"
            );
        }
    }
}
