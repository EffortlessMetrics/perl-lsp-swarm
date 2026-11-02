//! Contract tests for the native parser incremental authority ledger.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Debug, Deserialize)]
struct AuthorityManifest {
    schema_version: u32,
    owner_issue: u64,
    canonical: CanonicalSurface,
    compatibility: CompatibilitySurface,
    lower_tier: Vec<LowerTierSurface>,
    modules: Vec<ModuleSurface>,
}

#[derive(Debug, Deserialize)]
struct CanonicalSurface {
    module: String,
    path: String,
    entry_points: Vec<String>,
    production_eligible: bool,
    eligibility_blockers: Vec<u64>,
    decision: String,
}

#[derive(Debug, Deserialize)]
struct CompatibilitySurface {
    package: String,
    status: String,
    implementation_owner: String,
    behavior_authority: bool,
    exit_issue: u64,
}

#[derive(Debug, Deserialize)]
struct LowerTierSurface {
    package: String,
    module: String,
    path: String,
    status: String,
    entry_points: Vec<String>,
    behavior_authority: bool,
    production_eligible: bool,
    allowed_consumers: Vec<AllowedConsumer>,
    next_action: String,
    owner_issue: u64,
    decision: String,
}

#[derive(Debug, Deserialize)]
struct AllowedConsumer {
    symbol: String,
    source_path: String,
}

#[derive(Debug, Deserialize)]
struct ModuleSurface {
    module: String,
    status: String,
    public_reexport: bool,
    production_eligible: bool,
    next_action: String,
    owner_issue: u64,
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_root() -> PathBuf {
    let crate_root = crate_root();
    crate_root.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or(crate_root)
}

fn read(path: impl AsRef<Path>) -> Result<String, std::io::Error> {
    fs::read_to_string(path)
}

fn load_manifest() -> Result<AuthorityManifest, Box<dyn std::error::Error>> {
    let source = read(crate_root().join("incremental_authority.json"))?;
    Ok(serde_json::from_str(&source)?)
}

fn compact_whitespace(source: &str) -> String {
    source.split_whitespace().collect()
}

/// Remove comments and string/char literal bodies so incidental mentions of
/// `incremental` do not classify a file as a lower-tier consumer.
fn strip_comments_and_string_literals(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = vec![b' '; bytes.len()];
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                if i + 1 < bytes.len() {
                    i += 2;
                }
            }
            b'"' => {
                out[i] = b'"';
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => {
                            if i + 1 < bytes.len() {
                                out[i] = bytes[i];
                                out[i + 1] = bytes[i + 1];
                            }
                            i += 2;
                        }
                        b'"' => {
                            out[i] = b'"';
                            i += 1;
                            break;
                        }
                        _ => {
                            out[i] = b' ';
                            i += 1;
                        }
                    }
                }
            }
            b'\'' => {
                out[i] = b'\'';
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => {
                            if i + 1 < bytes.len() {
                                out[i] = bytes[i];
                                out[i + 1] = bytes[i + 1];
                            }
                            i += 2;
                        }
                        b'\'' => {
                            out[i] = b'\'';
                            i += 1;
                            break;
                        }
                        _ => {
                            out[i] = b' ';
                            i += 1;
                        }
                    }
                }
            }
            b'r' => {
                if let Some((hashes, quote)) = raw_string_hashes(bytes, i) {
                    out[i] = b'r';
                    i += 1;
                    for _ in 0..hashes {
                        out[i] = b'#';
                        i += 1;
                    }
                    out[i] = b'"';
                    i = skip_raw_string(bytes, quote, hashes);
                } else {
                    out[i] = b'r';
                    i += 1;
                }
            }
            other => {
                out[i] = other;
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn raw_string_hashes(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut hashes = 0usize;
    let mut j = start + 1;
    while j < bytes.len() && bytes[j] == b'#' {
        hashes += 1;
        j += 1;
    }
    if j < bytes.len() && bytes[j] == b'"' { Some((hashes, j)) } else { None }
}

fn skip_raw_string(bytes: &[u8], open: usize, hashes: usize) -> usize {
    let mut j = open + 1;
    while j < bytes.len() {
        if bytes[j] == b'"' {
            let mut k = j + 1;
            let mut seen = 0usize;
            while k < bytes.len() && bytes[k] == b'#' && seen < hashes {
                seen += 1;
                k += 1;
            }
            if seen == hashes {
                return k;
            }
        }
        j += 1;
    }
    bytes.len()
}

fn dependency_key_to_rust_path(key: &str) -> String {
    key.replace('-', "_")
}

fn dependency_spec_package_name(key: &str, spec: &toml::Value) -> Option<String> {
    match spec {
        toml::Value::String(_) | toml::Value::Integer(_) | toml::Value::Boolean(_) => {
            Some(key.replace('_', "-"))
        }
        toml::Value::Table(table) => {
            if let Some(package) = table.get("package").and_then(|value| value.as_str()) {
                return Some(package.to_string());
            }
            Some(key.replace('_', "-"))
        }
        _ => None,
    }
}

fn dependency_keys_targeting_package(
    manifest_source: &str,
    package: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let manifest: toml::Value = toml::from_str(manifest_source)?;
    let mut keys = BTreeSet::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let Some(deps) = manifest.get(section).and_then(|value| value.as_table()) else {
            continue;
        };
        for (key, spec) in deps {
            let Some(resolved) = dependency_spec_package_name(key, spec) else {
                continue;
            };
            if resolved == package {
                keys.insert(dependency_key_to_rust_path(key));
            }
        }
    }
    Ok(keys.into_iter().collect())
}

fn lower_tier_crate_paths_for_crate(
    crate_dir: &Path,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let manifest = read(crate_dir.join("Cargo.toml"))?;
    dependency_keys_targeting_package(&manifest, "perl-parser-core")
}

fn lower_tier_paths_for_source_scanning(crate_paths: &[String]) -> Vec<String> {
    let mut paths = crate_paths.to_vec();
    if !paths.iter().any(|path| path == "perl_parser_core") {
        paths.push("perl_parser_core".to_string());
    }
    paths.sort();
    paths.dedup();
    paths
}

fn public_incremental_modules(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("pub mod ")
                .and_then(|rest| rest.strip_suffix(';'))
                .filter(|module| module.starts_with("incremental_"))
                .map(str::to_owned)
        })
        .collect()
}

fn facade_incremental_reexports(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("pub use incremental::")
                .and_then(|rest| rest.strip_suffix(';'))
                .filter(|module| module.starts_with("incremental_"))
                .map(str::to_owned)
        })
        .collect()
}

fn rust_source_files(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file()
                && path.extension().is_some_and(|extension| extension == OsStr::new("rs"))
            {
                files.push(path);
            }
        }
    }

    files.sort();
    Ok(files)
}

fn production_rust_sources() -> Result<Vec<(PathBuf, PathBuf)>, Box<dyn std::error::Error>> {
    let crates_root = workspace_root().join("crates");
    let mut crate_entries = fs::read_dir(&crates_root)?.collect::<Result<Vec<_>, _>>()?;
    crate_entries.sort_by_key(|entry| entry.file_name());

    let mut sources = Vec::new();
    for entry in crate_entries {
        if !entry.file_type()?.is_dir() || entry.file_name() == OsStr::new("perl-parser-core") {
            continue;
        }

        let crate_dir = entry.path();
        let src = crate_dir.join("src");
        if src.is_dir() {
            for path in rust_source_files(&src)? {
                sources.push((path, crate_dir.clone()));
            }
        }
    }
    sources.sort_by(|(left, _), (right, _)| left.cmp(right));
    Ok(sources)
}

fn uses_lower_tier_incremental(source: &str, crate_paths: &[String]) -> bool {
    let compact = compact_whitespace(&strip_comments_and_string_literals(source));
    crate_paths.iter().any(|crate_path| lower_tier_usage_in_compact(&compact, crate_path))
}

fn lower_tier_usage_in_compact(compact: &str, crate_path: &str) -> bool {
    let direct = format!("{crate_path}::incremental");
    if compact.contains(&direct) {
        return true;
    }

    let braced_marker = format!("{crate_path}::{{");
    let mut cursor = 0;
    while let Some(offset) = compact[cursor..].find(&braced_marker) {
        let statement = &compact[cursor + offset..];
        let imports = &statement[..statement.find(';').unwrap_or(statement.len())];
        if imports.contains("incremental::{")
            || imports.contains("incremental,")
            || imports.contains("incremental}")
            || imports.contains("incrementalas")
        {
            return true;
        }
        cursor += offset + braced_marker.len();
    }

    let use_as = format!("use{crate_path}as");
    let extern_as = format!("externcrate{crate_path}as");
    compact.split(';').any(|statement| {
        [use_as.as_str(), extern_as.as_str()].iter().any(|prefix| {
            statement.strip_prefix(prefix).is_some_and(|remainder| {
                let alias = remainder.split([':', '{', ',', '}']).next().unwrap_or_default();
                !alias.is_empty() && compact.contains(&format!("{alias}::incremental"))
            })
        })
    })
}

fn normalized_workspace_path(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let relative = path.strip_prefix(workspace_root())?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn discovered_lower_tier_consumers() -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let mut consumers = BTreeSet::new();
    let mut crate_paths_cache = BTreeMap::<PathBuf, Vec<String>>::new();
    for (path, crate_dir) in production_rust_sources()? {
        let crate_paths = crate_paths_cache.entry(crate_dir.clone()).or_insert_with(|| {
            lower_tier_crate_paths_for_crate(&crate_dir)
                .map(|paths| lower_tier_paths_for_source_scanning(&paths))
                .unwrap_or_else(|_| vec!["perl_parser_core".to_string()])
        });
        let source = read(&path)?;
        if uses_lower_tier_incremental(&source, crate_paths) {
            consumers.insert(normalized_workspace_path(&path)?);
        }
    }
    Ok(consumers)
}

fn declared_consumers_by_source(
    consumers: &[AllowedConsumer],
) -> BTreeMap<String, Vec<&AllowedConsumer>> {
    let mut by_source = BTreeMap::<String, Vec<&AllowedConsumer>>::new();
    for consumer in consumers {
        by_source.entry(consumer.source_path.clone()).or_default().push(consumer);
    }
    by_source
}

fn public_function_anchor(symbol: &str) -> String {
    let function = symbol.rsplit("::").next().unwrap_or(symbol);
    format!("pubfn{function}(")
}

fn lower_tier_reparse_call_count(source: &str) -> usize {
    compact_whitespace(&strip_comments_and_string_literals(source))
        .match_indices(".reparse(")
        .count()
}

fn default_lower_tier_paths() -> Vec<String> {
    vec!["perl_parser_core".to_string()]
}

#[test]
fn ledger_names_one_canonical_candidate_without_claiming_readiness() -> TestResult {
    let manifest = load_manifest()?;

    assert_eq!(manifest.schema_version, 3);
    assert_eq!(manifest.owner_issue, 6701);
    assert_eq!(manifest.canonical.module, "incremental");
    assert_eq!(manifest.canonical.path, "perl_parser::incremental");
    assert_eq!(
        manifest.canonical.entry_points.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "Edit".to_string(),
            "IncrementalState".to_string(),
            "apply_edits".to_string(),
        ])
    );
    assert!(
        !manifest.canonical.production_eligible,
        "canonical ownership must not be mistaken for production readiness"
    );
    assert_eq!(
        manifest.canonical.eligibility_blockers.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([2327, 6704, 6710, 6714])
    );
    assert!(!manifest.canonical.decision.trim().is_empty());

    assert_eq!(manifest.compatibility.package, "perl-incremental-parsing");
    assert_eq!(manifest.compatibility.status, "compatibility");
    assert_eq!(manifest.compatibility.implementation_owner, "perl-parser");
    assert!(!manifest.compatibility.behavior_authority);
    assert_eq!(manifest.compatibility.exit_issue, 6701);

    Ok(())
}

#[test]
fn every_public_incremental_generation_has_one_non_production_disposition() -> TestResult {
    let manifest = load_manifest()?;
    let module_source = read(crate_root().join("src/incremental/mod.rs"))?;
    let facade_source = read(crate_root().join("src/lib.rs"))?;

    let mut classified = BTreeMap::new();
    for surface in &manifest.modules {
        assert!(
            classified.insert(surface.module.clone(), surface).is_none(),
            "duplicate incremental authority entry: {}",
            surface.module
        );
        assert!(
            matches!(surface.status.as_str(), "experimental" | "internal" | "retire"),
            "unsupported incremental authority status for {}: {}",
            surface.module,
            surface.status
        );
        assert!(
            !surface.production_eligible,
            "non-canonical generation {} cannot be production-eligible",
            surface.module
        );
        assert!(!surface.next_action.trim().is_empty());
        assert!(surface.owner_issue > 0);
    }

    let declared = public_incremental_modules(&module_source);
    let classified_names = classified.keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(declared, classified_names);

    let facade_reexports = facade_incremental_reexports(&facade_source);
    let manifest_reexports = manifest
        .modules
        .iter()
        .filter(|surface| surface.public_reexport)
        .map(|surface| surface.module.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(facade_reexports, manifest_reexports);

    let retired_handler = classified
        .get("incremental_handler_v2")
        .ok_or("incremental_handler_v2 is missing from the authority ledger")?;
    assert_eq!(retired_handler.status, "retire");

    let compact_facade = compact_whitespace(&facade_source);
    assert!(
        compact_facade.contains("pubuseincremental::{Edit,IncrementalState,apply_edits};"),
        "the canonical state/edit/apply_edits facade re-export is missing"
    );

    Ok(())
}

#[test]
fn lower_tier_consumer_detector_covers_direct_nested_and_aliased_imports() {
    let paths = default_lower_tier_paths();
    assert!(uses_lower_tier_incremental(
        "use perl_parser_core::incremental::IncrementalState;",
        &paths
    ));
    assert!(uses_lower_tier_incremental(
        "use perl_parser_core::{ParseOutput, incremental::{IncrementalEdit, IncrementalState}};",
        &paths
    ));
    assert!(uses_lower_tier_incremental(
        "use perl_parser_core::{incremental as core_incremental, ParseOutput};",
        &paths
    ));
    assert!(uses_lower_tier_incremental(
        "use perl_parser_core as core; core::incremental::IncrementalState;",
        &paths
    ));
    assert!(uses_lower_tier_incremental(
        "extern crate perl_parser_core as core; core::incremental::IncrementalState;",
        &paths
    ));
    assert!(!uses_lower_tier_incremental("use perl_parser_core::{ParseOutput, Parser};", &paths));
    // Prose mentioning the word "incremental" must not classify a file that
    // only imports sibling modules as a kernel consumer: a zero-reparse
    // consumer cannot be declared in the per-call-site allowlist.
    assert!(!uses_lower_tier_incremental(
        "use perl_parser_core::{ast::Node, edit::Edit};\n// reparse the tree with incremental, quoted text",
        &paths
    ));
    assert!(!uses_lower_tier_incremental(
        "// use perl_parser_core::incremental::IncrementalState;",
        &paths
    ));
    assert!(!uses_lower_tier_incremental(
        "const _: &str = \"perl_parser_core::incremental::IncrementalState\";",
        &paths
    ));
    assert!(uses_lower_tier_incremental("use perl_parser_core::{ast::Node, incremental};", &paths));
    assert!(uses_lower_tier_incremental(
        "use perl_parser_core::{ast::Node};\nuse perl_parser_core::{incremental::{IncrementalState}};",
        &paths
    ));
}

#[test]
fn lower_tier_consumer_detector_honors_cargo_dependency_renames() -> TestResult {
    let fixture = crate_root().join("tests/fixtures/incremental_authority_renamed_dep/Cargo.toml");
    let keys = dependency_keys_targeting_package(&read(fixture)?, "perl-parser-core")?;
    assert_eq!(keys, vec!["core_parser".to_string()]);

    let renamed_paths = lower_tier_paths_for_source_scanning(&keys);
    assert!(uses_lower_tier_incremental(
        "use core_parser::incremental::IncrementalState;",
        &renamed_paths
    ));
    assert!(uses_lower_tier_incremental(
        "use core_parser as kernel; kernel::incremental::IncrementalState;",
        &renamed_paths
    ));
    Ok(())
}

#[test]
fn lower_tier_callsite_counter_detects_multiple_consumers_in_one_file() {
    let source = "pub fn first() { state.reparse(source, edit); }\n\
                  pub fn second() { other.reparse(source, edit); }";
    assert_eq!(lower_tier_reparse_call_count(source), 2);
}

#[test]
fn active_lower_tier_kernel_and_consumer_are_explicitly_classified() -> TestResult {
    let manifest = load_manifest()?;
    assert_eq!(
        manifest.lower_tier.len(),
        1,
        "every active lower-tier incremental implementation needs one disposition"
    );

    let kernel = &manifest.lower_tier[0];
    assert_eq!(kernel.package, "perl-parser-core");
    assert_eq!(kernel.module, "incremental");
    assert_eq!(kernel.path, "perl_parser_core::incremental");
    assert_eq!(kernel.status, "lower_tier_kernel");
    assert_eq!(
        kernel.entry_points.iter().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "IncrementalEdit".to_string(),
            "IncrementalState".to_string(),
            "IncrementalState::reparse".to_string(),
        ])
    );
    assert!(!kernel.behavior_authority);
    assert!(!kernel.production_eligible);
    assert!(!kernel.next_action.trim().is_empty());
    assert_eq!(kernel.owner_issue, 6707);
    assert!(!kernel.decision.trim().is_empty());

    let declared_by_source = declared_consumers_by_source(&kernel.allowed_consumers);
    let allowed_sources = declared_by_source.keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        discovered_lower_tier_consumers()?,
        allowed_sources,
        "every production source consumer of perl_parser_core::incremental must be allowlisted"
    );

    let mut declared_symbols = BTreeSet::new();
    for consumer in &kernel.allowed_consumers {
        assert!(
            declared_symbols.insert(consumer.symbol.clone()),
            "duplicate lower-tier consumer symbol: {}",
            consumer.symbol
        );
        assert!(!consumer.symbol.trim().is_empty());
        assert!(!consumer.source_path.trim().is_empty());
    }

    for (source_path, consumers) in &declared_by_source {
        let source = read(workspace_root().join(source_path))?;
        let compact = compact_whitespace(&source);
        assert_eq!(
            lower_tier_reparse_call_count(&source),
            consumers.len(),
            "lower-tier reparse call sites in {source_path} must match declared consumer symbols"
        );
        for consumer in consumers {
            let anchor = public_function_anchor(&consumer.symbol);
            assert_eq!(
                compact.match_indices(&anchor).count(),
                1,
                "declared lower-tier consumer {} must have exactly one public function anchor in {}",
                consumer.symbol,
                consumer.source_path
            );
        }
    }

    let consumer = kernel
        .allowed_consumers
        .iter()
        .find(|consumer| consumer.symbol == "tree_sitter_perl_rs::Parser::parse_with_old_tree")
        .ok_or("the tree-sitter lower-tier consumer is missing from the authority ledger")?;
    assert_eq!(consumer.source_path, "crates/tree-sitter-perl-rs/src/parser.rs");

    let core_facade =
        compact_whitespace(&read(crate_root().join("../perl-parser-core/src/lib.rs"))?);
    let kernel_source =
        compact_whitespace(&read(crate_root().join("../perl-parser-core/src/incremental.rs"))?);

    assert!(
        core_facade.contains("pubmodincremental;"),
        "the classified lower-tier kernel is no longer publicly exported"
    );
    assert!(kernel_source.contains("pubstructIncrementalEdit"));
    assert!(kernel_source.contains("pubstructIncrementalState"));
    assert!(
        kernel_source.contains("pubfnreparse(&mutself,new_source:&str,edit:&IncrementalEdit)"),
        "the classified lower-tier reparse entry point changed"
    );

    Ok(())
}

#[test]
fn compatibility_crate_forwards_the_canonical_implementation() -> TestResult {
    let manifest = load_manifest()?;
    let compatibility_source = read(crate_root().join("../perl-incremental-parsing/src/lib.rs"))?;
    let compact = compact_whitespace(&compatibility_source);

    assert!(!manifest.compatibility.behavior_authority);
    assert!(
        compact.contains("pubuseperl_parser::incremental;"),
        "compatibility crate must forward the canonical incremental module"
    );
    assert!(
        compact.contains("pubuseperl_parser::incremental::*;"),
        "compatibility crate must forward the canonical incremental exports"
    );
    assert!(
        !compact.contains("modincremental"),
        "compatibility crate must not define a second incremental implementation"
    );

    Ok(())
}
