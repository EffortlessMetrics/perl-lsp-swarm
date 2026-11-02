//! External JSON round-trip proof for the package topology contract (#8913).
//!
//! The crate's own guidance requires round-trip coverage for records that
//! cross process and cache boundaries, driven from outside the crate so the
//! proof exercises the public API a consumer actually has.
#![deny(clippy::map_err_ignore)]

use std::error::Error;

use perl_semantic_facts::package_topology::{
    PACKAGE_TOPOLOGY_SCHEMA_VERSION, PackageDeclarationAnchor, PackageDeclarationContribution,
    PackageDeclarationForm, PackageDeclarationId, PackageEditAuthority, PackageId,
    PackageNameAnchor, PackageResolution, PackageResolutionRequest, PackageSourceTier,
};
use perl_semantic_facts::semantic_identity::{
    SemanticDependencyIdentity, SemanticDependencyKind, SemanticSubjectStatus,
};
use perl_semantic_facts::{
    AnchorId, BoundaryKind, Confidence, EntityId, EntityKind, FileId, Provenance,
    SemanticConfidence, SemanticProducer, SemanticProvenance, SourceAnchor,
};

fn span(start: u32, end: u32) -> SourceAnchor {
    SourceAnchor::new(Some(AnchorId(3)), FileId(21), start, end)
}

fn round_trip_package(id: &PackageId) -> Result<(), Box<dyn Error>> {
    let encoded = serde_json::to_string(id)?;
    let decoded: PackageId = serde_json::from_str(&encoded)?;
    decoded.validate()?;
    assert_eq!(&decoded, id);
    assert_eq!(decoded.fingerprint(), id.fingerprint());
    Ok(())
}

fn round_trip_resolution(resolution: &PackageResolution) -> Result<(), Box<dyn Error>> {
    let encoded = serde_json::to_string(resolution)?;
    let decoded: PackageResolution = serde_json::from_str(&encoded)?;
    decoded.validate()?;
    assert_eq!(&decoded, resolution);
    assert_eq!(decoded.fingerprint(), resolution.fingerprint());
    Ok(())
}

fn round_trip_contribution(
    contribution: &PackageDeclarationContribution,
) -> Result<(), Box<dyn Error>> {
    let encoded = serde_json::to_string(contribution)?;
    let decoded: PackageDeclarationContribution = serde_json::from_str(&encoded)?;
    decoded.validate()?;
    assert_eq!(&decoded, contribution);
    assert_eq!(decoded.fingerprint(), contribution.fingerprint());
    Ok(())
}

#[test]
fn every_resolution_variant_round_trips() -> Result<(), Box<dyn Error>> {
    assert_eq!(PACKAGE_TOPOLOGY_SCHEMA_VERSION, 1);
    let workspace = PackageId::new("project-app", "root-a", "Foo")?;
    let dependency = PackageId::new("project-app", "cpan:Foo-1.23", "Foo")?;
    let other = PackageId::new("project-app", "root-b", "Foo")?;
    let asked = PackageResolutionRequest::new("project-app", "root-a", "Foo")?;

    round_trip_package(&workspace)?;
    round_trip_resolution(&PackageResolution::workspace(workspace.clone())?)?;
    round_trip_resolution(&PackageResolution::resolved_dependency(
        dependency,
        PackageSourceTier::ResolvedDependency,
    )?)?;
    round_trip_resolution(&PackageResolution::ambiguous(asked.clone(), vec![workspace, other])?)?;
    round_trip_resolution(&PackageResolution::missing(asked.clone())?)?;
    round_trip_resolution(&PackageResolution::dynamic(
        asked.clone(),
        BoundaryKind::SymbolicReference,
    )?)?;
    round_trip_resolution(&PackageResolution::unsupported(asked.clone(), "no-resolver")?)?;
    round_trip_resolution(&PackageResolution::stale(asked, "gen-9", "gen-8")?)?;
    Ok(())
}

#[test]
fn declaration_contribution_round_trips_with_every_form() -> Result<(), Box<dyn Error>> {
    let forms = [
        (PackageDeclarationForm::PackageStatement, EntityKind::Package, "Foo"),
        (PackageDeclarationForm::PackageBlock, EntityKind::Package, "Bar"),
        (PackageDeclarationForm::Class, EntityKind::Class, "App::Thing"),
        (PackageDeclarationForm::Role, EntityKind::Role, "App::Role"),
    ];
    for (index, (form, kind, name)) in forms.into_iter().enumerate() {
        let package = PackageId::new("project-app", "root-a", name)?;
        let declaration = PackageDeclarationId::new(
            "root-a::lib/App.pm",
            "rev-3",
            "gen-4",
            form,
            Some(format!("name-digest-{name}")),
            u32::try_from(index)?,
        )?;
        let contribution = PackageDeclarationContribution::new(
            package,
            declaration,
            EntityId(u64::try_from(index)? + 2),
            kind,
            Some(PackageDeclarationAnchor::new(format!("decl-{name}"), Some(span(0, 20)))?),
            Some(PackageNameAnchor::new(
                name,
                format!("name-digest-{name}"),
                Some(span(8, 8 + u32::try_from(name.len())?)),
            )?),
            PackageSourceTier::Workspace,
            PackageEditAuthority::ReadOnly,
            SemanticProducer::Parser,
            SemanticProvenance::Known(Provenance::DesugaredAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticSubjectStatus::Complete,
            Vec::new(),
            vec![SemanticDependencyIdentity::new(SemanticDependencyKind::NamedFact, "fact-1")?],
        )?;
        round_trip_contribution(&contribution)?;
    }
    Ok(())
}

#[test]
fn recovered_contribution_without_name_span_round_trips() -> Result<(), Box<dyn Error>> {
    let contribution = PackageDeclarationContribution::new(
        PackageId::new("project-app", "root-a", "Foo")?,
        PackageDeclarationId::new(
            "root-a::lib/Foo.pm",
            "rev-1",
            "gen-1",
            PackageDeclarationForm::PackageStatement,
            None,
            0,
        )?,
        EntityId(5),
        EntityKind::Package,
        Some(PackageDeclarationAnchor::new("decl-recovered", None)?),
        None,
        PackageSourceTier::Generated,
        PackageEditAuthority::NotEditable,
        SemanticProducer::FrameworkAdapter,
        SemanticProvenance::Known(Provenance::FrameworkSynthesis),
        SemanticConfidence::Known(Confidence::Low),
        SemanticSubjectStatus::PartialRecovered,
        vec!["missing exact name span".to_string()],
        Vec::new(),
    )?;
    round_trip_contribution(&contribution)?;
    Ok(())
}

#[test]
fn forged_blank_dependency_target_deserializes_and_fails_validate() -> Result<(), Box<dyn Error>> {
    let contribution = PackageDeclarationContribution::new(
        PackageId::new("project-app", "root-a", "Foo")?,
        PackageDeclarationId::new(
            "root-a::lib/Foo.pm",
            "rev-1",
            "gen-1",
            PackageDeclarationForm::PackageStatement,
            Some("name-digest-Foo".to_string()),
            0,
        )?,
        EntityId(5),
        EntityKind::Package,
        Some(PackageDeclarationAnchor::new("decl-Foo", Some(span(0, 20)))?),
        Some(PackageNameAnchor::new("Foo", "name-digest-Foo", Some(span(8, 11)))?),
        PackageSourceTier::Workspace,
        PackageEditAuthority::ReadOnly,
        SemanticProducer::Parser,
        SemanticProvenance::Known(Provenance::DesugaredAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        vec![SemanticDependencyIdentity::new(SemanticDependencyKind::NamedFact, "fact-1")?],
    )?;
    let encoded = serde_json::to_string(&contribution)?;
    let mut forged: serde_json::Value = serde_json::from_str(&encoded)?;
    forged["dependencies"][0]["target_fingerprint"] = serde_json::json!(" ");
    let decoded: PackageDeclarationContribution = serde_json::from_value(forged)?;
    assert!(decoded.validate().is_err());
    Ok(())
}
