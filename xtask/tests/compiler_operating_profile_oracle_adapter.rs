//! Executable proof for the `oracle_receipt.v1` observation adapter (#12302,
//! train row COMP-PROFILE-E05A).
//!
//! The suite runs against the adapter's public surface, the same way the
//! evidence-set assembly lane (E07) will consume it. Every falsifier the
//! controlling issue names has a test here, plus the negative controls that
//! keep the falsifiers from passing vacuously: if the adapter rejected or
//! down-typed everything, `acceptance_*` would fail.

// The suite lives in a module named for the target so the controlling issue's
// `cargo test -p xtask --locked compiler_operating_profile_oracle_adapter`
// selects it by test path; a bare target name filters nothing.
mod compiler_operating_profile_oracle_adapter {
    use anyhow::{Result, bail};
    use serde_json::{Value, json};
    use xtask::compiler_profile_contract::ClaimCeiling;
    use xtask::compiler_profile_observation::{
        CompilerProfileObservationV1, CompletenessDisposition, CurrentnessDisposition,
        LimitationDisposition, ObservationDisposition, ReceiptFamily, SchemaVersion,
        SubjectDimension, SubjectDimensionKind, TerminalState, WorkDisposition,
    };
    use xtask::compiler_profile_oracle_adapter::{
        self as adapter, ADAPTER_ID, ADAPTER_VERSION, SOURCE_FAMILY, SOURCE_SCHEMA_VERSION,
    };

    // ---------------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------------

    // Receipt construction is test-only. Production exposes adaptation, not a
    // second fixture API that downstream evidence lanes could accidentally adopt.
    /// A fully agreeing, hermetic, complete receipt.
    fn agreeing_receipt() -> Value {
        json!({
            "schema_version": "oracle_receipt.v1",
            "receipt_id": "oracle-receipt-0001",
            "comparison_class": "IsaComposition",
            "fixture_id": "isa-composition-basic",
            "source_snapshot": {
                "path_class": "public_test_fixture",
                "fixture_source": "differential_oracle/isa_composition_basic.pl",
                "content_hash": "sha256:2f1c9a"
            },
            "rust_extractor": {
                "name": "perl-semantic-facts",
                "version": "0.8.3",
                "fact_model": "package-sub-table.v1"
            },
            "perl_oracle": {
                "interpreter": "declared_fixture_perl",
                "version": "v5.38.0",
                "invocation_mode": "declared_fixture_command"
            },
            "module_path_authority": {
                "authority": "declared_fixture_root",
                "declared_roots": ["fixtures/differential_oracle/lib"],
                "ambient_roots_reported": false
            },
            "environment": {
                "denied": ["PERL5LIB", "PERL5OPT", "local::lib"],
                "declared": ["PATH"],
                "redacted_values": true
            },
            "ambient_inputs": [],
            "generated_inputs": [],
            "dynamic_boundaries": [],
            "stale_facts": [],
            "unsupported_effects": [],
            "normalized_facts": {
                "rust": [fact("fact-isa-1", "Child::ISA"), fact("fact-isa-2", "Child::new")],
                "oracle": [fact("fact-isa-1", "Child::ISA"), fact("fact-isa-2", "Child::new")]
            },
            "comparisons": [
                comparison("oracle_agrees", "fact-isa-1", "supports_promotion"),
                comparison("oracle_agrees", "fact-isa-2", "supports_promotion")
            ],
            "provider_behavior_changed": false,
            "editor_runtime_dependency": false,
            "redaction": {
                "private_paths_redacted": true,
                "environment_values_redacted": true,
                "raw_launch_payloads_redacted": true
            },
            "claim_boundary": "one fixture, one comparison class, test-only oracle evidence"
        })
    }

    /// One fresh, high-confidence, explicit-source normalized fact.
    fn fact(fact_id: &str, name: &str) -> Value {
        json!({
            "fact_id": fact_id,
            "name": name,
            "provenance": "ExplicitSource",
            "confidence": "high",
            "freshness": "fresh",
            "fallback": "none",
            "source_range": {
                "path_class": "public_test_fixture",
                "start_line": 3,
                "start_character": 0,
                "end_line": 3,
                "end_character": 24
            }
        })
    }

    /// One typed comparison row.
    fn comparison(result_class: &str, fact_id: &str, promotion_effect: &str) -> Value {
        json!({
            "result_class": result_class,
            "fact_id": fact_id,
            "promotion_effect": promotion_effect,
            "message": "bounded explanatory text that is never parsed for semantics"
        })
    }

    fn adapt(receipt: &Value) -> Result<CompilerProfileObservationV1> {
        adapter::adapt_receipt_value(receipt)
    }

    /// Assert that a receipt is refused, and that the refusal names the reason.
    fn assert_rejected(receipt: &Value, needle: &str) -> Result<()> {
        match adapt(receipt) {
            Ok(_) => bail!("receipt must fail closed ({needle})"),
            Err(error) => {
                let text = format!("{error:#}");
                assert!(
                    text.contains(needle),
                    "refusal must name {needle:?}; adapter said: {text}"
                );
                Ok(())
            }
        }
    }

    fn identity(receipt: &Value) -> Result<String> {
        Ok(adapt(receipt)?.identity()?.as_str().to_owned())
    }

    fn digest(receipt: &Value) -> Result<String> {
        Ok(adapt(receipt)?.receipt.digest.as_str().to_owned())
    }

    fn dimension(observation: &CompilerProfileObservationV1, kind: SubjectDimensionKind) -> String {
        match observation.subject.dimension(kind) {
            SubjectDimension::Proven(value) => value,
            SubjectDimension::NotProven => "not_proven".to_owned(),
        }
    }

    /// A receipt that agrees on every axis but whose `content_hash` distinguishes
    /// it, so identity comparisons cannot be satisfied by an accidental match.
    fn agreeing_with(mutate: impl FnOnce(&mut Value)) -> Value {
        let mut receipt = agreeing_receipt();
        mutate(&mut receipt);
        receipt
    }

    // ---------------------------------------------------------------------------
    // Negative controls: the adapter really does accept good evidence
    // ---------------------------------------------------------------------------

    #[test]
    fn acceptance_agreeing_receipt_reaches_pass_and_accepted_compatibility() -> Result<()> {
        let observation = adapt(&agreeing_receipt())?;

        assert_eq!(observation.disposition, ObservationDisposition::Pass);
        assert_eq!(observation.currentness, CurrentnessDisposition::Current);
        assert_eq!(observation.completeness, CompletenessDisposition::Complete);
        assert_eq!(observation.limitation, LimitationDisposition::None);
        assert_eq!(observation.instrument.terminal, TerminalState::Completed);
        assert!(matches!(observation.work, WorkDisposition::Completed { .. }));
        assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::AcceptedCompatibility);
        assert_eq!(observation.adapter.id.as_str(), ADAPTER_ID);
        assert_eq!(observation.adapter.version.as_str(), ADAPTER_VERSION);
        assert_eq!(observation.receipt.producer.family.as_str(), SOURCE_FAMILY);
        assert_eq!(observation.receipt.producer.schema.get(), SOURCE_SCHEMA_VERSION);
        Ok(())
    }

    #[test]
    fn acceptance_adapted_observation_satisfies_its_own_registry() -> Result<()> {
        let registry = adapter::oracle_receipt_registry()?;
        let observation = adapt(&agreeing_receipt())?;

        registry.validate_observation(&observation)?;

        let selected =
            registry.select_adapter(&ReceiptFamily::new(SOURCE_FAMILY)?, SchemaVersion::new(1))?;
        assert_eq!(selected.id.as_str(), ADAPTER_ID);
        Ok(())
    }

    #[test]
    fn acceptance_adapter_vocabulary_matches_the_production_schema() -> Result<()> {
        adapter::ensure_vocabulary_current()
    }

    #[test]
    fn acceptance_unnamed_dimensions_stay_explicitly_not_proven() -> Result<()> {
        let observation = adapt(&agreeing_receipt())?;

        for kind in [
            SubjectDimensionKind::RepositoryTree,
            SubjectDimensionKind::BinaryArtifact,
            SubjectDimensionKind::Platform,
            SubjectDimensionKind::ObservationTime,
        ] {
            assert_eq!(
                observation.subject.dimension(kind),
                SubjectDimension::NotProven,
                "an oracle receipt cannot prove {}",
                kind.tag()
            );
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 01 — one comparison class / fixture / fact fills another
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_01_another_comparison_class_is_another_subject() -> Result<()> {
        let base = agreeing_receipt();
        let other = agreeing_with(|receipt| receipt["comparison_class"] = json!("CompileEffect"));

        assert_ne!(identity(&base)?, identity(&other)?);
        assert_ne!(
            dimension(&adapt(&base)?, SubjectDimensionKind::FixtureSeries),
            dimension(&adapt(&other)?, SubjectDimensionKind::FixtureSeries)
        );
        Ok(())
    }

    #[test]
    fn falsifier_01_another_fixture_is_another_subject() -> Result<()> {
        let base = agreeing_receipt();
        let other =
            agreeing_with(|receipt| receipt["fixture_id"] = json!("isa-composition-diamond"));

        assert_ne!(identity(&base)?, identity(&other)?);
        Ok(())
    }

    #[test]
    fn falsifier_01_a_comparison_over_an_unnamed_fact_is_not_complete() -> Result<()> {
        let receipt = agreeing_with(|receipt| {
            receipt["comparisons"][1]["fact_id"] = json!("fact-isa-absent");
        });

        let observation = adapt(&receipt)?;

        assert!(
            matches!(observation.completeness, CompletenessDisposition::Partial { .. }),
            "a comparison over a fact no set names cannot read as complete: {:?}",
            observation.completeness
        );
        assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 02 — changed identity reuses old evidence
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_02_changed_identity_cannot_reuse_old_evidence() -> Result<()> {
        let base = identity(&agreeing_receipt())?;

        let mutations: [(&str, fn(&mut Value)); 5] = [
            ("source content", |receipt| {
                receipt["source_snapshot"]["content_hash"] = json!("sha256:ffffff");
            }),
            ("extractor version", |receipt| {
                receipt["rust_extractor"]["version"] = json!("0.9.0");
            }),
            ("perl version", |receipt| {
                receipt["perl_oracle"]["version"] = json!("v5.40.0");
            }),
            ("module roots", |receipt| {
                receipt["module_path_authority"]["declared_roots"] = json!(["fixtures/other/lib"]);
            }),
            ("declared environment", |receipt| {
                receipt["environment"]["declared"] = json!(["PATH", "TMPDIR"]);
            }),
        ];

        for (name, mutate) in mutations {
            let changed = agreeing_with(mutate);
            assert_ne!(
                base,
                identity(&changed)?,
                "a changed {name} must not reuse the old identity"
            );
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 03 — equal output from another producer proves compiler authority
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_03_equal_output_from_another_producer_is_another_subject() -> Result<()> {
        let base = adapt(&agreeing_receipt())?;
        let shadow = adapt(&agreeing_with(|receipt| {
            // Byte-identical facts and comparisons; only the producing extractor
            // and its fact model differ.
            receipt["rust_extractor"]["name"] = json!("legacy-shadow-extractor");
            receipt["rust_extractor"]["fact_model"] = json!("legacy-shadow.v0");
        }))?;

        assert_ne!(base.identity()?.as_str(), shadow.identity()?.as_str());
        assert_ne!(
            dimension(&base, SubjectDimensionKind::Toolchain),
            dimension(&shadow, SubjectDimensionKind::Toolchain)
        );
        assert_ne!(
            dimension(&base, SubjectDimensionKind::CompilerPolicy),
            dimension(&shadow, SubjectDimensionKind::CompilerPolicy)
        );
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 04 — unknown / system / shadow Perl becomes an exact oracle
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_04_unknown_perl_subject_cannot_satisfy_an_exact_row() -> Result<()> {
        for pointer in ["interpreter", "invocation_mode"] {
            let receipt = agreeing_with(|receipt| {
                receipt["perl_oracle"][pointer] = json!("unknown");
            });

            let observation = adapt(&receipt)?;

            assert_eq!(
                observation.disposition,
                ObservationDisposition::NotProven,
                "an unknown {pointer} cannot pass"
            );
            assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        }
        Ok(())
    }

    #[test]
    fn falsifier_04_system_perl_and_shadow_commands_stay_bounded() -> Result<()> {
        let system = adapt(&agreeing_with(|receipt| {
            receipt["perl_oracle"]["interpreter"] = json!("system_perl");
        }))?;
        let shadow = adapt(&agreeing_with(|receipt| {
            receipt["perl_oracle"]["invocation_mode"] = json!("shadow_test_command");
        }))?;

        for observation in [&system, &shadow] {
            assert!(
                matches!(observation.limitation, LimitationDisposition::AcceptedDebt { .. }),
                "ambient or shadow oracle evidence stays visibly bounded: {:?}",
                observation.limitation
            );
            assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        }
        // They are also different subjects from the declared fixture oracle.
        assert_ne!(system.identity()?.as_str(), shadow.identity()?.as_str());
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 05 — ambient / unbounded authority becomes hermetic evidence
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_05_ambient_authority_cannot_satisfy_a_hermetic_row() -> Result<()> {
        let ambient_authority = agreeing_with(|receipt| {
            receipt["module_path_authority"]["authority"] = json!("ambient_reported");
        });
        let ambient_roots = agreeing_with(|receipt| {
            receipt["module_path_authority"]["ambient_roots_reported"] = json!(true);
        });
        let unbounded = agreeing_with(|receipt| {
            receipt["ambient_inputs"] = json!([{ "kind": "cwd", "authority": "unbounded" }]);
        });

        for receipt in [&ambient_authority, &ambient_roots, &unbounded] {
            let observation = adapt(receipt)?;
            assert_eq!(observation.disposition, ObservationDisposition::NotProven);
            assert_eq!(observation.currentness, CurrentnessDisposition::NotProven);
            assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        }
        Ok(())
    }

    #[test]
    fn falsifier_05_reported_only_ambient_input_stays_a_visible_limitation() -> Result<()> {
        let observation = adapt(&agreeing_with(|receipt| {
            receipt["ambient_inputs"] = json!([{ "kind": "locale", "authority": "reported_only" }]);
        }))?;

        assert!(matches!(observation.limitation, LimitationDisposition::AcceptedDebt { .. }));
        assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 06 — denied environment leakage is ignored
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_06_denied_environment_leakage_fails_the_instrument() -> Result<()> {
        let observation = adapt(&agreeing_with(|receipt| {
            receipt["environment"]["declared"] = json!(["PATH", "PERL5LIB"]);
        }))?;

        assert!(
            matches!(observation.instrument.terminal, TerminalState::InstrumentFailed { .. }),
            "a declared denied key is a hermeticity failure: {:?}",
            observation.instrument.terminal
        );
        assert_eq!(observation.disposition, ObservationDisposition::NotProven);
        assert_eq!(observation.completeness, CompletenessDisposition::NotProven);
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 07 — generated / dynamic / ambient / unknown becomes explicit source
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_07_non_explicit_provenance_cannot_become_explicit_source() -> Result<()> {
        let generated = agreeing_with(|receipt| {
            receipt["generated_inputs"] = json!([{
                "framework": "Moo",
                "provenance": "GeneratedNoSource",
                "source_range": null
            }]);
        });
        let dynamic_fact = agreeing_with(|receipt| {
            receipt["normalized_facts"]["rust"][0]["provenance"] = json!("DynamicBoundary");
        });
        let unknown_fact = agreeing_with(|receipt| {
            receipt["normalized_facts"]["oracle"][0]["provenance"] = json!("Unknown");
        });

        for receipt in [&generated, &dynamic_fact, &unknown_fact] {
            let observation = adapt(receipt)?;
            assert!(
                matches!(observation.completeness, CompletenessDisposition::Partial { .. }),
                "non-explicit provenance stays in the denominator: {:?}",
                observation.completeness
            );
            assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        }
        Ok(())
    }

    #[test]
    fn falsifier_07_dynamic_and_unsupported_boundaries_stay_in_the_denominator() -> Result<()> {
        let dynamic = adapt(&agreeing_with(|receipt| {
            receipt["dynamic_boundaries"] =
                json!([{ "kind": "symbolic-method-call", "source_range": null }]);
        }))?;
        let unsupported = adapt(&agreeing_with(|receipt| {
            receipt["unsupported_effects"] =
                json!([{ "kind": "BEGIN-eval", "source_range": null }]);
        }))?;

        assert!(matches!(dynamic.completeness, CompletenessDisposition::Partial { .. }));
        assert!(matches!(unsupported.completeness, CompletenessDisposition::Partial { .. }));
        assert!(matches!(unsupported.limitation, LimitationDisposition::AcceptedDebt { .. }));
        for observation in [&dynamic, &unsupported] {
            assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        }

        // The two arrays share a shape but not a meaning, and the axes they
        // take are a deliberate choice rather than an accident: pin it so a
        // later edit cannot silently swap them. A dynamic boundary is an
        // incompleteness of this run; an unsupported effect is also a standing
        // limitation on what the class can ever claim.
        assert_eq!(
            dynamic.limitation,
            LimitationDisposition::None,
            "a dynamic boundary is incompleteness now, not a standing limitation"
        );
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 08 — stale / unknown / low-confidence / fallback becomes support
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_08_stale_evidence_cannot_read_as_current() -> Result<()> {
        let declared_stale = adapt(&agreeing_with(|receipt| {
            receipt["stale_facts"] = json!([{ "fact_id": "fact-isa-1", "freshness": "stale" }]);
        }))?;
        let stale_fact = adapt(&agreeing_with(|receipt| {
            receipt["normalized_facts"]["rust"][0]["freshness"] = json!("stale");
        }))?;

        for observation in [&declared_stale, &stale_fact] {
            assert_eq!(observation.currentness, CurrentnessDisposition::Stale);
            assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        }
        Ok(())
    }

    #[test]
    fn falsifier_08_unknown_freshness_is_not_proven_current() -> Result<()> {
        let observation = adapt(&agreeing_with(|receipt| {
            receipt["normalized_facts"]["oracle"][1]["freshness"] = json!("unknown");
        }))?;

        assert_eq!(observation.currentness, CurrentnessDisposition::NotProven);
        assert_eq!(observation.disposition, ObservationDisposition::NotProven);
        Ok(())
    }

    #[test]
    fn falsifier_08_not_applicable_freshness_is_bounded_but_distinct_from_unknown() -> Result<()> {
        // The two states are separate members of the source vocabulary and
        // must not be flattened into each other. `unknown` blocks: nobody knows
        // whether the fact is current. `not_applicable` is a positive
        // declaration that the fact has no freshness dimension, so it does not
        // block — but it is not evidence of freshness either, so it stays a
        // visible bound and cannot reach the top ceiling silently.
        let inapplicable = adapt(&agreeing_with(|receipt| {
            receipt["normalized_facts"]["rust"][0]["freshness"] = json!("not_applicable");
            receipt["normalized_facts"]["oracle"][0]["freshness"] = json!("not_applicable");
        }))?;
        assert_eq!(inapplicable.disposition, ObservationDisposition::Pass);
        assert_eq!(inapplicable.currentness, CurrentnessDisposition::Current);
        assert!(
            matches!(inapplicable.limitation, LimitationDisposition::AcceptedDebt { .. }),
            "inapplicable freshness stays visible: {:?}",
            inapplicable.limitation
        );
        assert_eq!(inapplicable.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);

        // `unknown` is the stronger statement and keeps blocking.
        let unknown = adapt(&agreeing_with(|receipt| {
            receipt["normalized_facts"]["rust"][0]["freshness"] = json!("unknown");
            receipt["normalized_facts"]["oracle"][0]["freshness"] = json!("unknown");
        }))?;
        assert_eq!(unknown.disposition, ObservationDisposition::NotProven);
        assert_eq!(unknown.currentness, CurrentnessDisposition::NotProven);
        Ok(())
    }

    #[test]
    fn a_range_that_ends_before_it_starts_anchors_nothing() -> Result<()> {
        // The schema constrains each coordinate to be non-negative but cannot
        // relate them. Two sides carrying the *same* malformed range would
        // otherwise agree and count as source-backed.
        let reversed = json!({
            "path_class": "public_test_fixture",
            "start_line": 7, "start_character": 4,
            "end_line": 3, "end_character": 0
        });

        assert_rejected(
            &agreeing_with(|receipt| {
                receipt["normalized_facts"]["rust"][0]["source_range"] = reversed.clone();
                receipt["normalized_facts"]["oracle"][0]["source_range"] = reversed.clone();
            }),
            "anchors no region of the source",
        )?;
        assert_rejected(
            &agreeing_with(|receipt| {
                receipt["generated_inputs"] = json!([{
                    "framework": "Moo",
                    "provenance": "SourceBackedGenerated",
                    "source_range": reversed.clone()
                }]);
            }),
            "generated input",
        )?;
        assert_rejected(
            &agreeing_with(|receipt| {
                receipt["dynamic_boundaries"] =
                    json!([{ "kind": "symbolic-call", "source_range": reversed.clone() }]);
            }),
            "boundary",
        )?;

        // A zero-width range at one point is well formed, not reversed.
        adapt(&agreeing_with(|receipt| {
            receipt["normalized_facts"]["rust"][0]["source_range"] = json!({
                "path_class": "public_test_fixture",
                "start_line": 3, "start_character": 8,
                "end_line": 3, "end_character": 8
            });
            receipt["normalized_facts"]["oracle"][0]["source_range"] = json!({
                "path_class": "public_test_fixture",
                "start_line": 3, "start_character": 8,
                "end_line": 3, "end_character": 8
            });
        }))?;
        Ok(())
    }

    #[test]
    fn falsifier_08_fallback_and_low_confidence_stay_visible() -> Result<()> {
        let fallback = adapt(&agreeing_with(|receipt| {
            receipt["normalized_facts"]["rust"][0]["fallback"] = json!("legacy_provider");
        }))?;
        let low_confidence = adapt(&agreeing_with(|receipt| {
            receipt["normalized_facts"]["rust"][1]["confidence"] = json!("medium");
        }))?;

        for observation in [&fallback, &low_confidence] {
            assert!(
                matches!(observation.limitation, LimitationDisposition::AcceptedDebt { .. }),
                "fallback and sub-high confidence are never exact compiler support: {:?}",
                observation.limitation
            );
            assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 09 — Rust and oracle fact sets merged
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_09_rust_and_oracle_fact_sets_never_merge() -> Result<()> {
        // Both receipts name the same union of facts and carry byte-identical
        // comparisons; only the side that observed them differs. Under any
        // undifferentiated merge these two canonicalize to the same bytes, so the
        // receipt digest is the assertion that actually discriminates.
        let one_sided = |side: &'static str| {
            agreeing_with(move |receipt| {
                let facts =
                    json!([fact("fact-isa-1", "Child::ISA"), fact("fact-isa-2", "Child::new")]);
                let empty = json!([]);
                receipt["normalized_facts"] = json!({
                    "rust": if side == "rust" { facts.clone() } else { empty.clone() },
                    "oracle": if side == "oracle" { facts } else { empty }
                });
                // A one-sided fact set is exactly the partial state this result
                // class names, so the receipt stays internally coherent.
                receipt["comparisons"] = json!([
                    comparison("stale_or_partial", "fact-isa-1", "known_limitation"),
                    comparison("stale_or_partial", "fact-isa-2", "known_limitation")
                ]);
            })
        };
        let rust_side = one_sided("rust");
        let oracle_side = one_sided("oracle");

        assert_eq!(
            rust_side["comparisons"], oracle_side["comparisons"],
            "the two receipts differ only in which side observed the facts"
        );
        assert_ne!(
            digest(&rust_side)?,
            digest(&oracle_side)?,
            "an undifferentiated merge of the two fact sets would canonicalize to one digest"
        );
        assert_ne!(identity(&rust_side)?, identity(&oracle_side)?);
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 09b — a one-sided fact set still yields differential agreement
    //
    // Reported on the PR: `named` was the union of both sides, so a receipt
    // whose oracle observed nothing could still reach Pass/AcceptedCompatibility
    // on an `oracle_agrees` row it never gathered.
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_09b_one_sided_evidence_cannot_satisfy_oracle_agreement() -> Result<()> {
        for absent in ["rust", "oracle"] {
            let observation = adapt(&agreeing_with(|receipt| {
                receipt["normalized_facts"][absent] = json!([]);
            }))?;

            assert_eq!(
                observation.disposition,
                ObservationDisposition::NotProven,
                "an oracle_agrees row with no {absent} fact is not differential agreement"
            );
            assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        }
        Ok(())
    }

    #[test]
    fn falsifier_09b_directional_results_need_the_side_they_name() -> Result<()> {
        // `compiler_missing` says the oracle saw a fact the Rust set lacks; a
        // receipt where the Rust set does carry it contradicts itself. The
        // declared mismatch still stands — softening it to `not_proven` would
        // erase what the receipt reported — but the incoherence has to remain
        // visible, so it lands on the completeness axis.
        let contradicting = adapt(&agreeing_with(|receipt| {
            receipt["comparisons"][0]["result_class"] = json!("compiler_missing");
        }))?;
        assert_eq!(contradicting.disposition, ObservationDisposition::Failed);
        assert!(
            matches!(contradicting.completeness, CompletenessDisposition::Partial { .. }),
            "an incoherent comparison is never silently complete: {:?}",
            contradicting.completeness
        );

        // The coherent shape stays a plain contradiction, with nothing extra.
        let coherent = adapt(&agreeing_with(|receipt| {
            receipt["normalized_facts"]["rust"] = json!([fact("fact-isa-2", "Child::new")]);
            receipt["comparisons"] = json!([
                comparison("compiler_missing", "fact-isa-1", "blocks_promotion"),
                comparison("oracle_agrees", "fact-isa-2", "supports_promotion")
            ]);
        }))?;
        assert_eq!(coherent.disposition, ObservationDisposition::Failed);
        Ok(())
    }

    #[test]
    fn falsifier_09b_a_repeated_fact_identity_fails_closed() -> Result<()> {
        for side in ["rust", "oracle"] {
            let receipt = agreeing_with(|receipt| {
                receipt["normalized_facts"][side] =
                    json!([fact("fact-isa-1", "Child::ISA"), fact("fact-isa-1", "Child::ISA")]);
            });

            assert_rejected(&receipt, "repeats identity")?;
        }
        Ok(())
    }

    #[test]
    fn falsifier_09b_a_repeated_comparison_identity_fails_closed() -> Result<()> {
        // The same law on the comparison side. Both the completeness
        // denominator (a set of compared fact ids) and every disposition reason
        // (which names the single fact its row ranges over) already assume one
        // comparison per fact — an assumption that was documented in the
        // adapter without being enforced.
        let repeated = agreeing_with(|receipt| {
            receipt["comparisons"] = json!([
                comparison("oracle_agrees", "fact-isa-1", "supports_promotion"),
                comparison("oracle_agrees", "fact-isa-1", "supports_promotion"),
                comparison("oracle_agrees", "fact-isa-2", "supports_promotion")
            ]);
        });
        assert_rejected(&repeated, "repeats fact identity")?;

        // The dangerous shape is the self-contradicting one: two rows over one
        // fact that disagree. Merging them would require inventing a precedence
        // among result classes the source contract does not define, so this
        // fails closed rather than resolving to whichever row is stronger.
        let contradictory = agreeing_with(|receipt| {
            receipt["comparisons"] = json!([
                comparison("oracle_agrees", "fact-isa-1", "supports_promotion"),
                comparison("compiler_missing", "fact-isa-1", "blocks_promotion"),
                comparison("oracle_agrees", "fact-isa-2", "supports_promotion")
            ]);
        });
        assert_rejected(&contradictory, "repeats fact identity")?;

        // Negative control: distinct identities across the same classes are
        // legitimate and must still reach the strongest ceiling, so the check
        // cannot be satisfied by refusing comparison sets in general.
        let distinct = adapt(&agreeing_receipt())?;
        assert_eq!(distinct.ceiling.claim_ceiling(), ClaimCeiling::AcceptedCompatibility);
        Ok(())
    }

    #[test]
    fn falsifier_02_no_field_value_can_forge_a_subject_delimiter() -> Result<()> {
        // The schema allows arbitrary strings in these fields, so a subject
        // built by plain delimiter joining would let one value impersonate the
        // delimiter and two distinct receipts collide on a dimension that
        // claims exact, non-transferable identity.
        let split_left = agreeing_with(|receipt| {
            receipt["rust_extractor"]["name"] = json!("a");
            receipt["rust_extractor"]["version"] = json!("b@c");
        });
        let split_right = agreeing_with(|receipt| {
            receipt["rust_extractor"]["name"] = json!("a@b");
            receipt["rust_extractor"]["version"] = json!("c");
        });
        assert_ne!(
            dimension(&adapt(&split_left)?, SubjectDimensionKind::Toolchain),
            dimension(&adapt(&split_right)?, SubjectDimensionKind::Toolchain),
            "an extractor value must not be able to move the @ delimiter"
        );
        assert_ne!(identity(&split_left)?, identity(&split_right)?);

        // The same hazard on a list: a comma inside one environment key.
        let comma_left = agreeing_with(|receipt| {
            receipt["environment"]["declared"] = json!(["A,B"]);
        });
        let comma_right = agreeing_with(|receipt| {
            receipt["environment"]["declared"] = json!(["A", "B"]);
        });
        assert_ne!(
            dimension(&adapt(&comma_left)?, SubjectDimensionKind::ProducerConfiguration),
            dimension(&adapt(&comma_right)?, SubjectDimensionKind::ProducerConfiguration)
        );

        // And on the digest preimage: a root containing the unit separator.
        let sep_left = agreeing_with(|receipt| {
            receipt["module_path_authority"]["declared_roots"] = json!(["a\u{1f}b"]);
        });
        let sep_right = agreeing_with(|receipt| {
            receipt["module_path_authority"]["declared_roots"] = json!(["a", "b"]);
        });
        assert_ne!(
            dimension(&adapt(&sep_left)?, SubjectDimensionKind::CompilerPolicy),
            dimension(&adapt(&sep_right)?, SubjectDimensionKind::CompilerPolicy),
            "a root value must not be able to forge the digest separator"
        );

        // The fixture dimension composes three variable fields the same way.
        let fixture_left = agreeing_with(|receipt| {
            receipt["fixture_id"] = json!("x");
            receipt["source_snapshot"]["content_hash"] = json!("y z");
        });
        let fixture_right = agreeing_with(|receipt| {
            receipt["fixture_id"] = json!("x\" content_hash=\"y");
            receipt["source_snapshot"]["content_hash"] = json!("z");
        });
        assert_ne!(
            dimension(&adapt(&fixture_left)?, SubjectDimensionKind::FixtureSeries),
            dimension(&adapt(&fixture_right)?, SubjectDimensionKind::FixtureSeries)
        );
        Ok(())
    }

    #[test]
    fn falsifier_09c_agreement_over_disagreeing_facts_is_not_agreement() -> Result<()> {
        // Both sides name `fact-isa-1`, so the row is two-sided — but the two
        // observations differ in a field that has its own mismatch class, so
        // `oracle_agrees` is mislabelled rather than agreed.
        let cases: [(&str, fn(&mut Value)); 5] = [
            ("name", |r| r["normalized_facts"]["oracle"][0]["name"] = json!("Child::OTHER")),
            // A well-formed range at a different place — the point is that the
            // two sides disagree, not that either range is malformed.
            ("source range", |r| {
                r["normalized_facts"]["oracle"][0]["source_range"] = json!({
                    "path_class": "public_test_fixture",
                    "start_line": 99, "start_character": 0,
                    "end_line": 99, "end_character": 24
                })
            }),
            ("provenance", |r| {
                r["normalized_facts"]["oracle"][0]["provenance"] = json!("SourceBackedGenerated")
            }),
            ("confidence", |r| r["normalized_facts"]["oracle"][0]["confidence"] = json!("medium")),
            ("freshness", |r| {
                r["normalized_facts"]["oracle"][0]["freshness"] = json!("not_applicable")
            }),
        ];

        for (field, mutate) in cases {
            let observation = adapt(&agreeing_with(mutate))?;
            assert_eq!(
                observation.disposition,
                ObservationDisposition::NotProven,
                "differing {field} under one identity is not agreement"
            );
            assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        }

        // `fallback` is deliberately not part of agreement — it records what a
        // side fell back to while observing, not what it observed — so it stays
        // a limitation rather than an incoherence.
        let fallback = adapt(&agreeing_with(|r| {
            r["normalized_facts"]["oracle"][0]["fallback"] = json!("legacy_provider")
        }))?;
        assert_eq!(fallback.disposition, ObservationDisposition::Pass);
        assert!(matches!(fallback.limitation, LimitationDisposition::AcceptedDebt { .. }));
        Ok(())
    }

    #[test]
    fn falsifier_09c_a_declared_mismatch_over_agreeing_facts_is_not_coherent() -> Result<()> {
        // The mirror of the test above. Each mismatch class names exactly one
        // field pair the normalized facts carry, so a row declaring a mismatch
        // over two facts that agree on that very field contradicts its own
        // label just as a mislabelled `oracle_agrees` row does.
        //
        // Detecting this must not soften the verdict: the receipt declared a
        // mismatch, so the product axis stays `Failed`. What changes is that
        // the contradiction becomes visible rather than reading as a clean,
        // confident failure.
        let classes = [
            ("range_mismatch", "same source range"),
            ("provenance_mismatch", "same provenance"),
            ("confidence_or_freshness_mismatch", "same confidence and freshness"),
        ];

        for (class, needle) in classes {
            let receipt = agreeing_with(|receipt| {
                receipt["comparisons"] = json!([
                    comparison(class, "fact-isa-1", "blocks_promotion"),
                    comparison("oracle_agrees", "fact-isa-2", "supports_promotion")
                ]);
            });
            let observation = adapt(&receipt)?;

            assert_eq!(
                observation.disposition,
                ObservationDisposition::Failed,
                "{class}: a declared mismatch still outranks not_proven"
            );
            match &observation.completeness {
                CompletenessDisposition::Partial { remainder } => assert!(
                    remainder.contains(needle),
                    "{class}: the remainder must name the contradiction, got {remainder:?}"
                ),
                other => bail!(
                    "{class}: the self-contradiction must stay visible on the completeness \
                     axis, got {other:?}"
                ),
            }
            assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        }

        // Negative control: the same rows over facts that genuinely differ on
        // the field each class names are coherent. Without this, an adapter
        // that called every mismatch incoherent would pass the loop above.
        let genuine: [(&str, fn(&mut Value)); 3] = [
            ("range_mismatch", |r| {
                r["normalized_facts"]["oracle"][0]["source_range"] = json!({
                    "path_class": "public_test_fixture",
                    "start_line": 99, "start_character": 0,
                    "end_line": 99, "end_character": 24
                })
            }),
            ("provenance_mismatch", |r| {
                r["normalized_facts"]["oracle"][0]["provenance"] = json!("SourceBackedGenerated")
            }),
            ("confidence_or_freshness_mismatch", |r| {
                r["normalized_facts"]["oracle"][0]["confidence"] = json!("medium")
            }),
        ];

        for (class, mutate) in genuine {
            let mut receipt = agreeing_receipt();
            mutate(&mut receipt);
            receipt["comparisons"] = json!([
                comparison(class, "fact-isa-1", "blocks_promotion"),
                comparison("oracle_agrees", "fact-isa-2", "supports_promotion")
            ]);
            let observation = adapt(&receipt)?;

            assert_eq!(observation.disposition, ObservationDisposition::Failed);
            assert_eq!(
                observation.completeness,
                CompletenessDisposition::Complete,
                "{class}: a genuine mismatch closes its denominator and is not incoherent"
            );
        }
        Ok(())
    }

    #[test]
    fn an_identifier_the_envelope_cannot_carry_fails_closed_by_name() -> Result<()> {
        // The schema accepts any non-empty identifier, but #12188's
        // private-safety contract governs the envelope text these reach and
        // cannot be weakened. The refusal must name the offending source field
        // rather than surfacing from inside a subject dimension.
        assert_rejected(
            &agreeing_with(|r| r["normalized_facts"]["rust"][0]["fact_id"] = json!("workflow-1")),
            "normalized fact id",
        )?;
        assert_rejected(
            &agreeing_with(|r| r["comparisons"][0]["fact_id"] = json!("run.log")),
            "comparison fact id",
        )?;
        assert_rejected(
            &agreeing_with(|r| r["fixture_id"] = json!("github-fixture")),
            "fixture_id",
        )?;

        // A redacted private fixture never has its source named, so an
        // unrepresentable one there is not a refusal.
        adapt(&agreeing_with(|r| {
            r["source_snapshot"]["path_class"] = json!("redacted_private_fixture");
            r["source_snapshot"]["fixture_source"] = json!("/home/runner/private.pl");
        }))?;
        Ok(())
    }

    #[test]
    fn falsifier_07_a_source_claim_without_a_source_range_is_not_source_backed() -> Result<()> {
        // `ExplicitSource` / `SourceBackedGenerated` assert a source anchor. A
        // null range carries none, so the claim stays in the denominator.
        let generated = adapt(&agreeing_with(|receipt| {
            receipt["generated_inputs"] = json!([{
                "framework": "Moo",
                "provenance": "SourceBackedGenerated",
                "source_range": null
            }]);
        }))?;
        let fact_without_range = adapt(&agreeing_with(|receipt| {
            receipt["normalized_facts"]["rust"][0]["source_range"] = json!(null);
            receipt["normalized_facts"]["oracle"][0]["source_range"] = json!(null);
        }))?;

        for observation in [&generated, &fact_without_range] {
            assert!(
                matches!(observation.completeness, CompletenessDisposition::Partial { .. }),
                "an unanchored source claim is not complete evidence: {:?}",
                observation.completeness
            );
            assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        }

        // A generated input that does anchor its source stays eligible.
        let anchored = adapt(&agreeing_with(|receipt| {
            receipt["generated_inputs"] = json!([{
                "framework": "Moo",
                "provenance": "SourceBackedGenerated",
                "source_range": {
                    "path_class": "public_test_fixture",
                    "start_line": 1, "start_character": 0,
                    "end_line": 1, "end_character": 8
                }
            }]);
        }))?;
        assert_eq!(anchored.completeness, CompletenessDisposition::Complete);
        Ok(())
    }

    #[test]
    fn a_stale_declaration_over_an_unobserved_fact_does_not_close() -> Result<()> {
        let observation = adapt(&agreeing_with(|receipt| {
            receipt["stale_facts"] = json!([{ "fact_id": "fact-absent", "freshness": "stale" }]);
        }))?;

        assert!(
            matches!(observation.completeness, CompletenessDisposition::Partial { .. }),
            "a staleness claim over a fact no side names cannot be checked: {:?}",
            observation.completeness
        );
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 06b — silence about a closed startup input reads as hermetic
    //
    // Reported on the PR: the hermeticity check only intersected denied with
    // declared, so omitting a dangerous key from both sets was accepted.
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_06b_an_unaccounted_startup_input_is_not_hermetic() -> Result<()> {
        let observation = adapt(&agreeing_with(|receipt| {
            receipt["environment"]["denied"] = json!(["PERL5OPT", "local::lib"]);
        }))?;

        assert_eq!(
            observation.disposition,
            ObservationDisposition::NotProven,
            "a receipt silent about PERL5LIB has not shown the run was hermetic"
        );
        assert_eq!(observation.currentness, CurrentnessDisposition::NotProven);
        assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        Ok(())
    }

    #[test]
    fn falsifier_06b_declared_without_denial_is_not_hermetic() -> Result<()> {
        let observation = adapt(&agreeing_with(|receipt| {
            receipt["environment"]["denied"] = json!(["PERL5OPT", "local::lib"]);
            receipt["environment"]["declared"] = json!(["PATH", "PERL5LIB"]);
        }))?;

        // Admitting the ambient input is more honest than silence, but it is
        // still an ambient input: the instrument did not fail, the claim did.
        assert_eq!(observation.instrument.terminal, TerminalState::Completed);
        assert_eq!(observation.disposition, ObservationDisposition::NotProven);
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 10 — mismatch / unknown / limitation becomes agreement
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_10_mismatch_results_cannot_become_agreement() -> Result<()> {
        for result_class in [
            "compiler_missing",
            "compiler_extra",
            "range_mismatch",
            "provenance_mismatch",
            "confidence_or_freshness_mismatch",
        ] {
            let observation = adapt(&agreeing_with(|receipt| {
                receipt["comparisons"][0]["result_class"] = json!(result_class);
            }))?;

            assert_eq!(
                observation.disposition,
                ObservationDisposition::Failed,
                "{result_class} is a contradiction, not agreement"
            );
            assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        }
        Ok(())
    }

    #[test]
    fn falsifier_10_unproven_results_cannot_become_agreement() -> Result<()> {
        for result_class in
            ["dynamic_or_unsupported", "oracle_ambient_unbounded", "stale_or_partial", "unknown"]
        {
            let observation = adapt(&agreeing_with(|receipt| {
                receipt["comparisons"][0]["result_class"] = json!(result_class);
            }))?;

            assert_eq!(
                observation.disposition,
                ObservationDisposition::NotProven,
                "{result_class} proves nothing"
            );
        }
        Ok(())
    }

    #[test]
    fn a_comparison_reason_names_the_fact_it_ranges_over() -> Result<()> {
        // A receipt carries one comparison per named fact, so once several are
        // aggregated the reason text is the only handle a consumer has on which
        // fact produced the verdict.
        let limited = adapt(&agreeing_with(|receipt| {
            receipt["comparisons"][1]["promotion_effect"] = json!("known_limitation");
        }))?;

        let LimitationDisposition::AcceptedDebt { reason, .. } = &limited.limitation else {
            bail!("a known limitation is accepted debt: {:?}", limited.limitation);
        };
        assert!(
            reason.contains("fact-isa-2"),
            "the limitation must name its fact, not just its class: {reason}"
        );
        assert!(!reason.contains("fact-isa-1"), "and only the fact that caused it: {reason}");
        Ok(())
    }

    #[test]
    fn falsifier_10_promotion_effects_stay_distinct() -> Result<()> {
        let blocks = adapt(&agreeing_with(|receipt| {
            receipt["comparisons"][0]["promotion_effect"] = json!("blocks_promotion");
        }))?;
        let unknown = adapt(&agreeing_with(|receipt| {
            receipt["comparisons"][0]["promotion_effect"] = json!("unknown");
        }))?;
        let limited = adapt(&agreeing_with(|receipt| {
            receipt["comparisons"][0]["promotion_effect"] = json!("known_limitation");
        }))?;

        assert_eq!(blocks.disposition, ObservationDisposition::Failed);
        assert_eq!(unknown.disposition, ObservationDisposition::NotProven);
        assert!(matches!(limited.limitation, LimitationDisposition::AcceptedDebt { .. }));
        assert_eq!(limited.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 11 — supports_promotion becomes authorization
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_11_supports_promotion_authorizes_nothing_above_accepted_compatibility()
    -> Result<()> {
        let descriptor = adapter::oracle_receipt_adapter()?;

        assert_eq!(
            descriptor.observation_claim_ceiling.claim_ceiling(),
            ClaimCeiling::AcceptedCompatibility,
            "a differential oracle receipt never reaches a bounded public claim"
        );
        assert_eq!(descriptor.source_claim_ceiling, ClaimCeiling::AcceptedCompatibility);

        // A strengthened observation is refused by the registry that declares the
        // adapter, so no consumer can promote the source metadata by relabelling.
        let mut strengthened = adapt(&agreeing_receipt())?;
        strengthened.ceiling = xtask::compiler_profile_observation::ObservedClaimCeiling::new(
            ClaimCeiling::BoundedPublicClaim,
        );
        let registry = adapter::oracle_receipt_registry()?;
        assert!(registry.validate_observation(&strengthened).is_err());
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 12 — one passing comparison erases another mismatch or boundary
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_12_a_passing_comparison_cannot_erase_a_selected_mismatch() -> Result<()> {
        let observation = adapt(&agreeing_with(|receipt| {
            receipt["comparisons"] = json!([
                comparison("oracle_agrees", "fact-isa-1", "supports_promotion"),
                comparison("compiler_missing", "fact-isa-2", "blocks_promotion")
            ]);
        }))?;

        assert_eq!(observation.disposition, ObservationDisposition::Failed);
        assert_eq!(observation.ceiling.claim_ceiling(), ClaimCeiling::ObservedEvidence);
        Ok(())
    }

    #[test]
    fn falsifier_12_a_named_fact_without_a_comparison_is_not_complete() -> Result<()> {
        let observation = adapt(&agreeing_with(|receipt| {
            receipt["comparisons"] =
                json!([comparison("oracle_agrees", "fact-isa-1", "supports_promotion")]);
        }))?;

        assert!(
            matches!(observation.completeness, CompletenessDisposition::Partial { .. }),
            "an uncompared named fact is a missing row, not silent support: {:?}",
            observation.completeness
        );
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 13 — message text is parsed to reconstruct semantics
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_13_message_text_is_content_not_semantics() -> Result<()> {
        let misleading = adapt(&agreeing_with(|receipt| {
            receipt["comparisons"][0]["result_class"] = json!("compiler_missing");
            receipt["comparisons"][0]["message"] =
                json!("oracle_agrees supports_promotion fresh high ExplicitSource");
        }))?;
        assert_eq!(
            misleading.disposition,
            ObservationDisposition::Failed,
            "message prose can never reconstruct a typed result"
        );

        // Changing only the message changes the receipt's content digest but no
        // semantic axis: message is preserved as content, never read as meaning.
        let base = adapt(&agreeing_receipt())?;
        let reworded = adapt(&agreeing_with(|receipt| {
            receipt["comparisons"][0]["message"] =
                json!("a differently worded bounded explanation");
        }))?;

        assert_ne!(base.receipt.digest.as_str(), reworded.receipt.digest.as_str());
        assert_eq!(base.disposition, reworded.disposition);
        assert_eq!(base.currentness, reworded.currentness);
        assert_eq!(base.completeness, reworded.completeness);
        assert_eq!(base.limitation, reworded.limitation);
        assert_eq!(base.ceiling.claim_ceiling(), reworded.ceiling.claim_ceiling());
        assert_eq!(base.subject, reworded.subject);
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 14 — oracle agreement becomes gold, EIR, provider, or client proof
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_14_oracle_agreement_stays_parser_internal_real_perl_evidence() -> Result<()> {
        use xtask::compiler_profile_contract::{ClaimFamily, ProofClass};
        use xtask::compiler_profile_observation::ObservationClass;

        let descriptor = adapter::oracle_receipt_adapter()?;
        assert_eq!(
            descriptor.emitted_classes.len(),
            1,
            "the adapter owns exactly one proposition family and proof axis"
        );

        let observation = adapt(&agreeing_receipt())?;
        assert_eq!(observation.class.family, ClaimFamily::ParserInternal);
        assert_eq!(observation.class.proof_class, ProofClass::RealPerlOracle);

        let registry = adapter::oracle_receipt_registry()?;
        for class in [
            ObservationClass {
                family: ClaimFamily::ParserInternal,
                proof_class: ProofClass::EirMechanism,
            },
            ObservationClass {
                family: ClaimFamily::ParserInternal,
                proof_class: ProofClass::CuratedExpectation,
            },
            ObservationClass {
                family: ClaimFamily::Provider,
                proof_class: ProofClass::RealPerlOracle,
            },
            ObservationClass {
                family: ClaimFamily::InstalledHost,
                proof_class: ProofClass::RealPerlOracle,
            },
            ObservationClass {
                family: ClaimFamily::ActualClient,
                proof_class: ProofClass::RealPerlOracle,
            },
        ] {
            let mut relabelled = observation.clone();
            relabelled.class = class;
            assert!(
                registry.validate_observation(&relabelled).is_err(),
                "oracle evidence may not be relabelled {}/{}",
                class.family.tag(),
                class.proof_class.tag()
            );
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Falsifier 15 — editor_runtime_dependency = true is accepted
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_15_editor_runtime_dependency_is_structurally_refused() -> Result<()> {
        let receipt = agreeing_with(|receipt| receipt["editor_runtime_dependency"] = json!(true));

        assert_rejected(&receipt, "schema")
    }

    // ---------------------------------------------------------------------------
    // Falsifier 16/17 — redaction and privacy
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_16_false_redaction_is_an_instrument_failure_not_a_verdict() -> Result<()> {
        for flag in [
            "private_paths_redacted",
            "environment_values_redacted",
            "raw_launch_payloads_redacted",
        ] {
            let observation = adapt(&agreeing_with(|receipt| {
                receipt["redaction"][flag] = json!(false);
            }))?;

            assert!(
                matches!(observation.instrument.terminal, TerminalState::InstrumentFailed { .. }),
                "{flag}=false is an instrument/privacy failure"
            );
            assert_eq!(observation.disposition, ObservationDisposition::NotProven);
            assert_eq!(observation.completeness, CompletenessDisposition::NotProven);
        }

        let unredacted_environment = adapt(&agreeing_with(|receipt| {
            receipt["environment"]["redacted_values"] = json!(false);
        }))?;
        assert!(matches!(
            unredacted_environment.instrument.terminal,
            TerminalState::InstrumentFailed { .. }
        ));
        Ok(())
    }

    #[test]
    fn falsifier_16_missing_redaction_block_fails_closed() -> Result<()> {
        let mut receipt = agreeing_receipt();
        if let Some(object) = receipt.as_object_mut() {
            object.remove("redaction");
        }

        assert_rejected(&receipt, "schema")
    }

    #[test]
    fn falsifier_17_private_fixture_identity_never_crosses_the_boundary() -> Result<()> {
        let first = agreeing_with(|receipt| {
            receipt["source_snapshot"]["path_class"] = json!("redacted_private_fixture");
            receipt["source_snapshot"]["fixture_source"] = json!("redacted-private-fixture-alpha");
        });
        let second = agreeing_with(|receipt| {
            receipt["source_snapshot"]["path_class"] = json!("redacted_private_fixture");
            receipt["source_snapshot"]["fixture_source"] = json!("redacted-private-fixture-beta");
        });

        let first_observation = adapt(&first)?;
        let second_observation = adapt(&second)?;
        let first_canonical = first_observation.canonical_semantic_text()?;
        let second_canonical = second_observation.canonical_semantic_text()?;

        for canonical in [&first_canonical, &second_canonical] {
            assert!(
                !canonical.contains("redacted-private-fixture-alpha")
                    && !canonical.contains("redacted-private-fixture-beta"),
                "a redacted private fixture identity must not appear in normalized output"
            );
            assert!(canonical.contains("redacted_private_fixture"), "the path class stays visible");
        }
        assert_eq!(
            first_observation.subject, second_observation.subject,
            "private source labels do not become normalized subject text"
        );
        assert_ne!(
            first_observation.receipt.digest, second_observation.receipt.digest,
            "the private source receipt remains content-distinct through its digest"
        );
        assert_ne!(
            first_observation.identity()?,
            second_observation.identity()?,
            "distinct canonical receipts remain distinct observations"
        );
        Ok(())
    }

    #[test]
    fn falsifier_17_module_roots_and_environment_values_never_cross_the_boundary() -> Result<()> {
        let observation = adapt(&agreeing_with(|receipt| {
            receipt["module_path_authority"]["declared_roots"] =
                json!(["fixtures/private-lane/lib", "fixtures/private-lane/blib"]);
        }))?;

        let canonical = observation.canonical_semantic_text()?;
        assert!(
            !canonical.contains("private-lane"),
            "declared module roots are digested, never carried: {canonical}"
        );
        assert!(
            canonical.contains("roots_digest="),
            "the roots stay load-bearing through a digest"
        );
        Ok(())
    }

    #[test]
    fn falsifier_17_the_module_root_digest_crosses_whole_not_truncated() -> Result<()> {
        // A truncated digest is a hint, not the exact non-transferable identity
        // this dimension claims: a 64-bit prefix can collide where the full
        // digest cannot, and truncation buys no privacy the whole digest does
        // not already give.
        let observation = adapt(&agreeing_receipt())?;
        let policy = dimension(&observation, SubjectDimensionKind::CompilerPolicy);

        let digest = policy
            .split("roots_digest=")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .unwrap_or_default();
        assert_eq!(digest.len(), 64, "the whole sha256 digest crosses: {policy}");
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        Ok(())
    }

    #[test]
    fn falsifier_17_evidence_fields_are_deliberately_not_subject_fields() -> Result<()> {
        // The line this suite draws, pinned so it is not re-litigated field by
        // field: a subject dimension carries the *configuration under which the
        // observation was made*; it must never carry *what the observation
        // found*. If findings entered the subject, two runs over one fixture
        // that disagreed would become two different subjects, and nothing could
        // ever be compared — which is the whole point of binding a subject.
        //
        // `generated_inputs` is a finding, not a configuration: it records what
        // the extractor discovered in the fixture. Its inputs are already bound
        // exactly — `source_snapshot.content_hash` crosses whole in
        // FixtureSeries, and the extractor name, version, and fact model cross
        // in Toolchain and CompilerPolicy — so under a deterministic extractor
        // it is a function of the subject rather than a component of it, and
        // adds no discriminating identity.
        //
        // This is exactly why `ambient_inputs` goes the other way: an ambient
        // input is external configuration, determined by nothing the receipt
        // otherwise binds. Being a `preserved_field` is not the test — so are
        // `normalized_facts.rust` and `stale_facts`, which plainly must not
        // enter the subject.
        let moo = agreeing_with(|receipt| {
            receipt["generated_inputs"] = json!([{
                "framework": "Moo",
                "provenance": "SourceBackedGenerated",
                "source_range": {
                    "path_class": "public_test_fixture",
                    "start_line": 7, "start_character": 0,
                    "end_line": 7, "end_character": 12
                }
            }]);
        });
        let moose = agreeing_with(|receipt| {
            receipt["generated_inputs"] = json!([{
                "framework": "Moose",
                "provenance": "SourceBackedGenerated",
                "source_range": {
                    "path_class": "public_test_fixture",
                    "start_line": 9, "start_character": 0,
                    "end_line": 9, "end_character": 20
                }
            }]);
        });

        // Both are accepted, so this is a statement about accepted evidence.
        for receipt in [&moo, &moose] {
            assert_eq!(
                adapt(receipt)?.ceiling.claim_ceiling(),
                ClaimCeiling::AcceptedCompatibility
            );
        }

        // Same subject on every bound dimension: these are two observations
        // *of one candidate*, which is what makes them comparable at all.
        assert_eq!(
            adapt(&moo)?.subject,
            adapt(&moose)?.subject,
            "a differing finding must not manufacture a different candidate"
        );

        // ...and yet the difference is not lost. It lands where a finding
        // belongs: in the receipt digest, and so in the observation identity,
        // which hashes the whole semantic text rather than the subject alone.
        // Without this half the test would also pass if the adapter simply
        // discarded `generated_inputs`.
        assert_ne!(
            digest(&moo)?,
            digest(&moose)?,
            "a differing finding must still change the receipt digest"
        );
        assert_ne!(
            identity(&moo)?,
            identity(&moose)?,
            "two observations of one subject are still two observations"
        );
        Ok(())
    }

    #[test]
    fn falsifier_17_declared_ambient_inputs_are_load_bearing_in_the_subject() -> Result<()> {
        // `ambient_inputs.authority` is a preserved field, and a declared
        // ambient input is part of the environment the evidence was gathered
        // under. Two receipts that declare different ambient inputs are two
        // environments; if they shared a candidate subject, evidence gathered
        // under one would read as covering the other. A differing receipt
        // digest is not enough — the digest ranges over the whole document,
        // while the subject is what binds the evidence to a candidate.
        //
        // Unbounded and reported-only inputs are already blocked or bounded
        // elsewhere, so the inputs that survive into an accepted row are
        // exactly the declared ones — the case where a shared subject is wrong.
        let locale = agreeing_with(|receipt| {
            receipt["ambient_inputs"] =
                json!([{ "kind": "locale", "authority": "declared_input" }]);
        });
        let tmpdir = agreeing_with(|receipt| {
            receipt["ambient_inputs"] =
                json!([{ "kind": "tmpdir", "authority": "declared_input" }]);
        });

        // Not vacuous: both really do reach the strongest ceiling, so this is a
        // statement about accepted evidence rather than about two refusals.
        for receipt in [&locale, &tmpdir] {
            assert_eq!(
                adapt(receipt)?.ceiling.claim_ceiling(),
                ClaimCeiling::AcceptedCompatibility
            );
        }

        assert_ne!(
            dimension(&adapt(&locale)?, SubjectDimensionKind::ProducerConfiguration),
            dimension(&adapt(&tmpdir)?, SubjectDimensionKind::ProducerConfiguration),
            "a different declared ambient input is a different producer configuration"
        );
        assert_ne!(
            identity(&locale)?,
            identity(&tmpdir)?,
            "the candidate subject must separate them, not only the receipt digest"
        );

        // The entries are canonical-JSON encoded with a quoted kind, so no kind
        // value can forge an entry or field separator and collide two distinct
        // environments onto one dimension. Under a plain join these two render
        // identically: one entry whose kind carries the separator, against the
        // two entries it impersonates.
        let split_left = agreeing_with(|receipt| {
            receipt["ambient_inputs"] = json!([{ "kind": "locale authority=unbounded, kind=x", "authority": "declared_input" }]);
        });
        let split_right = agreeing_with(|receipt| {
            receipt["ambient_inputs"] = json!([
                { "kind": "locale", "authority": "unbounded" },
                { "kind": "x", "authority": "declared_input" }
            ]);
        });
        assert_ne!(
            dimension(&adapt(&split_left)?, SubjectDimensionKind::ProducerConfiguration),
            dimension(&adapt(&split_right)?, SubjectDimensionKind::ProducerConfiguration),
            "an ambient kind must not be able to forge an entry separator"
        );
        Ok(())
    }

    #[test]
    fn falsifier_17_a_host_absolute_fixture_path_fails_closed() -> Result<()> {
        let receipt = agreeing_with(|receipt| {
            receipt["source_snapshot"]["fixture_source"] = json!("/home/runner/fixtures/isa.pl");
        });

        // The refusal names the source field and the actual problem, rather
        // than surfacing from inside the subject dimension it would have been
        // formatted into.
        assert_rejected(&receipt, "source_snapshot.fixture_source")?;
        assert_rejected(&receipt, "host-specific absolute path")
    }

    // ---------------------------------------------------------------------------
    // Falsifier 18 — non-semantic input ordering changes normalized bytes
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_18_non_semantic_ordering_cannot_change_normalized_bytes() -> Result<()> {
        let ordered = agreeing_with(|receipt| {
            receipt["ambient_inputs"] = json!([
                { "kind": "locale", "authority": "declared_input" },
                { "kind": "tmpdir", "authority": "declared_input" }
            ]);
            receipt["generated_inputs"] = json!([
                { "framework": "Moo", "provenance": "SourceBackedGenerated", "source_range": null },
                { "framework": "Moose", "provenance": "SourceBackedGenerated", "source_range": null }
            ]);
            receipt["environment"]["declared"] = json!(["PATH", "TMPDIR"]);
            receipt["environment"]["denied"] = json!(["PERL5LIB", "PERL5OPT", "local::lib"]);
        });
        let shuffled = agreeing_with(|receipt| {
            receipt["ambient_inputs"] = json!([
                { "kind": "tmpdir", "authority": "declared_input" },
                { "kind": "locale", "authority": "declared_input" }
            ]);
            receipt["generated_inputs"] = json!([
                { "framework": "Moose", "provenance": "SourceBackedGenerated", "source_range": null },
                { "framework": "Moo", "provenance": "SourceBackedGenerated", "source_range": null }
            ]);
            receipt["environment"]["declared"] = json!(["TMPDIR", "PATH"]);
            receipt["environment"]["denied"] = json!(["local::lib", "PERL5OPT", "PERL5LIB"]);
            receipt["normalized_facts"]["rust"] =
                json!([fact("fact-isa-2", "Child::new"), fact("fact-isa-1", "Child::ISA")]);
            receipt["comparisons"] = json!([
                comparison("oracle_agrees", "fact-isa-2", "supports_promotion"),
                comparison("oracle_agrees", "fact-isa-1", "supports_promotion")
            ]);
        });

        assert_ne!(ordered, shuffled, "the two documents really do differ byte for byte");
        assert_eq!(digest(&ordered)?, digest(&shuffled)?);
        assert_eq!(identity(&ordered)?, identity(&shuffled)?);
        Ok(())
    }

    #[test]
    fn falsifier_18_module_root_precedence_is_semantic_and_must_not_be_sorted() -> Result<()> {
        // Declared module roots are the one ordered collection in the receipt:
        // Perl resolves an include path by first match, so the same roots in
        // another precedence are another execution and must not collapse into
        // one subject. The schema agrees — it marks `environment.denied` and
        // `environment.declared` `uniqueItems` and deliberately does not mark
        // these. An earlier revision sorted them, and this suite asserted the
        // collapse.
        let first = agreeing_with(|receipt| {
            receipt["module_path_authority"]["declared_roots"] = json!(["a/lib", "b/lib"]);
        });
        let reversed = agreeing_with(|receipt| {
            receipt["module_path_authority"]["declared_roots"] = json!(["b/lib", "a/lib"]);
        });

        assert_ne!(
            digest(&first)?,
            digest(&reversed)?,
            "reordering include-path precedence is a different execution"
        );
        assert_ne!(identity(&first)?, identity(&reversed)?);
        assert_ne!(
            dimension(&adapt(&first)?, SubjectDimensionKind::CompilerPolicy),
            dimension(&adapt(&reversed)?, SubjectDimensionKind::CompilerPolicy)
        );
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Fail-closed source-schema and vocabulary boundaries
    // ---------------------------------------------------------------------------

    #[test]
    fn falsifier_unknown_or_future_source_schema_fails_closed() -> Result<()> {
        let future =
            agreeing_with(|receipt| receipt["schema_version"] = json!("oracle_receipt.v2"));

        assert_rejected(&future, "unknown or future")?;

        let registry = adapter::oracle_receipt_registry()?;
        assert!(
            registry
                .select_adapter(&ReceiptFamily::new(SOURCE_FAMILY)?, SchemaVersion::new(2))
                .is_err(),
            "no adapter owns oracle_receipt schema 2"
        );
        assert!(
            registry
                .select_adapter(&ReceiptFamily::new("install_transition")?, SchemaVersion::new(1))
                .is_err(),
            "this adapter can never claim another receipt family"
        );
        Ok(())
    }

    #[test]
    fn falsifier_unknown_vocabulary_members_fail_closed() -> Result<()> {
        let cases: [(&str, fn(&mut Value)); 5] = [
            ("comparison_class", |receipt| {
                receipt["comparison_class"] = json!("RoleComposition");
            }),
            ("result_class", |receipt| {
                receipt["comparisons"][0]["result_class"] = json!("oracle_probably_agrees");
            }),
            ("promotion_effect", |receipt| {
                receipt["comparisons"][0]["promotion_effect"] = json!("supports_release");
            }),
            ("provenance", |receipt| {
                receipt["normalized_facts"]["rust"][0]["provenance"] = json!("InferredSource");
            }),
            ("interpreter", |receipt| {
                receipt["perl_oracle"]["interpreter"] = json!("vendor_perl");
            }),
        ];

        for (name, mutate) in cases {
            let receipt = agreeing_with(mutate);
            if adapt(&receipt).is_ok() {
                bail!("an unknown {name} member must fail closed");
            }
        }
        Ok(())
    }

    #[test]
    fn falsifier_unknown_receipt_fields_fail_closed() -> Result<()> {
        let receipt = agreeing_with(|receipt| receipt["promoted"] = json!(true));

        assert_rejected(&receipt, "'promoted' was unexpected")
    }

    #[test]
    fn falsifier_empty_comparison_set_fails_closed() -> Result<()> {
        let receipt = agreeing_with(|receipt| receipt["comparisons"] = json!([]));

        assert_rejected(&receipt, "schema")
    }

    // ---------------------------------------------------------------------------
    // Zero work and receipt identity
    // ---------------------------------------------------------------------------

    #[test]
    fn zero_fact_evidence_is_typed_zero_work_and_never_pass() -> Result<()> {
        let observation = adapt(&agreeing_with(|receipt| {
            receipt["normalized_facts"] = json!({ "rust": [], "oracle": [] });
        }))?;

        assert_eq!(observation.work, WorkDisposition::ZeroWork);
        assert_eq!(observation.disposition, ObservationDisposition::NotProven);

        // The axes are independent by design, but zero work cannot also read
        // as complete here: the schema requires at least one comparison, and
        // with no facts every comparison ranges over an unnamed one, so the
        // denominator never closes.
        assert!(
            matches!(observation.completeness, CompletenessDisposition::Partial { .. }),
            "zero fact evidence cannot be complete: {:?}",
            observation.completeness
        );
        Ok(())
    }

    #[test]
    fn one_receipt_identity_owns_one_observation() -> Result<()> {
        let observations = adapter::adapt_receipts(&[
            agreeing_receipt(),
            agreeing_with(|receipt| {
                receipt["receipt_id"] = json!("oracle-receipt-0002");
                receipt["fixture_id"] = json!("isa-composition-diamond");
            }),
        ])?;
        assert_eq!(observations.len(), 2);

        let duplicate = adapter::adapt_receipts(&[
            agreeing_receipt(),
            agreeing_with(|receipt| receipt["fixture_id"] = json!("isa-composition-diamond")),
        ]);
        assert!(duplicate.is_err(), "one receipt id owns exactly one observation");
        Ok(())
    }

    #[test]
    fn adapting_from_json_text_matches_adapting_from_a_value() -> Result<()> {
        let receipt = agreeing_receipt();
        let text = serde_json::to_string(&receipt)?;

        assert_eq!(
            adapter::adapt_receipt_json(&text)?.identity()?.as_str(),
            adapt(&receipt)?.identity()?.as_str()
        );
        Ok(())
    }
}
