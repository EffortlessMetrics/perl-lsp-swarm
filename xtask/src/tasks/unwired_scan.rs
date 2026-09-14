//! Unwired infrastructure scanner (issue #2667)
//!
//! Scans the workspace for crates that have been built and tested but are not
//! connected to the perl-lsp-rs production code path. The three patterns caught by
//! this tool in Era 7 session 2 each required only 10-50 lines to wire; this
//! scanner makes them findable before they age further.
//!
//! ## Detection strategy
//!
//! For each workspace crate that is NOT `perl-lsp-rs` itself:
//! 1. Count `#[test]` annotations in its `src/` tree — proxy for "has real logic".
//! 2. Check whether the crate name appears in `perl-lsp-rs`'s direct dependency list.
//! 3. Scan for TODO/FIXME comments that mention wiring (e.g. `TODO: wire`, `TODO: connect`).
//!
//! A crate is **flagged** when it has ≥1 test AND is not a direct dep of perl-lsp-rs.
//! The tool also surfaces any matching TODO/FIXME comments across all crates.
//!
//! ## Limitations
//!
//! The dependency check is direct-only (reads the perl-lsp-rs Cargo.toml). Transitive
//! deps (A → B → perl-lsp-rs) are not excluded. That produces false positives for leaf
//! crates consumed via an intermediate crate; reviewers should check the output
//! against the real dependency tree before filing follow-up issues.
//!
//! ## Usage
//!
//! ```bash
//! cargo xtask unwired-scan              # human-readable report
//! cargo xtask unwired-scan --json       # JSON to stdout
//! cargo xtask unwired-scan --check      # exit 1 if any flagged crate found
//! ```

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result};
use serde::Serialize;

// ---------------------------------------------------------------------------
// Public configuration
// ---------------------------------------------------------------------------

/// Configuration for the unwired-scan subcommand.
pub struct UnwiredScanConfig {
    /// Name of the root LSP crate to check dependencies of.
    pub lsp_crate: String,
    /// Emit JSON instead of human-readable output.
    pub json: bool,
    /// Exit with code 1 if any unwired crates are found.
    pub check: bool,
}

impl Default for UnwiredScanConfig {
    fn default() -> Self {
        Self { lsp_crate: "perl-lsp-rs".to_string(), json: false, check: false }
    }
}

// ---------------------------------------------------------------------------
// Report types
// ---------------------------------------------------------------------------

/// Describes one wiring-comment hit inside a source file.
#[derive(Debug, Clone, Serialize)]
pub struct WiringComment {
    /// Relative path from workspace root to the source file.
    pub file: String,
    /// Trimmed source line containing the TODO/FIXME keyword.
    pub line: String,
}

/// Per-crate summary produced by the scanner.
#[derive(Debug, Clone, Serialize)]
pub struct CrateReport {
    /// Crate name as it appears in Cargo.toml.
    pub name: String,
    /// Path relative to workspace root (e.g. `crates/perl-foo`).
    pub path: String,
    /// Number of `#[test]` attributes found in `src/`.
    pub test_count: u32,
    /// Whether `perl-lsp`'s Cargo.toml lists this crate as a direct dependency.
    pub is_direct_dep_of_lsp: bool,
    /// TODO/FIXME wiring comments found in `src/`.
    pub wiring_comments: Vec<WiringComment>,
}

impl CrateReport {
    /// Returns `true` when this crate should be flagged as unwired:
    /// it has at least one test and is not a direct dep of the LSP crate.
    pub fn is_flagged(&self) -> bool {
        self.test_count > 0 && !self.is_direct_dep_of_lsp
    }
}

/// Full scan result.
#[derive(Debug, Serialize)]
pub struct ScanReport {
    /// Name of the root LSP crate that was used as the reference point.
    pub lsp_crate: String,
    /// All crates examined (excluding the LSP crate itself).
    pub crates: Vec<CrateReport>,
    /// Crates flagged as potentially unwired (has tests, not a direct dep).
    pub flagged: Vec<String>,
    /// Total crates examined.
    pub total_crates: usize,
    /// Total flagged crates.
    pub total_flagged: usize,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the unwired infrastructure scan.
pub fn run(config: UnwiredScanConfig) -> Result<()> {
    let root = crate::utils::project_root()?;
    let report = scan(&root, &config.lsp_crate)?;

    if config.json {
        let json = serde_json::to_string_pretty(&report).context("serialize report")?;
        println!("{json}");
    } else {
        print_report(&report);
    }

    if config.check && report.total_flagged > 0 {
        color_eyre::eyre::bail!(
            "{} unwired crate(s) found — run `cargo xtask unwired-scan` to inspect",
            report.total_flagged
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Core scanner
// ---------------------------------------------------------------------------

/// Scan `workspace_root/crates/` and return a full `ScanReport`.
pub fn scan(workspace_root: &Path, lsp_crate: &str) -> Result<ScanReport> {
    let crates_dir = workspace_root.join("crates");

    let workspace_crates = load_workspace_crates(&crates_dir);
    let lsp_package = workspace_crates.iter().find(|package| package.name == lsp_crate);
    let Some(lsp_package) = lsp_package else {
        color_eyre::eyre::bail!(
            "LSP crate package not found: {lsp_crate} — pass the correct package name via --lsp-crate"
        );
    };

    let lsp_deps = parse_crate_deps(&lsp_package.manifest_path);

    let mut crate_reports: Vec<CrateReport> = Vec::new();

    for package in workspace_crates {
        // Skip the LSP crate itself — it is the reference point.
        if package.name == lsp_crate {
            continue;
        }

        let src_dir = package.dir.join("src");
        let test_count = count_tests_in_dir(&src_dir);
        let is_direct_dep = lsp_deps.contains(&package.name);
        let raw_wiring = scan_wiring_comments(&src_dir, workspace_root);

        let rel_path = match package.dir.strip_prefix(workspace_root) {
            Ok(p) => p.display().to_string(),
            Err(_) => package.dir.display().to_string(),
        };

        crate_reports.push(CrateReport {
            name: package.name,
            path: rel_path,
            test_count,
            is_direct_dep_of_lsp: is_direct_dep,
            wiring_comments: raw_wiring,
        });
    }

    // Sort for stable output.
    crate_reports.sort_by(|a, b| a.name.cmp(&b.name));

    let flagged: Vec<String> =
        crate_reports.iter().filter(|r| r.is_flagged()).map(|r| r.name.clone()).collect();

    let total_crates = crate_reports.len();
    let total_flagged = flagged.len();

    Ok(ScanReport {
        lsp_crate: lsp_crate.to_string(),
        crates: crate_reports,
        flagged,
        total_crates,
        total_flagged,
    })
}

// ---------------------------------------------------------------------------
// Primitives (also used from integration tests)
// ---------------------------------------------------------------------------

/// Count `#[test]` occurrences by walking `src_dir` recursively.
/// Returns 0 if the directory does not exist or cannot be read.
pub fn count_tests_in_dir(src_dir: &Path) -> u32 {
    let Ok(first) = fs::read_dir(src_dir) else {
        return 0;
    };
    let mut count = 0u32;
    let mut queue: Vec<PathBuf> = first.filter_map(|e| e.ok().map(|e| e.path())).collect();
    while let Some(path) = queue.pop() {
        if path.is_dir() {
            if let Ok(entries) = fs::read_dir(&path) {
                queue.extend(entries.filter_map(|e| e.ok().map(|e| e.path())));
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs")
            && let Ok(content) = fs::read_to_string(&path)
        {
            count += content.matches("#[test]").count() as u32;
        }
    }
    count
}

/// Parse direct dependency names from a `Cargo.toml` file.
/// Reads `[dependencies]`, `[dev-dependencies]`, and `[build-dependencies]`.
/// Returns an empty set if the file is missing or unparseable.
pub fn parse_crate_deps(cargo_toml: &Path) -> HashSet<String> {
    let Ok(content) = fs::read_to_string(cargo_toml) else {
        return HashSet::new();
    };
    let Ok(parsed) = content.parse::<toml::Table>() else {
        return HashSet::new();
    };
    let mut deps = HashSet::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(toml::Value::Table(table)) = parsed.get(section) {
            for (key, value) in table {
                deps.insert(dependency_package_name(key, value));
            }
        }
    }
    deps
}

#[derive(Debug, Clone)]
struct WorkspaceCrate {
    name: String,
    dir: PathBuf,
    manifest_path: PathBuf,
}

fn load_workspace_crates(crates_dir: &Path) -> Vec<WorkspaceCrate> {
    let Ok(entries) = fs::read_dir(crates_dir) else {
        return Vec::new();
    };

    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|crate_dir| {
            let manifest_path = crate_dir.join("Cargo.toml");
            if !manifest_path.exists() {
                return None;
            }

            let name = parse_package_name(&manifest_path)?;
            Some(WorkspaceCrate { name, dir: crate_dir, manifest_path })
        })
        .collect()
}

fn dependency_package_name(dep_key: &str, dep_value: &toml::Value) -> String {
    dep_value
        .as_table()
        .and_then(|table| table.get("package"))
        .and_then(toml::Value::as_str)
        .unwrap_or(dep_key)
        .to_string()
}

fn parse_package_name(cargo_toml: &Path) -> Option<String> {
    let content = fs::read_to_string(cargo_toml).ok()?;
    let parsed = content.parse::<toml::Table>().ok()?;
    parsed
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .map(ToString::to_string)
}

/// Keywords that suggest a source line is a wiring TODO/FIXME comment.
/// More specific variants (e.g. "TODO: wire this") are subsets of "TODO: wire"
/// and are therefore not listed separately.
const WIRING_KEYWORDS: &[&str] = &["todo: wire", "todo: connect", "fixme: not called"];

/// Scan `src_dir` recursively for TODO/FIXME wiring comments in `.rs` files.
/// Returns one `WiringComment` per matching line. `workspace_root` is used to
/// relativise file paths in the output.
pub fn scan_wiring_comments(src_dir: &Path, workspace_root: &Path) -> Vec<WiringComment> {
    let mut results = Vec::new();
    let mut queue = vec![src_dir.to_path_buf()];
    while let Some(path) = queue.pop() {
        if path.is_dir() {
            if let Ok(entries) = fs::read_dir(&path) {
                queue.extend(entries.filter_map(|e| e.ok().map(|e| e.path())));
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs")
            && let Ok(content) = fs::read_to_string(&path)
        {
            for line in content.lines() {
                let line_lower = line.to_ascii_lowercase();
                for kw in WIRING_KEYWORDS {
                    if line_lower.contains(kw) {
                        let rel = path
                            .strip_prefix(workspace_root)
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|_| path.display().to_string());
                        results.push(WiringComment { file: rel, line: line.trim().to_string() });
                        break;
                    }
                }
            }
        }
    }
    results
}

// ---------------------------------------------------------------------------
// Human-readable output
// ---------------------------------------------------------------------------

fn print_report(report: &ScanReport) {
    println!("[INFO] Unwired Infrastructure Scan");
    println!("[INFO] LSP crate: {}", report.lsp_crate);
    println!("[INFO] Crates examined: {}", report.total_crates);
    println!();

    // Flagged crates (have tests, not a direct dep)
    let flagged: Vec<&CrateReport> = report.crates.iter().filter(|r| r.is_flagged()).collect();
    if flagged.is_empty() {
        println!("[OK] No unwired crates found.");
    } else {
        println!(
            "[WARN] {} crate(s) have tests but are not a direct dep of {}:",
            flagged.len(),
            report.lsp_crate
        );
        println!();
        for cr in &flagged {
            println!("  {} ({}) — {} test(s)", cr.name, cr.path, cr.test_count);
            for wc in &cr.wiring_comments {
                println!("    Wiring hint: {} — {}", wc.file, wc.line);
            }
        }
        println!();
    }

    // Wiring comments across ALL crates (including wired ones)
    let all_wiring: Vec<(&CrateReport, &WiringComment)> = report
        .crates
        .iter()
        .flat_map(|cr| cr.wiring_comments.iter().map(move |wc| (cr, wc)))
        .collect();

    if !all_wiring.is_empty() {
        println!("[INFO] Wiring TODO/FIXME comments ({} total):", all_wiring.len());
        for (cr, wc) in &all_wiring {
            println!("  [{}] {} — {}", cr.name, wc.file, wc.line);
        }
        println!();
    }

    if report.total_flagged > 0 {
        println!(
            "[SUMMARY] {} unwired crate(s) detected. Review each and either wire it into {} or document why it is intentionally isolated.",
            report.total_flagged, report.lsp_crate
        );
    } else {
        println!("[SUMMARY] All tested crates are directly wired into {}.", report.lsp_crate);
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::{must_err_with, must_some_with, must_with};
    use std::fs;
    use tempfile::TempDir;

    #[track_caller]
    fn tempdir() -> TempDir {
        must_with(TempDir::new(), "tempdir")
    }

    #[track_caller]
    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            must_with(fs::create_dir_all(parent), "create fixture parent");
        }
        must_with(fs::write(path, content), "write fixture");
    }

    #[track_caller]
    fn scan_ok(workspace: &Path, lsp_crate: &str) -> ScanReport {
        must_with(scan(workspace, lsp_crate), "scan workspace")
    }

    #[track_caller]
    fn crate_named<'a>(report: &'a ScanReport, name: &str) -> &'a CrateReport {
        must_some_with(
            report.crates.iter().find(|r| r.name == name),
            "crate present in scan report",
        )
    }

    #[track_caller]
    fn first_wiring_hit(hits: &[WiringComment]) -> &WiringComment {
        must_some_with(hits.first(), "wiring comment hit")
    }

    #[test]
    fn test_count_tests_finds_attribute() {
        let dir = tempdir();
        write(dir.path(), "src/lib.rs", "#[test]\nfn a() {}\n#[test]\nfn b() {}\n");
        assert_eq!(count_tests_in_dir(&dir.path().join("src")), 2);
    }

    #[test]
    fn test_count_tests_empty_dir() {
        let dir = tempdir();
        must_with(fs::create_dir_all(dir.path().join("src")), "create empty src dir");
        assert_eq!(count_tests_in_dir(&dir.path().join("src")), 0);
    }

    #[test]
    fn test_count_tests_nonexistent() {
        assert_eq!(count_tests_in_dir(Path::new("/no/such/dir")), 0);
    }

    #[test]
    fn test_parse_deps_basic() {
        let dir = tempdir();
        write(
            dir.path(),
            "Cargo.toml",
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nserde = \"1.0\"\nfoo = { path = \"../foo\" }\n",
        );
        let deps = parse_crate_deps(&dir.path().join("Cargo.toml"));
        assert!(deps.contains("serde"));
        assert!(deps.contains("foo"));
    }

    #[test]
    fn test_parse_deps_missing_file() {
        let deps = parse_crate_deps(Path::new("/no/such/Cargo.toml"));
        assert!(deps.is_empty());
    }

    #[test]
    fn test_scan_wiring_comments_todo_wire() {
        let dir = tempdir();
        write(dir.path(), "src/lib.rs", "// TODO: wire this into diagnostics\npub fn f() {}\n");
        let hits = scan_wiring_comments(&dir.path().join("src"), dir.path());
        assert_eq!(hits.len(), 1);
        assert!(first_wiring_hit(&hits).line.contains("TODO: wire"));
    }

    #[test]
    fn test_scan_wiring_comments_fixme_not_called() {
        let dir = tempdir();
        write(dir.path(), "src/lib.rs", "// FIXME: not called from anywhere\npub fn g() {}\n");
        let hits = scan_wiring_comments(&dir.path().join("src"), dir.path());
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn test_scan_wiring_comments_lowercase_todo_wire() {
        let dir = tempdir();
        write(dir.path(), "src/lib.rs", "// todo: wire this into diagnostics\npub fn f() {}\n");
        let hits = scan_wiring_comments(&dir.path().join("src"), dir.path());
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn test_scan_wiring_comments_no_hits() {
        let dir = tempdir();
        write(dir.path(), "src/lib.rs", "pub fn clean_code() {}\n");
        let hits = scan_wiring_comments(&dir.path().join("src"), dir.path());
        assert!(hits.is_empty());
    }

    #[test]
    fn test_crate_report_is_flagged() {
        let flagged = CrateReport {
            name: "perl-orphan".into(),
            path: "crates/perl-orphan".into(),
            test_count: 3,
            is_direct_dep_of_lsp: false,
            wiring_comments: vec![],
        };
        assert!(flagged.is_flagged());

        let wired = CrateReport {
            name: "perl-wired".into(),
            path: "crates/perl-wired".into(),
            test_count: 3,
            is_direct_dep_of_lsp: true,
            wiring_comments: vec![],
        };
        assert!(!wired.is_flagged());

        let no_tests = CrateReport {
            name: "perl-empty".into(),
            path: "crates/perl-empty".into(),
            test_count: 0,
            is_direct_dep_of_lsp: false,
            wiring_comments: vec![],
        };
        assert!(!no_tests.is_flagged());
    }

    /// Build a minimal fake workspace and run the full scan.
    #[track_caller]
    fn fake_workspace() -> TempDir {
        let dir = tempdir();
        let root = dir.path();

        write(
            root,
            "crates/perl-lsp-rs/Cargo.toml",
            "[package]\nname = \"perl-lsp-rs\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nperl-wired = { path = \"../perl-wired\" }\n",
        );
        write(root, "crates/perl-lsp-rs/src/lib.rs", "");

        write(
            root,
            "crates/perl-wired/Cargo.toml",
            "[package]\nname = \"perl-wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        write(root, "crates/perl-wired/src/lib.rs", "pub fn f() {}\n#[test]\nfn t() {}\n");

        write(
            root,
            "crates/perl-unwired/Cargo.toml",
            "[package]\nname = \"perl-unwired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        write(
            root,
            "crates/perl-unwired/src/lib.rs",
            "pub fn g() {}\n#[test]\nfn t1() {}\n#[test]\nfn t2() {}\n",
        );

        write(
            root,
            "crates/perl-no-tests/Cargo.toml",
            "[package]\nname = \"perl-no-tests\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        write(root, "crates/perl-no-tests/src/lib.rs", "pub fn h() {}\n");

        dir
    }

    #[test]
    fn test_scan_identifies_unwired() {
        let workspace = fake_workspace();
        let report = scan_ok(workspace.path(), "perl-lsp-rs");
        assert!(report.flagged.contains(&"perl-unwired".to_string()));
        assert!(!report.flagged.contains(&"perl-wired".to_string()));
        assert!(!report.flagged.contains(&"perl-no-tests".to_string()));
    }

    #[test]
    fn test_scan_counts_correctly() {
        let workspace = fake_workspace();
        let report = scan_ok(workspace.path(), "perl-lsp-rs");
        let unwired = crate_named(&report, "perl-unwired");
        assert_eq!(unwired.test_count, 2);
        assert!(!unwired.is_direct_dep_of_lsp);
    }

    #[test]
    fn test_scan_excludes_lsp_crate_itself() {
        let workspace = fake_workspace();
        let report = scan_ok(workspace.path(), "perl-lsp-rs");
        assert!(!report.crates.iter().any(|r| r.name == "perl-lsp-rs"));
    }

    /// Passing a nonexistent --lsp-crate must return an error, not silently
    /// flag every crate in the workspace.
    #[test]
    fn test_scan_errors_on_missing_lsp_crate() {
        let workspace = fake_workspace();
        let err = must_err_with(
            scan(workspace.path(), "nonexistent-crate"),
            "scan() must return Err when the lsp_crate Cargo.toml does not exist",
        );
        let msg = err.to_string();
        assert!(
            msg.contains("not found") || msg.contains("nonexistent-crate"),
            "error message should mention what was missing; got: {msg}"
        );
    }

    #[test]
    fn unwired_scan_converted_must_wrappers_carry_track_caller() {
        let src = include_str!("unwired_scan.rs");
        let mut failures = Vec::new();
        for helper in [
            "fn tempdir(",
            "fn write(",
            "fn scan_ok(",
            "fn crate_named<",
            "fn first_wiring_hit(",
            "fn fake_workspace(",
        ] {
            let Some(idx) = src.find(helper) else {
                failures.push(format!("missing wrapper {helper}"));
                continue;
            };
            let preceding = src.get(..idx).unwrap_or("");
            let last_attr_line =
                preceding.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("");
            if last_attr_line.trim() != "#[track_caller]" {
                failures.push(format!(
                    "{helper} is not immediately preceded by #[track_caller] (found {last_attr_line:?})"
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    #[should_panic(expected = "must: create fixture parent:")]
    fn unwired_scan_converted_write_helper_still_fails_when_parent_is_a_file() {
        let dir = tempdir();
        write(dir.path(), "blocked", "not a directory");
        write(dir.path(), "blocked/child.rs", "unreachable");
    }

    #[test]
    #[should_panic(expected = "must: scan workspace:")]
    fn unwired_scan_converted_result_assertion_still_fails_when_lsp_crate_is_missing() {
        let workspace = fake_workspace();
        let _ = scan_ok(workspace.path(), "nonexistent-crate");
    }

    #[test]
    #[should_panic(expected = "must_some: crate present in scan report:")]
    fn unwired_scan_converted_option_assertion_still_fails_when_crate_is_absent() {
        let workspace = fake_workspace();
        let report = scan_ok(workspace.path(), "perl-lsp-rs");
        let _ = crate_named(&report, "does-not-exist");
    }

    #[test]
    #[should_panic(expected = "must_err: missing lsp crate must not succeed:")]
    fn unwired_scan_converted_err_assertion_still_fails_when_scan_succeeds() {
        let workspace = fake_workspace();
        let _ = must_err_with(
            scan(workspace.path(), "perl-lsp-rs"),
            "missing lsp crate must not succeed",
        );
    }

    #[test]
    #[should_panic(expected = "must_some: wiring comment hit:")]
    fn unwired_scan_converted_wiring_hit_assertion_still_fails_when_hits_are_empty() {
        let _ = first_wiring_hit(&[]);
    }
}
