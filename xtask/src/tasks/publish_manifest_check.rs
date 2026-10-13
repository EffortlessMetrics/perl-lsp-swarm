//! Offline manifest validation for the publish pipeline.
//!
//! Three checks (the first two use `cargo metadata --no-deps`; no network
//! contact):
//!
//! 1. **Allowlist drift** — every entry in `[workspace.metadata.publish.allow]`
//!    must be a publishable workspace member (`publish` field is `None` or
//!    non-empty), AND every publishable workspace member must appear in the
//!    allowlist.  Replaces the `scripts/publish-topo.py --check-drift`
//!    invocation in `publish-dry-run.yml`.
//!
//! 2. **LICENSE present** — every allowlisted crate must carry a non-empty
//!    `license` or `license_file` field in the cargo metadata output.
//!    Workspace-inherited values (`license.workspace = true`) are already
//!    resolved to the actual string by cargo before this code sees them.
//!
//! 3. **API ratchet coverage** (#14607) — `.ci/public-api-baselines/ratchet-crates.txt`
//!    is the single list both API ratchets (`just public-api-check` and the
//!    semver rails) read. Every crate whose manifest opts in with an explicit
//!    `publish = true` must be listed; every listed crate must be allowlisted
//!    and have a non-empty committed baseline; no baseline may exist for an
//!    unlisted crate. Cargo metadata cannot distinguish an explicit
//!    `publish = true` from the default, so that fact is read from the
//!    manifest text.
//!
//! Prints all violations before exiting non-zero so the developer can fix
//! everything in one pass.

use crate::utils::{load_publish_allowlist, project_root, run_cargo_metadata};
use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::path::Path;

/// Repository-relative path of the shared API ratchet crate list.
pub(crate) const RATCHET_LIST_PATH: &str = ".ci/public-api-baselines/ratchet-crates.txt";
/// Repository-relative directory holding the committed public-API baselines.
pub(crate) const BASELINE_DIR: &str = ".ci/public-api-baselines";

// ---------------------------------------------------------------------------
// Cargo metadata types (no-deps variant)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct NoDepsMetadata {
    packages: Vec<NoDepsPackage>,
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct NoDepsPackage {
    name: String,
    id: String,
    /// `None` = publish everywhere; `Some([])` = `publish = false`.
    publish: Option<Vec<String>>,
    license: Option<String>,
    license_file: Option<String>,
    manifest_path: String,
}

/// Inputs to the API ratchet coverage check, gathered from the repository.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ApiRatchetInputs {
    /// Crates listed in `ratchet-crates.txt`, in file order (duplicates kept).
    pub(crate) listed: Vec<String>,
    /// Crates with a committed baseline file, mapped to whether it is non-empty.
    pub(crate) baselines: HashMap<String, bool>,
    /// Workspace crates whose manifest carries an explicit `publish = true`.
    pub(crate) explicit_publish: BTreeSet<String>,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Entry point for `cargo xtask publish-manifest-check`.
///
/// Runs allowlist-drift, LICENSE-present, and API-ratchet-coverage checks
/// against the current workspace state.  Exits non-zero if any violations
/// are found.
pub fn run() -> Result<()> {
    let allowlist = load_publish_allowlist()?;
    let bytes = run_cargo_metadata(true)?;
    let meta: NoDepsMetadata =
        serde_json::from_slice(&bytes).map_err(|e| eyre!("Failed to parse cargo metadata: {e}"))?;

    let mut violations = check_metadata(&meta, &allowlist);
    let ratchet = load_api_ratchet_inputs(&project_root()?, &meta)?;
    violations.extend(check_api_ratchet(&allowlist, &ratchet));

    if !violations.is_empty() {
        for v in &violations {
            eprintln!("ERROR: publish-manifest-check: {v}");
        }
        bail!("publish-manifest-check failed ({} violation(s))", violations.len());
    }

    println!(
        "publish-manifest-check: OK ({} crates checked, {} API-ratcheted, 0 violations)",
        allowlist.len(),
        ratchet.listed.len()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// API ratchet coverage inputs (filesystem reads, no cargo invocation)
// ---------------------------------------------------------------------------

/// Parse `ratchet-crates.txt` content: one crate per line; everything after a
/// `#` is a comment, surrounding whitespace is trimmed, and blank lines are
/// skipped. The justfile helper and the workflow loops apply the same rule.
pub(crate) fn parse_ratchet_list(content: &str) -> Vec<String> {
    content
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// True when a `Cargo.toml` text opts in with an explicit `publish = true`
/// in its `[package]` table. Parsed as TOML, so every spelling Cargo accepts
/// (`[ package ]`, quoted keys, dotted `package.publish`) is seen; a manifest
/// that does not parse is an error rather than a silent `false`.
pub(crate) fn manifest_declares_publish_true(manifest: &str) -> Result<bool> {
    let value: toml::Value = toml::from_str(manifest).map_err(|e| eyre!("invalid TOML: {e}"))?;
    Ok(value.get("package").and_then(|package| package.get("publish"))
        == Some(&toml::Value::Boolean(true)))
}

fn load_api_ratchet_inputs(root: &Path, meta: &NoDepsMetadata) -> Result<ApiRatchetInputs> {
    let list_path = root.join(RATCHET_LIST_PATH);
    let list_content = fs::read_to_string(&list_path)
        .wrap_err_with(|| format!("reading {}", list_path.display()))?;
    let listed = parse_ratchet_list(&list_content);

    let baseline_dir = root.join(BASELINE_DIR);
    let mut baselines = HashMap::new();
    for entry in fs::read_dir(&baseline_dir)
        .wrap_err_with(|| format!("reading {}", baseline_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension() != Some(OsStr::new("txt")) {
            continue;
        }
        // A baseline whose stem is not UTF-8 cannot name a crate; fail closed
        // rather than skipping it, so an orphan cannot hide behind its name.
        let crate_name = path
            .file_stem()
            .and_then(OsStr::to_str)
            .ok_or_else(|| eyre!("non-UTF-8 baseline file name: {}", path.display()))?;
        if crate_name == "ratchet-crates" {
            continue;
        }
        // Only a regular file is a baseline; a directory or special file under
        // a `.txt` name has a non-zero length but no API surface, so it must
        // not count as a non-empty baseline. "Non-empty" means at least one
        // line with content: a whitespace-only file records no API either.
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            bail!("{} is not a regular file; only baseline files belong here", path.display());
        }
        let content =
            fs::read_to_string(&path).wrap_err_with(|| format!("reading {}", path.display()))?;
        let non_empty = content.lines().any(|line| !line.trim().is_empty());
        baselines.insert(crate_name.to_string(), non_empty);
    }

    let ws_ids: HashSet<&str> = meta.workspace_members.iter().map(String::as_str).collect();
    let mut explicit_publish = BTreeSet::new();
    for pkg in meta.packages.iter().filter(|p| ws_ids.contains(p.id.as_str())) {
        let manifest = fs::read_to_string(&pkg.manifest_path)
            .wrap_err_with(|| format!("reading {}", pkg.manifest_path))?;
        if manifest_declares_publish_true(&manifest)
            .wrap_err_with(|| format!("parsing {}", pkg.manifest_path))?
        {
            explicit_publish.insert(pkg.name.clone());
        }
    }

    Ok(ApiRatchetInputs { listed, baselines, explicit_publish })
}

/// API ratchet coverage rule (#14607); returns violation messages (empty = pass).
pub(crate) fn check_api_ratchet(allowlist: &[String], inputs: &ApiRatchetInputs) -> Vec<String> {
    let allowlist_set: HashSet<&str> = allowlist.iter().map(String::as_str).collect();
    let mut violations = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();

    // Name the vacuous state directly rather than letting it surface only as
    // "explicit publish crate not listed" (or not at all if none opt in).
    if inputs.listed.is_empty() {
        violations.push(format!(
            "ratchet: {RATCHET_LIST_PATH} lists no crates; both API ratchets would be vacuous"
        ));
    }

    for name in &inputs.listed {
        if !seen.insert(name.as_str()) {
            violations.push(format!("ratchet: '{name}' is listed twice in {RATCHET_LIST_PATH}"));
            continue;
        }
        if !allowlist_set.contains(name.as_str()) {
            violations.push(format!(
                "ratchet: '{name}' is listed in {RATCHET_LIST_PATH} but is not in \
                 [workspace.metadata.publish.allow]"
            ));
        }
        match inputs.baselines.get(name.as_str()) {
            Some(true) => {}
            Some(false) => violations.push(format!(
                "ratchet: '{name}' is listed in {RATCHET_LIST_PATH} but \
                 {BASELINE_DIR}/{name}.txt is empty (run: just public-api-update)"
            )),
            None => violations.push(format!(
                "ratchet: '{name}' is listed in {RATCHET_LIST_PATH} but has no \
                 {BASELINE_DIR}/{name}.txt (run: just public-api-update)"
            )),
        }
    }

    let mut orphan_baselines: Vec<&str> =
        inputs.baselines.keys().map(String::as_str).filter(|name| !seen.contains(name)).collect();
    orphan_baselines.sort_unstable();
    for name in orphan_baselines {
        violations.push(format!(
            "ratchet: {BASELINE_DIR}/{name}.txt exists but '{name}' is not listed in \
             {RATCHET_LIST_PATH} (list it or delete the baseline)"
        ));
    }

    for name in &inputs.explicit_publish {
        if !seen.contains(name.as_str()) {
            violations.push(format!(
                "ratchet: '{name}' opts in with `publish = true` but is not listed in \
                 {RATCHET_LIST_PATH}; list it and run `just public-api-update`"
            ));
        }
    }

    violations
}

// ---------------------------------------------------------------------------
// Pure check logic (extracted for unit tests)
// ---------------------------------------------------------------------------

/// Run all manifest checks and return violation messages (empty = pass).
///
/// Extracted as a pure function so unit tests can drive it without spawning
/// a real `cargo metadata` process.
pub(crate) fn check_metadata(meta: &NoDepsMetadata, allowlist: &[String]) -> Vec<String> {
    let allowlist_set: HashSet<&str> = allowlist.iter().map(String::as_str).collect();
    let ws_ids: HashSet<&str> = meta.workspace_members.iter().map(String::as_str).collect();

    // Map name -> package for workspace members only.
    let pkg_map: HashMap<&str, &NoDepsPackage> = meta
        .packages
        .iter()
        .filter(|p| ws_ids.contains(p.id.as_str()))
        .map(|p| (p.name.as_str(), p))
        .collect();

    // Publishable = workspace member whose `publish` field is `None` or non-empty.
    let publishable: HashSet<&str> = meta
        .packages
        .iter()
        .filter(|p| ws_ids.contains(p.id.as_str()))
        .filter(|p| p.publish.as_ref().is_none_or(|v| !v.is_empty()))
        .map(|p| p.name.as_str())
        .collect();

    let mut violations: Vec<String> = Vec::new();

    // Drift A: allowlist entry is not a publishable workspace member.
    for name in allowlist {
        if !publishable.contains(name.as_str()) {
            violations.push(format!(
                "drift: '{name}' is in [workspace.metadata.publish.allow] but \
                 has publish=false in Cargo.toml (or is not a workspace member)"
            ));
        }
    }

    // Drift B: publishable workspace member absent from allowlist.
    for name in &publishable {
        if !allowlist_set.contains(*name) {
            violations.push(format!(
                "drift: '{name}' is publishable (no publish=false) but is \
                 absent from [workspace.metadata.publish.allow]"
            ));
        }
    }

    // LICENSE check for every allowlisted crate.
    for name in allowlist {
        let Some(pkg) = pkg_map.get(name.as_str()) else {
            // Name is not a workspace member at all — already flagged by drift
            // check A as "not a workspace member".  A publish=false workspace
            // member IS still in pkg_map (it is a workspace member; it just
            // cannot be published), so it does NOT hit this branch.
            continue;
        };
        let has_license = pkg.license.as_deref().is_some_and(|l| !l.is_empty())
            || pkg.license_file.as_deref().is_some_and(|f| !f.is_empty());
        if !has_license {
            violations.push(format!(
                "license: '{name}' has no `license` or `license-file` — \
                 crates.io will reject it at upload"
            ));
        }
    }

    violations
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pkg(name: &str, publish_false: bool, license: Option<&str>) -> NoDepsPackage {
        NoDepsPackage {
            name: name.to_string(),
            id: format!("{name} 0.1.0 (path+file:///fake)"),
            publish: if publish_false { Some(vec![]) } else { None },
            license: license.map(str::to_string),
            license_file: None,
            manifest_path: format!("/fake/{name}/Cargo.toml"),
        }
    }

    fn strings(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn ratchet_inputs(
        listed: &[&str],
        baselines: &[(&str, bool)],
        explicit_publish: &[&str],
    ) -> ApiRatchetInputs {
        ApiRatchetInputs {
            listed: strings(listed),
            baselines: baselines.iter().map(|(n, ok)| (n.to_string(), *ok)).collect(),
            explicit_publish: explicit_publish.iter().map(|n| n.to_string()).collect(),
        }
    }

    #[test]
    fn ratchet_list_parser_skips_comments_blank_lines_and_whitespace() {
        let content = "# header\n\n  perl-uri  # facade\nperl-dap\n\n#perl-ghost\n";
        assert_eq!(parse_ratchet_list(content), strings(&["perl-uri", "perl-dap"]));
    }

    fn publish_true(manifest: &str) -> bool {
        match manifest_declares_publish_true(manifest) {
            Ok(value) => value,
            Err(err) => panic!("manifest should parse: {err}"),
        }
    }

    #[test]
    fn explicit_publish_true_is_detected_in_every_toml_spelling() {
        assert!(publish_true(
            "[package]\nname = \"x\"\npublish = true # opt in\n\n[dependencies]\n"
        ));
        assert!(publish_true("[ package ]\nname = \"x\"\npublish=true\n"));
        assert!(publish_true("[package]\nname = \"x\"\n\"publish\" = true\n"));
        assert!(publish_true("package.name = \"x\"\npackage.publish = true\n"));
    }

    #[test]
    fn explicit_publish_true_is_not_inferred_from_other_shapes() {
        assert!(!publish_true("[package]\nname = \"x\"\npublish = false\n"));
        assert!(!publish_true("[package]\nname = \"x\"\n"));
        // A registry list is not the boolean opt-in.
        assert!(!publish_true("[package]\nname = \"x\"\npublish = [\"crates-io\"]\n"));
        // A `publish = true` outside `[package]` (e.g. metadata) is not an opt-in.
        assert!(!publish_true(
            "[package]\nname = \"x\"\n\n[package.metadata.docs]\npublish = true\n"
        ));
        // Commented-out opt-ins do not count.
        assert!(!publish_true("[package]\nname = \"x\"\n# publish = true\n"));
    }

    #[test]
    fn unparseable_manifest_is_an_error_not_a_silent_false() {
        assert!(
            manifest_declares_publish_true("[package\nname = \"x\"\npublish = true\n").is_err()
        );
    }

    #[test]
    fn ratchet_coverage_clean_when_lists_agree() {
        let allowlist = strings(&["perl-uri", "perl-module", "perl-token"]);
        let inputs = ratchet_inputs(
            &["perl-uri", "perl-module"],
            &[("perl-uri", true), ("perl-module", true)],
            &["perl-module"],
        );
        assert!(check_api_ratchet(&allowlist, &inputs).is_empty());
    }

    #[test]
    fn ratchet_coverage_names_an_empty_list_even_when_nothing_opts_in() {
        let allowlist = strings(&["perl-uri"]);
        let inputs = ratchet_inputs(&[], &[], &[]);
        let v = check_api_ratchet(&allowlist, &inputs);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].contains("lists no crates"), "{v:?}");
    }

    #[test]
    fn baseline_walk_rejects_a_directory_under_a_txt_name() {
        let root = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(err) => panic!("tempdir: {err}"),
        };
        let baseline_dir = root.path().join(BASELINE_DIR);
        if let Err(err) = fs::create_dir_all(baseline_dir.join("perl-uri.txt")) {
            panic!("create fixture: {err}");
        }
        if let Err(err) = fs::write(baseline_dir.join("ratchet-crates.txt"), "perl-uri\n") {
            panic!("write list: {err}");
        }
        let meta = make_meta(vec![]);
        let err = match load_api_ratchet_inputs(root.path(), &meta) {
            Ok(inputs) => panic!("directory accepted as a baseline: {inputs:?}"),
            Err(err) => err.to_string(),
        };
        assert!(err.contains("perl-uri.txt is not a regular file"), "{err}");
    }

    #[test]
    fn baseline_walk_reads_regular_files_and_their_emptiness() {
        let root = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(err) => panic!("tempdir: {err}"),
        };
        let baseline_dir = root.path().join(BASELINE_DIR);
        if let Err(err) = fs::create_dir_all(&baseline_dir) {
            panic!("create fixture: {err}");
        }
        for (name, content) in [
            ("ratchet-crates.txt", "perl-uri # facade\n"),
            ("perl-uri.txt", "pub fn f()\n"),
            ("perl-empty.txt", ""),
            ("perl-blank.txt", "\n  \n\t\n"),
            ("notes.md", "not a baseline\n"),
        ] {
            if let Err(err) = fs::write(baseline_dir.join(name), content) {
                panic!("write {name}: {err}");
            }
        }
        let meta = make_meta(vec![]);
        let inputs = match load_api_ratchet_inputs(root.path(), &meta) {
            Ok(inputs) => inputs,
            Err(err) => panic!("load: {err}"),
        };
        assert_eq!(
            inputs,
            ratchet_inputs(
                &["perl-uri"],
                &[("perl-uri", true), ("perl-empty", false), ("perl-blank", false)],
                &[]
            )
        );
    }

    #[test]
    fn ratchet_coverage_requires_explicit_publish_crates_to_be_listed() {
        let allowlist = strings(&["perl-uri", "perl-module"]);
        let inputs = ratchet_inputs(&["perl-uri"], &[("perl-uri", true)], &["perl-module"]);
        let v = check_api_ratchet(&allowlist, &inputs);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].contains("'perl-module' opts in with `publish = true`"), "{v:?}");
    }

    #[test]
    fn ratchet_coverage_rejects_listed_crate_without_baseline_or_allowlist_entry() {
        let allowlist = strings(&["perl-uri"]);
        let inputs = ratchet_inputs(&["perl-uri", "perl-ghost"], &[("perl-uri", true)], &[]);
        let v = check_api_ratchet(&allowlist, &inputs);
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(
            v.iter().any(|s| s.contains("'perl-ghost' is listed") && s.contains("publish.allow"))
        );
        assert!(v.iter().any(|s| s.contains("has no .ci/public-api-baselines/perl-ghost.txt")));
    }

    #[test]
    fn ratchet_coverage_rejects_empty_baseline_orphan_baseline_and_duplicate_listing() {
        let allowlist = strings(&["perl-uri", "perl-dap", "perl-lexer"]);
        let inputs = ratchet_inputs(
            &["perl-uri", "perl-dap", "perl-dap"],
            &[("perl-uri", true), ("perl-dap", false), ("perl-lexer", true)],
            &[],
        );
        let v = check_api_ratchet(&allowlist, &inputs);
        assert_eq!(v.len(), 3, "{v:?}");
        assert!(v.iter().any(|s| s.contains("perl-dap.txt is empty")), "{v:?}");
        assert!(v.iter().any(|s| s.contains("'perl-dap' is listed twice")), "{v:?}");
        assert!(
            v.iter().any(|s| s.contains("perl-lexer.txt exists but 'perl-lexer' is not listed")),
            "{v:?}"
        );
    }

    fn make_meta(pkgs: Vec<NoDepsPackage>) -> NoDepsMetadata {
        let workspace_members = pkgs.iter().map(|p| p.id.clone()).collect();
        NoDepsMetadata { packages: pkgs, workspace_members }
    }

    #[test]
    fn clean_metadata_no_violations() {
        let meta = make_meta(vec![make_pkg("perl-token", false, Some("MIT OR Apache-2.0"))]);
        let allowlist = vec!["perl-token".to_string()];
        assert!(check_metadata(&meta, &allowlist).is_empty());
    }

    #[test]
    fn drift_a_allowlist_has_publish_false_crate() {
        let meta = make_meta(vec![
            make_pkg("perl-token", true, Some("MIT OR Apache-2.0")), // publish = false
        ]);
        let allowlist = vec!["perl-token".to_string()];
        let v = check_metadata(&meta, &allowlist);
        assert!(v.iter().any(|s| s.contains("drift:")), "expected drift violation, got: {v:?}");
    }

    #[test]
    fn drift_b_publishable_crate_absent_from_allowlist() {
        let meta = make_meta(vec![make_pkg("perl-token", false, Some("MIT OR Apache-2.0"))]);
        let allowlist: Vec<String> = vec![]; // perl-token missing from allowlist
        let v = check_metadata(&meta, &allowlist);
        assert!(v.iter().any(|s| s.contains("drift:")), "expected drift violation, got: {v:?}");
    }

    #[test]
    fn missing_license_detected() {
        let meta = make_meta(vec![make_pkg("perl-token", false, None)]);
        let allowlist = vec!["perl-token".to_string()];
        let v = check_metadata(&meta, &allowlist);
        assert!(v.iter().any(|s| s.contains("license:")), "expected license violation, got: {v:?}");
    }

    /// A `publish=false` workspace crate in the allowlist generates a drift
    /// violation but NOT a license violation — because the crate IS still in
    /// pkg_map (workspace membership and publishability are orthogonal).
    /// This test documents that a drift-A crate with a valid license only
    /// produces exactly one violation (the drift, not the license).
    #[test]
    fn drift_a_crate_with_license_produces_only_drift_violation() {
        let meta = make_meta(vec![
            make_pkg("perl-token", true, Some("MIT OR Apache-2.0")), // publish=false, has license
        ]);
        let allowlist = vec!["perl-token".to_string()];
        let v = check_metadata(&meta, &allowlist);
        assert_eq!(v.len(), 1, "expected exactly 1 violation (drift), got: {v:?}");
        assert!(v[0].contains("drift:"), "expected drift violation, got: {v:?}");
    }

    /// A stale allowlist entry (name not present as a workspace member at all)
    /// generates a drift-A violation and the license check skips it via the
    /// `continue` branch — no spurious license violation for a non-existent crate.
    #[test]
    fn stale_allowlist_entry_not_in_workspace_skips_license_check() {
        // Workspace is empty; allowlist has a ghost crate
        let meta = make_meta(vec![]);
        let allowlist = vec!["deleted-crate".to_string()];
        let v = check_metadata(&meta, &allowlist);
        // Should get exactly one drift violation, not a license violation
        assert_eq!(v.len(), 1, "expected exactly 1 (drift) violation, got: {v:?}");
        assert!(v[0].contains("drift:"), "expected drift violation, got: {v:?}");
        assert!(!v[0].contains("license:"), "should not have license violation for ghost crate");
    }

    /// `license_file` alone (without a `license` field) satisfies the license check.
    #[test]
    fn license_file_alone_satisfies_license_check() {
        let mut pkg = make_pkg("perl-token", false, None); // no license field
        pkg.license_file = Some("LICENSE-MIT".to_string());
        let meta = make_meta(vec![pkg]);
        let allowlist = vec!["perl-token".to_string()];
        let v = check_metadata(&meta, &allowlist);
        assert!(v.is_empty(), "license_file alone should satisfy license check, got: {v:?}");
    }
}
