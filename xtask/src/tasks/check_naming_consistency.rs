//! Crate directory ↔ package name linter (issue #2933 AC#3).
//!
//! Enforces the invariant that every `crates/<dir>/` directory in the workspace
//! has a Cargo package name that exactly equals `<dir>`.  A mismatch causes the
//! kind of silent basename-inference bug that motivated issue #4512.
//!
//! ## What is checked
//!
//! For every direct subdirectory of `<root>/crates/` that contains a `Cargo.toml`:
//!
//! * Read `[package] name = "…"` from the manifest.
//! * Compare it against the directory basename.
//! * Report a finding if they differ.
//! * Directories without a `Cargo.toml` are skipped with a notice (e.g.
//!   `crates/tree-sitter-perl` is a JavaScript project, not a Rust crate).
//!
//! ## Exit behaviour
//!
//! * Exit 0 — all checked directories pass.
//! * Exit 1 (via `bail!`) — at least one mismatch found; mismatches printed to
//!   stdout so callers can capture them.
//!
//! ## Usage
//!
//! ```text
//! cargo xtask check-naming-consistency [--root <path>]
//! ```
//!
//! `--root` defaults to the workspace root enclosing the current working
//! directory, so the command works from a member directory such as
//! `crates/perl-parser`. Tests still point it at a synthetic workspace by
//! setting `current_dir` on the subprocess. Override `--root` explicitly to
//! check a workspace elsewhere on disk.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result, bail};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Configuration for the naming-consistency check.
pub struct NamingConsistencyConfig {
    /// Workspace root; `crates/` is expected at `root/crates/`.
    pub root: PathBuf,
}

/// A single naming mismatch finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// Relative path to the crate directory, e.g. `crates/perl-lsp-rs`.
    pub crate_dir: String,
    /// The directory basename, e.g. `perl-lsp-rs`.
    pub dir_name: String,
    /// The `[package] name` value read from `Cargo.toml`, e.g. `perl-lsp`.
    pub package_name: String,
}

/// Run the naming-consistency check.
///
/// Prints a summary and returns `Ok(())` if all checked crate directories
/// have package names that match their basename.  Returns an error if any
/// mismatches are found, so the xtask process exits with a non-zero status.
pub fn run(config: NamingConsistencyConfig) -> Result<()> {
    let root = &config.root;
    let crates_dir = root.join("crates");

    if !crates_dir.exists() {
        bail!("Expected `crates/` directory at {}", crates_dir.display());
    }

    let CheckResult { mismatches, skipped, checked } = collect_mismatches(&crates_dir)
        .with_context(|| format!("Failed to scan crates directory: {}", crates_dir.display()))?;

    println!("Crate directory ↔ package-name consistency check");
    println!("=================================================");
    println!("  Root: {}", root.display());
    println!("  Directories checked: {checked}");
    println!("  Directories skipped (no Cargo.toml): {}", skipped.len());

    if !skipped.is_empty() {
        println!();
        println!("Skipped (no Cargo.toml — not a Rust crate):");
        for dir in &skipped {
            println!("  • {dir}");
        }
    }

    if mismatches.is_empty() {
        println!();
        println!("✅ All {checked} crate directories have matching package names.");
        return Ok(());
    }

    println!();
    println!("❌ {} mismatch(es) found:", mismatches.len());
    println!();
    for m in &mismatches {
        println!("  {}", m.crate_dir);
        println!("    directory basename : {}", m.dir_name);
        println!("    Cargo.toml name    : {}", m.package_name);
        println!();
    }

    println!("Each `crates/<dir>/` directory must have a Cargo package name equal to `<dir>`.");
    println!("Rename the directory or update `[package] name` in the crate's Cargo.toml to fix.");

    bail!("{} crate directory/package-name mismatch(es) found", mismatches.len());
}

/// Convenience entry point.
///
/// When `root` is `Some`, use it directly. When `None`, search upward from the
/// process working directory for the enclosing workspace root, falling back to
/// the working directory itself when no `[workspace]` manifest is found.
///
/// Searching upward is what makes `cargo xtask check-naming-consistency` work
/// from a member directory such as `crates/perl-parser`: Cargo resolves the
/// alias from `.cargo/config.toml` wherever it is invoked, so the bare working
/// directory is not reliably the workspace root.
///
/// Note: `project_root()` is compile-time-pinned to the real workspace root via
/// `CARGO_MANIFEST_DIR`; using it here would bypass `current_dir`, which tests
/// set to point the command at a synthetic fixture workspace. Upward search
/// keeps that seam — a fixture root carrying `[workspace]` is found first.
pub fn run_default(root: Option<PathBuf>) -> Result<()> {
    let root = match root {
        Some(r) => r,
        None => {
            let cwd = std::env::current_dir().map_err(|e| {
                color_eyre::eyre::eyre!("Failed to get current working directory: {e}")
            })?;
            enclosing_workspace_root(&cwd).unwrap_or(cwd)
        }
    };
    run(NamingConsistencyConfig { root })
}

/// Find the nearest ancestor of `start` whose `Cargo.toml` declares
/// `[workspace]`.
///
/// Unreadable or unparsable manifests are skipped rather than failing the
/// search: this is a best-effort locator, and [`run`] reports the real error if
/// the resolved root turns out to have no `crates/` directory.
fn enclosing_workspace_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| {
            fs::read_to_string(dir.join("Cargo.toml"))
                .ok()
                .and_then(|content| toml::from_str::<toml::Value>(&content).ok())
                .is_some_and(|manifest| manifest.get("workspace").is_some())
        })
        .map(Path::to_path_buf)
}

// ---------------------------------------------------------------------------
// Core logic (pub(crate) for unit testing)
// ---------------------------------------------------------------------------

pub(crate) struct CheckResult {
    pub mismatches: Vec<Mismatch>,
    /// Directories that had no `Cargo.toml` and were skipped.
    pub skipped: Vec<String>,
    /// Number of directories that had a `Cargo.toml` and were checked.
    pub checked: usize,
}

/// Scan `crates_dir` and return all findings.
pub(crate) fn collect_mismatches(crates_dir: &Path) -> Result<CheckResult> {
    // Collect entries in a deterministic (sorted) order so output is stable.
    let mut entries: BTreeMap<String, PathBuf> = BTreeMap::new();
    // Directory names that are not valid UTF-8. A Cargo package name is always
    // valid UTF-8, so such a directory can never satisfy the invariant — and
    // silently dropping it would let the check report "all directories match"
    // while one direct child of `crates/` was never examined. Fail closed.
    let mut unrepresentable: Vec<PathBuf> = Vec::new();

    let read_dir = fs::read_dir(crates_dir)
        .with_context(|| format!("Failed to read directory: {}", crates_dir.display()))?;

    for entry in read_dir {
        let entry = entry.with_context(|| {
            format!("Failed to read directory entry in {}", crates_dir.display())
        })?;
        let path = entry.path();
        if path.is_dir() {
            match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => {
                    entries.insert(name.to_owned(), path);
                }
                None => unrepresentable.push(path),
            }
        }
    }

    if !unrepresentable.is_empty() {
        let listed = unrepresentable
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "Crate directory name is not valid UTF-8, so it cannot equal any Cargo package \
             name: {listed}. Rename the directory to a UTF-8 name matching its \
             `[package] name`."
        );
    }

    let mut mismatches = Vec::new();
    let mut skipped = Vec::new();
    let mut checked: usize = 0;

    for (dir_name, path) in &entries {
        let manifest = path.join("Cargo.toml");
        if !manifest.exists() {
            skipped.push(format!("crates/{dir_name}"));
            continue;
        }

        let package_name = read_package_name(&manifest)
            .with_context(|| format!("Failed to read package name from {}", manifest.display()))?;

        checked += 1;

        if &package_name != dir_name {
            mismatches.push(Mismatch {
                crate_dir: format!("crates/{dir_name}"),
                dir_name: dir_name.clone(),
                package_name,
            });
        }
    }

    Ok(CheckResult { mismatches, skipped, checked })
}

/// Read the `[package] name` value from a `Cargo.toml` file.
///
/// Returns Cargo's *decoded* package name; does not follow workspace
/// inheritance (workspace packages must have an explicit `name` field, so this
/// is always a literal value in practice).
///
/// A manifest that is not valid TOML is reported as a parse failure rather than
/// as a missing `name`, so the two causes stay distinguishable to the caller.
pub(crate) fn read_package_name(manifest_path: &Path) -> Result<String> {
    let content = fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;

    let manifest: toml::Value = toml::from_str(&content)
        .with_context(|| format!("Failed to parse TOML in {}", manifest_path.display()))?;

    package_name_of(&manifest).map(str::to_string).ok_or_else(|| {
        color_eyre::eyre::eyre!("No `[package] name` field found in {}", manifest_path.display())
    })
}

/// Look up `package.name` in an already-parsed manifest.
///
/// Single authority for the lookup path, shared by [`read_package_name`] and
/// [`parse_package_name_from_toml`]. Resolving through the parsed document is
/// what makes `[package.metadata.*]` subtables non-shadowing: a `name` key
/// there is `package.metadata.….name`, never `package.name`.
fn package_name_of(manifest: &toml::Value) -> Option<&str> {
    manifest.get("package")?.get("name")?.as_str()
}

/// Extract the `[package] name` value from raw TOML text.
///
/// Parses with the `toml` crate — already an xtask dependency, and already how
/// xtask reads Cargo manifests elsewhere — so the value returned is Cargo's own
/// decoded package name. That matters for manifests a hand-rolled line scanner
/// gets wrong: literal strings (`name = 'foo'`), dotted keys
/// (`package.name = "foo"`), and escapes inside basic strings, all of which are
/// valid Cargo and must not be reported as a missing or differently-spelled
/// name.
///
/// Returns `None` for both "not valid TOML" and "no `package.name`";
/// [`read_package_name`] distinguishes the two for its error message.
///
/// Test-only: production reads manifests from disk through
/// [`read_package_name`], which shares the same [`package_name_of`] lookup.
#[cfg(test)]
pub(crate) fn parse_package_name_from_toml(content: &str) -> Option<String> {
    let manifest: toml::Value = toml::from_str(content).ok()?;
    package_name_of(&manifest).map(str::to_string)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_package_name_from_toml ---

    #[test]
    fn parses_simple_package_name() {
        let toml = r#"
[package]
name = "perl-lsp-rs"
version = "0.1.0"
"#;
        assert_eq!(parse_package_name_from_toml(toml).as_deref(), Some("perl-lsp-rs"));
    }

    #[test]
    fn parses_name_with_surrounding_whitespace_in_value() {
        let toml = r#"
[package]
name = "my-crate"
"#;
        assert_eq!(parse_package_name_from_toml(toml).as_deref(), Some("my-crate"));
    }

    #[test]
    fn parses_package_name_with_inline_comments() {
        let toml = r#"
[package] # package metadata
name = "commented-crate" # keep this package name
"#;
        assert_eq!(parse_package_name_from_toml(toml).as_deref(), Some("commented-crate"));
    }

    #[test]
    fn ignores_name_outside_package_section() {
        let toml = r#"
[dependencies]
name = "should-not-match"

[package]
name = "real-crate"
"#;
        assert_eq!(parse_package_name_from_toml(toml).as_deref(), Some("real-crate"));
    }

    #[test]
    fn returns_none_for_missing_name() {
        let toml = r#"
[package]
version = "0.1.0"
"#;
        assert!(parse_package_name_from_toml(toml).is_none());
    }

    #[test]
    fn returns_none_for_empty_content() {
        assert!(parse_package_name_from_toml("").is_none());
    }

    #[test]
    fn parses_literal_string_package_name() {
        // Valid Cargo: TOML literal strings are as legal as basic strings.
        let toml = r#"
[package]
name = 'literal-crate'
"#;
        assert_eq!(parse_package_name_from_toml(toml).as_deref(), Some("literal-crate"));
    }

    #[test]
    fn parses_dotted_key_package_name() {
        let toml = r#"
package.name = "dotted-crate"
package.version = "0.1.0"
"#;
        assert_eq!(parse_package_name_from_toml(toml).as_deref(), Some("dotted-crate"));
    }

    #[test]
    fn returns_cargos_decoded_name_for_escaped_basic_string() {
        // `\u002D` is an escaped `-`. Cargo sees the decoded name, so the
        // decoded form is what a directory basename must be compared against;
        // returning the raw encoded text would report a bogus mismatch.
        let toml = r#"
[package]
name = "escaped\u002Dcrate"
"#;
        assert_eq!(parse_package_name_from_toml(toml).as_deref(), Some("escaped-crate"));
    }

    #[test]
    fn package_metadata_subtable_name_does_not_shadow_package_name() {
        let toml = r#"
[package]
name = "real-crate"

[package.metadata.deb]
name = "not-the-package-name"
"#;
        assert_eq!(parse_package_name_from_toml(toml).as_deref(), Some("real-crate"));
    }

    #[test]
    fn returns_none_for_malformed_toml() {
        assert!(parse_package_name_from_toml("[package\nname = \"x\"").is_none());
    }

    #[test]
    fn parses_package_name_after_other_sections() {
        let toml = r#"
[workspace]
members = ["crates/foo"]

[package]
name = "foo"
version = "0.1.0"
"#;
        assert_eq!(parse_package_name_from_toml(toml).as_deref(), Some("foo"));
    }

    #[test]
    fn stops_at_next_section_after_package() {
        // `name` appears only in [package]; should find "actual-name".
        let toml = r#"
[package]
name = "actual-name"

[dependencies]
name = "not-a-package-name"
"#;
        assert_eq!(parse_package_name_from_toml(toml).as_deref(), Some("actual-name"));
    }

    // --- collect_mismatches ---

    #[test]
    fn detects_mismatch_in_fixture_workspace() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let crates_dir = tmp.path().join("crates");
        let crate_dir = crates_dir.join("my-dir");
        std::fs::create_dir_all(crate_dir.join("src"))?;
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"my-package\"\nversion = \"0.1.0\"\n",
        )?;

        let result = collect_mismatches(&crates_dir)?;
        assert_eq!(result.checked, 1);
        assert_eq!(result.mismatches.len(), 1);
        let m = &result.mismatches[0];
        assert_eq!(m.crate_dir, "crates/my-dir");
        assert_eq!(m.dir_name, "my-dir");
        assert_eq!(m.package_name, "my-package");
        Ok(())
    }

    #[test]
    fn no_findings_when_names_match() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let crates_dir = tmp.path().join("crates");
        let crate_dir = crates_dir.join("perl-parser");
        std::fs::create_dir_all(crate_dir.join("src"))?;
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"perl-parser\"\nversion = \"0.1.0\"\n",
        )?;

        let result = collect_mismatches(&crates_dir)?;
        assert_eq!(result.checked, 1);
        assert!(result.mismatches.is_empty());
        assert!(result.skipped.is_empty());
        Ok(())
    }

    #[test]
    fn skips_directories_without_cargo_toml() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let crates_dir = tmp.path().join("crates");
        // A dir without Cargo.toml (e.g. crates/tree-sitter-perl).
        std::fs::create_dir_all(crates_dir.join("tree-sitter-perl"))?;

        let result = collect_mismatches(&crates_dir)?;
        assert_eq!(result.checked, 0);
        assert!(result.mismatches.is_empty());
        assert_eq!(result.skipped, vec!["crates/tree-sitter-perl"]);
        Ok(())
    }

    #[test]
    fn handles_multiple_crates_mixed() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let crates_dir = tmp.path().join("crates");

        // Matching crate.
        let ok_dir = crates_dir.join("perl-lexer");
        std::fs::create_dir_all(ok_dir.join("src"))?;
        std::fs::write(
            ok_dir.join("Cargo.toml"),
            "[package]\nname = \"perl-lexer\"\nversion = \"0.1.0\"\n",
        )?;

        // Mismatched crate.
        let bad_dir = crates_dir.join("perl-lsp");
        std::fs::create_dir_all(bad_dir.join("src"))?;
        std::fs::write(
            bad_dir.join("Cargo.toml"),
            "[package]\nname = \"perl-lsp-rs\"\nversion = \"0.1.0\"\n",
        )?;

        // Non-Rust directory.
        std::fs::create_dir_all(crates_dir.join("tree-sitter-perl"))?;

        let result = collect_mismatches(&crates_dir)?;
        assert_eq!(result.checked, 2);
        assert_eq!(result.mismatches.len(), 1);
        let m = &result.mismatches[0];
        assert_eq!(m.dir_name, "perl-lsp");
        assert_eq!(m.package_name, "perl-lsp-rs");
        assert_eq!(result.skipped.len(), 1);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_crate_directory_fails_closed_instead_of_being_dropped() -> Result<()> {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let tmp = tempfile::TempDir::new()?;
        let crates_dir = tmp.path().join("crates");

        // A well-formed crate, so the scan has something it could legitimately
        // report as "1 checked, 0 mismatches" if the bad entry were dropped.
        let ok_dir = crates_dir.join("perl-lexer");
        std::fs::create_dir_all(ok_dir.join("src"))?;
        std::fs::write(
            ok_dir.join("Cargo.toml"),
            "[package]\nname = \"perl-lexer\"\nversion = \"0.1.0\"\n",
        )?;

        // 0xFF can never appear in valid UTF-8.
        std::fs::create_dir(crates_dir.join(OsStr::from_bytes(b"bad\xFFname")))?;

        let error = collect_mismatches(&crates_dir).err().ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "expected a non-UTF-8 crate directory to fail the check, not be skipped silently"
            )
        })?;
        let message = format!("{error}");
        assert!(
            message.contains("not valid UTF-8"),
            "error should name the unrepresentable directory, got: {message}"
        );
        Ok(())
    }

    #[test]
    fn current_workspace_is_fully_consistent() -> Result<()> {
        // Verify the real workspace passes. This is the end-to-end guard that
        // prevents regressions when crate directories or package names are renamed.
        let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        root.pop(); // xtask/ -> workspace root
        let crates_dir = root.join("crates");
        let result = collect_mismatches(&crates_dir)?;
        assert!(
            result.mismatches.is_empty(),
            "Workspace has crate directory/package-name mismatches:\n{:#?}",
            result.mismatches
        );
        Ok(())
    }
}
