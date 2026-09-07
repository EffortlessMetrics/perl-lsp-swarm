//! Executable compatibility fixtures for the `perl_parser::dead_code` API
//! disposition ledger (#9777).
//!
//! Every fixture here is named `dcapi-*` and is referenced by exactly one row or
//! declaration in `policy/dead-code-api-ledger.toml`. The ledger records what the
//! bounded compatibility surface can and cannot represent; these fixtures make
//! those claims falsifiable instead of aspirational.
//!
//! Two mechanisms carry the weight:
//!
//! * **Compiler-enforced exhaustiveness.** The variant partition and the struct
//!   destructurings below name every variant and every field without a `..`
//!   rest pattern, so adding a `DeadCodeType` variant or a public field to
//!   `DeadCode` / `DeadCodeStats` / `DeadCodeAnalysis` fails to compile until it
//!   is dispositioned here and in the ledger.
//! * **Behavioral negative controls.** The remaining fixtures prove the declared
//!   lossiness is real: variants no producer emits, configuration that changes
//!   nothing, a constant that is not graded proof, and a report that cannot
//!   render two of its own variants.
//!
//! These fixtures deliberately assert *current compatibility behavior*. They are
//! not a specification of desired behavior: the replacement authorities named in
//! the ledger (#8118 local, #10935 workspace) own that. A fixture failing here
//! means the frozen record drifted, not that the replacement arrived.

use perl_parser::dead_code::{
    DeadCode, DeadCodeAnalysis, DeadCodeDetector, DeadCodeStats, DeadCodeType, generate_report,
};
use perl_parser::workspace_index::WorkspaceIndex;
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The constant every current producer stamps on `DeadCode::confidence`.
const CONSTANT_CONFIDENCE: f32 = 0.9;

/// Index one initial source commit.
///
/// Uses the canonical `index_initial_file_str` entry point rather than the
/// `index_file_str` compatibility surface: the #11301 caller ledger caps
/// compatibility-API growth, and a new test has no reason to add to that
/// baseline.
fn index_initial(index: &WorkspaceIndex, uri: &str, code: &str) -> Result<(), String> {
    let indexed_uri = match uri.strip_prefix("file://") {
        Some(path) => perl_uri::fs_path_to_uri(PathBuf::from(path)),
        None => Ok(uri.to_string()),
    }?;
    index.index_initial_file_str(&indexed_uri, code)
}

/// A workspace that exercises every construct a reader would expect to produce
/// an import or export finding, alongside constructs that genuinely do produce
/// findings so the corpus cannot pass by analysing nothing.
fn import_and_export_workspace() -> Result<WorkspaceIndex, String> {
    let index = WorkspaceIndex::new();
    // `POSIX` and `List::Util::first` are imported and never used; `Exporter`
    // machinery declares exports that nothing outside the package calls.
    index_initial(
        &index,
        "file:///Imports.pm",
        "package Imports;\n\
         use strict;\n\
         use warnings;\n\
         use POSIX qw(floor);\n\
         use List::Util qw(first);\n\
         our @EXPORT_OK = qw(never_called_elsewhere);\n\
         sub never_called_elsewhere { return 1; }\n\
         sub locally_dead {\n\
         return 2;\n\
         print 'unreachable in Imports';\n\
         }\n\
         1;\n",
    )?;
    // A second module chosen so that, together with `Imports.pm`, every variant
    // any current producer emits appears at least once. Without this the
    // constant-confidence proof would sample only two of the six and could not
    // see a change to the other four.
    index_initial(
        &index,
        "file:///Wide.pm",
        "package Wide;\n\
         use constant UNUSED_CONST => 5;\n\
         our $unused_package_var = 1;\n\
         sub never_called { return 1; }\n\
         sub with_dead {\n\
         if (0) {\n\
         print 'dead branch';\n\
         }\n\
         return 3;\n\
         print 'unreachable';\n\
         }\n\
         1;\n",
    )?;
    index_initial(&index, "file:///main.pl", "use Imports;\nreturn 1;\nprint 'x';\n")?;
    Ok(index)
}

/// The variants `variant_is_produced` claims a producer for. The corpus must
/// exhibit every one of them, or a proof that ranges over "all findings" is
/// silently ranging over a subset.
const EXPECTED_PRODUCED: [DeadCodeType; 6] = [
    DeadCodeType::UnusedSubroutine,
    DeadCodeType::UnusedVariable,
    DeadCodeType::UnusedConstant,
    DeadCodeType::UnusedPackage,
    DeadCodeType::UnreachableCode,
    DeadCodeType::DeadBranch,
];

/// The per-file findings this corpus produces in `Imports.pm` — the file that
/// is deliberately *not* declared as an entry point by
/// `dcapi-entry-point-is-inert`. If entry points ever start filtering, these are
/// the findings that disappear.
fn imports_pm_unreachable_count(analysis: &DeadCodeAnalysis) -> usize {
    analysis
        .dead_code
        .iter()
        .filter(|d| {
            d.code_type == DeadCodeType::UnreachableCode && d.file_path.ends_with("Imports.pm")
        })
        .count()
}

// ---------------------------------------------------------------------------
// dcapi-variant-producer-partition
// ---------------------------------------------------------------------------

/// Every `DeadCodeType` variant is partitioned into "some current producer emits
/// it" and "no current producer emits it".
///
/// The `match` is exhaustive with no wildcard arm, so a new variant cannot be
/// added to the enum without a deliberate decision here and a matching ledger
/// row. `false` is the honest answer for a variant the compatibility surface
/// advertises but never constructs.
fn variant_is_produced(variant: DeadCodeType) -> bool {
    match variant {
        DeadCodeType::UnusedSubroutine => true,
        DeadCodeType::UnusedVariable => true,
        DeadCodeType::UnusedConstant => true,
        DeadCodeType::UnusedPackage => true,
        DeadCodeType::UnreachableCode => true,
        DeadCodeType::DeadBranch => true,
        // Advertised by the enum and by serde, constructed by nothing in the
        // workspace. `dcapi-import-export-never-produced` proves it.
        DeadCodeType::UnusedImport => false,
        DeadCodeType::UnusedExport => false,
    }
}

/// Fixture `dcapi-variant-producer-partition`.
#[test]
fn dcapi_variant_producer_partition_is_exhaustive_and_non_vacuous() {
    const ALL: [DeadCodeType; 8] = [
        DeadCodeType::UnusedSubroutine,
        DeadCodeType::UnusedVariable,
        DeadCodeType::UnusedConstant,
        DeadCodeType::UnusedPackage,
        DeadCodeType::UnreachableCode,
        DeadCodeType::DeadBranch,
        DeadCodeType::UnusedImport,
        DeadCodeType::UnusedExport,
    ];

    let produced = ALL.iter().filter(|v| variant_is_produced(**v)).count();
    let never_produced = ALL.len() - produced;

    // The produced half must be exactly the set the corpus exhibits, so the two
    // lists cannot drift apart.
    for expected in EXPECTED_PRODUCED {
        assert!(variant_is_produced(expected), "{expected:?} is exhibited by the corpus");
    }
    assert_eq!(produced, EXPECTED_PRODUCED.len());

    // Both sides are non-empty: a partition that collapsed to "everything is
    // produced" would silently restore the overclaim this ledger exists to
    // record.
    assert_eq!(produced, 6, "expected exactly six variants with a current producer");
    assert_eq!(never_produced, 2, "expected exactly two variants with no current producer");
}

// ---------------------------------------------------------------------------
// dcapi-import-export-never-produced
// ---------------------------------------------------------------------------

/// Fixture `dcapi-import-export-never-produced`.
///
/// Neither `analyze_workspace` nor `analyze_file` ever constructs
/// `UnusedImport` or `UnusedExport`, even over a workspace built specifically to
/// contain unused imports and uncalled exported subs. The two variants are
/// serializable vocabulary with no producer.
#[test]
fn dcapi_import_export_never_produced() -> TestResult {
    let index = import_and_export_workspace()?;
    let detector = DeadCodeDetector::new(index);

    let analysis = detector.analyze_workspace();

    // Non-vacuous: the corpus really does produce findings, so an empty result
    // cannot be mistaken for a passing negative control.
    assert!(
        !analysis.dead_code.is_empty(),
        "corpus must produce some findings or the negative control is vacuous"
    );

    for item in &analysis.dead_code {
        assert!(
            item.code_type != DeadCodeType::UnusedImport
                && item.code_type != DeadCodeType::UnusedExport,
            "no current producer may emit {:?}; found {item:?}",
            item.code_type
        );
    }

    let per_file = detector.analyze_file(&PathBuf::from("/Imports.pm"))?;
    for item in &per_file {
        assert!(
            item.code_type != DeadCodeType::UnusedImport
                && item.code_type != DeadCodeType::UnusedExport,
            "analyze_file may not emit {:?} either; found {item:?}",
            item.code_type
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// dcapi-entry-point-is-inert
// ---------------------------------------------------------------------------

/// Fixture `dcapi-entry-point-is-inert`.
///
/// `add_entry_point` records a path and nothing reads it. Adding entry points —
/// including one that names the only real script in the workspace — cannot
/// change a single finding. The API therefore offers root/entry configuration it
/// does not honour, which is why the ledger dispositions it `deprecate` with an
/// `inert` producer rather than mapping it to canonical root identity.
#[test]
fn dcapi_entry_point_is_inert() -> TestResult {
    let baseline = {
        let index = import_and_export_workspace()?;
        let detector = DeadCodeDetector::new(index);
        detector.analyze_workspace()
    };

    let configured = {
        let index = import_and_export_workspace()?;
        let mut detector = DeadCodeDetector::new(index);
        // A *strict subset*: `Imports.pm` is deliberately left undeclared. An
        // implementation that honoured entry points at all would have to treat
        // the undeclared file differently from the declared one, so this is the
        // shape that actually discriminates. Declaring every document would make
        // the comparison vacuous.
        detector.add_entry_point(PathBuf::from("/main.pl"));
        detector.analyze_workspace()
    };

    assert!(
        !baseline.dead_code.is_empty(),
        "baseline must be non-empty or the comparison proves nothing"
    );
    assert!(
        imports_pm_unreachable_count(&baseline) > 0,
        "the undeclared file must contribute findings of its own, or entry-point \
         filtering would be unobservable and this control would be vacuous"
    );
    assert_eq!(
        baseline.dead_code.len(),
        configured.dead_code.len(),
        "declaring entry points changed the finding count; the ledger's `inert` disposition is stale"
    );

    // Compared as a multiset, not a sequence. `analyze_workspace` walks the
    // document store, whose iteration order is not stable between runs, so the
    // *contents* are the claim here and an order difference is not a finding.
    // (That instability is itself consistent with the ledger, which records
    // `DeadCodeAnalysis::dead_code` as an unordered accumulation.)
    let keys = |analysis: &DeadCodeAnalysis| {
        let mut keys: Vec<String> = analysis
            .dead_code
            .iter()
            .map(|d| format!("{:?}|{:?}|{}", d.code_type, d.name, d.start_line))
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(
        keys(&baseline),
        keys(&configured),
        "declaring entry points changed the findings; the ledger's `inert` disposition is stale"
    );

    Ok(())
}

/// Fixture `dcapi-entry-point-accepts-unresolvable-path`.
///
/// The entry-point API performs no validation: a path that exists in no root,
/// and a bare relative path that cannot identify a root at all, are both
/// accepted silently. Nothing refuses the ambiguity, because nothing consults
/// the value.
#[test]
fn dcapi_entry_point_accepts_unresolvable_path() -> TestResult {
    let index = import_and_export_workspace()?;
    let mut detector = DeadCodeDetector::new(index);

    detector.add_entry_point(PathBuf::from("/definitely/not/in/this/workspace.pl"));
    // The same relative path could belong to any root; the API cannot say which.
    detector.add_entry_point(PathBuf::from("lib/App.pm"));

    let analysis = detector.analyze_workspace();
    assert!(
        !analysis.dead_code.is_empty(),
        "unresolvable entry points neither refused nor degraded the analysis"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// dcapi-confidence-is-a-constant
// ---------------------------------------------------------------------------

/// Fixture `dcapi-confidence-is-a-constant`.
///
/// `DeadCode::confidence` is typed as a graded `0.0..=1.0` score but every
/// current producer stamps the same literal. It carries no information, and in
/// particular it does not distinguish a text-scan guess from an index-backed
/// finding. The ledger records `proof_ceiling = none` for this field; a consumer
/// must not read it as proof strength or as deletion authority.
#[test]
fn dcapi_confidence_is_a_constant() -> TestResult {
    let index = import_and_export_workspace()?;
    let detector = DeadCodeDetector::new(index);
    let analysis = detector.analyze_workspace();

    assert!(!analysis.dead_code.is_empty(), "need findings to inspect confidence");

    let distinct: Vec<f32> = {
        let mut seen: Vec<f32> = Vec::new();
        for item in &analysis.dead_code {
            if !seen.iter().any(|v| (v - item.confidence).abs() < f32::EPSILON) {
                seen.push(item.confidence);
            }
        }
        seen
    };

    assert_eq!(
        distinct.len(),
        1,
        "confidence took more than one value: it may have become meaningful, which the ledger does not record"
    );
    assert!(
        (distinct[0] - CONSTANT_CONFIDENCE).abs() < f32::EPSILON,
        "expected the constant {CONSTANT_CONFIDENCE}, found {}",
        distinct[0]
    );

    // The constant holds across *every* variant a producer emits, not just a
    // sampled pair — so a change to any one producer's confidence breaks this.
    // Both proof classes are represented (index-backed unused symbols and
    // text-scan unreachable/dead-branch findings), so the field cannot be used
    // to tell the two apart either.
    for expected in EXPECTED_PRODUCED {
        assert!(
            analysis.dead_code.iter().any(|d| d.code_type == expected),
            "corpus must exhibit {expected:?} for the constant-confidence proof to cover it"
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// dcapi-result-shape-cannot-represent-failure
// ---------------------------------------------------------------------------

/// Fixture `dcapi-result-shape-cannot-represent-failure`.
///
/// Exhaustive destructuring of the three public result types. There is no rest
/// pattern, so a new public field breaks this test until it is dispositioned.
///
/// The assertion the ledger depends on is structural: none of the fields below
/// can carry a partial, cancelled, deadline, resource, stale, product-failure or
/// instrument-failure state, and none carries root, project, component or
/// generation identity. `analyze_workspace` discards every per-file
/// `analyze_file` error, so a file that failed to analyse and a file with no
/// findings produce byte-identical output. `files_analyzed` counts documents
/// visited, not documents successfully analysed, so it cannot recover the
/// difference either.
#[test]
fn dcapi_result_shape_cannot_represent_failure() {
    let analysis = DeadCodeAnalysis {
        dead_code: vec![],
        stats: DeadCodeStats::default(),
        files_analyzed: 3,
        total_lines: 30,
    };

    // Every public field of DeadCodeAnalysis, named exhaustively.
    let DeadCodeAnalysis { dead_code, stats, files_analyzed, total_lines } = &analysis;
    assert!(dead_code.is_empty());
    assert_eq!(*files_analyzed, 3);
    assert_eq!(*total_lines, 30);

    // Every public field of DeadCodeStats, named exhaustively. All seven are
    // plain counters; none can hold a terminal outcome or a refusal.
    let DeadCodeStats {
        unused_subroutines,
        unused_variables,
        unused_constants,
        unused_packages,
        unreachable_statements,
        dead_branches,
        total_dead_lines,
    } = stats;
    assert_eq!(
        *unused_subroutines
            + *unused_variables
            + *unused_constants
            + *unused_packages
            + *unreachable_statements
            + *dead_branches
            + *total_dead_lines,
        0,
        "Default must be all-zero; a non-zero default would make an empty analysis unreadable"
    );

    // Every public field of DeadCode, named exhaustively. `file_path` is the
    // only location identity: there is no root, project, component or document
    // generation, so the same relative path under two roots is indistinguishable
    // once the absolute prefix is stripped by any consumer.
    let item = DeadCode {
        code_type: DeadCodeType::UnusedSubroutine,
        name: Some("foo".to_string()),
        file_path: PathBuf::from("/a/lib/App.pm"),
        start_line: 1,
        end_line: 1,
        reason: "Symbol is never used".to_string(),
        confidence: CONSTANT_CONFIDENCE,
        suggestion: Some("Remove or use this symbol".to_string()),
    };
    let DeadCode {
        code_type,
        name,
        file_path,
        start_line,
        end_line,
        reason,
        confidence,
        suggestion,
    } = &item;
    assert_eq!(*code_type, DeadCodeType::UnusedSubroutine);
    assert_eq!(name.as_deref(), Some("foo"));
    assert_eq!(file_path, &PathBuf::from("/a/lib/App.pm"));
    assert_eq!(*start_line, 1);
    assert_eq!(*end_line, 1);
    assert!(!reason.is_empty());
    assert!((*confidence - CONSTANT_CONFIDENCE).abs() < f32::EPSILON);
    // `suggestion` is advisory prose, not an edit: it carries no range, no
    // replacement text and no authorization to remove anything.
    assert_eq!(suggestion.as_deref(), Some("Remove or use this symbol"));
}

// ---------------------------------------------------------------------------
// dcapi-report-cannot-render-import-export
// ---------------------------------------------------------------------------

/// Fixture `dcapi-report-cannot-render-import-export`.
///
/// `generate_report` renders only the seven `DeadCodeStats` counters. Because
/// `DeadCodeStats` has no counter for `UnusedImport` or `UnusedExport`, items of
/// those two variants are invisible in the report body even when they are
/// present in `dead_code`. The only place they survive is the untyped
/// `Dead code items:` total.
///
/// This is constructed by hand rather than produced, precisely because no
/// producer emits these variants; it proves the *renderer's* boundary, not a
/// reachable production state.
#[test]
fn dcapi_report_cannot_render_import_export() {
    let import_item = DeadCode {
        code_type: DeadCodeType::UnusedImport,
        name: Some("POSIX".to_string()),
        file_path: PathBuf::from("/Imports.pm"),
        start_line: 4,
        end_line: 4,
        reason: "import is never used".to_string(),
        confidence: CONSTANT_CONFIDENCE,
        suggestion: None,
    };
    let export_item = DeadCode {
        code_type: DeadCodeType::UnusedExport,
        name: Some("never_called_elsewhere".to_string()),
        file_path: PathBuf::from("/Imports.pm"),
        start_line: 6,
        end_line: 6,
        reason: "export is never used".to_string(),
        confidence: CONSTANT_CONFIDENCE,
        suggestion: None,
    };

    let analysis = DeadCodeAnalysis {
        dead_code: vec![import_item, export_item],
        // Stats deliberately zero: `analyze_workspace`'s counting `match` has a
        // `_ => {}` arm for exactly these two variants, so a real run would also
        // leave them uncounted.
        stats: DeadCodeStats::default(),
        files_analyzed: 1,
        total_lines: 9,
    };

    let report = generate_report(&analysis);

    assert!(report.contains("Dead code items: 2"), "the untyped total does see the two items");
    for line in report.lines() {
        assert!(
            !line.to_ascii_lowercase().contains("import"),
            "report gained an import row without a ledger update: {line}"
        );
        assert!(
            !line.to_ascii_lowercase().contains("export"),
            "report gained an export row without a ledger update: {line}"
        );
    }

    // The report also never claims an item is safe to remove: it renders counts
    // only. No wording in it may be read as deletion authority.
    assert!(!report.to_ascii_lowercase().contains("safe to remove"));
    assert!(!report.to_ascii_lowercase().contains("safe to delete"));
}

// ---------------------------------------------------------------------------
// dcapi-export-paths-resolve-to-one-module
// ---------------------------------------------------------------------------

/// Fixture `dcapi-export-paths-resolve-to-one-module`.
///
/// The surface is reachable by three public paths: the canonical
/// `perl_parser::dead_code`, the backwards-compatibility alias
/// `perl_parser::dead_code_detector`, and the `perl_parser::prelude`
/// re-export. All three name the same items, so the ledger dispositions the
/// module once and records the alias and prelude as projections rather than
/// separate authorities.
#[test]
fn dcapi_export_paths_resolve_to_one_module() {
    fn takes_canonical(_: perl_parser::dead_code::DeadCodeType) {}

    // Alias path and prelude path both feed the canonical parameter type, which
    // only compiles if every path resolves to one type.
    takes_canonical(perl_parser::dead_code_detector::DeadCodeType::DeadBranch);
    takes_canonical(perl_parser::prelude::DeadCodeType::DeadBranch);

    let via_alias: perl_parser::dead_code_detector::DeadCodeStats = Default::default();
    let via_prelude: perl_parser::prelude::DeadCodeStats = Default::default();
    assert_eq!(via_alias.total_dead_lines, via_prelude.total_dead_lines);
}
