use super::*;

#[test]
fn test_collect_per_crate_mutation_from_mock_file() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let out_dir = dir.path().join("mutants.out");
    fs::create_dir_all(&out_dir)?;
    let json = r#"[
        {"package":"perl-quote","file":"crates/perl-quote/src/lib.rs","genre":"FnValue"},
        {"package":"perl-quote","file":"crates/perl-quote/src/lib.rs","genre":"BinaryOperator"},
        {"package":"perl-parser","file":"crates/perl-parser/src/lib.rs","genre":"FnValue"}
    ]"#;
    fs::write(out_dir.join("mutants.json"), json)?;
    let result = collect_per_crate_mutation(dir.path());
    assert_eq!(result.get("perl-quote"), Some(&2));
    assert_eq!(result.get("perl-parser"), Some(&1));
    Ok(())
}

#[test]
fn test_collect_per_crate_mutation_ignores_entries_without_package() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let out_dir = dir.path().join("mutants.out");
    fs::create_dir_all(&out_dir)?;
    let json = r#"[
        {"package":"perl-quote","file":"crates/perl-quote/src/lib.rs"},
        {"file":"crates/perl-parser/src/lib.rs","genre":"FnValue"},
        {"package":null,"file":"crates/perl-parser/src/lib.rs"},
        {"package":"perl-quote","file":"crates/perl-quote/src/lib.rs"}
    ]"#;
    fs::write(out_dir.join("mutants.json"), json)?;
    let result = collect_per_crate_mutation(dir.path());
    assert_eq!(result.len(), 1);
    assert_eq!(result.get("perl-quote"), Some(&2));
    Ok(())
}

#[test]
fn test_collect_per_crate_mutation_ignores_blank_package_names() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let out_dir = dir.path().join("mutants.out");
    fs::create_dir_all(&out_dir)?;
    let json = r#"[
        {"package":"perl-quote","file":"crates/perl-quote/src/lib.rs"},
        {"package":"","file":"crates/perl-parser/src/lib.rs"},
        {"package":"   ","file":"crates/perl-parser/src/lib.rs"}
    ]"#;
    fs::write(out_dir.join("mutants.json"), json)?;
    let result = collect_per_crate_mutation(dir.path());
    assert_eq!(result.len(), 1);
    assert_eq!(result.get("perl-quote"), Some(&1));
    Ok(())
}

#[test]
fn test_collect_per_crate_mutation_invalid_json_returns_empty_map() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let out_dir = dir.path().join("mutants.out");
    fs::create_dir_all(&out_dir)?;
    fs::write(out_dir.join("mutants.json"), "{not-json")?;
    let result = collect_per_crate_mutation(dir.path());
    assert!(result.is_empty());
    Ok(())
}

#[test]
fn test_format_crate_quality_table_has_header_and_data() {
    let mut mutation = BTreeMap::new();
    mutation.insert("perl-quote".to_string(), 249);
    let tests = PerCrateTestCounts {
        by_crate: BTreeMap::from([(String::from("perl-quote"), 42)]),
        unattributed: 0,
    };
    let table = format_crate_quality_table(&mutation, &tests);
    assert!(
        table.contains("Crate")
            && table.contains("perl-quote")
            && table.contains("249")
            && table.contains("42")
    );
}

#[test]
fn test_format_crate_quality_table_empty_maps() {
    let table = format_crate_quality_table(&BTreeMap::new(), &PerCrateTestCounts::default());
    assert!(table.contains("no data yet"));
}

#[test]
fn test_format_crate_quality_table_keeps_unattributed_tests_out_of_crate_rows() {
    let mutation = BTreeMap::new();
    let tests = PerCrateTestCounts { by_crate: BTreeMap::new(), unattributed: 2 };
    let table = format_crate_quality_table(&mutation, &tests);

    assert!(!table.contains("| unattributed |"));
    assert!(table.contains("2 discovered test(s) had no crate attribution"));
}

// ---------------------------------------------------------------------------
// Receipt-reading tests
// ---------------------------------------------------------------------------

#[test]
fn test_read_diagnostics_p50_ms_parses_editor_ux_md() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let status_dir = dir.path().join("docs/project/status");
    fs::create_dir_all(&status_dir)?;
    let md = "# Editor UX Scorecard\n\n\
        ## Latency (ms)\n\n\
        | Request class | p50 | p50 baseline | p95 | p95 baseline |\n\
        |---|---:|---:|---:|---:|\n\
        | completion | 27.00 | 27.00 | 35.00 | 35.00 |\n\
        | diagnostics | 53.00 | 53.00 | 66.00 | 66.00 |\n\
        | hover | 24.00 | 24.00 | 31.00 | 31.00 |\n";
    fs::write(status_dir.join("editor_ux.md"), md)?;
    let p50 = read_diagnostics_p50_ms(dir.path());
    assert_eq!(p50, Some(53.0));
    Ok(())
}

#[test]
fn test_read_diagnostics_p50_ms_returns_none_when_file_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let result = read_diagnostics_p50_ms(dir.path());
    assert!(result.is_none());
}

#[test]
fn test_read_diagnostics_p50_ms_returns_none_when_row_absent() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let status_dir = dir.path().join("docs/project/status");
    fs::create_dir_all(&status_dir)?;
    fs::write(status_dir.join("editor_ux.md"), "# no latency table here\n")?;
    let result = read_diagnostics_p50_ms(dir.path());
    assert!(result.is_none());
    Ok(())
}

#[test]
fn test_read_incremental_parse_range_ns_parses_scorecard_json() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let status_dir = dir.path().join("docs/project/status");
    fs::create_dir_all(&status_dir)?;
    let json = r#"{
        "schema_version": 1,
        "generated_at_epoch_s": 1234567890,
        "metrics": {
            "incremental_small_edit": {"iterations": 35, "median_ns": 73307, "p95_ns": 148249, "mean_ns": 78530},
            "incremental_multiple_edits": {"iterations": 35, "median_ns": 36733, "p95_ns": 182845, "mean_ns": 50285}
        }
    }"#;
    fs::write(status_dir.join("parser_performance_scorecard.json"), json)?;
    let range = read_incremental_parse_range_ns(dir.path());
    // 36733 is lower, 73307 is upper
    assert_eq!(range, Some((36733, 73307)));
    Ok(())
}

#[test]
fn test_read_incremental_parse_range_ns_returns_none_when_file_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let result = read_incremental_parse_range_ns(dir.path());
    assert!(result.is_none());
}

#[test]
fn test_read_incremental_parse_range_ns_returns_none_on_invalid_json() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let status_dir = dir.path().join("docs/project/status");
    fs::create_dir_all(&status_dir)?;
    fs::write(status_dir.join("parser_performance_scorecard.json"), "{bad}")?;
    let result = read_incremental_parse_range_ns(dir.path());
    assert!(result.is_none());
    Ok(())
}

#[test]
fn test_format_quality_metrics_bullet_with_both_receipts() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let status_dir = dir.path().join("docs/project/status");
    fs::create_dir_all(&status_dir)?;
    // Write mock editor_ux.md
    let md = "# Editor UX Scorecard\n\
        ## Latency (ms)\n\
        | Request class | p50 | p50 baseline | p95 | p95 baseline |\n\
        |---|---:|---:|---:|---:|\n\
        | diagnostics | 53.00 | 53.00 | 66.00 | 66.00 |\n";
    fs::write(status_dir.join("editor_ux.md"), md)?;
    // Write mock parser_performance_scorecard.json
    let json = r#"{
        "schema_version": 1,
        "generated_at_epoch_s": 1234567890,
        "metrics": {
            "incremental_small_edit": {"iterations": 35, "median_ns": 73307, "p95_ns": 148249, "mean_ns": 78530},
            "incremental_multiple_edits": {"iterations": 35, "median_ns": 36733, "p95_ns": 182845, "mean_ns": 50285}
        }
    }"#;
    fs::write(status_dir.join("parser_performance_scorecard.json"), json)?;
    let bullet = format_quality_metrics_bullet(dir.path());
    // Preserve the measurements and rounding while linking to their receipts.
    assert_eq!(
        bullet,
        "diagnostics p50 = 53 ms (receipt: [editor_ux.md](editor_ux.md)); \
         incremental parse median = 37–73 µs \
         (receipt: [parser_performance_scorecard.json](parser_performance_scorecard.json))"
    );
    Ok(())
}

#[test]
fn test_format_quality_metrics_bullet_with_only_diagnostics_receipt() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let status_dir = dir.path().join("docs/project/status");
    fs::create_dir_all(&status_dir)?;
    let md = "# Editor UX Scorecard\n\
        ## Latency (ms)\n\
        | Request class | p50 | p50 baseline | p95 | p95 baseline |\n\
        |---|---:|---:|---:|---:|\n\
        | diagnostics | 54.25 | 53.00 | 66.00 | 66.00 |\n";
    fs::write(status_dir.join("editor_ux.md"), md)?;

    assert_eq!(
        format_quality_metrics_bullet(dir.path()),
        "diagnostics p50 = 54 ms (receipt: [editor_ux.md](editor_ux.md)); \
         incremental parse median = unmeasured"
    );
    Ok(())
}

#[test]
fn test_format_quality_metrics_bullet_with_only_parser_receipt() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let status_dir = dir.path().join("docs/project/status");
    fs::create_dir_all(&status_dir)?;
    let json = r#"{
        "schema_version": 1,
        "generated_at_epoch_s": 1234567890,
        "metrics": {
            "incremental_small_edit": {"iterations": 35, "median_ns": 2500, "p95_ns": 3000, "mean_ns": 2600},
            "incremental_multiple_edits": {"iterations": 35, "median_ns": 1499, "p95_ns": 2000, "mean_ns": 1600}
        }
    }"#;
    fs::write(status_dir.join("parser_performance_scorecard.json"), json)?;

    assert_eq!(
        format_quality_metrics_bullet(dir.path()),
        "diagnostics p50 = unmeasured; incremental parse median = 1–3 µs \
         (receipt: [parser_performance_scorecard.json](parser_performance_scorecard.json))"
    );
    Ok(())
}

#[test]
fn test_format_quality_metrics_bullet_fallback_when_receipts_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bullet = format_quality_metrics_bullet(dir.path());
    assert!(bullet.contains("unmeasured"));
    assert!(!bullet.contains("931ns"));
    assert!(!bullet.contains("<50ms"));
}

#[test]
fn test_generate_quality_status_reports_observations_without_execution_verdict() -> Result<()> {
    let original = "# Quality fixture\n\n\
        Human-owned introduction.\n\n\
        <!-- BEGIN: QUALITY_METRICS_BULLETS -->\n\
        - **Production Status**: LSP server public beta (`just ci-gate` passing)\n\
        <!-- END: QUALITY_METRICS_BULLETS -->\n\n\
        <!-- BEGIN: QUALITY_CRATE_TABLE -->\n\
        stale crate inventory\n\
        <!-- END: QUALITY_CRATE_TABLE -->\n\n\
        <!-- BEGIN: FLAKY_TESTS_SUMMARY -->\n\
        stale flaky summary\n\
        <!-- END: FLAKY_TESTS_SUMMARY -->\n\n\
        Human-owned closing guidance.\n";
    let tests = PerCrateTestCounts {
        by_crate: BTreeMap::from([(String::from("perl-quote"), 7)]),
        unattributed: 0,
    };
    let cases = [
        ("missing registry", None, "UNVERIFIED", "UNVERIFIED"),
        ("invalid registry", Some("{not-json"), "UNVERIFIED", "UNVERIFIED"),
        ("explicit zero", Some(r#"{"summary":{"active":0,"resolved":0}}"#), "0", "0"),
        ("observed counts", Some(r#"{"summary":{"active":2,"resolved":5}}"#), "2", "5"),
    ];
    for (case, registry, active, resolved) in cases {
        let dir = tempfile::tempdir()?;
        if let Some(json) = registry {
            let ci_dir = dir.path().join(".ci");
            fs::create_dir_all(&ci_dir)?;
            fs::write(ci_dir.join("flaky-tests.json"), json)?;
        }
        let mutation = collect_per_crate_mutation(dir.path());
        let observations = generate_quality_observations(dir.path(), original, &mutation)?;
        let updated = generate_quality_status(dir.path(), original, &tests)?;
        let expected_table = "| Crate | Mutants listed | Tests (lib) |\n\
            |-------|---------------|-------------|\n\
            | perl-quote | — | 7 |";
        assert_eq!(
            updated,
            observations.replace("stale crate inventory", expected_table),
            "full rendering must share observations and replace only the crate table: {case}"
        );

        assert!(!updated.contains("Production Status"), "{case}");
        assert!(!updated.contains("`just ci-gate` passing"), "{case}");
        assert!(!updated.contains("pending first nightly CI run"), "{case}");
        assert!(updated.contains("no mutation inventory entries available"), "{case}");
        assert!(
            updated.contains("**Evidence scope**: Discovered test and mutation inventories"),
            "{case}"
        );
        assert!(updated.contains("does not execute the listed tests"), "{case}");
        assert!(
            updated.contains("candidate's execution receipts and current required checks"),
            "{case}"
        );
        assert!(
            updated.contains("[verification protocol](../protocols/verification.md)"),
            "{case}"
        );
        assert!(updated.contains("performance metrics unmeasured"), "{case}");
        assert!(updated.contains("| perl-quote | — | 7 |"), "{case}");
        assert!(updated.contains(&format!("| Active | {active} |")), "{case}");
        assert!(updated.contains(&format!("| Resolved | {resolved} |")), "{case}");
        if active == "UNVERIFIED" {
            assert!(!updated.contains("| Active | 0 |"), "{case}");
            assert!(!updated.contains("| Resolved | 0 |"), "{case}");
        }
        assert!(updated.contains("`just status-update quality`"), "{case}");
        assert!(!updated.contains("just status-update --only quality"), "{case}");
        assert!(updated.starts_with("# Quality fixture\n\nHuman-owned introduction."), "{case}");
        assert!(updated.ends_with("Human-owned closing guidance.\n"), "{case}");
        assert!(!updated.contains("stale crate inventory"), "{case}");
        assert!(!updated.contains("stale flaky summary"), "{case}");
    }
    Ok(())
}

#[test]
fn test_generate_quality_observations_preserves_unowned_bytes() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let prefix = "# Quality fixture\n\nHuman-owned introduction.  \n\n";
    let middle = "\n\nHuman-owned table guidance.\n\
        <!-- BEGIN: QUALITY_CRATE_TABLE -->\n\
        | Crate | Mutants listed | Tests (lib) |\n\
        | saved-crate | — | 901 |\n\n\
        Preserve this inventory note.  \n\
        <!-- END: QUALITY_CRATE_TABLE -->\n\n";
    let suffix = "\n\nHuman-owned closing guidance.\n";
    let original = format!(
        "{prefix}<!-- BEGIN: QUALITY_METRICS_BULLETS -->\nold measurements\n\
        <!-- END: QUALITY_METRICS_BULLETS -->{middle}\
        <!-- BEGIN: FLAKY_TESTS_SUMMARY -->\nold flakes\n\
        <!-- END: FLAKY_TESTS_SUMMARY -->{suffix}"
    );
    let mutation = BTreeMap::from([(String::from("perl-quote"), 3)]);
    let updated = generate_quality_observations(dir.path(), &original, &mutation)?;
    assert_eq!(
        updated.split_once("<!-- BEGIN: QUALITY_METRICS_BULLETS -->").map(|parts| parts.0),
        Some(prefix)
    );
    let after_bullets = updated.split_once("<!-- END: QUALITY_METRICS_BULLETS -->").map(|p| p.1);
    assert_eq!(
        after_bullets
            .and_then(|tail| tail.split_once("<!-- BEGIN: FLAKY_TESTS_SUMMARY -->"))
            .map(|p| p.0),
        Some(middle)
    );
    assert_eq!(
        updated.split_once("<!-- END: FLAKY_TESTS_SUMMARY -->").map(|parts| parts.1),
        Some(suffix)
    );
    assert!(updated.contains("per-crate data from"), "use the supplied mutation observation");
    assert!(!updated.contains("written by nightly CI"));
    assert!(!updated.contains("old measurements") && !updated.contains("old flakes"));
    Ok(())
}

#[test]
fn test_generate_quality_observations_requires_only_its_own_markers() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let original = "<!-- BEGIN: QUALITY_METRICS_BULLETS -->\nold measurements\n\
        <!-- END: QUALITY_METRICS_BULLETS -->\n\
        <!-- BEGIN: FLAKY_TESTS_SUMMARY -->\nold flakes\n\
        <!-- END: FLAKY_TESTS_SUMMARY -->";
    let mutation = BTreeMap::new();
    assert!(generate_quality_observations(dir.path(), original, &mutation).is_ok());
    assert!(generate_quality_status(dir.path(), original, &PerCrateTestCounts::default()).is_err());
    for marker in [
        "<!-- BEGIN: QUALITY_METRICS_BULLETS -->",
        "<!-- END: QUALITY_METRICS_BULLETS -->",
        "<!-- BEGIN: FLAKY_TESTS_SUMMARY -->",
        "<!-- END: FLAKY_TESTS_SUMMARY -->",
    ] {
        let broken = original.replace(marker, "");
        assert!(generate_quality_observations(dir.path(), &broken, &mutation).is_err(), "{marker}");
    }
    Ok(())
}
