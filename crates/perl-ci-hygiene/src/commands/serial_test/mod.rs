//! Parallel-unsafe test serialization policy (`#[serial]`).
//!
//! Issue #1269 action item 3: a blocking policy that keeps a new parallel-unsafe
//! test function from entering the workspace without a serialization guard.
//!
//! A test function is *parallel-unsafe* when its own body mutates process-global
//! state:
//!
//! - process environment: `env::set_var` / `env::remove_var` (including the
//!   `std::env::` path and bare calls via `use std::env;`);
//! - process working directory: `env::set_current_dir`.
//!
//! Such a function must carry a serialization guard from the `serial_test`
//! crate (`#[serial]`, `#[serial(..)]`, `#[serial_test::serial(..)]`,
//! `#[serial_test::file_serial(..)]`), or it must be listed in the accepted
//! identity registry (`ci/serial_test_identities.json`) with a reason.
//!
//! The registry follows the `panic_test` identity-registry convention:
//! `schema_version` 1, `active` rows must stay present in the inventory,
//! `retired` rows must stay absent, and unknown identities fail. Repairing a
//! registered site (annotating it) turns the gate red until the row is retired,
//! so the accepted set only shrinks.
//!
//! Deliberately out of scope (documented for #1269): TCP port binds (current
//! main binds are ephemeral port 0 by construction) and `static` counter
//! mutation (line-local detection cannot separate a shared global from a
//! test-local object). The brace-matching body scan is a source-text heuristic
//! biased toward under-detection; string literals and block comments are not
//! parsed. Attribute-on-same-line-as-fn forms are not scanned because the
//! enforced `cargo fmt` gate normalizes attributes onto their own lines.

use color_eyre::eyre::{Result, eyre};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use perl_ci_hygiene::walk_rs_files;

use crate::{NC, RED, YELLOW, first_cfg_test_line_number, read_lines};

static SIGNAL_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"\b(set_var|remove_var|set_current_dir)\s*\("));
static TEST_ATTR_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^#\[\s*(test|tokio::test|rstest)\b"));
static SERIAL_ATTR_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^#\[\s*(serial|serial_test::(serial|file_serial))\b"));
static FN_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)"));
static COMMENT_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| Regex::new(r"^\s*//"));
static EXTERNAL_TEST_MOD_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;"));

/// Test-support crates excluded from the scan, mirroring `panic_test`.
const CI_REPORT_CRATES_EXCLUDE: [&str; 5] = [
    "tree-sitter-perl-c",
    "perl-parser-pest",
    "perl-tdd-support",
    "perl-test-must",
    "perl-ci-hygiene",
];

const DEFAULT_REGISTRY: &str = "ci/serial_test_identities.json";

fn regex_from_static(
    regex: &'static LazyLock<Result<Regex, regex::Error>>,
    label: &str,
) -> Result<&'static Regex> {
    regex.as_ref().map_err(|err| eyre!("{label} regex failed to compile: {err}"))
}

fn is_integration_test_file(path: &Path) -> bool {
    path.components().any(|component| component.as_os_str() == std::ffi::OsStr::new("tests"))
}

fn is_complete_test_source_file(path: &Path) -> bool {
    is_integration_test_file(path)
        || path.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
            ["_test.rs", "_tests.rs", "tests.rs"].iter().any(|suffix| name.ends_with(suffix))
        })
}

fn is_excluded_test_path(path: &Path) -> bool {
    if path.components().any(|component| {
        let value = component.as_os_str();
        value == "benches" || value == "examples" || value == "bin" || value == "target"
    }) {
        return true;
    }
    path.components().any(|component| {
        CI_REPORT_CRATES_EXCLUDE
            .iter()
            .any(|item| component.as_os_str() == std::ffi::OsStr::new(item))
    })
}

fn walk_workspace_rust_files(repo_root: &Path) -> Vec<PathBuf> {
    let mut files = BTreeSet::new();
    for root in [repo_root.join("crates"), repo_root.join("xtask")] {
        files.extend(walk_rs_files(&root));
    }
    files.into_iter().collect()
}

fn external_test_module_files(path: &Path, lines: &[String]) -> Result<Vec<PathBuf>> {
    let Some(start_line) = first_cfg_test_line_number(path).ok().filter(|line| *line != usize::MAX)
    else {
        return Ok(Vec::new());
    };

    let mod_re = regex_from_static(&EXTERNAL_TEST_MOD_RE, "external test mod")?;
    let mut files = Vec::new();
    for line in lines.iter().skip(start_line.saturating_sub(1)) {
        let Some(name) =
            mod_re.captures(line).and_then(|caps| caps.get(1)).map(|name| name.as_str().to_owned())
        else {
            continue;
        };
        let sibling = path.with_file_name(format!("{name}.rs"));
        let nested = path.with_file_name(name).join("mod.rs");
        if sibling.is_file() {
            files.push(sibling);
        }
        if nested.is_file() {
            files.push(nested);
        }
    }
    Ok(files)
}

/// Signal category for a matched global-state call.
fn signal_category(matched: &str) -> &'static str {
    match matched {
        "set_var" => "env_set",
        "remove_var" => "env_remove",
        _ => "cwd",
    }
}

/// A test function that mutates process-global state without a serialization
/// guard.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SerialSiteIdentity {
    path: String,
    test_function: String,
    signals: Vec<&'static str>,
    line: usize,
}

impl SerialSiteIdentity {
    fn key(&self) -> (String, String) {
        (self.path.clone(), self.test_function.clone())
    }
}

/// True when the text immediately before a bare signal-call match is a method
/// call receiver dot. `env::set_var(` keeps `:` before the match (the
/// path-qualified form) and stays included.
fn is_method_call(prefix: &str) -> bool {
    prefix.ends_with('.')
}

/// Collect attribute lines attached to the function starting at `line_index`.
///
/// Walks upward over contiguous attribute, blank, and comment lines, stopping
/// at the first other code line (typically the previous item's closing brace).
fn attached_attributes(lines: &[String], line_index: usize) -> Vec<String> {
    let mut attrs = Vec::new();
    for line in lines[..line_index].iter().rev() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[") {
            attrs.push(trimmed.to_owned());
        } else if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        } else {
            break;
        }
    }
    attrs
}

/// Detects a serialization guard anywhere in the attached attribute block or
/// on the function line itself.
fn has_serial_guard(attrs: &[String], fn_line: &str) -> Result<bool> {
    let serial_re = regex_from_static(&SERIAL_ATTR_RE, "serial attribute")?;
    Ok(attrs.iter().any(|attr| serial_re.is_match(attr))
        || serial_re.is_match(fn_line.trim_start()))
}

/// Brace-matched body extent `[start, end]` for the function starting at
/// `line_index`. Source-text heuristic: string literals containing unbalanced
/// braces can skew the extent; the bias is under-detection, which the
/// registry's NEW-identity check backstops for newly added sites.
fn body_extent(lines: &[String], line_index: usize) -> usize {
    let mut depth = 0usize;
    let mut started = false;
    let mut end = line_index;
    for (offset, line) in lines.iter().enumerate().skip(line_index) {
        for ch in line.chars() {
            if ch == '{' {
                depth += 1;
                started = true;
            } else if ch == '}' && depth > 0 {
                depth -= 1;
            }
        }
        end = offset;
        if started && depth == 0 {
            break;
        }
    }
    end
}

fn detect_signals(lines: &[String], start: usize, end: usize) -> Result<Vec<&'static str>> {
    let signal_re = regex_from_static(&SIGNAL_RE, "global-state signal")?;
    let comment_re = regex_from_static(&COMMENT_RE, "comment")?;
    let mut signals = BTreeSet::new();
    for line in lines.iter().take(end + 1).skip(start) {
        if comment_re.is_match(line) {
            continue;
        }
        for hit in signal_re.captures_iter(line) {
            let Some(full) = hit.get(0) else { continue };
            if is_method_call(&line[..full.start()]) {
                continue;
            }
            if let Some(name) = hit.get(1) {
                signals.insert(signal_category(name.as_str()));
            }
        }
    }
    Ok(signals.into_iter().collect())
}

fn scan_file_for_unserialized_sites(
    repo_root: &Path,
    path: &Path,
) -> Result<Vec<SerialSiteIdentity>> {
    let fn_re = regex_from_static(&FN_RE, "function")?;
    let test_attr_re = regex_from_static(&TEST_ATTR_RE, "test attribute")?;
    let lines = read_lines(path)?;
    let relative =
        path.strip_prefix(repo_root).unwrap_or(path).display().to_string().replace('\\', "/");
    let mut sites = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let Some(function_name) = fn_re
            .captures(&lines[index])
            .and_then(|caps| caps.get(1))
            .map(|name| name.as_str().to_owned())
        else {
            index += 1;
            continue;
        };
        let attrs = attached_attributes(&lines, index);
        if !attrs.iter().any(|attr| test_attr_re.is_match(attr)) {
            index += 1;
            continue;
        }
        let end = body_extent(&lines, index);
        let signals = detect_signals(&lines, index, end)?;
        if !signals.is_empty() && !has_serial_guard(&attrs, &lines[index])? {
            sites.push(SerialSiteIdentity {
                path: relative.clone(),
                test_function: function_name,
                signals,
                line: index + 1,
            });
        }
        index = end + 1;
    }
    Ok(sites)
}

fn complete_serial_site_inventory(repo_root: &Path) -> Result<Vec<SerialSiteIdentity>> {
    let workspace_files = walk_workspace_rust_files(repo_root);
    let complete_file_set: BTreeSet<PathBuf> = workspace_files
        .iter()
        .filter(|path| is_complete_test_source_file(path) && !is_excluded_test_path(path))
        .cloned()
        .collect();

    let mut files: BTreeMap<PathBuf, usize> =
        complete_file_set.iter().map(|path| (path.clone(), 1)).collect();
    for path in &workspace_files {
        if complete_file_set.contains(path) || is_excluded_test_path(path) {
            continue;
        }
        let inline_test_start = first_cfg_test_line_number(path).unwrap_or(usize::MAX);
        if inline_test_start != usize::MAX {
            let lines = read_lines(path)?;
            files.insert(path.clone(), inline_test_start);
            for module in external_test_module_files(path, &lines)? {
                if !is_excluded_test_path(&module) {
                    files.insert(module, 1);
                }
            }
        }
    }

    let mut sites = Vec::new();
    for (path, start_line) in files {
        let mut file_sites = scan_file_for_unserialized_sites(repo_root, &path)?;
        // Production files only contribute sites at or below their
        // `#[cfg(test)]` boundary; complete test files scan from the top.
        file_sites.retain(|site| site.line >= start_line);
        sites.extend(file_sites);
    }
    sites.sort();
    Ok(sites)
}

/// Emit the current parallel-unsafe inventory as JSON. This is the measurement
/// surface used to seed and adjudicate `ci/serial_test_identities.json`.
pub(crate) fn write_inventory(repo_root: &Path) -> Result<i32> {
    let sites = complete_serial_site_inventory(repo_root)?;
    let json_sites = sites
        .iter()
        .map(|site| {
            serde_json::json!({
                "path": site.path,
                "test_function": site.test_function,
                "signals": site.signals,
                "line": site.line,
            })
        })
        .collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&json_sites)?);
    Ok(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegistryState {
    Active,
    Retired,
}

#[derive(Debug)]
struct SerialSiteRecord {
    path: String,
    test_function: String,
    signals: String,
    accepted_reason: String,
    state: RegistryState,
}

impl SerialSiteRecord {
    fn key(&self) -> (String, String) {
        (self.path.clone(), self.test_function.clone())
    }
}

fn registry_path(repo_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { repo_root.join(path) }
}

fn read_identity_registry(path: &Path) -> Result<BTreeMap<(String, String), SerialSiteRecord>> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| eyre!("reading serial identity registry {:?}: {err}", path))?;
    let document: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|err| eyre!("parsing serial identity registry {:?}: {err}", path))?;
    let schema_version = document
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| eyre!("serial identity registry schema_version must be an integer"))?;
    if schema_version != 1 {
        return Err(eyre!(
            "unsupported serial identity registry schema_version {schema_version}; expected 1"
        ));
    }
    let sites = document
        .get("sites")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| eyre!("serial identity registry sites must be an array"))?;

    let mut records = BTreeMap::new();
    for (index, value) in sites.iter().enumerate() {
        let object = value.as_object().ok_or_else(|| {
            eyre!("serial identity registry entry {} must be an object", index + 1)
        })?;
        let required_string = |field: &str| {
            object.get(field).and_then(serde_json::Value::as_str).map(str::to_owned).ok_or_else(
                || eyre!("serial identity registry entry {} requires string {field}", index + 1),
            )
        };
        let state = match required_string("state")?.as_str() {
            "active" => RegistryState::Active,
            "retired" => RegistryState::Retired,
            other => {
                return Err(eyre!(
                    "serial identity registry entry {} has invalid state {other:?}",
                    index + 1
                ));
            }
        };
        let record = SerialSiteRecord {
            path: required_string("path")?,
            test_function: required_string("test_function")?,
            signals: required_string("signals")?,
            accepted_reason: required_string("accepted_reason")?,
            state,
        };
        let fields = [
            ("path", record.path.trim()),
            ("test_function", record.test_function.trim()),
            ("signals", record.signals.trim()),
            ("accepted_reason", record.accepted_reason.trim()),
        ];
        if let Some((field, _)) = fields.iter().find(|(_, value)| value.is_empty()) {
            return Err(eyre!("serial identity registry entry {} has an empty {field}", index + 1));
        }
        if records.insert(record.key(), record).is_some() {
            return Err(eyre!(
                "serial identity registry entry {} duplicates a stable site identity",
                index + 1
            ));
        }
    }
    Ok(records)
}

/// Validate the parallel-unsafe inventory against the accepted registry.
///
/// Fail-closed transitions:
/// - an unserialized parallel-unsafe test fn absent from the registry is NEW
///   and fails the gate (this is the regression door);
/// - an `active` registry row whose site is no longer detected means the site
///   was repaired — retire the row before the gate passes (ratchet tightens);
/// - a `retired` row detected again means the guard was removed — fails.
pub(crate) fn check_serial_test_with_registry(repo_root: &Path, path: &Path) -> Result<i32> {
    let resolved = registry_path(repo_root, path);
    let registry = read_identity_registry(&resolved)?;
    let inventory = complete_serial_site_inventory(repo_root)?;
    let mut current = BTreeMap::new();
    for site in inventory {
        if current.insert(site.key(), site).is_some() {
            return Err(eyre!(
                "parallel-unsafe inventory contains duplicate stable identity (path, test_function); \
                 rename one of the colliding test functions"
            ));
        }
    }

    let active_count =
        registry.values().filter(|record| record.state == RegistryState::Active).count();
    println!(
        "parallel-unsafe test identities: current={} active_registry={} registry={:?}",
        current.len(),
        active_count,
        resolved
    );

    let mut failures = Vec::new();
    for (key, site) in &current {
        match registry.get(key) {
            None => failures.push(format!(
                "NEW parallel-unsafe test: {}:{} {} ({}) — add #[serial] or adjudicate a registry row",
                site.path,
                site.line,
                site.test_function,
                site.signals.join(",")
            )),
            Some(record) if record.state == RegistryState::Retired => failures.push(format!(
                "RETIRED identity returned: {} {} ({})",
                site.path,
                site.test_function,
                site.signals.join(",")
            )),
            Some(_) => {}
        }
    }
    for (key, record) in &registry {
        if record.state == RegistryState::Active && !current.contains_key(key) {
            failures.push(format!(
                "ACTIVE identity no longer detected: {} {} — if repaired, retire the registry row",
                record.path, record.test_function
            ));
        }
    }

    if failures.is_empty() {
        println!("{NC}PASS: every parallel-unsafe test is serialized or adjudicated{NC}");
        return Ok(0);
    }

    println!(
        "{RED}FAIL: {} parallel-unsafe test transition(s) require adjudication{NC}",
        failures.len()
    );
    for failure in failures {
        println!("{YELLOW}- {failure}{NC}");
    }
    Ok(1)
}

/// Enforce the parallel-unsafe test serialization registry.
pub(crate) fn check_serial_test(repo_root: &Path) -> Result<i32> {
    let default_path = repo_root.join(DEFAULT_REGISTRY);
    if !default_path.is_file() {
        return Err(eyre!(
            "serial identity registry {:?} is missing; the parallel-unsafe test gate cannot run",
            default_path
        ));
    }
    check_serial_test_with_registry(repo_root, &default_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn new(label: &str) -> Result<Self> {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let path = std::env::temp_dir().join(format!(
                "perl-ci-hygiene-serial-test-{label}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(path.join("ci"))?;
            fs::create_dir_all(path.join("crates/demo/tests/support"))?;
            fs::create_dir_all(path.join("crates/demo/src"))?;
            Ok(Self { path })
        }

        fn write_test(&self, contents: &str) -> Result<()> {
            fs::write(self.path.join("crates/demo/tests/demo.rs"), contents)
                .map_err(color_eyre::eyre::Report::from)
        }

        fn write_registry(&self, sites: serde_json::Value) -> Result<PathBuf> {
            let path = self.path.join("ci/serial_test_identities.json");
            fs::write(&path, serde_json::to_vec_pretty(&sites)?)?;
            Ok(path)
        }

        fn empty_registry(&self) -> Result<PathBuf> {
            self.write_registry(serde_json::json!({ "schema_version": 1, "sites": [] }))
        }

        fn check(&self, registry: &Path) -> Result<i32> {
            check_serial_test_with_registry(&self.path, registry)
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn registry_row(
        path: &str,
        test_function: &str,
        signals: &str,
        reason: &str,
        state: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "path": path,
            "test_function": test_function,
            "signals": signals,
            "accepted_reason": reason,
            "state": state,
        })
    }

    const UNANNOTATED_ENV_TEST: &str = "#[test]\nfn flips_toolchain_env() {\n    unsafe {\n        std::env::set_var(\"PERLBREW_ROOT\", \"/tmp/plenv\");\n    }\n}\n";

    /// Discriminating mutant: injecting an unannotated parallel-unsafe test
    /// must fail the gate.
    #[test]
    fn mutant_unannotated_parallel_unsafe_test_fails() -> Result<()> {
        let repo = TempRepo::new("mutant-new")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    /// The identical function with the repo's serialization guard passes.
    #[test]
    fn annotated_twin_passes() -> Result<()> {
        let repo = TempRepo::new("mutant-annotated")?;
        let annotated = UNANNOTATED_ENV_TEST.replace("#[test]", "#[test]\n#[serial]");
        repo.write_test(&annotated)?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn keyed_and_reexported_serial_idioms_pass() -> Result<()> {
        let repo = TempRepo::new("serial-idioms")?;
        repo.write_test(concat!(
            "#[test]\n#[serial(env_toolchain)]\nfn keyed() {\n    std::env::set_var(\"A\", \"1\");\n}\n",
            "#[test]\n#[serial_test::serial]\nfn reexported() {\n    std::env::remove_var(\"B\");\n}\n",
        ))?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn tokio_and_rstest_surfaces_are_detected() -> Result<()> {
        let repo = TempRepo::new("attr-macros")?;
        repo.write_test(concat!(
            "#[tokio::test]\nasync fn async_env_flip() {\n    std::env::set_var(\"A\", \"1\");\n}\n",
            "#[rstest]\n#[case(1)]\nfn rstest_env_flip() {\n    std::env::set_var(\"B\", \"1\");\n}\n",
        ))?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn helper_functions_are_not_flagged() -> Result<()> {
        let repo = TempRepo::new("helpers")?;
        fs::write(
            repo.path.join("crates/demo/tests/support/env_guard.rs"),
            "pub fn set(key: &str, value: &str) {\n    unsafe { std::env::set_var(key, value) };\n}\n",
        )?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn method_call_set_var_is_not_process_env() -> Result<()> {
        let repo = TempRepo::new("method-call")?;
        repo.write_test(
            "#[test]\nfn builds_child_env() {\n    let mut cmd = std::process::Command::new(\"perl\");\n    cmd.envs([(\"A\", \"1\")]);\n    let mut harness = Harness::default();\n    harness.set_var(\"A\", \"1\");\n}\n",
        )?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn cwd_mutation_requires_serialization() -> Result<()> {
        let repo = TempRepo::new("cwd")?;
        repo.write_test(
            "#[test]\nfn chdirs_into_fixture() {\n    std::env::set_current_dir(\"/tmp\");\n}\n",
        )?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn registered_identity_passes_and_new_still_fails() -> Result<()> {
        let repo = TempRepo::new("registry-mixed")?;
        repo.write_test(concat!(
            "#[test]\nfn legacy_env_flip() {\n    std::env::set_var(\"A\", \"1\");\n}\n",
            "#[test]\nfn fresh_env_flip() {\n    std::env::set_var(\"B\", \"1\");\n}\n",
        ))?;
        let path = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "legacy_env_flip",
                "env_set",
                "tracked #1269 long tail",
                "active"
            )]
        }))?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn repairing_active_row_requires_registry_retirement() -> Result<()> {
        let repo = TempRepo::new("repair")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let active = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_set",
                "tracked #1269 long tail",
                "active"
            )]
        }))?;
        assert_eq!(repo.check(&active)?, 0);

        // The site is repaired with #[serial]; the stale active row now fails.
        let annotated = UNANNOTATED_ENV_TEST.replace("#[test]", "#[test]\n#[serial]");
        repo.write_test(&annotated)?;
        assert_eq!(repo.check(&active)?, 1);

        // Retiring the row restores green and tightens the accepted set.
        let retired = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_set",
                "repaired with #[serial]",
                "retired"
            )]
        }))?;
        assert_eq!(repo.check(&retired)?, 0);
        Ok(())
    }

    #[test]
    fn retired_identity_returning_fails() -> Result<()> {
        let repo = TempRepo::new("retired-return")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let path = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_set",
                "previously repaired",
                "retired"
            )]
        }))?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn inline_cfg_test_module_is_scanned() -> Result<()> {
        let repo = TempRepo::new("inline-mod")?;
        fs::write(
            repo.path.join("crates/demo/src/lib.rs"),
            concat!(
                "pub fn identity(value: u8) -> u8 { value }\n",
                "#[cfg(test)]\nmod tests {\n",
                "    #[test]\n    fn inline_env_flip() {\n        std::env::set_var(\"A\", \"1\");\n    }\n",
                "}\n",
            ),
        )?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 1);
        Ok(())
    }

    #[test]
    fn production_code_is_not_flagged() -> Result<()> {
        let repo = TempRepo::new("production")?;
        fs::write(
            repo.path.join("crates/demo/src/lib.rs"),
            "pub fn configure(key: &str, value: &str) {\n    unsafe { std::env::set_var(key, value) };\n}\n",
        )?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn excluded_crate_surface_is_skipped() -> Result<()> {
        let repo = TempRepo::new("excluded")?;
        fs::create_dir_all(repo.path.join("crates/perl-tdd-support/tests"))?;
        fs::write(repo.path.join("crates/perl-tdd-support/tests/runner.rs"), UNANNOTATED_ENV_TEST)?;
        let path = repo.empty_registry()?;
        assert_eq!(repo.check(&path)?, 0);
        Ok(())
    }

    #[test]
    fn missing_default_registry_is_a_structural_error() -> Result<()> {
        let repo = TempRepo::new("missing-registry")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let mut err_text = String::new();
        if let Err(err) = check_serial_test(&repo.path) {
            err_text = err.to_string();
        }
        assert!(
            err_text.contains("serial identity registry"),
            "expected missing-registry error, got: {err_text}"
        );
        Ok(())
    }

    #[test]
    fn empty_reason_and_duplicate_rows_are_rejected() -> Result<()> {
        let repo = TempRepo::new("invalid-rows")?;
        let empty_reason = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [registry_row(
                "crates/demo/tests/demo.rs",
                "flips_toolchain_env",
                "env_set",
                " ",
                "active"
            )]
        }))?;
        let mut empty_reason_err = String::new();
        if let Err(err) = read_identity_registry(&empty_reason) {
            empty_reason_err = err.to_string();
        }
        assert!(
            empty_reason_err.contains("accepted_reason"),
            "expected empty-reason error, got: {empty_reason_err}"
        );

        let duplicate = repo.write_registry(serde_json::json!({
            "schema_version": 1,
            "sites": [
                registry_row(
                    "crates/demo/tests/demo.rs",
                    "flips_toolchain_env",
                    "env_set",
                    "first",
                    "active"
                ),
                registry_row(
                    "crates/demo/tests/demo.rs",
                    "flips_toolchain_env",
                    "env_set",
                    "second",
                    "retired"
                )
            ]
        }))?;
        let mut duplicate_err = String::new();
        if let Err(err) = read_identity_registry(&duplicate) {
            duplicate_err = err.to_string();
        }
        assert!(
            duplicate_err.contains("duplicates"),
            "expected duplicate-row error, got: {duplicate_err}"
        );
        Ok(())
    }

    #[test]
    fn inventory_reports_site_identity() -> Result<()> {
        let repo = TempRepo::new("inventory")?;
        repo.write_test(UNANNOTATED_ENV_TEST)?;
        let sites = complete_serial_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, "crates/demo/tests/demo.rs");
        assert_eq!(sites[0].test_function, "flips_toolchain_env");
        assert_eq!(sites[0].signals, vec!["env_set"]);
        Ok(())
    }
}
