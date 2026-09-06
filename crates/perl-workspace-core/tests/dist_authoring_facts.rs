//! Discriminating proof for #7177: bounded Makefile.PL / Build.PL / dist.ini facts.
#![expect(
    clippy::unwrap_used,
    reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]
#![deny(clippy::map_err_ignore)]

use std::path::PathBuf;

use perl_workspace_core::{Digest, FileId};
use perl_workspace_core::{
    DistAuthoringBuildTool, DistAuthoringSource, DistFactAgreement, DistMetadataSource,
    FactClasses, ProjectModelRequest, build_project_model, compare_authoring_with_meta,
    parse_build_pl, parse_dist_ini, parse_makefile_pl,
};

fn fid(path: &str, content: &str) -> FileId {
    FileId::new(path, &Digest::of(content))
}

fn build(
    dir: &str,
    files: &[(&str, &str)],
    classes: FactClasses,
) -> perl_workspace_core::ProjectModel {
    let root: PathBuf = std::env::temp_dir().join(format!("pwc-authoring-{dir}"));
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

const MAKEFILE: &str = r#"
use strict;
use warnings;
use ExtUtils::MakeMaker;
WriteMakefile(
    NAME             => 'Foo::Bar',
    VERSION          => '1.23',
    ABSTRACT         => "does foo to bar",
    LICENSE          => 'perl',
    PREREQ_PM        => {
        'Moo' => '2.0',
        'Carp' => 0,
    },
    CONFIGURE_REQUIRES => {
        'ExtUtils::MakeMaker' => '6.64',
    },
    BUILD_REQUIRES   => {
        'Module::Build' => '0.42',
    },
    TEST_REQUIRES    => {
        'Test::More' => '0.98',
    },
    META_MERGE => {
        resources => {
            homepage   => 'https://example.invalid/foo',
            repository => {
                type => 'git',
                url  => 'https://example.invalid/foo.git',
                web  => 'https://example.invalid/foo',
            },
        },
        provides => {
            'Foo::Bar' => { file => 'lib/Foo/Bar.pm', version => '1.23' },
        },
    },
);
"#;

const BUILD_PL: &str = r#"
use Module::Build;
Module::Build->new(
    module_name        => 'Foo::Bar',
    dist_version       => '1.23',
    dist_abstract      => 'does foo to bar',
    license            => 'perl',
    requires           => { 'Moo' => '2.0' },
    configure_requires => { 'Module::Build' => '0.42' },
    build_requires     => { 'ExtUtils::CBuilder' => '0' },
    test_requires      => { 'Test::More' => '0.98' },
    recommends         => { 'Path::Tiny' => '0.100' },
    resources          => { homepage => 'https://example.invalid/foo' },
)->create_build_script;
"#;

const DIST_INI: &str = r#"
name     = Foo-Bar
version  = 1.23
abstract = does foo to bar
license  = Perl_5
author   = Example <ex@example.invalid>

[Prereqs]
Moo = 2.0

[Prereqs / TestRequires]
Test::More = 0.98

[Prereqs / RuntimeRecommends]
Path::Tiny = 0.100

[MetaResources]
homepage        = https://example.invalid/foo
repository.url  = https://example.invalid/foo.git
bugtracker.web  = https://example.invalid/foo/issues

[MakeMaker]
"#;

#[test]
fn makefile_pl_extracts_literal_identity_and_v1_prereq_phases() {
    let facts = parse_makefile_pl(fid("Makefile.PL", MAKEFILE), MAKEFILE);
    assert_eq!(facts.source, DistAuthoringSource::MakefilePl);
    assert_eq!(facts.build_tool, DistAuthoringBuildTool::ExtUtilsMakeMaker);
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
    assert_eq!(facts.version.as_deref(), Some("1.23"));
    assert_eq!(facts.summary.as_deref(), Some("does foo to bar"));
    assert_eq!(facts.licenses, vec!["perl_5"]);
    let mapped: Vec<_> = facts
        .prereqs
        .iter()
        .map(|p| (p.module.as_str(), p.phase.as_str(), p.relation.as_str(), p.version.as_deref()))
        .collect();
    assert!(mapped.contains(&("Moo", "runtime", "requires", Some("2.0"))));
    assert!(mapped.contains(&("ExtUtils::MakeMaker", "configure", "requires", Some("6.64"))));
    assert!(mapped.contains(&("Module::Build", "build", "requires", Some("0.42"))));
    assert!(mapped.contains(&("Test::More", "test", "requires", Some("0.98"))));
    assert!(facts.resources.iter().any(|r| r.kind == "homepage"));
    assert!(
        facts
            .resources
            .iter()
            .any(|r| r.kind == "repository" && r.type_name.as_deref() == Some("git"))
    );
    assert!(facts.provides.iter().any(|p| p.module == "Foo::Bar"));
    assert!(!facts.declarations.is_empty());
    assert!(facts.declarations.iter().all(|d| d.range.end_byte >= d.range.start_byte));
}

#[test]
fn makefile_pl_records_version_from_without_following_the_module() {
    let src = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    VERSION_FROM => 'lib/Foo/Bar.pm',
    ABSTRACT_FROM => 'lib/Foo/Bar.pm',
);
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.version_from.as_deref(), Some("lib/Foo/Bar.pm"));
    assert_eq!(facts.abstract_from.as_deref(), Some("lib/Foo/Bar.pm"));
    assert!(facts.version.is_none(), "VERSION_FROM is not executed");
    assert!(facts.limitations.iter().any(|l| l.kind == "version_from"));
    assert!(facts.limitations.iter().any(|l| l.kind == "abstract_from"));
}

#[test]
fn makefile_pl_quote_forms_comments_multiline_and_unicode() {
    let src = "WriteMakefile(\n    NAME => q{Föo::Bar}, # comment with 'quotes'\n    VERSION => \"1.00\",\n    LICENSE => qq{perl},\n);\n";
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Föo-Bar"));
    assert_eq!(facts.version.as_deref(), Some("1.00"));
    assert_eq!(facts.licenses, vec!["perl_5"]);
    assert!(!facts.prereqs.iter().any(|p| p.module == "quotes"));
}

#[test]
fn makefile_pl_helper_variable_and_computed_values_are_limitations() {
    let src = r#"
my $ver = sprintf '%s', 1;
my %WriteMakefileArgs = (
    NAME => 'Foo::Bar',
    VERSION => $ver,
    PREREQ_PM => { 'Moo' => $Moo::VERSION },
);
WriteMakefile(%WriteMakefileArgs);
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
    assert!(facts.version.is_none());
    assert!(facts.prereqs.iter().any(|p| p.module == "Moo" && p.dynamic));
    assert!(facts.limitations.iter().any(|l| l.kind == "dynamic_value"));
}

#[test]
fn makefile_pl_does_not_execute_system_or_eval() {
    let src = r#"
system("touch /tmp/perl-lsp-authoring-should-not-exist");
eval "die 'executed'";
WriteMakefile(NAME => 'Safe::Parse', VERSION => '1');
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Safe-Parse"));
    assert!(facts.limitations.iter().any(|l| l.kind == "executable_construct"));
    assert!(!std::path::Path::new("/tmp/perl-lsp-authoring-should-not-exist").exists());
}

#[test]
fn makefile_pl_skips_native_build_flags() {
    let src = r#"
WriteMakefile(
    NAME => 'Foo::XS',
    INC => '-I/opt/include',
    LIBS => ['-lfoo'],
    DEFINE => '-DFOO',
    OBJECT => 'foo.o',
    PREREQ_PM => { 'XSLoader' => 0 },
);
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert!(
        facts
            .declarations
            .iter()
            .all(|d| { !matches!(d.key.as_str(), "INC" | "LIBS" | "DEFINE" | "OBJECT") })
    );
    assert!(facts.prereqs.iter().any(|p| p.module == "XSLoader"));
}

#[test]
fn makefile_pl_duplicate_and_conflicting_names() {
    let src = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    DISTNAME => 'Other-Name',
    VERSION => '1.0',
    PREREQ_PM => { 'Moo' => '1.0', 'Moo' => '2.0' },
);
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert!(facts.conflicts.iter().any(|c| c.field == "name"));
    assert!(facts.name.is_none());
}

#[test]
fn makefile_pl_malformed_still_recovers_literals() {
    let src = "WriteMakefile(\n    NAME => 'Foo::Bar',\n    VERSION => '1.0'\n";
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
}

#[test]
fn build_pl_extracts_literal_module_build_forms() {
    let facts = parse_build_pl(fid("Build.PL", BUILD_PL), BUILD_PL);
    assert_eq!(facts.source, DistAuthoringSource::BuildPl);
    assert_eq!(facts.build_tool, DistAuthoringBuildTool::ModuleBuild);
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
    assert_eq!(facts.version.as_deref(), Some("1.23"));
    assert!(facts.prereqs.iter().any(|p| p.module == "Moo" && p.phase == "runtime"));
    assert!(facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"));
    assert!(facts.prereqs.iter().any(|p| p.module == "Path::Tiny" && p.relation == "recommends"));
    assert!(facts.resources.iter().any(|r| r.kind == "homepage"));
}

#[test]
fn dist_ini_extracts_root_prereqs_resources_and_plugin_limitations() {
    let src = format!("{DIST_INI}\n[AutoPrereqs]\n[GitHub::Meta]\n");
    let facts = parse_dist_ini(fid("dist.ini", &src), &src);
    assert_eq!(facts.source, DistAuthoringSource::DistIni);
    assert_eq!(facts.build_tool, DistAuthoringBuildTool::DistZilla);
    assert_eq!(facts.generated_build_tool, Some(DistAuthoringBuildTool::ExtUtilsMakeMaker));
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
    assert_eq!(facts.version.as_deref(), Some("1.23"));
    assert_eq!(facts.licenses, vec!["perl_5"]);
    assert!(facts.prereqs.iter().any(|p| p.module == "Moo" && p.phase == "runtime"));
    assert!(facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"));
    assert!(facts.prereqs.iter().any(|p| p.module == "Path::Tiny" && p.relation == "recommends"));
    assert!(facts.resources.iter().any(|r| r.kind == "homepage"));
    assert!(facts.resources.iter().any(|r| r.kind == "repository"));
    assert!(facts.limitations.iter().any(|l| l.kind == "plugin_generated"));
    assert!(facts.plugins.iter().any(|p| p.contains("AutoPrereqs")));
}

#[test]
fn dist_ini_phase_override_and_comments() {
    let src = "[Prereqs]\n-phase = test\n-relationship = requires\nTest::More = 0.88 ; comment\n";
    let facts = parse_dist_ini(fid("dist.ini", src), src);
    let prereq = facts.prereqs.iter().find(|p| p.module == "Test::More").unwrap();
    assert_eq!(prereq.phase, "test");
    assert_eq!(prereq.relation, "requires");
}

#[test]
fn build_pl_records_version_from_without_execution() {
    let src = r#"
Module::Build->new(
    module_name => 'Foo::Bar',
    dist_version_from => 'lib/Foo/Bar.pm',
);
"#;
    let facts = parse_build_pl(fid("Build.PL", src), src);
    assert_eq!(facts.version_from.as_deref(), Some("lib/Foo/Bar.pm"));
    assert!(facts.version.is_none());
}

#[test]
fn fingerprints_are_deterministic() {
    let first = parse_makefile_pl(fid("Makefile.PL", MAKEFILE), MAKEFILE);
    let second = parse_makefile_pl(fid("Makefile.PL", MAKEFILE), MAKEFILE);
    assert_eq!(first.source_fingerprint, second.source_fingerprint);
    assert_eq!(first.fact_fingerprint, second.fact_fingerprint);
    assert_ne!(first.source_fingerprint.as_str(), first.fact_fingerprint.as_str());
}

#[test]
fn authoring_and_meta_stay_separate_and_comparable() {
    let makefile = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    VERSION => '1.00',
    LICENSE => 'perl',
    PREREQ_PM => { 'Moo' => '2.0' },
);
"#;
    let meta = r#"{"name":"Foo-Bar","version":"1.23","license":["perl_5"],
        "prereqs":{"runtime":{"requires":{"Moo":"2.0"}}}}"#;
    let model = build(
        "compare",
        &[("Makefile.PL", makefile), ("META.json", meta)],
        FactClasses::FILES | FactClasses::DIST,
    );
    assert_eq!(model.dist_authoring.len(), 1);
    assert_eq!(model.dist_metadata.len(), 1);
    assert_eq!(model.dist_metadata[0].source, DistMetadataSource::MetaJson);
    assert_eq!(model.dist_authoring[0].source, DistAuthoringSource::MakefilePl);
    assert!(
        !model.all_prereqs().is_empty() && model.all_prereqs().iter().all(|p| p.module == "Moo"),
        "all_prereqs stays on final metadata and does not swallow authoring"
    );
    assert_eq!(
        model.all_prereqs().len(),
        model.dist_metadata[0].prereqs.len(),
        "authoring prereqs must not be merged into all_prereqs"
    );
    let compared = model.compare_authoring_with_metadata();
    assert!(compared.iter().any(|c| c.field == "name" && c.agreement == DistFactAgreement::Agree));
    assert!(
        compared.iter().any(|c| c.field == "version" && c.agreement == DistFactAgreement::Disagree)
    );
    assert!(compared.iter().any(|c| {
        c.field.contains("prereq:runtime:requires:Moo") && c.agreement == DistFactAgreement::Agree
    }));
    let direct = compare_authoring_with_meta(&model.dist_authoring[0], &model.dist_metadata[0]);
    assert_eq!(direct, compared);
}

#[test]
fn builder_extracts_all_three_authoring_sources() {
    let model = build(
        "three",
        &[("Makefile.PL", MAKEFILE), ("Build.PL", BUILD_PL), ("dist.ini", DIST_INI)],
        FactClasses::FILES | FactClasses::DIST,
    );
    let sources: Vec<_> = model.dist_authoring.iter().map(|f| f.source).collect();
    assert!(sources.contains(&DistAuthoringSource::MakefilePl));
    assert!(sources.contains(&DistAuthoringSource::BuildPl));
    assert!(sources.contains(&DistAuthoringSource::DistIni));
}

#[test]
fn authoring_absent_when_dist_not_requested() {
    let model = build("no-dist", &[("Makefile.PL", MAKEFILE)], FactClasses::FILES);
    assert!(model.file_by_path("Makefile.PL").is_some());
    assert!(model.dist_authoring.is_empty());
}

#[test]
fn provenance_is_present_and_honest() {
    let facts = parse_makefile_pl(fid("Makefile.PL", MAKEFILE), MAKEFILE);
    assert_eq!(facts.provenance.producer.name, "perl-workspace-core");
    assert_eq!(facts.provenance.source, perl_workspace_core::EvidenceSource::DistMetadata);
}
