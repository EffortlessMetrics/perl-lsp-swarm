use color_eyre::eyre::{Result, eyre};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use perl_ci_hygiene::walk_rs_files;

use crate::{first_cfg_test_line_number, read_lines};

static PANIC_MACRO_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"panic!\s*[\(\{]"));
static COMMENT_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| Regex::new(r"^\s*//"));

/// Crates excluded from integration-test panic scanning — mirrors `CI_REPORT_CRATES_EXCLUDE`
/// in `main.rs` (legacy/test-support crates and self-check harnesses).
const CI_REPORT_CRATES_EXCLUDE: [&str; 5] = [
    "tree-sitter-perl-c",
    "perl-parser-pest",
    "perl-tdd-support",
    "perl-test-must",
    "perl-ci-hygiene",
];

fn regex_from_static(
    regex: &'static LazyLock<Result<Regex, regex::Error>>,
    label: &str,
) -> Result<&'static Regex> {
    regex.as_ref().map_err(|err| eyre!("{label} regex failed to compile: {err}"))
}

fn is_integration_test_file(path: &Path) -> bool {
    path.components().any(|component| component.as_os_str() == "tests")
}

fn is_complete_test_source_file(path: &Path) -> bool {
    is_integration_test_file(path)
        || path.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
            ["_test.rs", "_tests.rs", "tests.rs"].iter().any(|suffix| name.ends_with(suffix))
        })
}

fn is_excluded_integration_test_path(path: &Path) -> bool {
    if path.components().any(|component| {
        let value = component.as_os_str();
        value == "benches" || value == "examples" || value == "bin"
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

fn walk_complete_test_source_files(repo_root: &Path) -> Vec<PathBuf> {
    walk_workspace_rust_files(repo_root)
        .into_iter()
        .filter(|path| {
            is_complete_test_source_file(path) && !is_excluded_integration_test_path(path)
        })
        .collect()
}

fn external_test_module_files(path: &Path, lines: &[String]) -> Vec<PathBuf> {
    let Some(start_line) = first_cfg_test_line_number(path).ok().filter(|line| *line != usize::MAX)
    else {
        return Vec::new();
    };

    let mut files = Vec::new();
    for line in lines.iter().skip(start_line.saturating_sub(1)) {
        let Some(name) = line
            .trim()
            .strip_prefix("mod ")
            .and_then(|name| name.strip_suffix(';'))
            .map(str::trim)
            .filter(|name| {
                !name.is_empty() && name.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
            })
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
    files
}

#[derive(Debug, PartialEq, Eq)]
struct PanicSiteIdentity {
    path: String,
    enclosing_test_or_function: String,
    macro_family: &'static str,
    normalized_snippet: String,
    selector_identity: String,
    line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PanicSiteKey {
    path: String,
    enclosing_test_or_function: String,
    macro_family: String,
    normalized_snippet: String,
    selector_identity: String,
}

impl PanicSiteIdentity {
    fn key(&self) -> PanicSiteKey {
        PanicSiteKey {
            path: self.path.clone(),
            enclosing_test_or_function: self.enclosing_test_or_function.clone(),
            macro_family: self.macro_family.to_string(),
            normalized_snippet: self.normalized_snippet.clone(),
            selector_identity: self.selector_identity.clone(),
        }
    }
}

#[derive(Debug)]
struct PanicSiteRecord {
    path: String,
    enclosing_test_or_function: String,
    macro_family: String,
    normalized_snippet: String,
    selector_identity: String,
    accepted_reason: String,
    state: PanicSiteState,
}

#[derive(Debug, PartialEq, Eq)]
enum PanicSiteState {
    Active,
    Retired,
}

impl PanicSiteRecord {
    fn key(&self) -> PanicSiteKey {
        PanicSiteKey {
            path: self.path.clone(),
            enclosing_test_or_function: self.enclosing_test_or_function.clone(),
            macro_family: self.macro_family.clone(),
            normalized_snippet: self.normalized_snippet.clone(),
            selector_identity: self.selector_identity.clone(),
        }
    }
}

fn enclosing_test_or_function(lines: &[String], line_index: usize) -> String {
    lines[..=line_index]
        .iter()
        .rev()
        .find_map(|line| {
            if line.trim_start().starts_with("//") {
                return None;
            }
            let start = line.find("fn ")? + 3;
            let name = line[start..].split(['(', '<', '{', ' ']).next()?.trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .unwrap_or_else(|| "<unknown>".to_string())
}

fn normalized_panic_invocation(lines: &[String], line_index: usize, column: usize) -> String {
    let mut text = String::new();
    let mut depth = 0usize;
    let mut started = false;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, line) in lines.iter().enumerate().skip(line_index).take(16) {
        let fragment = line.as_str();
        let scan_fragment =
            if offset == line_index { line.get(column..).unwrap_or("") } else { line.as_str() };
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(fragment);

        for ch in scan_fragment.chars() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }
            if ch == '"' {
                in_string = true;
                continue;
            }
            if ch == '(' || ch == '{' {
                started = true;
                depth += 1;
            } else if (ch == ')' || ch == '}') && started {
                depth = depth.saturating_sub(1);
            }
        }
        if started && depth == 0 {
            break;
        }
    }

    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn selector_identity(snippet: &str, occurrence: usize) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in snippet.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("invocation:{hash:016x}:occurrence:{occurrence}")
}

fn complete_panic_site_inventory(repo_root: &Path) -> Result<Vec<PanicSiteIdentity>> {
    let panic_re = regex_from_static(&PANIC_MACRO_RE, "panic macro")?;
    let comment_re = regex_from_static(&COMMENT_RE, "comment")?;
    let mut sites = Vec::new();

    let complete_files = walk_complete_test_source_files(repo_root);
    let complete_file_set = complete_files.iter().cloned().collect::<BTreeSet<_>>();
    let mut files = complete_files.into_iter().map(|path| (path, 1)).collect::<BTreeMap<_, _>>();
    for path in walk_workspace_rust_files(repo_root) {
        if complete_file_set.contains(&path) || is_excluded_integration_test_path(&path) {
            continue;
        }
        let inline_test_start = first_cfg_test_line_number(&path).unwrap_or(usize::MAX);
        if inline_test_start != usize::MAX {
            let lines = read_lines(&path)?;
            files.insert(path.clone(), inline_test_start);
            for module in external_test_module_files(&path, &lines) {
                if !is_excluded_integration_test_path(&module) {
                    files.insert(module, 1);
                }
            }
        }
    }

    for (path, start_line) in files {
        let lines = read_lines(&path)?;
        let relative =
            path.strip_prefix(repo_root).unwrap_or(&path).display().to_string().replace('\\', "/");
        for (index, line) in lines.iter().enumerate().skip(start_line.saturating_sub(1)) {
            if comment_re.is_match(line) {
                continue;
            }
            for panic_match in panic_re.find_iter(line) {
                sites.push(PanicSiteIdentity {
                    path: relative.clone(),
                    enclosing_test_or_function: enclosing_test_or_function(&lines, index),
                    macro_family: "panic!",
                    normalized_snippet: normalized_panic_invocation(
                        &lines,
                        index,
                        panic_match.start(),
                    ),
                    selector_identity: String::new(),
                    line: index + 1,
                });
            }
        }
    }
    sites.sort_by(|left, right| {
        (&left.path, left.line, &left.enclosing_test_or_function).cmp(&(
            &right.path,
            right.line,
            &right.enclosing_test_or_function,
        ))
    });
    let mut selector_counts = BTreeMap::<(String, String, &'static str, String), usize>::new();
    for site in &mut sites {
        let selector_key = (
            site.path.clone(),
            site.enclosing_test_or_function.clone(),
            site.macro_family,
            site.normalized_snippet.clone(),
        );
        let occurrence = selector_counts.entry(selector_key).or_default();
        *occurrence += 1;
        site.selector_identity = selector_identity(&site.normalized_snippet, *occurrence);
    }
    Ok(sites)
}

/// Emit the complete test-source panic identity inventory without changing the
/// established count gate. This is the measurement surface for #2332 while
/// the accepted identity registry is being adjudicated.
pub(crate) fn write_inventory(repo_root: &Path) -> Result<i32> {
    let sites = complete_panic_site_inventory(repo_root)?;
    let json_sites = sites
        .iter()
        .map(|site| {
            serde_json::json!({
                "path": site.path,
                "enclosing_test_or_function": site.enclosing_test_or_function,
                "macro_family": site.macro_family,
                "normalized_snippet": site.normalized_snippet,
                "selector_identity": site.selector_identity,
                "line": site.line,
            })
        })
        .collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&json_sites)?);
    Ok(0)
}

fn read_identity_registry(path: &Path) -> Result<BTreeMap<PanicSiteKey, PanicSiteRecord>> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| eyre!("reading panic identity registry {:?}: {err}", path))?;
    let document: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|err| eyre!("parsing panic identity registry {:?}: {err}", path))?;
    let schema_version = document
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| eyre!("panic identity registry schema_version must be an integer"))?;
    if schema_version != 1 {
        return Err(eyre!(
            "unsupported panic identity registry schema_version {}; expected 1",
            schema_version
        ));
    }
    let sites = document
        .get("sites")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| eyre!("panic identity registry sites must be an array"))?;

    let mut records = BTreeMap::new();
    for (index, value) in sites.iter().enumerate() {
        let object = value.as_object().ok_or_else(|| {
            eyre!("panic identity registry entry {} must be an object", index + 1)
        })?;
        let required_string = |field: &str| {
            object.get(field).and_then(serde_json::Value::as_str).map(str::to_owned).ok_or_else(
                || eyre!("panic identity registry entry {} requires string {field}", index + 1),
            )
        };
        let state = match required_string("state")?.as_str() {
            "active" => PanicSiteState::Active,
            "retired" => PanicSiteState::Retired,
            other => {
                return Err(eyre!(
                    "panic identity registry entry {} has invalid state {other:?}",
                    index + 1
                ));
            }
        };
        let record = PanicSiteRecord {
            path: required_string("path")?,
            enclosing_test_or_function: required_string("enclosing_test_or_function")?,
            macro_family: required_string("macro_family")?,
            normalized_snippet: required_string("normalized_snippet")?,
            selector_identity: required_string("selector_identity")?,
            accepted_reason: required_string("accepted_reason")?,
            state,
        };
        let fields = [
            ("path", record.path.trim()),
            ("enclosing_test_or_function", record.enclosing_test_or_function.trim()),
            ("macro_family", record.macro_family.trim()),
            ("normalized_snippet", record.normalized_snippet.trim()),
            ("selector_identity", record.selector_identity.trim()),
            ("accepted_reason", record.accepted_reason.trim()),
        ];
        if let Some((field, _)) = fields.iter().find(|(_, value)| value.is_empty()) {
            return Err(eyre!("panic identity registry entry {} has an empty {field}", index + 1));
        }
        let key = record.key();
        if records.insert(key, record).is_some() {
            return Err(eyre!(
                "panic identity registry entry {} duplicates a stable site identity",
                index + 1
            ));
        }
    }
    Ok(records)
}

fn registry_path(repo_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { repo_root.join(path) }
}

/// Validate the complete inventory against accepted stable identities.
///
/// This is deliberately opt-in until the measured population has been
/// semantically adjudicated. The registry is an identity authority, not a
/// replacement scalar baseline: active entries must be present, new entries
/// fail, and a retired entry returning fails.
pub(crate) fn check_panic_test_with_registry(repo_root: &Path, path: &Path) -> Result<i32> {
    let registry_path = registry_path(repo_root, path);
    let registry = read_identity_registry(&registry_path)?;
    let inventory = complete_panic_site_inventory(repo_root)?;
    let mut current = BTreeMap::new();
    for site in inventory {
        let key = site.key();
        if current.insert(key, site).is_some() {
            return Err(eyre!(
                "complete panic inventory contains duplicate stable identity; add a selector identity for same-line sites"
            ));
        }
    }

    let active_count =
        registry.values().filter(|record| record.state == PanicSiteState::Active).count();
    println!(
        "test panic! identities: current={} active_registry={} registry={:?}",
        current.len(),
        active_count,
        registry_path
    );

    let mut failures = Vec::new();
    for (key, site) in &current {
        match registry.get(key) {
            None => failures.push(format!(
                "NEW identity: {}:{} {} ({})",
                site.path, site.line, site.enclosing_test_or_function, site.normalized_snippet
            )),
            Some(record) if record.state == PanicSiteState::Retired => failures.push(format!(
                "RETIRED identity returned: {}:{} {} ({})",
                site.path, site.line, site.enclosing_test_or_function, site.normalized_snippet
            )),
            Some(_) => {}
        }
    }
    for (key, record) in &registry {
        if record.state == PanicSiteState::Active && !current.contains_key(key) {
            failures.push(format!(
                "ACTIVE identity missing from current inventory: {} {} ({})",
                record.path, record.enclosing_test_or_function, record.normalized_snippet
            ));
        }
    }

    if failures.is_empty() {
        println!("PASS: every current identity is accepted and active identities are present");
        return Ok(0);
    }

    println!("FAIL: {} panic identity transition(s) require adjudication", failures.len());
    for failure in failures {
        println!("- {failure}");
    }
    Ok(1)
}

/// Enforce the complete test-source panic identity registry.
pub(crate) fn check_panic_test(repo_root: &Path) -> Result<i32> {
    let registry_path = repo_root.join("ci/panic_test_identities.json");
    if !registry_path.is_file() {
        return Err(eyre!(
            "panic identity registry {:?} is missing; the complete identity gate cannot run",
            registry_path
        ));
    }
    check_panic_test_with_registry(repo_root, &registry_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_test_must::must_err_with;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempRepo {
        path: std::path::PathBuf,
    }

    impl TempRepo {
        fn new(label: &str) -> Result<Self> {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let path = std::env::temp_dir()
                .join(format!("perl-ci-hygiene-panic-test-{label}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(path.join("ci"))?;
            fs::create_dir_all(path.join("crates/demo/tests"))?;
            fs::create_dir_all(path.join("crates/demo/src"))?;
            fs::create_dir_all(path.join("crates/demo/benches"))?;
            fs::create_dir_all(path.join("xtask/tests"))?;
            fs::write(path.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/demo\"]\n")?;
            fs::write(
                path.join("crates/demo/Cargo.toml"),
                "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            )?;
            Ok(Self { path })
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_registry(repo: &TempRepo, sites: serde_json::Value) -> Result<PathBuf> {
        let path = repo.path.join("ci/panic_test_identities.json");
        fs::write(&path, serde_json::to_vec_pretty(&sites)?)?;
        Ok(path)
    }

    fn registry_site(site: &PanicSiteIdentity, state: &str, reason: &str) -> serde_json::Value {
        serde_json::json!({
            "path": site.path,
            "enclosing_test_or_function": site.enclosing_test_or_function,
            "macro_family": site.macro_family,
            "normalized_snippet": site.normalized_snippet,
            "selector_identity": site.selector_identity,
            "accepted_reason": reason,
            "state": state,
        })
    }

    #[test]
    fn complete_inventory_counts_integration_tests() -> Result<()> {
        let repo = TempRepo::new("integration")?;
        fs::write(
            repo.path.join("crates/demo/tests/demo.rs"),
            r#"
#[test]
fn demo() {
    panic!("boom");
}
"#,
        )?;
        assert_eq!(complete_panic_site_inventory(&repo.path)?.len(), 1);
        Ok(())
    }

    #[test]
    fn complete_inventory_counts_test_suffixed_source_files() -> Result<()> {
        let repo = TempRepo::new("test-suffix")?;
        fs::write(
            repo.path.join("crates/demo/src/parser_tests.rs"),
            "#[test]\nfn demo() { panic!(\"boom\"); }\n",
        )?;
        let inventory = complete_panic_site_inventory(&repo.path)?;
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].path, "crates/demo/src/parser_tests.rs");
        assert_eq!(inventory[0].enclosing_test_or_function, "demo");
        Ok(())
    }

    #[test]
    fn complete_inventory_covers_workspace_and_inline_tests() -> Result<()> {
        let repo = TempRepo::new("complete-sources")?;
        fs::write(
            repo.path.join("crates/demo/src/parser_tests.rs"),
            "#[test]\nfn suffixed() { panic!(\"one\"); panic!(\"two\"); }\n",
        )?;
        fs::write(
            repo.path.join("crates/demo/src/lib.rs"),
            "#[cfg(test)]\nmod tests {\n    fn inline() {\n        // see fn misleading above\n        panic!(\"inline\");\n    }\n}\n",
        )?;
        fs::write(
            repo.path.join("xtask/tests/fixture.rs"),
            "#[test]\nfn xtask_fixture() { panic!(\"xtask\"); }\n",
        )?;
        fs::write(
            repo.path.join("crates/demo/benches/bench_tests.rs"),
            "fn bench_fixture() { panic!(\"ignored\"); }\n",
        )?;

        let inventory = complete_panic_site_inventory(&repo.path)?;
        assert_eq!(inventory.len(), 4);
        assert!(inventory.iter().any(|site| site.path == "xtask/tests/fixture.rs"));
        assert!(inventory.iter().any(|site| {
            site.path == "crates/demo/src/lib.rs" && site.enclosing_test_or_function == "inline"
        }));
        assert!(inventory.iter().all(|site| !site.path.contains("benches")));
        assert_eq!(inventory[0].path, "crates/demo/src/lib.rs");
        Ok(())
    }

    #[test]
    fn complete_inventory_follows_cfg_test_external_modules() -> Result<()> {
        let repo = TempRepo::new("external-test-module")?;
        fs::write(repo.path.join("crates/demo/src/lib.rs"), "#[cfg(test)]\nmod external;\n")?;
        fs::write(
            repo.path.join("crates/demo/src/external.rs"),
            "#[test]\nfn external() { panic!(\"external\"); }\n",
        )?;

        let inventory = complete_panic_site_inventory(&repo.path)?;
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].path, "crates/demo/src/external.rs");
        Ok(())
    }

    #[test]
    fn complete_inventory_ignores_production_code() -> Result<()> {
        let repo = TempRepo::new("production")?;
        fs::write(
            repo.path.join("crates/demo/src/lib.rs"),
            r#"
pub fn boom() {
    panic!("production");
}
"#,
        )?;
        assert!(complete_panic_site_inventory(&repo.path)?.is_empty());
        Ok(())
    }

    #[test]
    fn check_panic_test_uses_identity_registry() -> Result<()> {
        let repo = TempRepo::new("registry-default")?;
        fs::write(
            repo.path.join("crates/demo/tests/demo.rs"),
            "#[test]\nfn demo() { panic!(\"boom\"); }\n",
        )?;
        let site = complete_panic_site_inventory(&repo.path)?
            .into_iter()
            .next()
            .ok_or_else(|| eyre!("fixture should contain one panic site"))?;
        write_registry(
            &repo,
            serde_json::json!({
                "schema_version": 1,
                "sites": [registry_site(&site, "active", "intentional fixture panic")]
            }),
        )?;
        assert_eq!(check_panic_test(&repo.path)?, 0);
        Ok(())
    }

    #[test]
    fn check_panic_test_errors_when_identity_registry_missing() -> Result<()> {
        let repo = TempRepo::new("registry-missing-default")?;
        let err = must_err_with(
            check_panic_test(&repo.path),
            "missing registry must fail the default checker",
        );
        assert!(
            err.to_string().contains("identity registry"),
            "expected missing-registry error, got: {err}"
        );
        Ok(())
    }

    #[test]
    fn identity_registry_accepts_active_current_site() -> Result<()> {
        let repo = TempRepo::new("registry-accept")?;
        fs::write(
            repo.path.join("crates/demo/tests/demo.rs"),
            "#[test]\nfn demo() { panic!(\"boom\"); }\n",
        )?;
        let site = complete_panic_site_inventory(&repo.path)?
            .into_iter()
            .next()
            .ok_or_else(|| eyre!("fixture should contain one panic site"))?;
        let path = write_registry(
            &repo,
            serde_json::json!({
                "schema_version": 1,
                "sites": [registry_site(&site, "active", "intentional fixture panic")]
            }),
        )?;
        assert_eq!(check_panic_test_with_registry(&repo.path, &path)?, 0);
        Ok(())
    }

    #[test]
    fn identity_registry_ignores_line_number_changes() -> Result<()> {
        let repo = TempRepo::new("registry-line-shift")?;
        let source = repo.path.join("crates/demo/tests/demo.rs");
        fs::write(&source, "#[test]\nfn demo() { panic!(\"boom\"); }\n")?;
        let site = complete_panic_site_inventory(&repo.path)?
            .into_iter()
            .next()
            .ok_or_else(|| eyre!("fixture should contain one panic site"))?;
        let path = write_registry(
            &repo,
            serde_json::json!({
                "schema_version": 1,
                "sites": [registry_site(&site, "active", "intentional fixture panic")]
            }),
        )?;
        fs::write(&source, "\n\n#[test]\nfn demo() { panic!(\"boom\"); }\n")?;
        assert_eq!(check_panic_test_with_registry(&repo.path, &path)?, 0);
        Ok(())
    }

    #[test]
    fn identity_registry_rejects_changed_multiline_invocation() -> Result<()> {
        let repo = TempRepo::new("registry-multiline-invocation")?;
        let source = repo.path.join("crates/demo/tests/demo.rs");
        fs::write(
            &source,
            "#[test]\nfn demo() {\n    panic!(\n        \"first\"\n    );\n    panic!(\n        \"second\"\n    );\n}\n",
        )?;
        let sites = complete_panic_site_inventory(&repo.path)?;
        assert_eq!(sites.len(), 2);
        assert!(sites[0].normalized_snippet.contains("first"));
        assert!(sites[0].selector_identity.starts_with("invocation:"));
        let path = write_registry(
            &repo,
            serde_json::json!({
                "schema_version": 1,
                "sites": sites.iter().map(|site| registry_site(site, "active", "intentional fixture panic")).collect::<Vec<_>>()
            }),
        )?;
        fs::write(
            &source,
            "#[test]\nfn demo() {\n    panic!(\n        \"changed\"\n    );\n    panic!(\n        \"second\"\n    );\n}\n",
        )?;
        assert_eq!(check_panic_test_with_registry(&repo.path, &path)?, 1);
        Ok(())
    }

    #[test]
    fn identity_registry_rejects_new_identity() -> Result<()> {
        let repo = TempRepo::new("registry-new")?;
        fs::write(
            repo.path.join("crates/demo/tests/demo.rs"),
            "#[test]\nfn demo() { panic!(\"boom\"); }\n",
        )?;
        let path = write_registry(&repo, serde_json::json!({"schema_version": 1, "sites": []}))?;
        assert_eq!(check_panic_test_with_registry(&repo.path, &path)?, 1);
        Ok(())
    }

    #[test]
    fn identity_registry_rejects_returned_retired_identity() -> Result<()> {
        let repo = TempRepo::new("registry-retired")?;
        fs::write(
            repo.path.join("crates/demo/tests/demo.rs"),
            "#[test]\nfn demo() { panic!(\"boom\"); }\n",
        )?;
        let site = complete_panic_site_inventory(&repo.path)?
            .into_iter()
            .next()
            .ok_or_else(|| eyre!("fixture should contain one panic site"))?;
        let path = write_registry(
            &repo,
            serde_json::json!({
                "schema_version": 1,
                "sites": [registry_site(&site, "retired", "removed accidental panic")]
            }),
        )?;
        assert_eq!(check_panic_test_with_registry(&repo.path, &path)?, 1);
        Ok(())
    }

    #[test]
    fn identity_registry_rejects_active_identity_missing_from_inventory() -> Result<()> {
        let repo = TempRepo::new("registry-missing")?;
        fs::write(
            repo.path.join("crates/demo/tests/demo.rs"),
            "#[test]\nfn demo() { panic!(\"boom\"); }\n",
        )?;
        let path = write_registry(
            &repo,
            serde_json::json!({
                "schema_version": 1,
                "sites": [
                    {
                        "path": "crates/demo/tests/removed.rs",
                        "enclosing_test_or_function": "removed",
                        "macro_family": "panic!",
                        "normalized_snippet": "panic!(\"removed\");",
                        "selector_identity": "occurrence:1",
                        "accepted_reason": "removed fixture",
                        "state": "active"
                    }
                ]
            }),
        )?;
        assert_eq!(check_panic_test_with_registry(&repo.path, &path)?, 1);
        Ok(())
    }

    #[test]
    fn identity_registry_rejects_empty_reason_and_duplicate_identity() -> Result<()> {
        let repo = TempRepo::new("registry-invalid")?;
        fs::write(
            repo.path.join("crates/demo/tests/demo.rs"),
            "#[test]\nfn demo() { panic!(\"boom\"); }\n",
        )?;
        let site = complete_panic_site_inventory(&repo.path)?
            .into_iter()
            .next()
            .ok_or_else(|| eyre!("fixture should contain one panic site"))?;
        let empty_reason = write_registry(
            &repo,
            serde_json::json!({
                "schema_version": 1,
                "sites": [registry_site(&site, "active", " ")]
            }),
        )?;
        let err = must_err_with(
            read_identity_registry(&empty_reason),
            "an empty accepted reason must be rejected",
        );
        assert!(err.to_string().contains("accepted_reason"));

        let duplicate = write_registry(
            &repo,
            serde_json::json!({
                "schema_version": 1,
                "sites": [
                    registry_site(&site, "active", "first"),
                    registry_site(&site, "retired", "second")
                ]
            }),
        )?;
        let err = must_err_with(
            read_identity_registry(&duplicate),
            "duplicate stable identities must be rejected",
        );
        assert!(err.to_string().contains("duplicates"));
        Ok(())
    }
}
