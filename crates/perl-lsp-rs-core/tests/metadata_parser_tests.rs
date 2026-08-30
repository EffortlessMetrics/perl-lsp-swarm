use perl_lsp_rs_core::config::{
    DeclaredDependency, DeclaredDependencySource, WorkspaceConfig, detect_declared_dependencies,
    extract_build_pl_requirements, extract_cpanfile_requirements, extract_dist_ini_requirements,
    extract_makefile_pl_requirements, extract_meta_json_requirements,
    extract_meta_yml_requirements,
};
use std::fs;
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn extracts_cpanfile_requirements_with_source_and_kind() {
    let cpanfile = r#"
        requires 'Moo', '2.0';
        requires "JSON::PP";
        recommends 'Data::Dumper';
        test_requires 'Test::More', '1.00';
        # requires 'Ignored::Comment';
    "#;

    let deps = extract_cpanfile_requirements(cpanfile);

    assert!(deps.contains(&DeclaredDependency::new(
        "Moo",
        Some("2.0"),
        "requires",
        DeclaredDependencySource::Cpanfile,
    )));
    assert!(deps.contains(&DeclaredDependency::new(
        "JSON::PP",
        None,
        "requires",
        DeclaredDependencySource::Cpanfile,
    )));
    assert!(deps.contains(&DeclaredDependency::new(
        "Data::Dumper",
        None,
        "recommends",
        DeclaredDependencySource::Cpanfile,
    )));
    assert!(deps.contains(&DeclaredDependency::new(
        "Test::More",
        Some("1.00"),
        "test_requires",
        DeclaredDependencySource::Cpanfile,
    )));
    assert_eq!(deps.len(), 4);
}

#[test]
fn declared_dependency_sources_have_user_facing_display_names() {
    let names = [
        (DeclaredDependencySource::Cpanfile, "cpanfile"),
        (DeclaredDependencySource::MakefilePl, "Makefile.PL"),
        (DeclaredDependencySource::BuildPl, "Build.PL"),
        (DeclaredDependencySource::DistIni, "dist.ini"),
        (DeclaredDependencySource::MetaJson, "META.json"),
        (DeclaredDependencySource::MetaYml, "META.yml"),
    ];

    for (source, expected) in names {
        assert_eq!(source.display_name(), expected);
    }
}

#[test]
fn cpanfile_parser_ignores_dynamic_or_malformed_requires() {
    let cpanfile = r#"
        requires $dynamic_module;
        requires;
        requires 'Valid::Module';
        requires 'Valid::Module';
    "#;

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        deps,
        vec![DeclaredDependency::new(
            "Valid::Module",
            None,
            "requires",
            DeclaredDependencySource::Cpanfile,
        )],
    );
}

#[test]
fn perl_metadata_parsers_ignore_nonliteral_and_malformed_entries() {
    let makefile_pl = r#"
        WriteMakefile(
            PREREQ_PM => 'Not::A::Hash',
            TEST_REQUIRES => {
                'Valid::Test' => '1.00',
            },
        );
    "#;
    let dist_ini = r#"
        author = Someone

        [Prereqs::FromCPANfile]
        Ignored::FromCpanfile = 1

        [Prereqs]
        missing separator
        Bad Module = 1
        Valid::Module = 2.0
        Valid::Module = 3.0
    "#;
    let meta_yml = r#"
name: My-Dist
BareLine
requires:
  MissingSeparator
  Bad Module: 1
  Valid::Yaml: 0.42
"#;

    assert_eq!(
        extract_makefile_pl_requirements(makefile_pl),
        vec![DeclaredDependency::new(
            "Valid::Test",
            Some("1.00"),
            "TEST_REQUIRES",
            DeclaredDependencySource::MakefilePl,
        )],
    );
    assert_eq!(
        extract_dist_ini_requirements(dist_ini),
        vec![DeclaredDependency::new(
            "Valid::Module",
            Some("2.0"),
            "Prereqs",
            DeclaredDependencySource::DistIni,
        )],
    );
    assert_eq!(
        extract_meta_yml_requirements(meta_yml),
        vec![DeclaredDependency::new(
            "Valid::Yaml",
            Some("0.42"),
            "requires",
            DeclaredDependencySource::MetaYml,
        )],
    );
    assert!(extract_meta_json_requirements("{").is_empty());
}

#[test]
fn perl_metadata_parsers_keep_scanning_after_edge_shapes() {
    let makefile_pl = r#"
        my $ignored = 'PREREQ_PM\\not_a_key';
        WriteMakefile(
            PREREQ_PM => { 'Broken::Hash' => '1.0',
            TEST_REQUIRES => {
                'Bad Module' => '1.00',
                'No::Version',
                'Valid::Test' => '1.00',
            },
        );
    "#;
    let nested_meta_json = r#"
        {
          "prereqs": {
            "runtime": {
              "requires": {
                "Bad Module": "1.0",
                "Nested::Number": 2,
                "Nested::Unspecified": null
              }
            }
          }
        }
    "#;
    let top_level_meta_json = r#"
        {
          "requires": {
            "Bad Module": "1.0",
            "Top::Number": 3,
            "Top::Unspecified": null
          }
        }
    "#;

    assert_eq!(
        extract_makefile_pl_requirements(makefile_pl),
        vec![
            DeclaredDependency::new(
                "No::Version",
                None,
                "TEST_REQUIRES",
                DeclaredDependencySource::MakefilePl,
            ),
            DeclaredDependency::new(
                "Valid::Test",
                Some("1.00"),
                "TEST_REQUIRES",
                DeclaredDependencySource::MakefilePl,
            ),
        ],
    );

    assert_eq!(
        extract_meta_json_requirements(nested_meta_json),
        vec![
            DeclaredDependency::new(
                "Nested::Number",
                Some("2"),
                "runtime.requires",
                DeclaredDependencySource::MetaJson,
            ),
            DeclaredDependency::new(
                "Nested::Unspecified",
                None,
                "runtime.requires",
                DeclaredDependencySource::MetaJson,
            ),
        ],
    );
    assert_eq!(
        extract_meta_json_requirements(top_level_meta_json),
        vec![
            DeclaredDependency::new(
                "Top::Number",
                Some("3"),
                "requires",
                DeclaredDependencySource::MetaJson,
            ),
            DeclaredDependency::new(
                "Top::Unspecified",
                None,
                "requires",
                DeclaredDependencySource::MetaJson,
            ),
        ],
    );
}

#[test]
fn extracts_makefile_and_build_pl_requirements() {
    let makefile_pl = r#"
        WriteMakefile(
            NAME => 'My::App',
            PREREQ_PM => {
                'JSON::PP' => '4.0',
                "Moo" => 2.005,
            },
            TEST_REQUIRES => {
                'Test::More' => '1.00',
            },
        );
    "#;
    let build_pl = r#"
        Module::Build->new(
            requires => {
                'Try::Tiny' => '0.31',
            },
            build_requires => {
                'Module::Build' => 0,
            },
        );
    "#;

    let makefile_deps = extract_makefile_pl_requirements(makefile_pl);
    let build_deps = extract_build_pl_requirements(build_pl);

    assert!(makefile_deps.contains(&DeclaredDependency::new(
        "JSON::PP",
        Some("4.0"),
        "PREREQ_PM",
        DeclaredDependencySource::MakefilePl,
    )));
    assert!(makefile_deps.contains(&DeclaredDependency::new(
        "Test::More",
        Some("1.00"),
        "TEST_REQUIRES",
        DeclaredDependencySource::MakefilePl,
    )));
    assert!(build_deps.contains(&DeclaredDependency::new(
        "Try::Tiny",
        Some("0.31"),
        "requires",
        DeclaredDependencySource::BuildPl,
    )));
    assert!(build_deps.contains(&DeclaredDependency::new(
        "Module::Build",
        Some("0"),
        "build_requires",
        DeclaredDependencySource::BuildPl,
    )));
}

#[test]
fn extracts_meta_json_and_dist_ini_requirements() {
    let meta_json = r#"
        {
          "prereqs": {
            "runtime": { "requires": { "JSON::PP": "4.0" } },
            "test": { "requires": { "Test::More": "1.00" } }
          }
        }
    "#;
    let dist_ini = r#"
        [Prereqs]
        Moo = 2.0
        JSON::PP = 4.0

        [Prereqs / TestRequires]
        Test::More = 1.00
    "#;

    let meta_deps = extract_meta_json_requirements(meta_json);
    let dist_ini_deps = extract_dist_ini_requirements(dist_ini);

    assert!(meta_deps.contains(&DeclaredDependency::new(
        "JSON::PP",
        Some("4.0"),
        "runtime.requires",
        DeclaredDependencySource::MetaJson,
    )));
    assert!(meta_deps.contains(&DeclaredDependency::new(
        "Test::More",
        Some("1.00"),
        "test.requires",
        DeclaredDependencySource::MetaJson,
    )));
    assert!(dist_ini_deps.contains(&DeclaredDependency::new(
        "Moo",
        Some("2.0"),
        "Prereqs",
        DeclaredDependencySource::DistIni,
    )));
    assert!(dist_ini_deps.contains(&DeclaredDependency::new(
        "Test::More",
        Some("1.00"),
        "Prereqs / TestRequires",
        DeclaredDependencySource::DistIni,
    )));
}

#[test]
fn extracts_meta_yml_requirements() {
    let meta_yml = r#"
prereqs:
  runtime:
    requires:
      JSON::PP: 4.0
  test:
    requires:
      Test::More: 1.00
"#;

    let deps = extract_meta_yml_requirements(meta_yml);

    assert!(deps.contains(&DeclaredDependency::new(
        "JSON::PP",
        Some("4.0"),
        "requires",
        DeclaredDependencySource::MetaYml,
    )));
    assert!(deps.contains(&DeclaredDependency::new(
        "Test::More",
        Some("1.00"),
        "requires",
        DeclaredDependencySource::MetaYml,
    )));
}

#[test]
fn detects_declared_dependencies_from_workspace_metadata_files() -> TestResult {
    let temp = TempDir::new()?;
    fs::write(
        temp.path().join("cpanfile"),
        "requires 'JSON::PP', '4.0';\ntest_requires 'Test::More';\n",
    )?;
    fs::write(
        temp.path().join("Makefile.PL"),
        "WriteMakefile(PREREQ_PM => { 'Moo' => '2.0' });\n",
    )?;
    fs::write(
        temp.path().join("Build.PL"),
        "Module::Build->new(requires => { 'Try::Tiny' => '0.31' });\n",
    )?;
    fs::write(temp.path().join("dist.ini"), "[Prereqs]\nPath::Tiny = 0.146\n")?;
    fs::write(temp.path().join("META.json"), r#"{"requires":{"CPAN::Meta":"2.150010"}}"#)?;
    fs::write(temp.path().join("META.yml"), "requires:\n  YAML::PP: 0.38\n")?;

    let deps = detect_declared_dependencies(temp.path());

    assert!(deps.contains(&DeclaredDependency::new(
        "JSON::PP",
        Some("4.0"),
        "requires",
        DeclaredDependencySource::Cpanfile,
    )));
    assert!(deps.contains(&DeclaredDependency::new(
        "Moo",
        Some("2.0"),
        "PREREQ_PM",
        DeclaredDependencySource::MakefilePl,
    )));
    assert!(deps.contains(&DeclaredDependency::new(
        "Try::Tiny",
        Some("0.31"),
        "requires",
        DeclaredDependencySource::BuildPl,
    )));
    assert!(deps.contains(&DeclaredDependency::new(
        "Path::Tiny",
        Some("0.146"),
        "Prereqs",
        DeclaredDependencySource::DistIni,
    )));
    assert!(deps.contains(&DeclaredDependency::new(
        "CPAN::Meta",
        Some("2.150010"),
        "requires",
        DeclaredDependencySource::MetaJson,
    )));
    assert!(deps.contains(&DeclaredDependency::new(
        "YAML::PP",
        Some("0.38"),
        "requires",
        DeclaredDependencySource::MetaYml,
    )));
    assert_eq!(deps.len(), 7);
    Ok(())
}

#[test]
fn workspace_config_refreshes_declared_dependency_cache() -> TestResult {
    let temp = TempDir::new()?;
    fs::write(temp.path().join("cpanfile"), "requires 'JSON::PP', '4.0';\n")?;

    let mut config = WorkspaceConfig::default();
    config.refresh_declared_dependencies(temp.path());

    assert_eq!(
        config.declared_dependencies,
        vec![DeclaredDependency::new(
            "JSON::PP",
            Some("4.0"),
            "requires",
            DeclaredDependencySource::Cpanfile,
        )],
    );
    Ok(())
}

// ─── Conditional cpanfile declarations (#13695) ─────────────────────────────
//
// Canonical `on '<phase>' => sub { ... }` declarations must keep their phase,
// and declarations inside `feature`, platform conditionals, loops, and
// arbitrary callback blocks must produce no unconditional advisory fact,
// because `DeclaredDependency` cannot retain their predicates.

fn cpanfile_dependency<'a>(
    deps: &'a [DeclaredDependency],
    module: &str,
) -> Option<&'a DeclaredDependency> {
    deps.iter().find(|dependency| dependency.module == module)
}

#[test]
fn cpanfile_on_block_declarations_keep_their_phase() {
    let cpanfile = r#"
        requires 'Plack';
        on 'test' => sub {
            requires 'Test::More', '0.98';
            recommends 'Test::Deep';
        };
        on 'develop' => sub { requires 'Perl::Critic'; };
    "#;

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        cpanfile_dependency(&deps, "Test::More"),
        Some(&DeclaredDependency::new(
            "Test::More",
            Some("0.98"),
            "test.requires",
            DeclaredDependencySource::Cpanfile,
        )),
    );
    assert_eq!(
        cpanfile_dependency(&deps, "Test::Deep"),
        Some(&DeclaredDependency::new(
            "Test::Deep",
            None,
            "test.recommends",
            DeclaredDependencySource::Cpanfile,
        )),
    );
    assert_eq!(
        cpanfile_dependency(&deps, "Perl::Critic"),
        Some(&DeclaredDependency::new(
            "Perl::Critic",
            None,
            "develop.requires",
            DeclaredDependencySource::Cpanfile,
        )),
    );
    assert_eq!(
        cpanfile_dependency(&deps, "Plack"),
        Some(&DeclaredDependency::new(
            "Plack",
            None,
            "requires",
            DeclaredDependencySource::Cpanfile,
        )),
    );
}

#[test]
fn cpanfile_conditional_blocks_produce_no_unconditional_advisory() {
    let cpanfile = r#"
        requires 'Plack';
        feature 'soup' => sub {
            recommends 'JSON::XS';
            requires 'Feature::Bound';
        };
        if ($ENV{PERL_LSP_SWARM}) {
            suggests 'Conditional::Block';
            requires 'Env::Bound';
        }
        foreach my $mod ('Loop::One', 'Loop::Two') {
            requires 'Loop::Bound';
        }
        my $callback = sub {
            requires 'Callback::Bound';
        };
    "#;

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        deps,
        vec![DeclaredDependency::new(
            "Plack",
            None,
            "requires",
            DeclaredDependencySource::Cpanfile,
        )]
    );
}

#[test]
fn cpanfile_unsupported_blocks_suppress_nested_canonical_blocks() {
    let cpanfile = r#"
        feature 'soup' => sub {
            on 'test' => sub {
                requires 'Nested::Suppressed';
                recommends 'Nested::SuppressedToo';
            };
        };
        on 'test' => sub { requires 'Real::Test'; };
    "#;

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        deps,
        vec![DeclaredDependency::new(
            "Real::Test",
            None,
            "test.requires",
            DeclaredDependencySource::Cpanfile,
        )]
    );
}

#[test]
fn cpanfile_postfix_conditionals_produce_no_advisory_fact() {
    let cpanfile = r#"
        requires 'Win::Only' if $^O eq 'MSWin32';
        requires 'Old::Perl' unless $] >= 5.016;
        requires 'Plack';
    "#;

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        deps,
        vec![DeclaredDependency::new(
            "Plack",
            None,
            "requires",
            DeclaredDependencySource::Cpanfile,
        )]
    );
}

#[test]
fn cpanfile_unknown_on_phases_produce_no_advisory_fact() {
    let cpanfile = r#"
        on 'staging' => sub {
            requires 'Staging::Bound';
            recommends 'Staging::BoundToo';
        };
        on 'test' => sub { requires 'Real::Test'; };
    "#;

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        deps,
        vec![DeclaredDependency::new(
            "Real::Test",
            None,
            "test.requires",
            DeclaredDependencySource::Cpanfile,
        )]
    );
}

#[test]
fn cpanfile_quoted_braces_and_semicolons_do_not_alter_block_state() {
    let cpanfile = concat!(
        r#"my $advice = 'always; requires "Quoted::Leak";';"#,
        "\n",
        r#"my $open = '{'; my $close = '}';"#,
        "\n",
        r#"requires 'Real::One';"#,
        "\n",
        r#"recommends 'Real::Two';"#,
    );

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        deps,
        vec![
            DeclaredDependency::new(
                "Real::One",
                None,
                "requires",
                DeclaredDependencySource::Cpanfile,
            ),
            DeclaredDependency::new(
                "Real::Two",
                None,
                "recommends",
                DeclaredDependencySource::Cpanfile,
            ),
        ],
    );
}

#[test]
fn cpanfile_forced_phase_and_bareword_on_align_with_substrate() {
    let cpanfile = r#"
        on develop => sub {
            requires 'Bareword::Develop';
            test_requires 'Forced::Test';
        };
        on 'runtime' => sub {
            on 'test' => sub {
                requires 'Nested::Canonical';
            };
        };
        build_requires 'Build::Dep', '0.1';
    "#;

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        cpanfile_dependency(&deps, "Bareword::Develop"),
        Some(&DeclaredDependency::new(
            "Bareword::Develop",
            None,
            "develop.requires",
            DeclaredDependencySource::Cpanfile,
        )),
    );
    assert_eq!(
        cpanfile_dependency(&deps, "Forced::Test"),
        Some(&DeclaredDependency::new(
            "Forced::Test",
            None,
            "test.requires",
            DeclaredDependencySource::Cpanfile,
        )),
    );
    assert_eq!(
        cpanfile_dependency(&deps, "Nested::Canonical"),
        Some(&DeclaredDependency::new(
            "Nested::Canonical",
            None,
            "test.requires",
            DeclaredDependencySource::Cpanfile,
        )),
    );
    assert_eq!(
        cpanfile_dependency(&deps, "Build::Dep"),
        Some(&DeclaredDependency::new(
            "Build::Dep",
            Some("0.1"),
            "build_requires",
            DeclaredDependencySource::Cpanfile,
        )),
    );
    assert_eq!(deps.len(), 4);
}

#[test]
fn cpanfile_keyword_boundaries_and_substrate_keywords() {
    let cpanfile = r#"
        requires_extra 'Not::A::Prereq';
        on 'nonphase' => sub { requires 'Unknown::Phase'; };
        suggests 'Suggested::Module';
        conflicts 'Conflicting::Module';
        author_requires 'Author::Module';
        requires 'Real::One';
    "#;

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        deps,
        vec![
            DeclaredDependency::new(
                "Suggested::Module",
                None,
                "suggests",
                DeclaredDependencySource::Cpanfile,
            ),
            DeclaredDependency::new(
                "Conflicting::Module",
                None,
                "conflicts",
                DeclaredDependencySource::Cpanfile,
            ),
            DeclaredDependency::new(
                "Author::Module",
                None,
                "author_requires",
                DeclaredDependencySource::Cpanfile,
            ),
            DeclaredDependency::new(
                "Real::One",
                None,
                "requires",
                DeclaredDependencySource::Cpanfile,
            ),
        ],
    );
}

#[test]
fn cpanfile_subscript_braces_between_block_opener_and_declaration_keep_phase() {
    // Review finding repro: a hash subscript (`$ENV{...}`) between the block
    // opener and a declaration must not disturb canonical block attribution.
    let cpanfile = r#"
        on 'test' => sub {
            my $x = $ENV{WHATEVER};
            requires 'Test::More';
        };
    "#;

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        cpanfile_dependency(&deps, "Test::More"),
        Some(&DeclaredDependency::new(
            "Test::More",
            None,
            "test.requires",
            DeclaredDependencySource::Cpanfile,
        )),
    );
}

#[test]
fn cpanfile_regex_literal_braces_do_not_open_blocks() {
    // Review finding repro: a regex literal containing a brace before a
    // top-level declaration must not suppress the later declaration.
    let cpanfile = concat!(r#"my $pattern = qr/\{/;"#, "\n", r#"requires 'Always::There';"#,);

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        deps,
        vec![DeclaredDependency::new(
            "Always::There",
            None,
            "requires",
            DeclaredDependencySource::Cpanfile,
        )],
    );
}

#[test]
fn cpanfile_brace_delimited_regex_and_substitution_do_not_alter_block_state() {
    let cpanfile = concat!(
        r#"my $open = qr{\d+};"#,
        "\n",
        r#"my $clean = s/;requires "Leak::One";/ /gr;"#,
        "\n",
        r#"my $swap = tr/a-z/A-Z/;"#,
        "\n",
        r#"requires 'Real::One';"#,
        "\n",
        r#"on 'test' => sub { requires 'Real::Two'; };"#,
    );

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        deps,
        vec![
            DeclaredDependency::new(
                "Real::One",
                None,
                "requires",
                DeclaredDependencySource::Cpanfile,
            ),
            DeclaredDependency::new(
                "Real::Two",
                None,
                "test.requires",
                DeclaredDependencySource::Cpanfile,
            ),
        ],
    );
}

#[test]
fn cpanfile_dynamic_argument_expressions_produce_no_advisory_fact() {
    // Helper calls, ternaries, and concatenations are dynamic expressions:
    // strings nested inside them must not become unconditional advisories.
    let cpanfile = concat!(
        r#"requires feature_helper('Helper::Dep');"#,
        "\n",
        r#"requires($^O eq 'MSWin32' ? 'Win32::Only' : 'Unix::Only');"#,
        "\n",
        r#"requires 'Concat' . '::Tail';"#,
        "\n",
        r#"requires 'Real::One';"#,
    );

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        deps,
        vec![DeclaredDependency::new(
            "Real::One",
            None,
            "requires",
            DeclaredDependencySource::Cpanfile,
        )],
    );
}

#[test]
fn cpanfile_substitution_replacement_braces_do_not_corrupt_blocks() {
    // A slash-delimited replacement beginning with text and containing
    // braces must be consumed with the operand, not leaked into block state.
    let cpanfile = concat!(
        r#"my $count = s/pattern/b{r}ace/;"#,
        "\n",
        r#"my $odd = s/odd/x{y/;"#,
        "\n",
        r#"requires 'After::Subst';"#,
    );

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        deps,
        vec![DeclaredDependency::new(
            "After::Subst",
            None,
            "requires",
            DeclaredDependencySource::Cpanfile,
        )],
    );
}

#[test]
fn cpanfile_bare_numeric_version_forms_stay_unconditional() {
    // Exponents, `_` separators, and leading decimal points are literal
    // Perl numbers: they must not mark a declaration dynamic.
    let cpanfile = concat!(
        "requires 'Num::Version', 1.5_0;\n",
        "requires 'Exp::Version', 1.5e-2;\n",
        "requires 'Dot::Version', .5;\n",
        "requires 'Real::One';",
    );

    let deps = extract_cpanfile_requirements(cpanfile);

    assert_eq!(
        deps,
        vec![
            DeclaredDependency::new(
                "Num::Version",
                None,
                "requires",
                DeclaredDependencySource::Cpanfile,
            ),
            DeclaredDependency::new(
                "Exp::Version",
                None,
                "requires",
                DeclaredDependencySource::Cpanfile,
            ),
            DeclaredDependency::new(
                "Dot::Version",
                None,
                "requires",
                DeclaredDependencySource::Cpanfile,
            ),
            DeclaredDependency::new(
                "Real::One",
                None,
                "requires",
                DeclaredDependencySource::Cpanfile,
            ),
        ],
    );
}
