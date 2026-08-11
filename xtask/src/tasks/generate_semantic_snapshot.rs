//! Generate or check deterministic HIR semantic snapshots over a corpus slice.
//!
//! # Purpose — SNAPSHOT rail, never "gold"
//!
//! This subcommand exercises `lower_ast()` over a small set of corpus fixtures
//! and records the resulting HIR structure as a JSON manifest. It proves that
//! the lowering pipeline is *deterministic and stable* across commits — not
//! that the output is semantically correct.
//!
//! **Curated-gold correctness** (human-labeled expected facts) is a separate,
//! independent schema and is NOT built here.
//!
//! # KPI
//!
//! `semantic_snapshot_stability_rate` — fraction of snapshot entries that match
//! the recorded reference. NOT `semantic_gold_pass_rate`.
//!
//! # Modes
//!
//! - **generate** (default): run `lower_ast()` over the fixture slice, write
//!   the snapshot manifest to `--output`.
//! - **check**: re-run `lower_ast()` and compare against the recorded manifest
//!   at `--snapshot`. Exits non-zero on any drift.

use color_eyre::eyre::{Context, Result, bail};
use perl_corpus::snapshot::{
    HIR_SCHEMA_VERSION, HirSummary, SOURCE_HASH_ALGORITHM, SnapshotEntry, SnapshotManifest,
    source_hash,
};
use perl_parser_core::hir::{HirFile, HirKind, lower_ast};
use std::fs;
use std::path::{Path, PathBuf};

const SNAPSHOT_SCHEMA: &str = "semantic_snapshot.v1";
const SNAPSHOT_KPI: &str = "semantic_snapshot_stability_rate";

// ---------------------------------------------------------------------------
// Public API (called from main.rs)
// ---------------------------------------------------------------------------

/// Arguments for the `generate-semantic-snapshot` subcommand.
pub struct GenerateSemanticSnapshotArgs {
    /// Directory containing the corpus fixture `.pl` files.
    pub fixture_dir: PathBuf,
    /// Path to write (generate mode) or read (check mode) the snapshot manifest.
    pub output: PathBuf,
    /// When true, compare against `output` and fail on drift. When false, write.
    pub check: bool,
}

/// Entry point called from `main.rs`.
pub fn run(args: GenerateSemanticSnapshotArgs) -> Result<()> {
    let fixtures = collect_fixtures(&args.fixture_dir)?;
    if fixtures.is_empty() {
        bail!("No .pl fixture files found in {}", args.fixture_dir.display());
    }

    let fresh_entries = compute_snapshot_entries(&args.fixture_dir, &fixtures)?;

    if args.check {
        run_check(&args.output, &fresh_entries)
    } else {
        run_generate(&args.output, fresh_entries)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Collect `.pl` fixture files recursively, sorted by relative path for determinism.
fn collect_fixtures(root: &Path) -> Result<Vec<PathBuf>> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("Snapshot fixture root symlink is unsupported: {}", root.display());
        }
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => bail!("Snapshot fixture root is not a directory: {}", root.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            bail!("Fixture directory not found: {}", root.display());
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading fixture root metadata {}", root.display()));
        }
    }

    let mut paths = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory)
            .with_context(|| format!("reading fixture dir {}", directory.display()))?;

        for entry in entries {
            let entry = entry
                .with_context(|| format!("reading fixture entry in {}", directory.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("reading fixture type for {}", path.display()))?;
            let is_perl_fixture = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pl"));

            if file_type.is_symlink() {
                bail!("Snapshot fixture symlink is unsupported: {}", path.display());
            }
            if file_type.is_dir() {
                if is_perl_fixture {
                    bail!("Snapshot .pl entry is not a regular file: {}", path.display());
                }
                stack.push(path);
            } else if is_perl_fixture {
                if !file_type.is_file() {
                    bail!("Snapshot .pl entry is not a regular file: {}", path.display());
                }
                paths.push(path);
            }
        }
    }

    paths.sort();
    Ok(paths)
}

/// Run `lower_ast()` over each fixture and compute one `SnapshotEntry` per file.
fn compute_snapshot_entries(root: &Path, fixtures: &[PathBuf]) -> Result<Vec<SnapshotEntry>> {
    fixtures.iter().map(|path| compute_one_entry(root, path)).collect()
}

fn compute_one_entry(root: &Path, path: &Path) -> Result<SnapshotEntry> {
    let source =
        fs::read_to_string(path).with_context(|| format!("reading fixture {}", path.display()))?;
    let fixture_id = fixture_id(root, path)?;

    let hash = source_hash(&source);
    let hir = lower_source(&source);
    let summary = summarize_hir(&hir);

    Ok(SnapshotEntry {
        fixture_id,
        source_hash: hash,
        hir_schema_version: HIR_SCHEMA_VERSION.to_string(),
        hir_summary: summary,
    })
}

fn fixture_id(root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(root)
        .with_context(|| format!("fixture {} is outside {}", path.display(), root.display()))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let value = component.as_os_str().to_str().ok_or_else(|| {
            color_eyre::eyre::eyre!("fixture path is not UTF-8: {}", path.display())
        })?;
        if value.is_empty() || value == "." || value == ".." {
            bail!("fixture path is not canonical: {}", path.display());
        }
        parts.push(value);
    }
    if parts.is_empty() {
        bail!("fixture path has no relative identity: {}", path.display());
    }
    Ok(parts.join("/"))
}

/// Parse source and lower AST to HIR.
fn lower_source(source: &str) -> HirFile {
    let mut parser = perl_parser_core::Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

/// Build a deterministic structural summary from a `HirFile`.
///
/// Excludes raw source offsets (which change on whitespace edits) so that
/// semantics-preserving formatting changes do not cause snapshot drift.
fn summarize_hir(file: &HirFile) -> HirSummary {
    let item_kind_sequence: Vec<String> =
        file.items.iter().map(|item| item_kind_name(&item.kind).to_string()).collect();

    let item_count = item_kind_sequence.len();
    let scope_count = file.scope_graph.scopes.len();
    let binding_count = file.scope_graph.bindings.len();
    let package_count = file.stash_graph.packages.len();
    let slot_count: usize =
        file.stash_graph.packages.iter().map(|package| package.slots.len()).sum();
    let directive_count = file.compile_environment.directives.len();
    let module_request_count = file.compile_environment.module_requests.len();
    let dynamic_boundary_count = file.compile_environment.dynamic_boundaries.len();

    HirSummary {
        item_count,
        item_kind_sequence,
        scope_count,
        binding_count,
        package_count,
        slot_count,
        directive_count,
        module_request_count,
        dynamic_boundary_count,
    }
}

/// Map a `HirKind` to its stable string name for snapshot keying.
fn item_kind_name(kind: &HirKind) -> &'static str {
    match kind {
        HirKind::PackageDecl(_) => "PackageDecl",
        HirKind::SubDecl(_) => "SubDecl",
        HirKind::MethodDecl(_) => "MethodDecl",
        HirKind::UseDecl(_) => "UseDecl",
        HirKind::RequireDecl(_) => "RequireDecl",
        HirKind::VariableDecl(_) => "VariableDecl",
        HirKind::CallExpr(_) => "CallExpr",
        HirKind::MethodCallExpr(_) => "MethodCallExpr",
        HirKind::IndirectCallExpr(_) => "IndirectCallExpr",
        HirKind::LiteralExpr(_) => "LiteralExpr",
        HirKind::BarewordExpr(_) => "BarewordExpr",
        HirKind::BlockShell(_) => "BlockShell",
        HirKind::BranchShell(_) => "BranchShell",
        HirKind::LoopShell(_) => "LoopShell",
        HirKind::StatementModifierShell(_) => "StatementModifierShell",
        HirKind::ControlTransfer(_) => "ControlTransfer",
        HirKind::DerefExpr(_) => "DerefExpr",
        HirKind::DynamicBoundary(_) => "DynamicBoundary",
        // The HIR enum is non-exhaustive. Explicit unknown-kind failure is a
        // separate #6725 slice that must advance the summary schema deliberately.
        _ => "Unknown",
    }
}

// ---------------------------------------------------------------------------
// Generate mode
// ---------------------------------------------------------------------------

fn run_generate(output: &Path, entries: Vec<SnapshotEntry>) -> Result<()> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut manifest = SnapshotManifest::new(today);
    manifest.entries = entries;
    manifest
        .validate_exact_entry_set(&manifest.entries)
        .context("validating generated snapshot fixture set")?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(&manifest).context("serializing snapshot manifest")?;
    fs::write(output, &json)
        .with_context(|| format!("writing snapshot manifest to {}", output.display()))?;

    println!(
        "generate-semantic-snapshot: wrote {} entries to {}",
        manifest.entries.len(),
        output.display()
    );
    println!(
        "  kpi={} schema={} hir_schema_version={} source_hash_algorithm={}",
        manifest.kpi, manifest.schema, manifest.hir_schema_version, manifest.source_hash_algorithm,
    );
    println!("  claim_boundary: {}", manifest.claim_boundary);

    Ok(())
}

// ---------------------------------------------------------------------------
// Check mode
// ---------------------------------------------------------------------------

fn run_check(snapshot_path: &Path, fresh: &[SnapshotEntry]) -> Result<()> {
    if !snapshot_path.exists() {
        bail!(
            "Snapshot manifest not found at {}; run without --check to generate it",
            snapshot_path.display()
        );
    }

    let json = fs::read_to_string(snapshot_path)
        .with_context(|| format!("reading snapshot manifest {}", snapshot_path.display()))?;
    let recorded: SnapshotManifest =
        serde_json::from_str(&json).context("parsing snapshot manifest")?;

    if recorded.schema != SNAPSHOT_SCHEMA {
        bail!("Snapshot schema mismatch: recorded={} current={}", recorded.schema, SNAPSHOT_SCHEMA);
    }
    if recorded.kpi != SNAPSHOT_KPI {
        bail!("Snapshot KPI mismatch: recorded={} current={}", recorded.kpi, SNAPSHOT_KPI);
    }
    if recorded.source_hash_algorithm != SOURCE_HASH_ALGORITHM {
        bail!(
            "Source hash algorithm mismatch: recorded={} current={}; regenerate snapshot",
            recorded.source_hash_algorithm,
            SOURCE_HASH_ALGORITHM,
        );
    }
    if recorded.hir_schema_version != HIR_SCHEMA_VERSION {
        bail!(
            "HIR schema version mismatch: recorded={} current={}; regenerate snapshot",
            recorded.hir_schema_version,
            HIR_SCHEMA_VERSION,
        );
    }

    recorded
        .validate_exact_entry_set(fresh)
        .with_context(|| format!("validating snapshot set in {}", snapshot_path.display()))?;

    let (stable, total, rate) = recorded.stability_rate(fresh);
    let mut drift_found = false;

    for recorded_entry in &recorded.entries {
        let Some(fresh_entry) =
            fresh.iter().find(|entry| entry.fixture_id == recorded_entry.fixture_id)
        else {
            bail!("Snapshot set changed after validation: missing {}", recorded_entry.fixture_id);
        };

        if fresh_entry.source_hash != recorded_entry.source_hash {
            eprintln!(
                "  SOURCE CHANGED: {} (hash {} -> {})",
                recorded_entry.fixture_id, recorded_entry.source_hash, fresh_entry.source_hash,
            );
            drift_found = true;
        } else if fresh_entry.hir_schema_version != recorded_entry.hir_schema_version {
            eprintln!(
                "  ENTRY SCHEMA DRIFT: {} ({} -> {})",
                recorded_entry.fixture_id,
                recorded_entry.hir_schema_version,
                fresh_entry.hir_schema_version,
            );
            drift_found = true;
        } else if fresh_entry.hir_summary != recorded_entry.hir_summary {
            eprintln!(
                "  HIR DRIFT: {} (same source, different HIR structure)",
                recorded_entry.fixture_id
            );
            eprintln!(
                "    recorded item_count={} fresh item_count={}",
                recorded_entry.hir_summary.item_count, fresh_entry.hir_summary.item_count,
            );
            drift_found = true;
        } else {
            println!("  OK: {}", recorded_entry.fixture_id);
        }
    }

    println!("semantic_snapshot_stability_rate: {stable}/{total} = {:.1}%", rate * 100.0);

    if drift_found || stable < total {
        bail!(
            "Snapshot check failed: {stable}/{total} stable. \
             Run without --check to regenerate the snapshot manifest."
        );
    }

    println!("Snapshot check passed: all {total} entries stable.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::TempDir;

    fn write_fixture(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        let mut file = fs::File::create(&path).expect("create fixture");
        file.write_all(content.as_bytes()).expect("write fixture");
        path
    }

    fn generate_snapshot(fixture_dir: &Path, output: &Path) {
        run(GenerateSemanticSnapshotArgs {
            fixture_dir: fixture_dir.to_path_buf(),
            output: output.to_path_buf(),
            check: false,
        })
        .expect("generate snapshot");
    }

    fn check_snapshot(fixture_dir: &Path, output: &Path) -> Result<()> {
        run(GenerateSemanticSnapshotArgs {
            fixture_dir: fixture_dir.to_path_buf(),
            output: output.to_path_buf(),
            check: true,
        })
    }

    #[test]
    fn generates_snapshot_for_minimal_source() {
        let temporary = TempDir::new().expect("tempdir");
        let source = "package Foo;\nsub bar { return 1; }\n1;\n";
        write_fixture(temporary.path(), "minimal.pl", source);

        let output = temporary.path().join("snapshot.json");
        generate_snapshot(temporary.path(), &output);

        let json = fs::read_to_string(&output).expect("read output");
        let manifest: SnapshotManifest = serde_json::from_str(&json).expect("deserialize manifest");

        assert_eq!(manifest.schema, SNAPSHOT_SCHEMA);
        assert_eq!(manifest.kpi, SNAPSHOT_KPI);
        assert_eq!(manifest.hir_schema_version, HIR_SCHEMA_VERSION);
        assert_eq!(manifest.source_hash_algorithm, SOURCE_HASH_ALGORITHM);
        assert_eq!(manifest.entries.len(), 1);

        let entry = &manifest.entries[0];
        assert_eq!(entry.fixture_id, "minimal.pl");
        assert_eq!(entry.source_hash, source_hash(source));
        assert!(entry.hir_summary.item_count > 0, "expected HIR items from non-trivial source");
    }

    #[test]
    fn snapshot_names_typed_dereference_items() {
        let temporary = TempDir::new().expect("tempdir");
        let source = "my @arr = @{\"foo\"};\n";
        write_fixture(temporary.path(), "deref.pl", source);

        let output = temporary.path().join("snapshot.json");
        generate_snapshot(temporary.path(), &output);

        let json = fs::read_to_string(&output).expect("read output");
        let manifest: SnapshotManifest = serde_json::from_str(&json).expect("deserialize manifest");
        assert!(
            manifest.entries[0]
                .hir_summary
                .item_kind_sequence
                .iter()
                .any(|kind| kind == "DerefExpr"),
            "typed dereference items must have a stable snapshot name"
        );
    }

    #[test]
    fn check_mode_passes_when_snapshot_matches() {
        let temporary = TempDir::new().expect("tempdir");
        let source = "package Bar;\nsub baz { 0 }\n1;\n";
        write_fixture(temporary.path(), "bar.pl", source);
        let output = temporary.path().join("snapshot.json");

        generate_snapshot(temporary.path(), &output);
        check_snapshot(temporary.path(), &output).expect("check should pass on unchanged source");
    }

    #[test]
    fn check_mode_fails_on_source_change() {
        let temporary = TempDir::new().expect("tempdir");
        let path =
            write_fixture(temporary.path(), "changed.pl", "package Changed; sub first { 1 } 1;");
        let output = temporary.path().join("snapshot.json");
        generate_snapshot(temporary.path(), &output);

        fs::write(&path, "package Changed; sub first { 1 } sub second { 2 } 1;")
            .expect("write changed source");

        assert!(
            check_snapshot(temporary.path(), &output).is_err(),
            "check must fail when source changes"
        );
    }

    #[test]
    fn check_mode_fails_when_fresh_fixture_is_added() {
        let temporary = TempDir::new().expect("tempdir");
        write_fixture(temporary.path(), "first.pl", "1;");
        let output = temporary.path().join("snapshot.json");
        generate_snapshot(temporary.path(), &output);

        write_fixture(temporary.path(), "second.pl", "2;");

        assert!(
            check_snapshot(temporary.path(), &output).is_err(),
            "check must reject a fresh fixture absent from the manifest"
        );
    }

    #[test]
    fn check_mode_fails_when_recorded_fixture_is_removed() {
        let temporary = TempDir::new().expect("tempdir");
        let first = write_fixture(temporary.path(), "first.pl", "1;");
        write_fixture(temporary.path(), "second.pl", "2;");
        let output = temporary.path().join("snapshot.json");
        generate_snapshot(temporary.path(), &output);

        fs::remove_file(first).expect("remove recorded fixture");

        assert!(
            check_snapshot(temporary.path(), &output).is_err(),
            "check must reject a recorded fixture missing from fresh input"
        );
    }

    #[test]
    fn check_mode_fails_on_duplicate_recorded_id() {
        let temporary = TempDir::new().expect("tempdir");
        write_fixture(temporary.path(), "fixture.pl", "1;");
        let output = temporary.path().join("snapshot.json");
        generate_snapshot(temporary.path(), &output);

        let json = fs::read_to_string(&output).expect("read snapshot");
        let mut manifest: SnapshotManifest = serde_json::from_str(&json).expect("parse snapshot");
        manifest.entries.push(manifest.entries[0].clone());
        fs::write(
            &output,
            serde_json::to_string_pretty(&manifest).expect("serialize duplicate snapshot"),
        )
        .expect("write duplicate snapshot");

        assert!(
            check_snapshot(temporary.path(), &output).is_err(),
            "check must reject duplicate recorded fixture IDs"
        );
    }

    #[cfg(unix)]
    #[test]
    fn fixture_collection_rejects_perl_symlinks() {
        use std::os::unix::fs::symlink;

        let temporary = TempDir::new().expect("tempdir");
        let target = write_fixture(temporary.path(), "target.pl", "1;");
        let link = temporary.path().join("linked.pl");
        symlink(&target, &link).expect("create fixture symlink");

        let error = collect_fixtures(temporary.path())
            .expect_err("symlinked Perl fixtures must fail closed");
        assert!(error.to_string().contains("symlink is unsupported"), "unexpected error: {error}");
    }

    #[test]
    fn recursive_fixture_ids_preserve_relative_paths() {
        let temporary = TempDir::new().expect("tempdir");
        write_fixture(temporary.path(), "first/same.pl", "1;");
        write_fixture(temporary.path(), "second/same.pl", "2;");

        let fixtures = collect_fixtures(temporary.path()).expect("collect recursive fixtures");
        let entries =
            compute_snapshot_entries(temporary.path(), &fixtures).expect("compute entries");
        let ids = entries.iter().map(|entry| entry.fixture_id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids, vec!["first/same.pl", "second/same.pl"]);
    }

    #[cfg(unix)]
    #[test]
    fn fixture_collection_rejects_non_regular_perl_entries() {
        use std::process::Command;

        let temporary = TempDir::new().expect("tempdir");
        let fifo = temporary.path().join("blocked.pl");
        let status = Command::new("mkfifo").arg(&fifo).status().expect("run mkfifo");
        assert!(status.success(), "mkfifo must create the negative-control fixture");

        let error = collect_fixtures(temporary.path())
            .expect_err("non-regular Perl fixtures must fail closed");
        assert!(error.to_string().contains("not a regular file"), "unexpected error: {error}");
    }

    #[test]
    fn check_mode_rejects_hash_algorithm_mismatch() {
        let temporary = TempDir::new().expect("tempdir");
        write_fixture(temporary.path(), "fixture.pl", "1;");
        let output = temporary.path().join("snapshot.json");
        generate_snapshot(temporary.path(), &output);

        let json = fs::read_to_string(&output).expect("read snapshot");
        let mut manifest: SnapshotManifest = serde_json::from_str(&json).expect("parse snapshot");
        manifest.source_hash_algorithm = "legacy-unstable.v0".to_string();
        fs::write(
            &output,
            serde_json::to_string_pretty(&manifest).expect("serialize mismatched snapshot"),
        )
        .expect("write mismatched snapshot");

        assert!(
            check_snapshot(temporary.path(), &output).is_err(),
            "check must reject a manifest recorded with a different digest algorithm"
        );
    }

    #[test]
    fn snapshot_covers_slice_fixtures() {
        let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/perl-corpus/fixtures/snapshot-slice");

        if !fixture_dir.exists() {
            return;
        }

        let fixtures = collect_fixtures(&fixture_dir).expect("collect fixtures");
        assert!(fixtures.len() >= 3, "expected at least 3 slice fixtures, got {}", fixtures.len());

        let entries = compute_snapshot_entries(&fixture_dir, &fixtures).expect("compute entries");
        for entry in &entries {
            assert!(
                entry.hir_summary.item_count > 0,
                "fixture {} produced empty HIR",
                entry.fixture_id
            );
            assert_eq!(entry.hir_schema_version, HIR_SCHEMA_VERSION);
        }
    }
}
