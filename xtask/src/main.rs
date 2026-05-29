//! Xtask automation for tree-sitter-perl
//!
//! This binary provides custom automation tasks for building, testing,
//! and maintaining the tree-sitter-perl project.

// Task-runner binary — println!/eprintln! are intentional diagnostic output.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::{CommandFactory, Parser};
use color_eyre::eyre::{Result, eyre};
use std::path::PathBuf;

mod allocation_tracker;
mod cli;
mod tasks;
mod types;
mod utils;
use cli::srp::SrpCommand;
use tasks::check_test_wiring;
use tasks::dead_code::DeadCodeConfig;
use tasks::metrics;
use tasks::unwired_scan::UnwiredScanConfig;
use tasks::ux_scorecard::UxScorecardFormat;
use tasks::*;
use types::TestSuite;

use cli::commands::*;
fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    match cli.command {
        Commands::List => {
            print_top_level_commands();
            Ok(())
        }
        Commands::Ci { command } => match command {
            None => ci::run(),
            Some(CiSubcommand::Doctor) => ci_doctor::run(),
        },
        Commands::CheckOnly => ci::check_only(),
        Commands::CheckLintPolicy => check_lint_policy::run(),
        Commands::CheckToolchain { doctor } => check_toolchain::run(doctor),
        Commands::CheckDevexDocs => devex_docs::run(),
        Commands::CheckProviderConfidenceMatrix => provider_confidence_matrix::run(),
        Commands::CheckSupportClaims => provider_confidence_matrix::run_support_claims(),
        Commands::CheckActiveGoalManifest => active_goal_manifest::run(),
        Commands::CheckProviderPromotionLedger => provider_promotion_ledger::run(),
        Commands::CheckOracleFixtureManifest => oracle_fixture_manifest::run(),
        Commands::CheckOracleReceiptSchema => oracle_receipt_schema::run(),
        Commands::CheckSemanticTokenClasses => semantic_token_classes::run(),
        Commands::CheckLsp318Claims => lsp_318_claims::run(),
        Commands::GenerateLsp318Matrix { check } => lsp_318_matrix::run(check),
        Commands::CheckWorkspaceSymbolClasses => workspace_symbol_classes::run(),
        Commands::Queue { command } => match command {
            QueueCommand::Snapshot { out, fixture } => queue_snapshot::run_snapshot(out, fixture),
            QueueCommand::Health { receipt, fixture } => {
                queue_health::run(queue_health::QueueHealthArgs { receipt, fixture })
            }
            QueueCommand::ProjectLabels { state, dry_run, apply, receipt, config } => {
                label_projector::run_project_labels(label_projector::LabelProjectorArgs {
                    state,
                    dry_run,
                    apply,
                    receipt,
                    config,
                })
            }
        },
        Commands::Pr { command } => match command {
            PrSubcommand::TitleCheck { title, json, strict, no_gh } => {
                tasks::pr::title_check::run(tasks::pr::title_check::TitleCheckConfig {
                    title,
                    json,
                    strict,
                    no_gh,
                })
            }
        },
        Commands::Build { release, features, c_scanner, rust_scanner } => {
            build::run(release, features, c_scanner, rust_scanner)
        }
        Commands::Test { release, suite, features, verbose, coverage } => {
            test::run(release, suite, features, verbose, coverage)
        }
        Commands::Smoke { command } => match command {
            SmokeCommand::InlineCompletion { binary } => inline_completion_smoke::run(binary),
        },
        Commands::InlineCompletionSmoke { binary } => inline_completion_smoke::run(binary),
        Commands::InlineCompletionQuality { receipt } => inline_completion_quality::run(receipt),
        Commands::Badges { check } => badges::run(check),
        Commands::CoverageBaseline {
            lcov,
            receipt,
            codecov,
            patch_coverage,
            patch_base,
            scope,
            check,
        } => quality_baseline::run(quality_baseline::CoverageBaselineArgs {
            lcov,
            receipt,
            codecov,
            patch_coverage,
            patch_base,
            scope,
            check,
        }),
        Commands::QualityGate {
            mode,
            exception_policy,
            ripr_receipt,
            ripr_pr_receipt,
            review_receipt,
            coverage_receipt,
            codecov,
            patch_coverage,
            ripr_base,
            ripr_head,
            receipt,
            summary,
            check,
        } => quality_gate::run(quality_gate::QualityGateArgs {
            mode,
            exception_policy,
            ripr_receipt,
            ripr_pr_receipt,
            review_receipt,
            coverage_receipt,
            codecov,
            patch_coverage,
            ripr_base,
            ripr_head,
            receipt,
            summary,
            check,
        }),
        Commands::RiprPr { root, base, head, check } => {
            ripr_evidence::ripr_pr(&root, &base, &head, check)
        }
        Commands::RiprPlus { root, receipt, suppressions, check } => {
            ripr_evidence::ripr_plus(&root, &receipt, &suppressions, check)
        }
        Commands::RiprReviewComments { root, base, head, timeout_seconds, check } => {
            ripr_evidence::ripr_review_comments(&root, &base, &head, timeout_seconds, check)
        }
        Commands::RiprPrSummary { check } => ripr_evidence::ripr_pr_summary(check),
        Commands::RiprAnnotations { comments, out, check } => {
            ripr_evidence::ripr_annotations(&comments, &out, check)
        }
        Commands::ImpactedEvidence { pr_evidence, labels, labels_csv, check } => {
            ripr_evidence::impacted_evidence(&pr_evidence, &labels, labels_csv.as_deref(), check)
        }
        Commands::Bench { name, save, output } => bench::run(name, save, output),
        Commands::BenchRun { output, quick, category } => {
            benchmarks::run_benchmarks(output, quick, category)
        }
        Commands::BenchCompare { fail_on_regression } => {
            benchmarks::compare_benchmarks(fail_on_regression)
        }
        Commands::BenchFormat { receipt, markdown } => {
            benchmarks::format_benchmarks(receipt, markdown)
        }
        Commands::BenchExtract { base_path, output } => {
            benchmarks::extract_criterion(base_path, output)
        }
        Commands::BenchAlert { format, check } => benchmarks::alert_benchmarks(format, check),
        Commands::BenchAlertTest => benchmarks::test_alert_system(),
        Commands::InjectShaAssets {
            version,
            owner,
            repo,
            prefix,
            checksums,
            brew_out,
            asset_map_out,
        } => inject_sha_assets::run(inject_sha_assets::InjectShaAssetsConfig {
            version,
            owner,
            repo,
            prefix,
            checksums,
            brew_out,
            asset_map_out,
        }),
        Commands::UpdateHomebrew { version, owner, repo, prefix, output } => {
            update_homebrew::run(update_homebrew::UpdateHomebrewConfig {
                version,
                owner,
                repo,
                prefix,
                output,
            })
        }
        Commands::Compare {
            c_only,
            rust_only,
            scanner_only,
            validate_only,
            output_dir,
            check_gates,
            report,
        } => compare::run(
            c_only,
            rust_only,
            scanner_only,
            validate_only,
            output_dir,
            check_gates,
            report,
        ),
        Commands::Doc { open, all_features } => doc::run(open, all_features),
        Commands::Check { clippy, fmt, all } => check::run(clippy, fmt, all),
        Commands::Fmt { check, package } => fmt::run(check, package),
        #[cfg(feature = "legacy")]
        Commands::Corpus { path, scanner, diagnose, test } => {
            corpus::run(path, scanner, diagnose, test)
        }
        #[cfg(feature = "parser-tasks")]
        Commands::Highlight { path, scanner } => highlight::run(path, scanner),
        Commands::Clean { all } => clean::run(all),
        Commands::DeadCode { mode, strict } => dead_code::run(DeadCodeConfig { mode, strict }),
        #[cfg(feature = "parser-tasks")]
        Commands::Bindings { header, output } => bindings::run(header, output),
        Commands::Dev { watch, port } => dev::run(watch, port),
        Commands::DevexDoctor => devex_doctor::run(),
        Commands::Devex { command } => match command {
            DevexCommand::Plan { base } => devex_plan::run(devex_plan::DevexPlanConfig { base }),
            DevexCommand::Receipt { base, output } => {
                devex_plan::write_receipt(devex_plan::DevexReceiptConfig { base, output })
            }
            DevexCommand::Cockpit { base, receipt } => {
                devex_plan::cockpit(devex_plan::DevexCockpitConfig { base, receipt })
            }
            DevexCommand::PrBody { base, receipt } => {
                devex_plan::pr_body(devex_plan::DevexPrBodyConfig { base, receipt })
            }
        },
        Commands::ParseRust { source, sexp, ast, bench } => {
            parse_rust::run(source, sexp, ast, bench)
        }
        Commands::Release { command } => match command {
            ReleaseCommand::Prepare { version, yes } => release::run(version, yes),
            ReleaseCommand::Evidence { version, out } => release_evidence::scaffold(&version, &out),
            ReleaseCommand::VerifyEvidence { version, receipt, bundle_dir } => {
                let effective_bundle_dir = bundle_dir.unwrap_or_else(|| {
                    PathBuf::from(format!("target/release-evidence/v{version}"))
                });
                release_evidence::verify(&version, &effective_bundle_dir, &receipt)
            }
        },
        Commands::ReleaseNotes { tag, output, root } => release_notes::run(tag, output, root),
        Commands::ReleaseTurnkey {
            version,
            positional_version,
            prerelease,
            dry_run,
            skip_crates,
            skip_extension,
            skip_docker,
            base_branch,
            no_auto_merge,
            no_wait_pr_merge,
            no_wait_release,
            workflow_timeout,
        } => release_turnkey::run(release_turnkey::ReleaseTurnkeyConfig {
            version,
            positional_version,
            prerelease,
            dry_run,
            skip_crates,
            skip_extension,
            skip_docker,
            base_branch,
            no_auto_merge,
            no_wait_pr_merge,
            no_wait_release,
            workflow_timeout,
        }),
        Commands::PrepCratesIoLaunch { mode } => {
            prep_crates_io_launch::run(matches!(mode, PrepCratesMode::All))
        }
        Commands::TestHeredoc { release, verbose } => {
            // Run heredoc tests using the test module with heredoc suite
            test::run(
                release,
                Some(TestSuite::Heredoc),
                Some(vec!["pure-rust".to_string()]),
                verbose,
                false,
            )
        }
        Commands::TestEdgeCases { bench, coverage, test } => edge_cases::run(bench, coverage, test),
        Commands::CiAuditWorkflows => ci_audit_workflows::run(),
        Commands::WorkflowPolicyLint { receipt, fixture, check_lane_whitelist } => {
            workflow_policy_lint::run(workflow_policy_lint::WorkflowPolicyLintConfig {
                receipt,
                fixture,
                check_lane_whitelist,
            })
        }
        Commands::CiMeasure => ci_measure::run(),
        Commands::CiCostMonitor { days, json } => ci_metrics::run_cost_monitor(days, json),
        Commands::CiBaseline { branch, days, limit, output } => {
            ci_metrics::run_ci_baseline(branch, days, limit, output)
        }
        Commands::CiScope { base, format } => {
            ci_scope::run(ci_scope::CiScopeConfig { base, format })
        }
        Commands::CiPrSummary { base, dry_run } => {
            ci_pr_summary::run(ci_pr_summary::CiPrSummaryConfig { base, dry_run })
        }

        Commands::WorkflowTriggerLint { policy, receipt, fixture, format } => {
            workflow_trigger_lint::run(policy, receipt, fixture, format)
        }
        Commands::CheckVersionSync => check_version_sync::run(),
        Commands::SyncReleaseDocs { write } => sync_release_docs::run(write),
        Commands::CheckFromRaw => ci_policy::check_from_raw(),
        Commands::CheckMemoryLifecyclePolicy => ci_policy::check_memory_lifecycle(),
        Commands::CheckMemoryRetainedOwnerDrift { base, report_only } => {
            ci_policy::check_memory_retained_owner_drift(ci_policy::RetainedOwnerDriftConfig {
                base,
                report_only,
            })
        }
        Commands::MemoryTrends { command } => match command {
            MemoryTrendsCommand::Render { input_dir, history_dirs, baseline, output } => {
                memory_trends::render(memory_trends::MemoryTrendsConfig {
                    input_dir,
                    history_dirs,
                    baseline,
                    output,
                })
            }
        },
        Commands::NativeFormat { command } => match command {
            NativeFormatCommand::Check { fixtures, receipt_dir } => {
                native_format::check(native_format::NativeFormatCheckConfig {
                    fixtures,
                    receipt_dir,
                })
            }
            NativeFormatCommand::Corpus { roots, receipt, summary } => {
                native_format::corpus(native_format::NativeFormatCorpusConfig {
                    roots,
                    receipt,
                    summary,
                })
            }
            NativeFormatCommand::PerltidyCompat { profile, receipt, summary } => {
                native_format::perltidy_compat(native_format::NativeFormatPerltidyCompatConfig {
                    profile,
                    receipt,
                    summary,
                })
            }
            NativeFormatCommand::Config { workspace_root, receipt, summary } => {
                native_format::config(native_format::NativeFormatConfigReceiptConfig {
                    workspace_root,
                    receipt,
                    summary,
                })
            }
        },
        Commands::NativeCritic { command } => match command {
            NativeCriticCommand::Check {
                roots,
                profile,
                severity,
                include,
                exclude,
                receipt,
                summary,
            } => native_critic::check(native_critic::NativeCriticCheckConfig {
                roots,
                profile,
                severity,
                include,
                exclude,
                receipt,
                summary,
            }),
        },
        Commands::NativeTooling { command } => match command {
            NativeToolingCommand::Status {
                format_fixtures,
                format_receipt,
                format_corpus_receipt,
                format_perltidy_compat_receipt,
                format_config_receipt,
                critic_perlcritic_compat_receipt,
                critic_check_receipt,
                critic_false_positive_receipt,
                receipt,
                markdown,
            } => native_tooling::status(native_tooling::NativeToolingStatusConfig {
                format_fixtures,
                format_receipt,
                format_corpus_receipt,
                format_perltidy_compat_receipt,
                format_config_receipt,
                critic_perlcritic_compat_receipt,
                critic_check_receipt,
                critic_false_positive_receipt,
                receipt,
                markdown,
            }),
            NativeToolingCommand::PerlcriticCompat { profile, receipt, summary } => {
                native_tooling::perlcritic_compat(native_tooling::PerlcriticCompatConfig {
                    profile,
                    receipt,
                    summary,
                })
            }
            NativeToolingCommand::CheckDefaults { root } => {
                native_tooling::check_defaults(native_tooling::NativeToolingDefaultsConfig { root })
            }
            NativeToolingCommand::Readiness { status_receipt, receipt, markdown } => {
                native_tooling::readiness(native_tooling::NativeToolingReadinessConfig {
                    status_receipt,
                    receipt,
                    markdown,
                })
            }
        },
        Commands::SecurityHardening => hardening::security_hardening(),
        Commands::PerformanceHardening => hardening::performance_hardening(),
        Commands::ProductionGatesValidation => hardening::production_gates_validation(),
        Commands::ForensicsHarvest { pr } => forensics::run_harvest(&pr),
        Commands::ForensicsTemporal { pr } => forensics::run_temporal(&pr),
        Commands::ForensicsTelemetryQuick { pr } => forensics::run_telemetry_quick(&pr),
        Commands::ForensicsTelemetryFull { pr } => forensics::run_telemetry_full(&pr),
        Commands::ForensicsDossier { pr } => forensics::run_dossier(&pr),
        Commands::ForensicsRender { pr, format } => forensics::run_render(&pr, &format),
        Commands::VerifyPublicationFacts { args } => publication_facts::run(args),
        Commands::GhLabels => github::run_labels(),
        Commands::GhTriage { limit } => github::run_issues_needing_triage(limit),
        Commands::GhBackfillPrefixedLabels { apply } => github::run_backfill_prefixed_labels(apply),
        Commands::CorpusAudit { corpus_path, output, check, fresh } => {
            corpus_audit::run(corpus_audit::AuditConfig {
                corpus_path,
                output_path: output,
                timeout: std::time::Duration::from_secs(30),
                fresh,
                check,
            })
        }
        Commands::CorpusAuditParseOne { path } => corpus_audit::run_parse_one(path),
        Commands::ParserMatrix { report, output } => parser_matrix::run_with_paths(report, output),
        #[cfg(feature = "parser-tasks")]
        Commands::CompareThree { verbose, format } => {
            compare_parsers::run_three_way(verbose, format.as_str())
        }
        Commands::TestLsp { create_only, test, cleanup } => {
            test_lsp::run(create_only, test, cleanup)
        }
        Commands::BumpVersion { version } => bump_version::run(version),
        Commands::PublishCrates { yes, dry_run } => publish::publish_crates(yes, dry_run),
        Commands::PublishRelease { version, dry_run, git_ref } => {
            publish::publish_release(version, dry_run, git_ref)
        }
        Commands::HookCheck => hook_checks::run_hook_check(),
        Commands::HookRegistryCheck => hook_checks::run_hook_registry_check(),
        Commands::HookTests => hook_checks::run_hook_tests(),
        Commands::ForbidFatalConstructs { args } => forbid_fatal_constructs::run(args),
        Commands::CiHygiene { command, args } => ci_hygiene::run(command, args),
        Commands::PublishVscode { yes, token } => publish::publish_vscode(yes, token),
        Commands::PublishClosure { crate_name } => publish_closure::run(crate_name),
        Commands::PublishedCrateCount => count_ratchet::run(),
        Commands::PublishManifestCheck => publish_manifest_check::run(),
        Commands::SmokeTestRelease { version } => publish::smoke_test_release(version),
        Commands::PublishReceipts { date } => publish_receipts::run(date),
        Commands::ParserCorpusSweep {
            roots,
            manifest,
            output,
            baseline,
            enforce,
            verbose,
            receipt,
        } => {
            let base_roots = roots.unwrap_or_else(parser_corpus_sweep::default_base_roots);
            let corpus_roots = parser_corpus_sweep::resolve_corpus_roots(&base_roots);
            parser_corpus_sweep::run(parser_corpus_sweep::SweepConfig {
                corpus_profile: None,
                base_roots,
                corpus_roots,
                manifest_path: manifest,
                manifest_perl5lib: Vec::new(),
                output_path: output,
                baseline_path: baseline,
                enforce,
                verbose,
                receipt,
            })
        }
        Commands::ParserRatchet { command } => match command {
            ParserRatchetCommand::Run { profile, base, head, receipt, force_selected } => {
                parser_ratchet::run(parser_ratchet::ParserRatchetRunConfig {
                    profile,
                    base,
                    head,
                    receipt,
                    force_selected,
                })
            }
        },
        Commands::CpanCorpus { command } => {
            let mut config = cpan_corpus::CpanCorpusConfig::default();
            match command {
                CpanCorpusCommand::FetchList { top_n, output } => {
                    config.top_n = top_n;
                    if let Some(out) = output {
                        config.dist_list = out;
                    }
                    cpan_corpus::fetch_list(&config)
                }
                CpanCorpusCommand::Install { dist_list, install_dir, verbose, reset } => {
                    if let Some(dl) = dist_list {
                        config.dist_list = dl;
                    }
                    config.force_reset = reset;
                    if let Some(id) = install_dir {
                        config.install_dir = id;
                    }
                    config.verbose = verbose;
                    cpan_corpus::install(&config)
                }
                CpanCorpusCommand::Sweep { output, enforce, verbose, install_dir } => {
                    if let Some(id) = install_dir {
                        config.install_dir = id;
                    }
                    config.verbose = verbose;
                    cpan_corpus::sweep(&config, output, enforce)
                }
                CpanCorpusCommand::Ratchet { verbose, install_dir } => {
                    if let Some(id) = install_dir {
                        config.install_dir = id;
                    }
                    config.verbose = verbose;
                    cpan_corpus::ratchet(&config)
                }
            }
        }
        Commands::Receipts { tests_only, docs_only, output_dir, test_threads } => {
            receipts::run(receipts::ReceiptsConfig {
                tests_only,
                docs_only,
                output_dir,
                test_threads,
            })
        }
        Commands::AggregateReceipts { check, inputs, output, allow_noop } => {
            aggregate_receipts::run(aggregate_receipts::AggregateReceiptsConfig {
                check,
                inputs,
                output,
                allow_noop,
            })
        }
        Commands::FinalizeCheck { receipt, allow_noop, fail_on_advisory } => {
            finalize_check::run(finalize_check::FinalizeCheckConfig {
                receipt,
                allow_noop,
                fail_on_advisory,
            })
        }
        Commands::MergeReady { command } => match command {
            MergeReadyCommand::Emit { pr, receipt } => merge_ready::emit(pr, receipt),
            MergeReadyCommand::Verify { pr, fixture } => merge_ready::verify(pr, fixture),
            MergeReadyCommand::Reconcile { apply, dry_run } => {
                let run_dry = !apply || dry_run;
                merge_ready::reconcile(run_dry)
            }
            MergeReadyCommand::ReconcileQueue { apply: _, dry_run, pr, receipt } => {
                // Apply is the default. Only switch to dry-run when --dry-run is explicitly passed.
                let do_apply = !dry_run;
                queue_reconciler::reconcile_queue(do_apply, pr, receipt)
            }
        },
        Commands::IgnoredTests { update, check, verbose } => {
            ignored_tests::run(update, check, verbose)
        }
        Commands::DebtReport { check, json, summary, expired, ledger } => {
            debt_report::run(debt_report::DebtReportConfig {
                check,
                json,
                summary,
                expired,
                ledger,
            })
        }
        Commands::DocClaims => doc_claims::run(),
        Commands::InstallSurfaceCheck => install_surface_check::run(),
        Commands::IntentDiffGate { pr, fixture, receipt } => {
            intent_diff_gate::run(intent_diff_gate::IntentDiffGateConfig { pr, fixture, receipt })
        }
        Commands::Features { command } => match command {
            FeaturesCommand::SyncDocs => features::sync_docs(),
            FeaturesCommand::Verify => features::verify(),
            FeaturesCommand::Invariants => features::invariants(),
            FeaturesCommand::Report => features::report(),
        },
        Commands::Agent { command } => match command {
            AgentCommand::Lease { command } => match command {
                AgentLeaseCommand::Acquire { task, out } => agent_lease::acquire(&task, &out),
                AgentLeaseCommand::Verify { lease, current } => {
                    agent_lease::verify(&lease, &current)
                }
            },
            AgentCommand::Receipt { command } => match command {
                AgentReceiptCommand::Validate { receipt } => agent_receipt::validate(&receipt),
            },
            AgentCommand::Worktree { command } => worktree_allocator::run(command),
        },
        Commands::FixForward { command } => match command {
            FixForwardCommand::Classify { receipt, output } => {
                fix_forward::classify(receipt, output)
            }
            FixForwardCommand::ListPlaybooks => fix_forward::list_playbooks(),
        },
        Commands::UpdateStatus { write, check, only } => update_status::run(write, check, only),
        Commands::Srp { command } => match command {
            SrpCommand::Microcrates(args) => srp_microcrates::run(args.output),
            SrpCommand::LayerCheck => layer_check::run(),
            SrpCommand::UnwiredScan(args) => unwired_scan::run(UnwiredScanConfig {
                lsp_crate: args.lsp_crate,
                json: args.json,
                check: args.check,
            }),
            SrpCommand::CheckTestWiring => check_test_wiring::run(),
        },
        Commands::SrpMicrocrates { args } => srp_microcrates::run(args.output),
        Commands::LayerCheck => layer_check::run(),
        Commands::UnwiredScan { args } => unwired_scan::run(UnwiredScanConfig {
            lsp_crate: args.lsp_crate,
            json: args.json,
            check: args.check,
        }),
        Commands::CheckTestWiring => check_test_wiring::run(),
        Commands::Metrics { command } => match command {
            MetricsCommand::ParserStats { input, json } => metrics::parser_stats::run(input, json),
            MetricsCommand::ParserAccuracy {
                json,
                check,
                export_status_receipts,
                manifest,
                output,
                cadence,
            } => metrics::parser_accuracy::run(
                json,
                check,
                export_status_receipts,
                manifest,
                output,
                &cadence,
            ),
            MetricsCommand::HirCoverage { json, output, write_status, check } => {
                metrics::hir_coverage::run(json, output, write_status, check)
            }
            MetricsCommand::LspStats { json, receipt_dir } => {
                metrics::lsp_stats::run_with_receipt_dir(json, receipt_dir.as_deref())
            }
            MetricsCommand::WorkspaceStats => metrics::workspace_stats::run(),
            MetricsCommand::DiagnosticsStats => metrics::diagnostics_stats::run(),
            MetricsCommand::Memory {
                workload_json,
                plateau_json,
                scenario,
                receipt,
                commit,
                event,
                markdown,
            } => {
                let scenario = match scenario {
                    Some(scenario) => scenario,
                    None => metrics::memory::infer_scenario(&workload_json)
                        .map_err(|error| eyre!(error.to_string()))?,
                };
                metrics::memory::run(metrics::memory::MemoryMetricsConfig {
                    scenario,
                    workload_json,
                    plateau_json,
                    receipt,
                    commit,
                    event,
                    markdown,
                })
            }
            MetricsCommand::ReleaseHealth { days, json } => {
                metrics::release_health::run(days, json)
            }
            MetricsCommand::RatchetCheck { subsystem, current, record } => {
                let root = utils::project_root()?;
                metrics::ratchet::run_ratchet_check(&root, &subsystem, current, record)
            }
            MetricsCommand::PromoteBaseline { subsystem, delta_pct } => {
                let root = utils::project_root()?;
                metrics::ratchet::run_promote_baseline(&root, &subsystem, delta_pct)
            }
            MetricsCommand::SweepStats { input } => metrics::sweep_stats::run(input),
        },
        Commands::UxScorecard { format, input, output, status_md, ratchet_check } => {
            let format = match format {
                UxScorecardOutputFormat::Human => UxScorecardFormat::Human,
                UxScorecardOutputFormat::Json => UxScorecardFormat::Json,
            };
            ux_scorecard::run(format, input, output, status_md, ratchet_check)
        }
        Commands::SemanticScorecard { manifest, output, status_md, check } => {
            semantic_scorecard::run(manifest, output, status_md, check)
        }
        Commands::SemanticShadowCompare { output, status_md, check } => {
            semantic_shadow_compare::run(output, status_md, check)
        }
        Commands::UxRegressionReceipt { input, receipt, sha } => {
            ux_regression_receipt::run(ux_regression_receipt::UxRegressionReceiptConfig {
                input,
                receipt,
                sha,
            })
        }
        Commands::ValidateMemoryProfiler => compare::validate_memory_profiling(),
        Commands::E2eValidate { workspace_size, report, skip_workspace, skip_bench, verbose } => {
            e2e_validate::run(e2e_validate::E2eConfig {
                workspace_size,
                report_path: report,
                skip_workspace,
                skip_bench,
                verbose,
            })
        }
        Commands::Gates {
            tier,
            gate,
            base,
            list,
            format,
            receipt,
            receipt_path,
            diff,
            fail_fast,
            parallel,
            verbose,
        } => gates::run(gates::GateRunnerConfig {
            tier,
            gate_filter: gate,
            base_ref: base,
            output_format: format,
            emit_receipt: receipt,
            receipt_path,
            diff_baseline: diff,
            list_only: list,
            fail_fast,
            parallel,
            verbose,
        }),
        Commands::GatePolicy { command } => match command {
            GatePolicyCommand::Check => tasks::gate_policy::check(),
            GatePolicyCommand::Effective { profile } => tasks::gate_policy::effective(profile),
        },
        Commands::GateReceipts { command } => match command {
            GateReceiptsCommand::List { format } => {
                gate_receipts::list(convert_gate_receipts_format(format))
                    .map_err(|error| eyre!(error.to_string()))
            }
            GateReceiptsCommand::Validate { path, format } => {
                gate_receipts::validate(&path, convert_gate_receipts_format(format))
                    .map_err(|error| eyre!(error.to_string()))
            }
            GateReceiptsCommand::ValidateAll { dir, format } => {
                gate_receipts::validate_all(&dir, convert_gate_receipts_format(format))
                    .map_err(|error| eyre!(error.to_string()))
            }
        },
        Commands::MethodologyGate { fixture, pr, receipt, dry_run, enforce, format } => {
            methodology_gate::run(methodology_gate::MethodologyGateConfig {
                fixture,
                pr,
                receipt,
                dry_run,
                enforce,
                format,
            })
        }
        Commands::TargetedChecks { base, mode } => targeted_checks::run(base, mode),
        Commands::ResolvePackageName { crate_dir } => {
            // Use the current working directory as workspace root so this subcommand
            // works correctly both in the main workspace and in test synthetic workspaces.
            let root = std::env::current_dir()
                .map_err(|e| eyre!("Failed to get current working directory: {e}"))?;
            let name = tasks::targeted_checks::resolve_single_package_name(&root, &crate_dir)?;
            println!("{name}");
            Ok(())
        }
        Commands::WorktreeCleanup => worktrees::cleanup(),
        Commands::ValidateSwarmAgentRoster { root } => swarm_agent_roster::run(root),
        Commands::SwarmSummary { ops_dir, since, limit, format } => {
            swarm_summary::run(swarm_summary::SwarmSummaryConfig { ops_dir, since, limit, format })
        }
        Commands::PopulateBook => populate_book::run(),
        Commands::ValidateWorkspaceExclusions => validate_workspace_exclusions::run(),
        Commands::BuildTimingReceipt { clean, incremental, tests, output, baseline } => {
            build_timing::run_receipt(clean, incremental, tests, output, baseline)
        }
        Commands::CompareBuildTiming { baseline, current } => {
            build_timing::run_compare(baseline, current)
        }
        Commands::GeneratedFiles { command } => match command {
            GeneratedFilesCommand::List { fixture } => generated_files::list(fixture),
            GeneratedFilesCommand::Check {
                receipt,
                fixture,
                generator_receipt,
                allow_manual_edits,
            } => generated_files::check(receipt, fixture, generator_receipt, allow_manual_edits),
        },
        Commands::NonRust { command } => match command {
            NonRustCommand::Inventory => {
                let root = utils::project_root()?;
                tasks::file_policy::non_rust_inventory(&root)
            }
            NonRustCommand::Check { mode, json, allowlist, root: root_override } => {
                use tasks::file_policy::{CheckFilePolicyConfig, CheckFilePolicyMode};
                let root = utils::project_root()?;
                let mode = match mode {
                    CheckFilePolicyCliMode::Advisory => CheckFilePolicyMode::Advisory,
                    CheckFilePolicyCliMode::BlockingAllowlist => {
                        CheckFilePolicyMode::BlockingAllowlist
                    }
                    CheckFilePolicyCliMode::BlockingStrict => CheckFilePolicyMode::BlockingStrict,
                };
                tasks::file_policy::check_file_policy(
                    &root,
                    CheckFilePolicyConfig {
                        mode,
                        json_output: json,
                        allowlist_path: allowlist,
                        root_override,
                    },
                )
            }
            NonRustCommand::Propose { output_dir, group_by, root: root_override } => {
                use tasks::file_policy::{ProposeConfig, ProposeGroupBy};
                let root = utils::project_root()?;
                let group_by = match group_by {
                    ProposeGroupByArg::Directory => ProposeGroupBy::Directory,
                    ProposeGroupByArg::Extension => ProposeGroupBy::Extension,
                };
                tasks::file_policy::non_rust_propose(
                    &root,
                    ProposeConfig { output_dir, group_by, root_override },
                )
            }
            NonRustCommand::ValidatePolicy { allowlist, debt } => {
                use tasks::file_policy::ValidateNonRustPolicyConfig;
                tasks::file_policy::validate_non_rust_policy(ValidateNonRustPolicyConfig {
                    allowlist_path: allowlist,
                    debt_path: debt,
                })
            }
            NonRustCommand::MigrationCandidates { format, output, limit, root: root_override } => {
                use tasks::file_policy::{MigrationCandidateFormat, MigrationCandidatesConfig};
                let root = utils::project_root()?;
                let format = match format {
                    MigrationCandidateFormatArg::Markdown => MigrationCandidateFormat::Markdown,
                    MigrationCandidateFormatArg::Json => MigrationCandidateFormat::Json,
                };
                tasks::file_policy::non_rust_migration_candidates(
                    &root,
                    MigrationCandidatesConfig { format, output, limit, root_override },
                )
            }
        },
        Commands::CheckFilePolicy { mode, json, allowlist, root: root_override } => {
            use tasks::file_policy::{CheckFilePolicyConfig, CheckFilePolicyMode};
            let root = utils::project_root()?;
            let mode = match mode {
                CheckFilePolicyCliMode::Advisory => CheckFilePolicyMode::Advisory,
                CheckFilePolicyCliMode::BlockingAllowlist => CheckFilePolicyMode::BlockingAllowlist,
                CheckFilePolicyCliMode::BlockingStrict => CheckFilePolicyMode::BlockingStrict,
            };
            tasks::file_policy::check_file_policy(
                &root,
                CheckFilePolicyConfig {
                    mode,
                    json_output: json,
                    allowlist_path: allowlist,
                    root_override,
                },
            )
        }
        Commands::FreshnessCheck {
            base,
            mode,
            json,
            no_fetch,
            allow_historical,
            reason,
            binaries,
        } => {
            use tasks::freshness_check::{FreshnessCheckConfig, FreshnessMode};
            let mode = match mode {
                FreshnessCheckMode::Warn => FreshnessMode::Warn,
                FreshnessCheckMode::Block => FreshnessMode::Block,
            };
            tasks::freshness_check::run(FreshnessCheckConfig {
                base,
                mode,
                json_output: json,
                no_fetch,
                allow_historical,
                reason,
                check_binaries: binaries,
            })
        }
    }
}

fn print_top_level_commands() {
    let mut command_names = Cli::command()
        .get_subcommands()
        .map(|subcommand| subcommand.get_name().to_string())
        .collect::<Vec<_>>();
    command_names.sort_unstable();

    for command_name in command_names {
        println!("{command_name}");
    }
}

fn convert_gate_receipts_format(format: GateReceiptsFormat) -> gate_receipts::OutputFormat {
    match format {
        GateReceiptsFormat::Human => gate_receipts::OutputFormat::Human,
        GateReceiptsFormat::Json => gate_receipts::OutputFormat::Json,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    fn parse_devex_command(args: &[&str]) -> TestResult<DevexCommand> {
        match Cli::try_parse_from(args)?.command {
            Commands::Devex { command } => Ok(command),
            _ => Err(std::io::Error::other("expected devex command").into()),
        }
    }

    #[test]
    fn devex_commands_default_to_auto_base() -> TestResult {
        let cases = [
            (["xtask", "devex", "plan"].as_slice(), "plan"),
            (["xtask", "devex", "receipt"].as_slice(), "receipt"),
            (["xtask", "devex", "cockpit"].as_slice(), "cockpit"),
            (["xtask", "devex", "pr-body"].as_slice(), "pr-body"),
        ];

        for (args, name) in cases {
            let base = match parse_devex_command(args)? {
                DevexCommand::Plan { base }
                | DevexCommand::Receipt { base, .. }
                | DevexCommand::Cockpit { base, .. }
                | DevexCommand::PrBody { base, .. } => base,
            };
            assert_eq!(base, "auto", "{name} should auto-detect the diff base by default");
        }

        Ok(())
    }

    #[test]
    fn devex_plan_respects_explicit_base() -> TestResult {
        match parse_devex_command(&["xtask", "devex", "plan", "--base", "HEAD~1"])? {
            DevexCommand::Plan { base } => assert_eq!(base, "HEAD~1"),
            _ => return Err(std::io::Error::other("expected devex plan command").into()),
        }

        Ok(())
    }
}
