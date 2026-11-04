//! Fixtures and negative controls for the semantic identity contract (#12121).

use super::SemanticIdentityContractError;
use super::contribution::{
    SemanticContributionId, SemanticContributionOwner, SemanticDeclarationKey,
    SemanticDependencyIdentity, SemanticDependencyKind, SemanticFactFamily,
    SemanticOwnershipDisposition, SemanticSubjectStatus,
};
use super::scope::{
    SemanticAnchorRole, SemanticScopeIdentity, SemanticScopeKind, SemanticScopeRecovery,
    SemanticSemanticProfileIdentity, SemanticSourceAnchor, SemanticSourceOrderIdentity,
    SemanticSubjectGeneration,
};
use super::work::{
    SemanticInstrumentBudgetState, SemanticProducerStrategyIdentity, SemanticWorkSubjectIdentity,
};

type FixtureResult = Result<(), SemanticIdentityContractError>;

fn subject(
    logical_source_id: &str,
    source_generation: &str,
) -> Result<SemanticSubjectGeneration, SemanticIdentityContractError> {
    let profile = SemanticSemanticProfileIdentity::new("profile-a", "digest-a")?;
    SemanticSubjectGeneration::new(
        logical_source_id,
        source_generation,
        "parser-snapshot-1",
        "parser-config-1",
        profile,
    )
}

fn file_scope(
    subject_gen: SemanticSubjectGeneration,
) -> Result<SemanticScopeIdentity, SemanticIdentityContractError> {
    SemanticScopeIdentity::new(
        subject_gen,
        SemanticScopeKind::File,
        None,
        None,
        SemanticSourceAnchor::new(SemanticAnchorRole::Header, "file-header", 0)?,
        None,
        SemanticScopeRecovery::Exact,
    )
}

fn child_scope(
    subject_gen: SemanticSubjectGeneration,
    parent: &SemanticScopeIdentity,
    anchor_digest: &str,
    sibling_ordinal: u32,
) -> Result<SemanticScopeIdentity, SemanticIdentityContractError> {
    SemanticScopeIdentity::new(
        subject_gen,
        SemanticScopeKind::NamedSubroutine,
        Some(SemanticDeclarationKey::new("sub", anchor_digest, format!("digest-{anchor_digest}"))?),
        Some(parent.fingerprint()),
        SemanticSourceAnchor::new(SemanticAnchorRole::Header, anchor_digest, sibling_ordinal)?,
        None,
        SemanticScopeRecovery::Exact,
    )
}

/// Inserting an unrelated earlier scope preserves an unaffected logical
/// identity. The property is structural: a child's fingerprint folds only
/// its own inputs (subject, kind, declaration key, parent fingerprint,
/// anchor digest + ordinal, package context, recovery), and no parent
/// fingerprint folds its descendant set or construction order. The fixture
/// models the insertion explicitly: an earlier `prepare` sibling exists in
/// the after-state, and `cleanup`'s fingerprint is identical whether it is
/// constructed alone, before, or after the sibling, in any order.
#[test]
fn unrelated_earlier_insertion_preserves_unaffected_identity() -> FixtureResult {
    let subject_gen = subject("doc-1", "gen-1")?;
    let file = file_scope(subject_gen.clone())?;
    let parent_before = file.fingerprint();
    let cleanup_alone = child_scope(subject_gen.clone(), &file, "cleanup", 0)?;
    let prepare = child_scope(subject_gen.clone(), &file, "prepare", 0)?;
    // Constructing the earlier sibling does not move the parent fingerprint
    // (descendant set never enters any fingerprint) ...
    let parent_after = file_scope(subject_gen.clone())?.fingerprint();
    assert_eq!(parent_before, parent_after);
    // ... and `cleanup`'s identity is order-independent with respect to the
    // sibling: same fingerprint whether built alone or after `prepare`.
    let cleanup_after_insertion = child_scope(subject_gen, &file, "cleanup", 0)?;
    assert_eq!(cleanup_alone.fingerprint(), cleanup_after_insertion.fingerprint());
    assert_ne!(cleanup_alone.fingerprint(), prepare.fingerprint());
    Ok(())
}

/// Source-identical later generations remain distinct subjects.
#[test]
fn source_identical_later_generation_remains_distinct() -> FixtureResult {
    let a = file_scope(subject("doc-1", "gen-1")?)?;
    let b = file_scope(subject("doc-1", "gen-2")?)?;
    assert_ne!(a.fingerprint(), b.fingerprint());
    assert_ne!(a, b);
    Ok(())
}

/// Close/reopen of the same URI/bytes is a distinct document instance.
#[test]
fn close_reopen_instances_remain_distinct() -> FixtureResult {
    let a = file_scope(subject("doc-instance-1", "gen-1")?)?;
    let b = file_scope(subject("doc-instance-2", "gen-1")?)?;
    assert_ne!(a.fingerprint(), b.fingerprint());
    Ok(())
}

/// The same relative path/content in two roots is distinct.
#[test]
fn multi_root_same_content_remains_distinct() -> FixtureResult {
    let a = file_scope(subject("root-1::lib/App.pm", "gen-1")?)?;
    let b = file_scope(subject("root-2::lib/App.pm", "gen-1")?)?;
    assert_ne!(a.fingerprint(), b.fingerprint());
    Ok(())
}

/// Same-name/same-anchor siblings do not collapse.
#[test]
fn same_name_siblings_do_not_collapse() -> FixtureResult {
    let subject_gen = subject("doc-1", "gen-1")?;
    let file = file_scope(subject_gen.clone())?;
    let first = child_scope(subject_gen.clone(), &file, "helper", 0)?;
    let second = child_scope(subject_gen, &file, "helper", 1)?;
    assert_ne!(first.fingerprint(), second.fingerprint());
    Ok(())
}

/// Fingerprints are deterministic under input/map-order permutation: related
/// anchors and dependencies fold in canonical order.
#[test]
fn fingerprints_deterministic_under_order_permutation() -> FixtureResult {
    let subject_gen = subject("doc-1", "gen-1")?;
    let dep_b = SemanticDependencyIdentity::new(SemanticDependencyKind::PackageState, "pkg-b")?;
    let dep_a = SemanticDependencyIdentity::new(SemanticDependencyKind::Declaration, "decl-a")?;
    let owner_one = SemanticContributionOwner::new(
        subject_gen.clone(),
        SemanticOwnershipDisposition::FileGlobalOwned,
        SemanticFactFamily::PackageFact,
        "primary",
        vec!["related-b".to_string(), "related-a".to_string()],
        SemanticSubjectStatus::Complete,
        vec![dep_b.clone(), dep_a.clone()],
        Vec::new(),
    )?;
    let owner_two = SemanticContributionOwner::new(
        subject_gen,
        SemanticOwnershipDisposition::FileGlobalOwned,
        SemanticFactFamily::PackageFact,
        "primary",
        vec!["related-a".to_string(), "related-b".to_string()],
        SemanticSubjectStatus::Complete,
        vec![dep_a, dep_b],
        Vec::new(),
    )?;
    assert_eq!(owner_one.owner_fingerprint(), owner_two.owner_fingerprint());
    let id_one = owner_one.contribution_id(3)?;
    let id_two = owner_two.contribution_id(3)?;
    assert_eq!(id_one.fingerprint(), id_two.fingerprint());
    Ok(())
}

/// Producer identity never upgrades completeness: an unsupported/not-proven
/// owner cannot claim complete status, with or without a famous producer.
#[test]
fn producer_identity_never_upgrades_completeness() -> FixtureResult {
    let owner = SemanticContributionOwner::new(
        subject("doc-1", "gen-1")?,
        SemanticOwnershipDisposition::UnsupportedNotProven {
            reason: "dynamic construct".to_string(),
        },
        SemanticFactFamily::DynamicLimitation,
        "primary",
        Vec::new(),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        Vec::new(),
    );
    assert!(owner.is_err());
    Ok(())
}

/// A complete contribution must record no limitations.
#[test]
fn complete_status_rejects_limitations() -> FixtureResult {
    let owner = SemanticContributionOwner::new(
        subject("doc-1", "gen-1")?,
        SemanticOwnershipDisposition::FileGlobalOwned,
        SemanticFactFamily::PackageFact,
        "primary",
        Vec::new(),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        vec!["recovered region".to_string()],
    );
    assert!(owner.is_err());
    Ok(())
}

/// Scope-local fact families require a scope-owned disposition; package
/// transitions are not forced into a lexical-scope bucket.
#[test]
fn scope_local_family_requires_scope_owner() -> FixtureResult {
    let owner = SemanticContributionOwner::new(
        subject("doc-1", "gen-1")?,
        SemanticOwnershipDisposition::FileGlobalOwned,
        SemanticFactFamily::ScopeLocalDeclaration,
        "primary",
        Vec::new(),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        Vec::new(),
    );
    assert!(owner.is_err());
    Ok(())
}

/// Package-statement scopes require a package/source-order context.
#[test]
fn package_scope_requires_source_order_context() -> FixtureResult {
    let subject_gen = subject("doc-1", "gen-1")?;
    let file = file_scope(subject_gen.clone())?;
    let anchor = SemanticSourceAnchor::new(SemanticAnchorRole::ContextMarker, "package App;", 0)?;
    let scope = SemanticScopeIdentity::new(
        subject_gen,
        SemanticScopeKind::PackageStatement,
        Some(SemanticDeclarationKey::new("package", "App", "package App;")?),
        Some(file.fingerprint()),
        anchor.clone(),
        None,
        SemanticScopeRecovery::Exact,
    );
    assert!(scope.is_err()); // fail-closed at construction
    let with_context = SemanticScopeIdentity::new(
        subject("doc-1", "gen-1")?,
        SemanticScopeKind::PackageStatement,
        Some(SemanticDeclarationKey::new("package", "App", "package App;")?),
        Some(file.fingerprint()),
        anchor,
        Some(SemanticSourceOrderIdentity::new(0, "package App;")?),
        SemanticScopeRecovery::Exact,
    )?;
    assert!(with_context.validate().is_ok());
    Ok(())
}

/// Empty identity fields fail closed.
#[test]
fn empty_identity_fields_fail_closed() -> FixtureResult {
    assert!(SemanticSemanticProfileIdentity::new("", "digest").is_err());
    let valid_profile = SemanticSemanticProfileIdentity::new("p", "d")?;
    assert!(SemanticSubjectGeneration::new("", "g", "p", "c", valid_profile.clone()).is_err());
    assert!(SemanticSubjectGeneration::new("s", "", "p", "c", valid_profile.clone()).is_err());
    assert!(SemanticSubjectGeneration::new("s", "g", "", "c", valid_profile.clone()).is_err());
    assert!(SemanticSubjectGeneration::new("s", "g", "p", "", valid_profile).is_err());
    assert!(SemanticSourceAnchor::new(SemanticAnchorRole::Header, "  ", 0).is_err());
    assert!(SemanticSourceOrderIdentity::new(0, "").is_err());
    assert!(SemanticDependencyIdentity::new(SemanticDependencyKind::NamedFact, "").is_err());
    assert!(SemanticContributionId::new("", SemanticFactFamily::PackageFact, "a", 0).is_err());
    assert!(SemanticContributionId::new("owner", SemanticFactFamily::PackageFact, "", 0).is_err());
    Ok(())
}

/// A non-file scope without a parent, and a file scope with one, both fail.
#[test]
fn parent_rules_enforced() -> FixtureResult {
    let anchor = SemanticSourceAnchor::new(SemanticAnchorRole::Header, "h", 0)?;
    assert!(
        SemanticScopeIdentity::new(
            subject("doc-1", "gen-1")?,
            SemanticScopeKind::LexicalBlock,
            None,
            None,
            anchor.clone(),
            None,
            SemanticScopeRecovery::Exact,
        )
        .is_err()
    );
    assert!(
        SemanticScopeIdentity::new(
            subject("doc-1", "gen-1")?,
            SemanticScopeKind::File,
            None,
            Some("parent".to_string()),
            anchor,
            None,
            SemanticScopeRecovery::Exact,
        )
        .is_err()
    );
    Ok(())
}

/// Duplicate requested fact families fail the work-subject contract.
#[test]
fn duplicate_fact_families_rejected() -> FixtureResult {
    let producer = SemanticProducerStrategyIdentity::new("semantic-analyzer", "fresh-full")?;
    let result = SemanticWorkSubjectIdentity::new(
        subject("doc-1", "gen-1")?,
        producer,
        vec![SemanticFactFamily::PackageFact, SemanticFactFamily::PackageFact],
        1,
        1,
        SemanticInstrumentBudgetState::Nominal,
    );
    assert!(result.is_err());
    Ok(())
}

/// Identities round-trip through JSON with schema tags intact.
#[test]
fn identity_round_trips_json() -> Result<(), Box<dyn std::error::Error>> {
    let scope = file_scope(subject("doc-1", "gen-1")?)?;
    let encoded = serde_json::to_string(&scope)?;
    let decoded: SemanticScopeIdentity = serde_json::from_str(&encoded)?;
    assert_eq!(decoded, scope);
    assert_eq!(decoded.schema().tag(), "semantic-identity-v1");
    Ok(())
}

/// Separator-like component content cannot shift a field boundary: two
/// structurally different subjects whose flat concatenations would coincide
/// keep distinct fingerprints because every component is framed by its own
/// length and label.
#[test]
fn separator_content_cannot_shift_field_boundaries() -> FixtureResult {
    let a = file_scope(subject("doc\u{1e}split", "gen")?)?;
    let b = file_scope(subject("doc", "split\u{1e}gen")?)?;
    assert_ne!(a.fingerprint(), b.fingerprint());

    let dep_x = SemanticDependencyIdentity::new(SemanticDependencyKind::PackageState, "x\u{1e}y")?;
    let dep_y = SemanticDependencyIdentity::new(SemanticDependencyKind::PackageState, "x")?;
    let owner_x = SemanticContributionOwner::new(
        subject("doc-1", "gen-1")?,
        SemanticOwnershipDisposition::FileGlobalOwned,
        SemanticFactFamily::PackageFact,
        "primary",
        Vec::new(),
        SemanticSubjectStatus::Complete,
        vec![dep_x.clone()],
        Vec::new(),
    )?;
    let owner_y = SemanticContributionOwner::new(
        subject("doc-1", "gen-1")?,
        SemanticOwnershipDisposition::FileGlobalOwned,
        SemanticFactFamily::PackageFact,
        "primary",
        Vec::new(),
        SemanticSubjectStatus::Complete,
        vec![dep_y, dep_x.clone()],
        Vec::new(),
    )?;
    assert_ne!(owner_x.owner_fingerprint(), owner_y.owner_fingerprint());
    Ok(())
}

/// A blank owning-declaration key is rejected at the typed key
/// constructor, so no scope can fingerprint a blank key identically to
/// `None`.
#[test]
fn blank_declaration_key_rejected() -> FixtureResult {
    assert!(SemanticDeclarationKey::new("sub", "   ", "digest").is_err());
    assert!(SemanticDeclarationKey::new("", "name", "digest").is_err());
    assert!(SemanticDeclarationKey::new("sub", "name", " ").is_err());
    Ok(())
}

/// Duplicate related anchors and duplicate dependencies are rejected.
#[test]
fn duplicate_owner_relations_rejected() -> FixtureResult {
    let dup_dep = SemanticDependencyIdentity::new(SemanticDependencyKind::PackageState, "pkg")?;
    let distinct_dep = SemanticDependencyIdentity::new(SemanticDependencyKind::NamedFact, "pkg")?;
    assert!(
        SemanticContributionOwner::new(
            subject("doc-1", "gen-1")?,
            SemanticOwnershipDisposition::FileGlobalOwned,
            SemanticFactFamily::PackageFact,
            "primary",
            vec!["x".to_string(), "x".to_string()],
            SemanticSubjectStatus::Complete,
            Vec::new(),
            Vec::new(),
        )
        .is_err()
    );
    assert!(
        SemanticContributionOwner::new(
            subject("doc-1", "gen-1")?,
            SemanticOwnershipDisposition::FileGlobalOwned,
            SemanticFactFamily::PackageFact,
            "primary",
            Vec::new(),
            SemanticSubjectStatus::Complete,
            vec![dup_dep.clone(), dup_dep.clone()],
            Vec::new(),
        )
        .is_err()
    );
    assert!(
        SemanticContributionOwner::new(
            subject("doc-1", "gen-1")?,
            SemanticOwnershipDisposition::FileGlobalOwned,
            SemanticFactFamily::PackageFact,
            "primary",
            Vec::new(),
            SemanticSubjectStatus::Complete,
            vec![dup_dep.clone(), distinct_dep],
            Vec::new(),
        )
        .is_ok()
    );
    Ok(())
}

/// An unsupported/not-proven owner yields no reusable contribution identity.
#[test]
fn unsupported_owner_has_no_contribution_identity() -> FixtureResult {
    let owner = SemanticContributionOwner::new(
        subject("doc-1", "gen-1")?,
        SemanticOwnershipDisposition::UnsupportedNotProven {
            reason: "dynamic construct".to_string(),
        },
        SemanticFactFamily::DynamicLimitation,
        "primary",
        Vec::new(),
        SemanticSubjectStatus::NotProven,
        Vec::new(),
        Vec::new(),
    )?;
    assert!(owner.contribution_id(0).is_err());
    Ok(())
}

/// Fact-family ownership classes are exercised directly: scope-local
/// families and only those report `is_scope_local`, so later invalidation
/// policy cannot misfile a family class.
#[test]
fn fact_family_scope_local_classification() {
    for family in [
        SemanticFactFamily::ScopeLocalDeclaration,
        SemanticFactFamily::ScopeLocalReference,
        SemanticFactFamily::ScopeLocalToken,
        SemanticFactFamily::HoverFact,
    ] {
        assert!(family.is_scope_local(), "{:?} must be scope-local", family);
    }
    for family in [
        SemanticFactFamily::PackageFact,
        SemanticFactFamily::ExportFact,
        SemanticFactFamily::PragmaFact,
        SemanticFactFamily::ImportFact,
        SemanticFactFamily::PrototypeFact,
        SemanticFactFamily::FeatureFact,
        SemanticFactFamily::ClassInheritanceFact,
        SemanticFactFamily::GeneratedMemberFact,
        SemanticFactFamily::DataSectionFact,
        SemanticFactFamily::SourceBoundaryFact,
        SemanticFactFamily::DynamicLimitation,
        SemanticFactFamily::RecoveryLimitation,
    ] {
        assert!(!family.is_scope_local(), "{:?} must not be scope-local", family);
    }
}

/// Contribution-id construction binds the family and ordinal it was given.
#[test]
fn contribution_id_binds_family_and_ordinal() -> FixtureResult {
    let id = SemanticContributionId::new(
        "owner-fingerprint",
        SemanticFactFamily::ScopeLocalToken,
        "anchor",
        7,
    )?;
    assert_eq!(id.fact_family(), SemanticFactFamily::ScopeLocalToken);
    let same_args = SemanticContributionId::new(
        "owner-fingerprint",
        SemanticFactFamily::ScopeLocalToken,
        "anchor",
        7,
    )?;
    assert_eq!(id.fingerprint(), same_args.fingerprint());
    let other_ordinal = SemanticContributionId::new(
        "owner-fingerprint",
        SemanticFactFamily::ScopeLocalToken,
        "anchor",
        8,
    )?;
    assert_ne!(id.fingerprint(), other_ordinal.fingerprint());
    Ok(())
}

/// Deserialization is a wire shape, not an invariant guard: untrusted JSON
/// can construct records `new()` rejects, and post-transport `validate()`
/// must catch them before reuse.
#[test]
fn deserialized_invalid_records_fail_validation() -> Result<(), Box<dyn std::error::Error>> {
    let valid = SemanticContributionOwner::new(
        subject("doc-1", "gen-1")?,
        SemanticOwnershipDisposition::FileGlobalOwned,
        SemanticFactFamily::PackageFact,
        "primary",
        Vec::new(),
        SemanticSubjectStatus::Complete,
        Vec::new(),
        Vec::new(),
    )?;
    let encoded = serde_json::to_string(&valid)?;
    let mut forged: serde_json::Value = serde_json::from_str(&encoded)?;
    // Forge `complete` while smuggling a limitation past the constructor.
    forged["limitations"] = serde_json::json!(["recovered region"]);
    let decoded: SemanticContributionOwner = serde_json::from_value(forged)?;
    assert!(decoded.status().is_complete());
    assert!(decoded.validate().is_err());
    Ok(())
}

/// Architecture fence: the lower identity model carries no LSP, parser, or
/// provider types, and never uses a traversal-order `ScopeId(` constructor.
#[test]
fn architecture_fence() -> std::io::Result<()> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/semantic_identity");
    let mut checked = 0u32;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        // The fence protects the lower (non-test) identity model; test code
        // necessarily names the forbidden tokens it checks for.
        if path.file_stem().and_then(|s| s.to_str()) == Some("tests") {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        for forbidden in ["lsp_types", "perl_parser_core", "perl_lsp", "Uri", "Url", "ScopeId("] {
            assert!(
                !text.contains(forbidden),
                "{} must not appear in {}",
                forbidden,
                path.display()
            );
        }
        checked += 1;
    }
    assert!(checked >= 5, "fence must cover every module file (checked {checked})");
    Ok(())
}
