//! Integration tests for META.json/META.yml reconciliation (#7176).
#![expect(
    clippy::unwrap_used,
    reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]

use std::path::PathBuf;

use perl_workspace_core::{
    DistMetadataAgreement, DistMetadataSource, FactClasses, ProjectModelRequest,
    build_project_model, compare_dist_metadata, parse_meta_yml,
};
use perl_workspace_core::{MetaYmlParseState, MetaYmlSpecFindingKind};

fn build(dir: &str, files: &[(&str, &str)]) -> perl_workspace_core::ProjectModel {
    let root: PathBuf = std::env::temp_dir().join(format!("pwc-meta-reconcile-{dir}"));
    let _ = std::fs::remove_dir_all(&root);
    for (rel, content) in files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
    }
    let model = build_project_model(&ProjectModelRequest {
        root: root.to_str().unwrap(),
        fact_classes: FactClasses::FILES | FactClasses::DIST,
    })
    .unwrap();
    let _ = std::fs::remove_dir_all(&root);
    model
}

#[test]
fn equivalent_meta_json_and_yml_compare_as_agree() {
    let model = build(
        "agree",
        &[
            (
                "META.json",
                r#"{"name":"Foo-Bar","version":"1.23","license":["perl_5"],"abstract":"x",
                "prereqs":{"runtime":{"requires":{"Moo":"2.0"}}}}"#,
            ),
            (
                "META.yml",
                "---\nname: Foo-Bar\nversion: '1.23'\nabstract: x\nlicense:\n  - perl_5\nprereqs:\n  runtime:\n    requires:\n      Moo: '2.0'\n",
            ),
        ],
    );
    let compared = model.compare_meta_json_with_meta_yml();
    assert!(compared.iter().all(|row| row.agreement == DistMetadataAgreement::Agree));
    assert!(compared.iter().any(|row| row.field == "name"));
    assert!(compared.iter().any(|row| row.field.starts_with("prereq:")));
}

#[test]
fn meta_name_disagreement_is_explicit() {
    let model = build(
        "disagree-name",
        &[
            ("META.json", r#"{"name":"From-Json","version":"1","license":["perl_5"]}"#),
            ("META.yml", "name: From-Yml\nversion: '1'\nlicense: perl_5\n"),
        ],
    );
    let compared = model.compare_meta_json_with_meta_yml();
    let name = compared.iter().find(|row| row.field == "name").unwrap();
    assert_eq!(name.agreement, DistMetadataAgreement::Disagree);
    assert_eq!(name.left_source, DistMetadataSource::MetaJson);
    assert_eq!(name.right_source, DistMetadataSource::MetaYml);
}

#[test]
fn compare_skips_when_only_one_staged_metadata_source_exists() {
    let model =
        build("json-only", &[("META.json", r#"{"name":"X","version":"1","license":["perl_5"]}"#)]);
    assert!(model.compare_meta_json_with_meta_yml().is_empty());
}

#[test]
fn parse_and_spec_conformance_are_independently_observable() {
    use perl_workspace_core::id::{Digest, FileId};
    let file_id = FileId::new("META.yml", &Digest::of("x"));
    let outcome = parse_meta_yml(
        file_id,
        "meta-spec:\n  version: 2\nname: Foo-Bar\nversion: '1.0'\nlicense:\n  - perl_5\n",
    );
    assert_eq!(outcome.state, MetaYmlParseState::Parsed);
    assert!(outcome.spec_validation.recognized_spec);
    assert!(!outcome.spec_validation.conforms_to_supported_spec);
    assert!(
        outcome
            .spec_validation
            .findings
            .iter()
            .any(|f| f.kind == MetaYmlSpecFindingKind::MissingRequiredField)
    );
}

#[test]
fn direct_compare_helper_matches_model_surface() {
    let json = perl_workspace_core::DistMetadataFacts {
        file_id: perl_workspace_core::id::FileId::new(
            "META.json",
            &perl_workspace_core::id::Digest::of("j"),
        ),
        source: DistMetadataSource::MetaJson,
        name: Some("A".to_string()),
        version: Some("1".to_string()),
        summary: None,
        licenses: vec!["perl_5".to_string()],
        prereqs: vec![],
    };
    let yml = perl_workspace_core::DistMetadataFacts {
        file_id: perl_workspace_core::id::FileId::new(
            "META.yml",
            &perl_workspace_core::id::Digest::of("y"),
        ),
        source: DistMetadataSource::MetaYml,
        name: Some("B".to_string()),
        version: Some("1".to_string()),
        summary: None,
        licenses: vec!["perl_5".to_string()],
        prereqs: vec![],
    };
    let rows = compare_dist_metadata(&json, &yml);
    assert_eq!(
        rows.iter().find(|r| r.field == "name").unwrap().agreement,
        DistMetadataAgreement::Disagree
    );
}
