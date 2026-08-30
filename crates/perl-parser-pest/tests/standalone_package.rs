//! Standalone-resolution contract for the `perl-parser-pest` package (`#8771`).
//!
//! These tests own the claim that this package describes and tests itself
//! without borrowing identity, versions, dependencies, or test helpers from
//! the swarm workspace. `[lints]` is the one deliberate exception, retained
//! because a required gate demands it (`#13775`) and pinned here so it cannot
//! spread. They are structural assertions over the package's own manifest and
//! source tree; they do not invoke Cargo, reach the network, verify registry
//! presence, or claim that an unpacked copy has been executed in isolation
//! (that is the next train row's claim).
//!
//! Every assertion here fails closed: a `workspace = true` marker on any key
//! but `[lints]`, a path-only dependency, a dropped `[lints]` marker or a
//! local lint table appearing beside it, a falsely-external repository URL, an
//! unpackaged load-bearing asset, or a returning `perl_tdd_support` import is
//! a test failure, not a warning.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use toml::Value;

/// The lineage this package is still honestly owned by. The external
/// `perl-parser-pest` repository does not exist yet; the manifest must not
/// claim it.
const CURRENT_LINEAGE: &str = "https://github.com/EffortlessMetrics/perl-lsp";

/// Package keys that standalone resolution requires to be literal.
const REQUIRED_LITERAL_PACKAGE_KEYS: &[&str] = &[
    "name",
    "version",
    "edition",
    "rust-version",
    "authors",
    "license",
    "repository",
    "homepage",
    "description",
    "readme",
    "keywords",
    "categories",
];

/// Assets that must travel with the package for the library, the Pest grammar
/// derive, the public example, and the package-local proof population to work.
const REQUIRED_PACKAGED_ASSETS: &[&str] = &[
    "Cargo.toml",
    "README.md",
    "LICENSE-APACHE",
    "LICENSE-MIT",
    "src/lib.rs",
    "src/grammar.pest",
    "src/pure_rust_parser.rs",
    "examples/parse_basic.rs",
    "tests/fixtures/manifest.toml",
    "tests/fixture_manifest.rs",
    "tests/support/mod.rs",
    "tests/support/assert.rs",
    "tests/standalone_package.rs",
];

/// Swarm-only test helpers that must not be reachable from the packaged tree.
const FORBIDDEN_SWARM_IMPORTS: &[&str] = &["perl_tdd_support", "perl_test_must"];

fn package_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn parse_manifest(text: &str) -> Result<Value, Box<dyn Error>> {
    // `toml::Value`'s own `FromStr` parses a bare value, not a document, so the
    // document root is deserialized as a table and then wrapped.
    Ok(Value::Table(toml::from_str::<toml::Table>(text)?))
}

fn manifest() -> Result<Value, Box<dyn Error>> {
    let text = fs::read_to_string(package_root().join("Cargo.toml"))?;
    parse_manifest(&text)
}

/// Collect every dotted path at which a `workspace = true` inheritance marker
/// appears. `version.workspace = true`, `regex.workspace = true`, and a bare
/// `[lints] workspace = true` all parse into a table carrying a `workspace`
/// key, so one traversal catches every spelling.
///
/// The walk descends through arrays of tables (`[[bin]]`, `[[example]]`, …) as
/// well as tables, so no manifest shape is silently exempt from the check.
fn workspace_inherited_paths(value: &Value, prefix: &str, found: &mut Vec<String>) {
    if let Some(items) = value.as_array() {
        for (index, item) in items.iter().enumerate() {
            workspace_inherited_paths(item, &format!("{prefix}[{index}]"), found);
        }
        return;
    }
    let Some(table) = value.as_table() else {
        return;
    };
    if table.get("workspace").and_then(Value::as_bool) == Some(true) {
        found.push(if prefix.is_empty() { "workspace".to_string() } else { prefix.to_string() });
    }
    for (key, child) in table {
        let path =
            if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}", key = key) };
        workspace_inherited_paths(child, &path, found);
    }
}

fn inherited_paths(value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    workspace_inherited_paths(value, "", &mut found);
    found.sort();
    found
}

const DEPENDENCY_SECTIONS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// Every dependency table Cargo resolves for this package: the three top-level
/// sections plus their `[target.<cfg>.…]` counterparts.
///
/// Target-specific tables are included because a `path` dependency declared
/// under `[target.'cfg(unix)'.dependencies]` resolves exactly like a top-level
/// one, so omitting them would leave a silent hole in the standalone ratchet
/// this file exists to hold (#8771).
fn dependency_tables(manifest: &Value) -> Vec<(String, &toml::map::Map<String, Value>)> {
    let mut tables: Vec<(String, &toml::map::Map<String, Value>)> = DEPENDENCY_SECTIONS
        .into_iter()
        .filter_map(|section| Some((section.to_string(), manifest.get(section)?.as_table()?)))
        .collect();

    let Some(targets) = manifest.get("target").and_then(Value::as_table) else {
        return tables;
    };
    for (target, spec) in targets {
        let Some(spec) = spec.as_table() else {
            continue;
        };
        for section in DEPENDENCY_SECTIONS {
            let Some(table) = spec.get(section).and_then(Value::as_table) else {
                continue;
            };
            tables.push((format!("target.{target}.{section}"), table));
        }
    }
    tables
}

/// Minimal matcher for the `include` patterns this manifest actually uses:
/// a literal path, or a `dir/**` prefix.
fn include_pattern_covers(pattern: &str, relative: &str) -> bool {
    match pattern.strip_suffix("/**") {
        Some(prefix) => relative.starts_with(&format!("{prefix}/")),
        None => pattern == relative,
    }
}

fn rust_sources(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_sources(&path, out)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

#[test]
fn lints_is_the_only_workspace_inheritance_left() -> Result<(), Box<dyn Error>> {
    // `[lints] workspace = true` is required of every workspace member by
    // `cargo xtask check-lint-policy`, a required gate with no exemption
    // mechanism, so the lint half of #8771 cannot land while this crate is a
    // workspace member. Everything else must be literal.
    //
    // Asserting the exact set, rather than "no inheritance except lints",
    // keeps the exception from spreading: a second inherited key fails here
    // even though it is also `workspace = true`.
    let inherited = inherited_paths(&manifest()?);

    assert_eq!(
        inherited,
        vec!["lints".to_string()],
        "`[lints]` is the single permitted inheritance; every other key must be literal for \
         standalone resolution"
    );
    Ok(())
}

#[test]
fn inheritance_detector_reports_a_workspace_inherited_manifest() -> Result<(), Box<dyn Error>> {
    // Negative control: the detector above must actually discriminate. This is
    // the shape of the manifest before #8771 in all three spellings, plus an
    // array-of-tables entry proving the walk is not table-only.
    let before = r#"
[package]
name = "perl-parser-pest"
version.workspace = true

[dependencies]
regex.workspace = true

[lints]
workspace = true

[[example]]
name = "parse_basic"
required-features.workspace = true
"#;
    let inherited = inherited_paths(&parse_manifest(before)?);

    assert_eq!(
        inherited,
        vec![
            "dependencies.regex".to_string(),
            "example[0].required-features".to_string(),
            "lints".to_string(),
            "package.version".to_string(),
        ],
        "the detector must name every inherited key, or the positive test is vacuous"
    );
    Ok(())
}

#[test]
fn package_identity_keys_are_literal() -> Result<(), Box<dyn Error>> {
    let manifest = manifest()?;
    let package = manifest
        .get("package")
        .and_then(Value::as_table)
        .ok_or("manifest has no [package] table")?;

    for key in REQUIRED_LITERAL_PACKAGE_KEYS {
        let value = package.get(*key).ok_or_else(|| format!("[package] is missing `{key}`"))?;
        assert!(
            value.is_str() || value.is_array(),
            "[package] `{key}` must be a literal value for standalone resolution; got {value:?}"
        );
    }
    Ok(())
}

#[test]
fn every_dependency_is_versioned_and_path_free() -> Result<(), Box<dyn Error>> {
    let manifest = manifest()?;

    for (section, table) in dependency_tables(&manifest) {
        for (name, spec) in table {
            let versioned = match spec {
                Value::String(_) => true,
                Value::Table(detail) => detail.get("version").is_some_and(Value::is_str),
                _ => false,
            };
            assert!(versioned, "[{section}] `{name}` must declare an explicit version");

            let path_only = spec.as_table().is_some_and(|detail| detail.contains_key("path"));
            assert!(
                !path_only,
                "[{section}] `{name}` must not declare a path dependency; standalone resolution \
                 must use registry versions"
            );
        }
    }
    Ok(())
}

/// The path-free ratchet above is only as wide as the tables it walks, and a
/// manifest that never uses `[target.<cfg>.…]` cannot demonstrate that reach.
/// This drives the collector with a synthetic manifest that hides a path
/// dependency in a target section: before target tables were collected the
/// dependency was invisible here, so this fails closed on that regression
/// rather than waiting for a real one to be introduced.
#[test]
fn target_specific_path_dependencies_are_not_invisible() -> Result<(), Box<dyn Error>> {
    let manifest: Value = toml::from_str(
        r#"
        [dependencies]
        pest = { version = "2.7" }

        [target.'cfg(unix)'.dev-dependencies]
        sneaky = { path = "../sneaky" }
        "#,
    )?;

    let tables = dependency_tables(&manifest);
    let (section, table) = tables
        .iter()
        .find(|(section, _)| section.contains("target."))
        .ok_or("dependency_tables must collect [target.<cfg>.…] sections")?;
    assert_eq!(section, "target.cfg(unix).dev-dependencies");

    let spec = table.get("sneaky").ok_or("target section must expose its dependencies")?;
    assert!(
        spec.as_table().is_some_and(|detail| detail.contains_key("path")),
        "the target-section path dependency must reach the path-free assertion"
    );
    Ok(())
}

#[test]
fn dependency_feature_surface_stays_bounded() -> Result<(), Box<dyn Error>> {
    let manifest = manifest()?;
    let features = manifest
        .get("features")
        .and_then(Value::as_table)
        .ok_or("manifest has no [features] table")?;

    let mut names: Vec<&str> = features.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["default", "serde"],
        "the package feature surface is frozen; `serde` remains a no-op compatibility alias"
    );

    let default = features.get("default").and_then(Value::as_array).ok_or("no default feature")?;
    assert!(default.is_empty(), "`default` must stay empty; got {default:?}");
    Ok(())
}

#[test]
fn lint_policy_still_satisfies_the_required_workspace_invariant() -> Result<(), Box<dyn Error>> {
    // `cargo xtask check-lint-policy` (required) bails with
    // "workspace members missing [lints] workspace = true" for any member that
    // declares its own table. Dropping this marker to finish #8771's lint
    // decoupling turns that gate red, so the marker is pinned here and the
    // residual is tracked rather than silently attempted again.
    let manifest = manifest()?;
    let lints = manifest.get("lints").ok_or("manifest has no [lints] table")?;

    assert_eq!(
        lints.get("workspace").and_then(Value::as_bool),
        Some(true),
        "[lints] workspace = true is required of every workspace member"
    );
    assert!(
        lints.get("clippy").is_none() && lints.get("rust").is_none(),
        "a local [lints.clippy]/[lints.rust] table cannot coexist with the required inheritance"
    );
    Ok(())
}

#[test]
fn migration_metadata_is_not_falsely_external() -> Result<(), Box<dyn Error>> {
    let manifest = manifest()?;
    let package = manifest
        .get("package")
        .and_then(Value::as_table)
        .ok_or("manifest has no [package] table")?;

    for key in ["repository", "homepage"] {
        let value = package.get(key).and_then(Value::as_str).ok_or_else(|| format!("no {key}"))?;
        assert_eq!(
            value, CURRENT_LINEAGE,
            "`{key}` must name the current lineage until the external repository exists"
        );
    }

    let extraction = package
        .get("metadata")
        .and_then(|metadata| metadata.get("extraction"))
        .and_then(Value::as_table)
        .ok_or("the pending external owner must be recorded under [package.metadata.extraction]")?;
    assert_eq!(
        extraction.get("status").and_then(Value::as_str),
        Some("pending"),
        "extraction status must stay `pending` while the move has not happened"
    );
    Ok(())
}

#[test]
fn required_assets_exist_and_are_packaged() -> Result<(), Box<dyn Error>> {
    let manifest = manifest()?;
    let include: Vec<&str> = manifest
        .get("package")
        .and_then(|package| package.get("include"))
        .and_then(Value::as_array)
        .ok_or("manifest has no [package] include list")?
        .iter()
        .filter_map(Value::as_str)
        .collect();

    let root = package_root();
    for asset in REQUIRED_PACKAGED_ASSETS {
        assert!(root.join(asset).exists(), "declared package asset `{asset}` does not exist");
        assert!(
            include.iter().any(|pattern| include_pattern_covers(pattern, asset)),
            "declared package asset `{asset}` is not covered by the include list {include:?}"
        );
    }

    for pattern in &include {
        assert!(
            !pattern.starts_with("..") && !pattern.starts_with('/'),
            "include pattern `{pattern}` reaches outside the package tree"
        );
    }
    Ok(())
}

#[test]
fn include_matcher_rejects_uncovered_paths() -> Result<(), Box<dyn Error>> {
    // Negative control for the matcher used above.
    assert!(include_pattern_covers("src/**", "src/lib.rs"));
    assert!(include_pattern_covers("Cargo.toml", "Cargo.toml"));
    assert!(!include_pattern_covers("src/**", "examples/parse_basic.rs"));
    assert!(!include_pattern_covers("src/**", "src"));
    assert!(!include_pattern_covers("Cargo.toml", "Cargo.lock"));
    Ok(())
}

/// Build artifacts that are never published and carry no source.
const UNPUBLISHED_ROOT_ARTIFACTS: &[&str] = &["target", "Cargo.lock"];

/// No top-level entry may sit outside the `include` list.
///
/// `required_assets_exist_and_are_packaged` walks an enumerated list, so it
/// proves those assets ship but cannot notice a *new* one. Since `include`
/// covers whole subtrees (`src/**`, `tests/**`, `examples/**`) plus named root
/// files, the reachable gap is a new top-level entry — a `build.rs`, a
/// `benches/` — that would be silently absent from the published package while
/// every enumerated assertion still passed.
///
/// This closes that gap, so the crate's "no unpackaged load-bearing asset"
/// claim is a completeness check at the root rather than a spot-check.
#[test]
fn no_top_level_entry_is_left_unpackaged() -> Result<(), Box<dyn Error>> {
    let manifest = manifest()?;
    let include: Vec<&str> = manifest
        .get("package")
        .and_then(|package| package.get("include"))
        .and_then(Value::as_array)
        .ok_or("manifest has no [package] include list")?
        .iter()
        .filter_map(Value::as_str)
        .collect();

    for entry in fs::read_dir(package_root())? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || UNPUBLISHED_ROOT_ARTIFACTS.contains(&name.as_str()) {
            continue;
        }

        // `include_pattern_covers` deliberately reports `src/**` as not covering
        // the bare directory `src`, so directories match on the pattern prefix.
        let covered = if entry.file_type()?.is_dir() {
            include
                .iter()
                .any(|pattern| pattern.strip_suffix("/**").is_some_and(|prefix| prefix == name))
        } else {
            include.iter().any(|pattern| include_pattern_covers(pattern, &name))
        };

        assert!(
            covered,
            "top-level `{name}` is not covered by the include list {include:?}; it would be \
             missing from the published package"
        );
    }
    Ok(())
}

#[test]
fn no_source_reaches_for_a_swarm_test_helper() -> Result<(), Box<dyn Error>> {
    let root = package_root();
    let mut sources = Vec::new();
    for directory in ["src", "tests", "examples"] {
        rust_sources(&root.join(directory), &mut sources)?;
    }
    assert!(!sources.is_empty(), "found no Rust sources to scan");

    for path in sources {
        let text = fs::read_to_string(&path)?;
        for forbidden in FORBIDDEN_SWARM_IMPORTS {
            assert!(
                !text.contains(&format!("use {forbidden}")),
                "{path} imports `{forbidden}`; packaged tests must not require a swarm crate",
                path = path.display()
            );
        }
    }
    Ok(())
}
