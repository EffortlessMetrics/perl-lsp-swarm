//! Integration tests for PR 7: distribution-metadata facts via the builder.
#![expect(
    clippy::unwrap_used,
    reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use std::path::PathBuf;

use perl_workspace_core::{FactClasses, ProjectModelRequest, build_project_model};

fn build(
    dir: &str,
    files: &[(&str, &str)],
    classes: FactClasses,
) -> perl_workspace_core::ProjectModel {
    let root: PathBuf = std::env::temp_dir().join(format!("pwc-dist-{dir}"));
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
        fact_classes: classes,
    })
    .unwrap();
    let _ = std::fs::remove_dir_all(&root);
    model
}

#[test]
fn builder_extracts_meta_json_facts() {
    let model = build(
        "meta",
        &[(
            "META.json",
            r#"{"name":"Foo-Bar","version":"1.23","license":["perl_5"],
                "prereqs":{"runtime":{"requires":{"Moo":"2.0"}}}}"#,
        )],
        FactClasses::FILES | FactClasses::DIST,
    );
    let facts = model.dist_metadata.iter().find(|d| d.name.as_deref() == Some("Foo-Bar")).unwrap();
    assert_eq!(facts.version.as_deref(), Some("1.23"));
    assert_eq!(facts.licenses, vec!["perl_5"]);
    assert!(facts.prereqs.iter().any(|p| p.module == "Moo"));
    assert!(model.all_prereqs().iter().any(|p| p.module == "Moo"));
}

#[test]
fn builder_retains_meta_v1_configure_build_and_runtime_phases() {
    let model = build(
        "meta-v1-phases",
        &[(
            "META.json",
            r#"{
                "configure_requires": {"ExtUtils::MakeMaker": "6.64"},
                "build_requires": {"Test::More": "0.88"},
                "requires": {"Carp": "0"}
            }"#,
        )],
        FactClasses::FILES | FactClasses::DIST,
    );
    let facts = model
        .dist_metadata
        .iter()
        .find(|d| d.source == perl_workspace_core::DistMetadataSource::MetaJson)
        .unwrap();

    let mapped = facts
        .prereqs
        .iter()
        .map(|p| (p.module.as_str(), p.phase.as_str(), p.relation.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        mapped,
        vec![
            ("Test::More", "build", "requires"),
            ("ExtUtils::MakeMaker", "configure", "requires"),
            ("Carp", "runtime", "requires"),
        ],
        "builder -> DistMetadata -> META.json extraction preserves all canonical phases"
    );
}

#[test]
fn builder_v2_prereqs_suppress_flat_v1_fallback() {
    let model = build(
        "meta-v2-precedence",
        &[(
            "META.json",
            r#"{
                "prereqs": {"runtime": {"requires": {"V2::Only": "1"}}},
                "configure_requires": {"V1::Only": "1"},
                "requires": {"V1::Runtime": "1"}
            }"#,
        )],
        FactClasses::FILES | FactClasses::DIST,
    );
    let facts = model
        .dist_metadata
        .iter()
        .find(|d| d.source == perl_workspace_core::DistMetadataSource::MetaJson)
        .unwrap();

    assert_eq!(facts.prereqs.len(), 1);
    assert_eq!(facts.prereqs[0].module, "V2::Only");
    assert!(!facts.prereqs.iter().any(|p| p.module.starts_with("V1::")));
}

#[test]
fn builder_malformed_v2_maps_fall_back_without_fabricated_facts() {
    let model = build(
        "meta-malformed-v2",
        &[(
            "META.json",
            r#"{
                "prereqs": {
                    "runtime": {"requires": {"Not::A::Version": []}},
                    "test": "not a relation map"
                },
                "configure_requires": {"V1::Only": "1"},
                "build_requires": "not a module map",
                "requires": {"V1::Runtime": "1"}
            }"#,
        )],
        FactClasses::FILES | FactClasses::DIST,
    );
    let facts = model
        .dist_metadata
        .iter()
        .find(|d| d.source == perl_workspace_core::DistMetadataSource::MetaJson)
        .unwrap();

    assert_eq!(
        facts.prereqs.iter().map(|p| p.module.as_str()).collect::<Vec<_>>(),
        vec!["V1::Only", "V1::Runtime"]
    );
    assert!(!facts.prereqs.iter().any(|p| p.module == "Not::A::Version"));
}

#[test]
fn builder_extracts_cpanfile_facts() {
    let model = build(
        "cpan",
        &[("cpanfile", "requires 'Path::Tiny', '0.100';\ntest_requires 'Test::More';\n")],
        FactClasses::FILES | FactClasses::DIST,
    );
    let facts = model.dist_metadata.iter().find(|d| d.file_id.as_str().contains("fnv64")).unwrap();
    assert!(facts.prereqs.iter().any(|p| p.module == "Path::Tiny" && p.phase == "runtime"));
    assert!(facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"));
}

#[test]
fn dist_facts_absent_when_not_requested() {
    let model =
        build("no-dist", &[("META.json", r#"{"name":"X","version":"1"}"#)], FactClasses::FILES);
    // The file is still indexed…
    assert!(model.file_by_path("META.json").is_some());
    // …but its content is not parsed into dist facts.
    assert!(model.dist_metadata.is_empty(), "DIST not requested → no dist facts");
}
