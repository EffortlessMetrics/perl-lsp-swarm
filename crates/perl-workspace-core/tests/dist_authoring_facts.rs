//! Discriminating proof for #7177: bounded Makefile.PL / Build.PL / dist.ini facts.
#![expect(
    clippy::unwrap_used,
    reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]
#![deny(clippy::map_err_ignore)]

use std::path::PathBuf;

use perl_workspace_core::{Confidence, EvidenceSource};
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
    assert!(facts.conflicts.iter().any(|c| c.field == "prereq:runtime:requires:Moo"));
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
fn makefile_pl_meta_add_and_v2_prereq_overlay() {
    let src = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    META_ADD => {
        resources => { homepage => 'https://example.invalid/add' },
        prereqs => { runtime => { requires => { 'JSON::PP' => '4' } } },
    },
);
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert!(facts.resources.iter().any(|r| r.kind == "homepage"));
    assert!(facts.prereqs.iter().any(|p| p.module == "JSON::PP" && p.phase == "runtime"));
}

#[test]
fn makefile_pl_qw_license_list_keeps_every_token() {
    let src =
        "WriteMakefile(\n    NAME => 'Foo::Bar',\n    LICENSE => [qw(perl artistic_2)],\n);\n";
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert!(facts.licenses.iter().any(|l| l == "perl_5"));
    assert!(facts.licenses.iter().any(|l| l == "artistic_2"));
}

#[test]
fn makefile_pl_unbracketed_qw_is_not_a_license_list() {
    let src = "WriteMakefile(\n    NAME => 'Foo::Bar',\n    LICENSE => qw(perl artistic_2),\n);\n";
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert!(
        !facts.licenses.iter().any(|l| l == "artistic_2"),
        "unbracketed qw tokens flatten into WriteMakefile args, they are not extra LICENSE values"
    );
    assert!(facts.limitations.iter().any(|l| l.kind == "dynamic_value"));
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
fn dist_ini_develop_suggests_conflicts_and_unknown_section_label() {
    let src = r#"
[Prereqs / DevelopRequires]
Perl::Critic = 1.140

[Prereqs / RuntimeSuggests]
JSON::PP = 4

[Prereqs / RuntimeConflicts]
Broken::Mod = 0

[Prereqs / NotARealPhase]
Mystery::Mod = 1
"#;
    let facts = parse_dist_ini(fid("dist.ini", src), src);
    assert!(facts.prereqs.iter().any(|p| p.module == "Perl::Critic" && p.phase == "develop"));
    assert!(facts.prereqs.iter().any(|p| p.module == "JSON::PP" && p.relation == "suggests"));
    assert!(facts.prereqs.iter().any(|p| p.module == "Broken::Mod" && p.relation == "conflicts"));
    assert!(facts.limitations.iter().any(|l| l.kind == "unknown_prereq_section"));
    assert!(!facts.prereqs.iter().any(|p| p.module == "Mystery::Mod"));
}

#[test]
fn dist_ini_named_prereqs_instance_honors_phase_controls() {
    let src = r#"
[Prereqs / CustomName]
-phase = test
-relationship = requires
Test::More = 0.88
"#;
    let facts = parse_dist_ini(fid("dist.ini", src), src);
    let prereq = facts.prereqs.iter().find(|p| p.module == "Test::More").unwrap();
    assert_eq!(prereq.phase, "test");
    assert_eq!(prereq.relation, "requires");
    assert!(!facts.limitations.iter().any(|l| l.kind == "unknown_prereq_section"));
}

#[test]
fn dist_ini_named_prereqs_buffers_modules_until_controls() {
    let src = r#"
[Prereqs / CustomName]
Test::More = 0.88
-phase = develop
-relationship = suggests
"#;
    let facts = parse_dist_ini(fid("dist.ini", src), src);
    let prereq = facts.prereqs.iter().find(|p| p.module == "Test::More").unwrap();
    assert_eq!(prereq.phase, "develop");
    assert_eq!(prereq.relation, "suggests");
}

#[test]
fn dist_ini_prereq_controls_apply_to_the_whole_section() {
    let src = r#"
[Prereqs]
Early::Mod = 1
-phase = test
Mid::Mod = 1
-relationship = recommends
Late::Mod = 1
"#;
    let facts = parse_dist_ini(fid("dist.ini", src), src);
    for module in ["Early::Mod", "Mid::Mod", "Late::Mod"] {
        let prereq = facts.prereqs.iter().find(|p| p.module == module).unwrap();
        assert_eq!(prereq.phase, "test", "{module}");
        assert_eq!(prereq.relation, "recommends", "{module}");
    }
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
    assert_eq!(facts.provenance.source, EvidenceSource::Heuristic);
}

#[test]
fn interpolating_double_quotes_are_dynamic_not_static_literals() {
    let src = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    VERSION => "$VERSION",
    ABSTRACT => "cost is $price",
    LICENSE => qq{$lic},
);
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
    assert!(facts.version.is_none(), "VERSION => \"$VERSION\" is interpolation, not a literal");
    assert!(facts.summary.is_none());
    assert!(facts.licenses.is_empty());
    assert!(facts.declarations.iter().any(|d| d.key == "VERSION" && d.dynamic));
    assert!(facts.limitations.iter().any(|l| l.kind == "dynamic_value"));
}

#[test]
fn single_quoted_and_escaped_sigils_stay_static() {
    let src = r#"
WriteMakefile(
    NAME => 'Foo$Bar',
    VERSION => "1.00",
    LICENSE => q{$not_interpolated},
);
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Foo$Bar"));
    assert_eq!(facts.version.as_deref(), Some("1.00"));
    assert_eq!(facts.licenses, vec!["$not_interpolated"]);
}

#[test]
fn escaped_dollar_in_double_quotes_is_static() {
    let src = "WriteMakefile(\n    NAME => 'Foo::Bar',\n    VERSION => \"\\$VERSION\",\n);\n";
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.version.as_deref(), Some("$VERSION"));
}

#[test]
fn comparison_limits_only_the_dynamic_field() {
    let makefile = r#"
WriteMakefile(
    NAME => "$pkg",
    VERSION => '1.23',
    LICENSE => 'perl',
    PREREQ_PM => { 'Moo' => '2.0' },
);
"#;
    let meta = r#"{"name":"Foo-Bar","version":"1.23","license":["perl_5"],
        "prereqs":{"runtime":{"requires":{"Moo":"2.0"}}}}"#;
    let model = build(
        "field-limit-name",
        &[("Makefile.PL", makefile), ("META.json", meta)],
        FactClasses::FILES | FactClasses::DIST,
    );
    let compared = model.compare_authoring_with_metadata();
    assert!(
        compared.iter().any(|c| c.field == "name" && c.agreement == DistFactAgreement::Limited)
    );
    assert!(
        compared.iter().any(|c| c.field == "version" && c.agreement == DistFactAgreement::Agree)
    );
    assert!(
        compared.iter().any(|c| c.field == "license" && c.agreement == DistFactAgreement::Agree)
    );
}

#[test]
fn license_only_dynamic_does_not_limit_name_or_version() {
    let makefile = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    VERSION => '1.23',
    LICENSE => $lic,
);
"#;
    let meta = r#"{"name":"Foo-Bar","version":"1.23","license":["perl_5"]}"#;
    let model = build(
        "field-limit-license",
        &[("Makefile.PL", makefile), ("META.json", meta)],
        FactClasses::FILES | FactClasses::DIST,
    );
    let compared = model.compare_authoring_with_metadata();
    assert!(
        compared.iter().any(|c| c.field == "license" && c.agreement == DistFactAgreement::Limited)
    );
    assert!(compared.iter().any(|c| c.field == "name" && c.agreement == DistFactAgreement::Agree));
    assert!(
        compared.iter().any(|c| c.field == "version" && c.agreement == DistFactAgreement::Agree)
    );
}

#[test]
fn dynamic_prereq_version_does_not_limit_identity_fields() {
    let makefile = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    VERSION => '1.23',
    LICENSE => 'perl',
    PREREQ_PM => { 'Moo' => $ver },
);
"#;
    let meta = r#"{"name":"Foo-Bar","version":"1.23","license":["perl_5"],
        "prereqs":{"runtime":{"requires":{"Moo":"2.0"}}}}"#;
    let model = build(
        "field-limit-prereq",
        &[("Makefile.PL", makefile), ("META.json", meta)],
        FactClasses::FILES | FactClasses::DIST,
    );
    let compared = model.compare_authoring_with_metadata();
    assert!(compared.iter().any(|c| {
        c.field.contains("prereq:runtime:requires:Moo") && c.agreement == DistFactAgreement::Limited
    }));
    assert!(compared.iter().any(|c| c.field == "name" && c.agreement == DistFactAgreement::Agree));
    assert!(
        compared.iter().any(|c| c.field == "version" && c.agreement == DistFactAgreement::Agree)
    );
}

#[test]
fn build_pl_skips_unrelated_constructor_before_module_build() {
    let src = r#"
Some::Other->new(name => 'Wrong::Pkg', version => '9.99');
Module::Build->new(
    module_name => 'Foo::Bar',
    dist_version => '1.23',
);
"#;
    let facts = parse_build_pl(fid("Build.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
    assert_eq!(facts.version.as_deref(), Some("1.23"));
    assert!(!facts.declarations.iter().any(|d| d.value.as_deref() == Some("Wrong-Pkg")));
}

#[test]
fn build_pl_does_not_take_unrelated_new_without_module_build() {
    let src = r#"
Some::Other->new(
    module_name => 'Wrong::Pkg',
    dist_version => '9.99',
);
"#;
    let facts = parse_build_pl(fid("Build.PL", src), src);
    assert!(facts.name.is_none());
    assert!(facts.version.is_none());
    assert!(facts.limitations.iter().any(|l| l.kind == "missing_module_build_new"));
}

#[test]
fn concatenated_and_arithmetic_literals_are_dynamic() {
    let src = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    VERSION => '1'.'2',
    PREREQ_PM => { 'Moo' => 1 + 2 },
);
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
    assert!(facts.version.is_none(), "VERSION => '1'.'2' is concatenation, not 1");
    assert!(facts.prereqs.iter().any(|p| p.module == "Moo" && p.dynamic));
    assert!(facts.limitations.iter().any(|l| l.kind == "dynamic_value"));
}

#[test]
fn open_source_license_is_not_rewritten_as_perl_5() {
    let src = "WriteMakefile(\n    NAME => 'Foo::Bar',\n    LICENSE => 'open_source',\n);\n";
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.licenses, vec!["open_source"]);
}

#[test]
fn dist_ini_name_equals_name_ranges_cover_the_value() {
    let src = "name = name\nversion = 1.00\n";
    let facts = parse_dist_ini(fid("dist.ini", src), src);
    let decl = facts.declarations.iter().find(|d| d.key == "name").unwrap();
    let start = decl.range.start_byte as usize;
    let end = decl.range.end_byte as usize;
    assert_eq!(src.get(start..end).unwrap().trim(), "name = name");
}

#[test]
fn unterminated_quotes_are_dynamic_not_static() {
    let src = "WriteMakefile(\n    NAME => 'Foo::Bar',\n    VERSION => '1.23\n);\n";
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
    assert!(facts.version.is_none());
    assert!(facts.limitations.iter().any(|l| l.kind == "dynamic_value"));
}

#[test]
fn fact_fingerprint_includes_resource_web_type_and_provides_file() {
    let with_web = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    META_MERGE => {
        resources => {
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
    let other_web = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    META_MERGE => {
        resources => {
            repository => {
                type => 'git',
                url  => 'https://example.invalid/foo.git',
                web  => 'https://example.invalid/other',
            },
        },
        provides => {
            'Foo::Bar' => { file => 'lib/Foo/Bar.pm', version => '1.23' },
        },
    },
);
"#;
    let other_file = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    META_MERGE => {
        resources => {
            repository => {
                type => 'git',
                url  => 'https://example.invalid/foo.git',
                web  => 'https://example.invalid/foo',
            },
        },
        provides => {
            'Foo::Bar' => { file => 'lib/Foo/Other.pm', version => '1.23' },
        },
    },
);
"#;
    let spaced = r#"
WriteMakefile(

    NAME => 'Foo::Bar',

    META_MERGE => {
        resources => {
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
    let first = parse_makefile_pl(fid("Makefile.PL", with_web), with_web);
    let second = parse_makefile_pl(fid("Makefile.PL", other_web), other_web);
    let third = parse_makefile_pl(fid("Makefile.PL", other_file), other_file);
    let spaced_facts = parse_makefile_pl(fid("Makefile.PL", spaced), spaced);
    assert_ne!(first.fact_fingerprint, second.fact_fingerprint);
    assert_ne!(first.fact_fingerprint, third.fact_fingerprint);
    assert_eq!(first.fact_fingerprint, spaced_facts.fact_fingerprint);
    assert_ne!(first.source_fingerprint, spaced_facts.source_fingerprint);
}

#[test]
fn conditional_declaration_prevents_high_confidence() {
    let src = r#"
if ($ENV{RELEASE}) {
    Module::Build->new(
        module_name => 'Foo::Bar',
        dist_version => '1.23',
    );
}
"#;
    let facts = parse_build_pl(fid("Build.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
    assert!(facts.limitations.iter().any(|l| l.kind == "conditional_declaration"));
    assert_ne!(facts.provenance.confidence, Confidence::High);
}

#[test]
fn meta_yml_is_left_for_issue_8458() {
    let model = build(
        "meta-yml-coexist",
        &[("META.yml", "---\nname: Foo-Bar\nversion: 1.0\n")],
        FactClasses::FILES | FactClasses::DIST,
    );
    assert!(
        model.dist_metadata.is_empty(),
        "this PR must not absorb META.yml; #8458 / #14424 owns that source"
    );
    assert!(model.dist_authoring.is_empty());
}

#[test]
fn compare_authoring_skips_cpanfile_and_keeps_meta_json() {
    let makefile = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    VERSION => '1.00',
);
"#;
    let meta = r#"{"name":"Foo-Bar","version":"1.00"}"#;
    let cpanfile = "requires 'Moo', '2.0';\n";
    let model = build(
        "skip-cpanfile",
        &[("Makefile.PL", makefile), ("META.json", meta), ("cpanfile", cpanfile)],
        FactClasses::FILES | FactClasses::DIST,
    );
    assert!(model.dist_metadata.iter().any(|d| d.source == DistMetadataSource::MetaJson));
    assert!(model.dist_metadata.iter().any(|d| d.source == DistMetadataSource::Cpanfile));
    let compared = model.compare_authoring_with_metadata();
    assert!(!compared.is_empty());
    assert!(compared.iter().all(|c| c.metadata_source == DistMetadataSource::MetaJson));
}

#[test]
fn pod_and_quotelike_examples_do_not_replace_real_writemakefile() {
    let src = r#"
=head1 SYNOPSIS

    WriteMakefile(NAME => 'Wrong::Doc', VERSION => '9.99');

=cut

my $example = q{ WriteMakefile(NAME => 'Wrong::Quote') };

WriteMakefile(
    NAME => 'Foo::Bar',
    VERSION => '1.23',
);
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
    assert_eq!(facts.version.as_deref(), Some("1.23"));
}

#[test]
fn nested_distributions_do_not_cross_compare() {
    let root_mk = "WriteMakefile(NAME => 'Root::Dist', VERSION => '1.00');\n";
    let nested_mk = "WriteMakefile(NAME => 'Nested::Dist', VERSION => '2.00');\n";
    let root_meta = r#"{"name":"Root-Dist","version":"1.00"}"#;
    let nested_meta = r#"{"name":"Nested-Dist","version":"2.00"}"#;
    let model = build(
        "nested-dists",
        &[
            ("Makefile.PL", root_mk),
            ("META.json", root_meta),
            ("ext/Nested/Makefile.PL", nested_mk),
            ("ext/Nested/META.json", nested_meta),
        ],
        FactClasses::FILES | FactClasses::DIST,
    );
    let compared = model.compare_authoring_with_metadata();
    assert!(compared.iter().any(|c| {
        c.field == "name"
            && c.authoring_value.as_deref() == Some("Root-Dist")
            && c.agreement == DistFactAgreement::Agree
    }));
    assert!(compared.iter().any(|c| {
        c.field == "name"
            && c.authoring_value.as_deref() == Some("Nested-Dist")
            && c.agreement == DistFactAgreement::Agree
    }));
    assert!(!compared.iter().any(|c| {
        c.field == "name"
            && c.authoring_value.as_deref() == Some("Root-Dist")
            && c.metadata_value.as_deref() == Some("Nested-Dist")
    }));
}

#[test]
fn conflicting_identity_compares_as_limited_not_metadata_only() {
    let makefile = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    DISTNAME => 'Other-Name',
    VERSION => '1.0',
    PREREQ_PM => { 'Moo' => '1.0', 'Moo' => '2.0' },
);
"#;
    let meta =
        r#"{"name":"Foo-Bar","version":"1.0","prereqs":{"runtime":{"requires":{"Moo":"2.0"}}}}"#;
    let model = build(
        "conflict-compare",
        &[("Makefile.PL", makefile), ("META.json", meta)],
        FactClasses::FILES | FactClasses::DIST,
    );
    let compared = model.compare_authoring_with_metadata();
    assert!(
        compared.iter().any(|c| c.field == "name" && c.agreement == DistFactAgreement::Limited)
    );
    assert!(compared.iter().any(|c| {
        c.field.contains("prereq:runtime:requires:Moo") && c.agreement == DistFactAgreement::Limited
    }));
}

#[test]
fn fact_fingerprint_includes_conflict_field_values() {
    let a = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    DISTNAME => 'Other-Name',
    VERSION => '1.0',
);
"#;
    let b = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    DISTNAME => 'Third-Name',
    VERSION => '1.0',
);
"#;
    let first = parse_makefile_pl(fid("Makefile.PL", a), a);
    let second = parse_makefile_pl(fid("Makefile.PL", b), b);
    assert_eq!(first.conflicts.len(), second.conflicts.len());
    assert_ne!(first.fact_fingerprint, second.fact_fingerprint);
}

#[test]
fn single_quoted_paths_preserve_ordinary_backslashes() {
    let src = "WriteMakefile(\n    NAME => 'Foo::Bar',\n    ABSTRACT => 'C:\\foo\\bar',\n    VERSION => '1.0',\n);\n";
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.summary.as_deref(), Some(r"C:\foo\bar"));
}

#[test]
fn single_quoted_special_escapes_collapse() {
    let src = "WriteMakefile(\n    NAME => 'Foo::Bar',\n    ABSTRACT => 'it\\'s \\\\ok',\n);\n";
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.summary.as_deref(), Some(r"it's \ok"));
}

#[test]
fn escaped_quotelike_closers_stay_inside_the_literal() {
    let src = r#"
my $example = q{ \} WriteMakefile(NAME => 'Wrong::Quote') };
my @words = qw{ \} WriteMakefile(NAME => 'Wrong::Qw') };
WriteMakefile(
    NAME => q{Foo\}::Bar},
    VERSION => qq{1.00\},0},
    LICENSE => [qw{ perl artistic_2 \}}],
);
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Foo}-Bar"));
    assert_eq!(facts.version.as_deref(), Some("1.00},0"));
    assert!(facts.licenses.iter().any(|license| license == "perl_5"));
    assert!(facts.licenses.iter().any(|license| license == "}"));
    assert!(!facts.name.as_deref().unwrap().contains("Wrong"));
}

#[test]
fn scalar_writemakefileargs_is_not_a_helper_hash() {
    let src = r#"
my $WriteMakefileArgs = (
    NAME => 'Scalar::Trap',
    VERSION => '9.99',
);
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    assert!(facts.name.is_none());
    assert!(facts.limitations.iter().any(|l| l.kind == "missing_writemakefile"));
}

#[test]
fn scalar_build_args_is_not_a_helper_hash() {
    let src = r#"
my $args = (
    module_name => 'Scalar::Trap',
    dist_version => '9.99',
);
"#;
    let facts = parse_build_pl(fid("Build.PL", src), src);
    assert!(facts.name.is_none());
    assert!(facts.limitations.iter().any(|l| l.kind == "missing_module_build_new"));
}

#[test]
fn hash_args_fallback_still_recovers_module_build_literals() {
    let src = r#"
my %args = (
    module_name => 'Foo::Bar',
    dist_version => '1.00',
);
Module::Build->new(%args)->create_build_script;
"#;
    let facts = parse_build_pl(fid("Build.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
    assert_eq!(facts.version.as_deref(), Some("1.00"));
}

#[test]
fn unrelated_args_hash_is_not_stolen_from_empty_constructor() {
    let src = r#"
my %args = (
    module_name => 'Wrong::Args',
    dist_version => '9.99',
);
Module::Build->new(%custom)->create_build_script;
"#;
    let facts = parse_build_pl(fid("Build.PL", src), src);
    assert!(facts.name.is_none());
    assert!(facts.limitations.iter().any(|l| l.kind == "missing_module_build_new"));
}

#[test]
fn unreferenced_args_hash_is_not_authoring_without_constructor() {
    let src = r#"
my %args = (
    module_name => 'Wrong::Args',
    dist_version => '9.99',
);
"#;
    let facts = parse_build_pl(fid("Build.PL", src), src);
    assert!(facts.name.is_none());
    assert!(facts.limitations.iter().any(|l| l.kind == "missing_module_build_new"));
}

#[test]
fn commented_and_quoted_constructors_do_not_replace_real_module_build() {
    let src = r#"
# Module::Build->new(module_name => 'Wrong::Comment', dist_version => '9.99');
print "Module::Build->new(module_name => 'Wrong::String')\n";
my $example = q{ Module::Build->new(module_name => 'Wrong::Quote') };
Module::Build->new(
    module_name => 'Foo::Bar',
    dist_version => '1.23',
);
"#;
    let facts = parse_build_pl(fid("Build.PL", src), src);
    assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
    assert_eq!(facts.version.as_deref(), Some("1.23"));
    assert!(!facts.name.as_deref().unwrap().contains("Wrong"));
}

#[test]
fn nested_dynamic_resource_and_provides_fields_are_not_static() {
    let src = r#"
WriteMakefile(
    NAME => 'Foo::Bar',
    META_MERGE => {
        resources => {
            repository => {
                url => $repo_url,
                type => 'git',
            },
        },
        provides => {
            'Foo::Bar' => { file => $path, version => '1.23' },
        },
    },
);
"#;
    let facts = parse_makefile_pl(fid("Makefile.PL", src), src);
    let repo = facts.resources.iter().find(|r| r.kind == "repository").unwrap();
    assert!(repo.url.is_none());
    assert_eq!(repo.type_name.as_deref(), Some("git"));
    assert!(facts.declarations.iter().any(|d| d.key == "repository" && d.dynamic));
    let provided = facts.provides.iter().find(|p| p.module == "Foo::Bar").unwrap();
    assert!(provided.file.is_none());
    assert_eq!(provided.version.as_deref(), Some("1.23"));
    assert!(facts.declarations.iter().any(|d| d.key == "Foo::Bar" && d.dynamic));
    assert!(facts.limitations.iter().any(|l| l.kind == "dynamic_value"));
}
