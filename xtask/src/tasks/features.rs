use color_eyre::eyre::{Context, Result, bail, eyre};
use perl_lsp_rs_core::feature_catalog::{Catalog, Maturity};
use perl_lsp_rs_core::governance::{FeatureProfile, catalog_advertised_feature_ids};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// Public API functions called from main.rs
pub fn sync_docs() -> Result<()> {
    sync_docs_impl()
}

pub fn verify() -> Result<()> {
    verify_features()
}

pub fn report() -> Result<()> {
    generate_report()
}

pub fn invariants() -> Result<()> {
    check_invariants()
}

fn load_features() -> Result<Catalog> {
    let manifest_dir = env::current_dir().context("Failed to get current working directory")?;
    let (catalog, _) = perl_lsp_rs_core::feature_catalog::load_catalog_for_build(&manifest_dir)
        .context("Failed to load features catalog from features.toml")?;
    Ok(catalog)
}

fn repo_relative_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() { path.to_path_buf() } else { PathBuf::from(path) }
}

fn lsp_feature_snapshot_path() -> PathBuf {
    repo_relative_path(
        "crates/perl-lsp-rs/tests/snapshots/lsp_features_snapshot_test__advertised_vs_caps.snap",
    )
}

/// Crate-local vendored catalog projections generated from the root authority
/// (#7029).
pub const VENDORED_PROJECTIONS: &[&str] = &[
    "crates/perl-lsp-rs/features_sot.toml",
    "crates/perl-lsp-rs-core/features_sot.toml",
    "crates/perl-parser/features_sot.toml",
    "crates/perl-dap/features_sot.toml",
];

/// Regenerate every crate-local `features_sot.toml` as a byte projection of
/// the root authority (#7029). The copies are deterministic: the same root
/// file always produces the same bytes.
pub fn regen_vendored() -> Result<()> {
    println!("♻️  Regenerating vendored feature-catalog projections from features.toml...");

    let authority = fs::read("features.toml").context("reading root features.toml")?;
    for relative in VENDORED_PROJECTIONS {
        let path = repo_relative_path(relative);
        let previous = match fs::read(&path) {
            Ok(bytes) => bytes,
            // A missing projection simply needs generating; any other read
            // failure must not silently masquerade as drift (#7029).
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(error).context(format!("reading current projection {relative}"));
            }
        };
        if previous != authority {
            fs::write(&path, &authority)
                .with_context(|| format!("writing projection {relative}"))?;
            println!("  updated {relative}");
        } else {
            println!("  up-to-date {relative}");
        }
    }
    println!("✅ Vendored projections regenerated.");
    Ok(())
}

/// Fail when any vendored projection drifts from the root authority (#7029).
/// Runs as part of `features invariants` so CI catches drift without a test
/// harness.
fn check_vendored_projection_drift(violations: &mut Vec<String>) {
    let Ok(authority) = fs::read("features.toml") else {
        violations.push("DRIFT_READ: cannot read root features.toml".to_string());
        return;
    };
    for relative in VENDORED_PROJECTIONS {
        match fs::read(repo_relative_path(relative)) {
            Ok(bytes) if bytes == authority => {}
            Ok(_) => violations.push(format!(
                "VENDORED_DRIFT: {relative} differs from root features.toml (#7029); \
                 run `cargo xtask features regen-vendored`"
            )),
            Err(error) => violations.push(format!("DRIFT_READ: cannot read {relative}: {error}")),
        }
    }
}

fn snapshot_comparable_feature_ids() -> BTreeSet<String> {
    let mut ids: BTreeSet<String> =
        catalog_advertised_feature_ids(FeatureProfile::All).into_iter().map(String::from).collect();

    // The capability snapshot intentionally excludes formatting because
    // initialize capabilities depend on runtime perltidy availability.
    ids.remove("lsp.formatting");
    ids.remove("lsp.range_formatting");
    ids.remove("lsp.ranges_formatting");

    // These features are advertised and tested, but they do not currently round-trip
    // through the typed `ServerCapabilities` snapshot used here:
    // - `lsp.type_hierarchy` is injected as a raw top-level JSON field because
    //   `lsp-types` 0.97 lacks the field.
    // - `lsp.inline_completion` is served via proposed/experimental plumbing rather
    //   than a stable `ServerCapabilities` field in this stack.
    ids.remove("lsp.type_hierarchy");
    ids.remove("lsp.inline_completion");

    ids
}

fn check_invariants() -> Result<()> {
    println!("🔍 Checking features.toml invariants...");

    let catalog = load_features()?;
    let mut violations = Vec::new();
    let mut seen_ids = BTreeSet::new();

    for feature in catalog.features() {
        if !seen_ids.insert(feature.id.clone()) {
            violations.push(format!("DUPLICATE_ID: {:?} appears more than once", feature.id));
        }

        if feature.advertised && !feature.maturity.may_advertise() {
            violations.push(format!(
                "UNADVERTISABLE: {:?} is advertised but maturity {:?} can never advertise (#7029)",
                feature.id,
                feature.maturity.label()
            ));
        }

        if matches!(feature.maturity, Maturity::Proven) {
            if feature.evidence.is_empty() {
                violations.push(format!(
                    "PROVEN_WITHOUT_EVIDENCE: {:?} claims proven with no evidence entries (#7029)",
                    feature.id
                ));
            }
            for (field, value) in [
                ("direction", &feature.direction),
                ("capability_gate", &feature.capability_gate),
                ("registration", &feature.registration),
                ("implementation_owner", &feature.implementation_owner),
                ("state_owner", &feature.state_owner),
            ] {
                if value.is_empty() || value == "missing" {
                    violations.push(format!(
                        "PROVEN_METADATA_MISSING: {:?} records {field} as missing but claims \
                         proven (#7029)",
                        feature.id
                    ));
                }
            }
            if feature.claim_boundary.trim().is_empty() {
                violations.push(format!(
                    "PROVEN_BOUNDARY_MISSING: {:?} claims proven without a claim boundary \
                     (#7029)",
                    feature.id
                ));
            }
        } else if feature.advertised && feature.counts_in_coverage && feature.tests.is_empty() {
            violations.push(format!(
                "UNTESTED_ADVERTISED: {:?} is advertised and counted in coverage but has no \
                 named tests. Either add tests or set counts_in_coverage=false.",
                feature.id
            ));
        }
    }

    check_vendored_projection_drift(&mut violations);

    if !violations.is_empty() {
        let violations_count = violations.len();
        println!("FEATURE INVARIANT VIOLATIONS:");
        println!("{}", "=".repeat(50));
        for violation in violations {
            println!("  - {violation}");
        }
        println!("{}", "=".repeat(50));
        println!("{violations_count} violation(s) found.");
        bail!("feature invariants check failed");
    }

    let total = catalog.features().len();
    let proven = catalog.features().iter().filter(|f| f.maturity == Maturity::Proven).count();
    let not_proven =
        catalog.features().iter().filter(|f| f.maturity == Maturity::NotProven).count();
    let advertised = catalog.advertised_feature_ids().len();

    println!(
        "Feature invariants OK: {total} features, {proven} proven, {not_proven} not_proven, \
         {advertised} advertised"
    );
    Ok(())
}

fn sync_docs_impl() -> Result<()> {
    println!("📝 Syncing documentation from features.toml...");

    let catalog = load_features()?;
    // Update ROADMAP.md
    update_roadmap(&catalog)?;

    // Update LSP_ACTUAL_STATUS.md
    update_lsp_status(&catalog)?;

    println!("✅ Documentation synced successfully!");
    Ok(())
}

fn update_roadmap(catalog: &Catalog) -> Result<()> {
    let roadmap_path = Path::new("ROADMAP.md");
    let mut content = fs::read_to_string(roadmap_path)?;

    // Ensure fence markers exist
    ensure_fence(&content, "COMPLIANCE_TABLE")?;

    let table = perl_lsp_rs_core::feature_catalog::render_navigation_table(catalog);

    // Inject the compliance table into the fenced section
    content = replace_fence(&content, "COMPLIANCE_TABLE", &table)?;

    fs::write(roadmap_path, content)?;

    // Keep this side-effect so the BDD-style progress checks can fail fast when catalog
    // fields are missing or out of date.
    let version = catalog.meta.version.clone();
    if version.is_empty() {
        return Err(eyre!("Catalog version is missing"));
    }

    Ok(())
}

fn update_lsp_status(catalog: &Catalog) -> Result<()> {
    let status_path = Path::new("crates/perl-parser/LSP_ACTUAL_STATUS.md");

    // Check if file exists and has fence markers (for future use with fenced sections)
    if status_path.exists() {
        let existing = fs::read_to_string(status_path)?;
        if existing.contains("<!-- BEGIN:") && existing.contains("<!-- END:") {
            println!("Note: Fenced sections detected but full regeneration in use");
        }
    }

    let mut by_area: BTreeMap<String, Vec<&perl_lsp_rs_core::feature_catalog::Feature>> =
        BTreeMap::new();
    for feature in catalog.features() {
        by_area.entry(feature.area.clone()).or_default().push(feature);
    }

    let mut content = String::new();
    content.push_str("# LSP Feature Status\n\n");
    content.push_str("Auto-generated from `features.toml` - DO NOT EDIT\n\n");
    content.push_str(&format!(
        "Version: {} | LSP: {}\n\n",
        catalog.meta.version, catalog.meta.lsp_version
    ));

    for (area, features) in by_area {
        content.push_str(&format!("## {}\n\n", area.replace('_', " ")));
        content.push_str("| Feature | Spec | Status | Description |\n");
        content.push_str("|---------|------|--------|-------------|\n");

        for feature in features {
            let status = match (feature.maturity, feature.advertised) {
                (Maturity::Proven, true) => "✅ Proven",
                (Maturity::Preview, true) => "🔧 Preview",
                (Maturity::Planned | Maturity::Unsupported, _) => "❌ Planned/Unsupported",
                (Maturity::NotProven, true) => "⚠️ Not proven (advertised)",
                _ => "❌ Not Implemented",
            };

            content.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                feature.id.replace("lsp.", ""),
                feature.spec,
                status,
                feature.description
            ));
        }
        content.push('\n');
    }

    fs::write(status_path, content)?;
    Ok(())
}

/// Ensure fence markers exist in document
fn ensure_fence(content: &str, tag: &str) -> Result<()> {
    let begin_marker = format!("<!-- BEGIN: {tag} -->");
    let end_marker = format!("<!-- END: {tag} -->");

    if !content.contains(&begin_marker) || !content.contains(&end_marker) {
        return Err(eyre!(
            "Missing documentation fence for {} - expected both '{}' and '{}'",
            tag,
            begin_marker,
            end_marker
        ));
    }
    Ok(())
}

/// Replace content between `<!-- BEGIN: TAG -->` and `<!-- END: TAG -->` markers.
fn replace_fence(content: &str, tag: &str, new_body: &str) -> Result<String> {
    let begin_marker = format!("<!-- BEGIN: {tag} -->");
    let end_marker = format!("<!-- END: {tag} -->");

    let begin_pos =
        content.find(&begin_marker).ok_or_else(|| eyre!("Missing begin marker for {tag}"))?;
    let end_pos = content.find(&end_marker).ok_or_else(|| eyre!("Missing end marker for {tag}"))?;

    let mut result = String::with_capacity(content.len());
    result.push_str(&content[..begin_pos]);
    result.push_str(&begin_marker);
    result.push('\n');
    result.push_str(new_body);
    result.push_str(&end_marker);
    result.push_str(&content[end_pos + end_marker.len()..]);
    Ok(result)
}

fn snapshot_caps_from_content(content: &str) -> Result<Option<BTreeSet<String>>> {
    let Some(yaml_start) = content.find("---\n") else {
        return Ok(None);
    };

    let after_first_doc = &content[yaml_start + 4..];
    let yaml_content = if let Some(second_doc_start) = after_first_doc.find("\n---\n") {
        &after_first_doc[second_doc_start + 5..]
    } else {
        after_first_doc
    };
    let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml_content)?;
    let caps = yaml.get("caps").and_then(|value| value.as_sequence()).map(|caps| {
        caps.iter().filter_map(|value| value.as_str().map(String::from)).collect::<BTreeSet<_>>()
    });

    Ok(caps)
}

fn compare_snapshot_caps(
    catalog_advertised: &BTreeSet<String>,
    snapshot_caps: &BTreeSet<String>,
) -> (Vec<String>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    let missing_in_caps = catalog_advertised.difference(snapshot_caps).collect::<Vec<_>>();
    let extra_in_caps = snapshot_caps.difference(catalog_advertised).collect::<Vec<_>>();

    if !missing_in_caps.is_empty() {
        errors.push(format!(
            "Features advertised in catalog but not in capabilities: {}",
            missing_in_caps.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }

    if !extra_in_caps.is_empty() {
        warnings.push(format!(
            "Features in capabilities but not advertised in catalog: {}",
            extra_in_caps.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }

    (warnings, errors)
}

fn verify_features() -> Result<()> {
    println!("🔍 Verifying features match capabilities...");

    let catalog = load_features()?;
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    // Check for duplicate IDs and basic validity.
    if let Err(error) = catalog.validate() {
        errors.push(error.to_string());
    }

    // Check that all advertised features have tests.
    for feature in catalog.features() {
        if feature.advertised && feature.tests.is_empty() {
            warnings.push(format!("Feature advertised without tests: {}", feature.id));
        }
    }

    // Check that advertised features have at least one backing test file.
    for feature in catalog.features() {
        if feature.advertised && !feature.tests.is_empty() {
            for test in &feature.tests {
                let test_file = repo_relative_path(test);
                if !test_file.exists() {
                    warnings.push(format!("Test file not found for {}: {}", feature.id, test));
                }
            }
        }
    }

    // Check advertised feature IDs against the LSP snapshot.
    let snapshot_path = lsp_feature_snapshot_path();
    if snapshot_path.exists() {
        match fs::read_to_string(&snapshot_path) {
            Ok(content) => match snapshot_caps_from_content(&content) {
                Ok(Some(snapshot_caps)) => {
                    let catalog_advertised = snapshot_comparable_feature_ids();
                    let (snapshot_warnings, snapshot_errors) =
                        compare_snapshot_caps(&catalog_advertised, &snapshot_caps);

                    if snapshot_warnings.is_empty() && snapshot_errors.is_empty() {
                        println!("📋 Snapshot comparison: ✅ Perfect match");
                    }

                    warnings.extend(snapshot_warnings);
                    errors.extend(snapshot_errors);
                }
                Ok(None) => {
                    warnings.push("Snapshot file doesn't contain valid YAML section".to_string());
                }
                Err(error) => warnings.push(format!("Failed to parse snapshot YAML: {error}")),
            },
            Err(error) => warnings.push(format!("Failed to read snapshot file: {error}")),
        }
    } else {
        warnings.push(
            "Snapshot file not found - run 'cargo test -p perl-lsp --test lsp_features_snapshot_test' to generate"
                .to_string(),
        );
    }

    if !errors.is_empty() {
        println!("❌ Errors found:");
        for error in &errors {
            println!("  - {}", error);
        }
        return Err(eyre!("Feature verification failed with {} errors", errors.len()));
    }

    if !warnings.is_empty() {
        println!("⚠️  Warnings:");
        for warning in &warnings {
            println!("  - {}", warning);
        }
    }

    println!("✅ Feature verification complete!");
    Ok(())
}

fn generate_report() -> Result<()> {
    println!("📊 Generating feature declaration report...");

    let catalog = load_features()?;
    let area_stats = catalog.area_statistics();

    let total = catalog.feature.len();
    let advertised = catalog.feature.iter().filter(|f| f.advertised).count();
    let proven =
        catalog.feature.iter().filter(|f| f.maturity == Maturity::Proven && f.advertised).count();
    let preview = catalog
        .feature
        .iter()
        .filter(|f| matches!(f.maturity, Maturity::Preview) && f.advertised)
        .count();
    let not_proven =
        catalog.feature.iter().filter(|f| matches!(f.maturity, Maturity::NotProven)).count();
    let planned =
        catalog.feature.iter().filter(|f| matches!(f.maturity, Maturity::Planned)).count();
    let unsupported =
        catalog.feature.iter().filter(|f| matches!(f.maturity, Maturity::Unsupported)).count();

    println!("\n=== LSP Feature Declaration Report ===");
    println!("Version: {} | LSP: {}", catalog.meta.version, catalog.meta.lsp_version);
    println!("\nOverall declaration counts: {}/{} advertised", advertised, total);
    println!("\nBreakdown:");
    println!("  Proven:       {} features", proven);
    println!("  Preview:      {} features", preview);
    println!("  Not proven:   {} features", not_proven);
    println!("  Planned:      {} features", planned);
    println!("  Unsupported:  {} features", unsupported);

    println!("\nBy Area:");
    for (area, stats) in area_stats {
        println!("  {:20} {}/{} declared", area.replace('_', " "), stats.advertised, stats.total);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        compare_snapshot_caps, ensure_fence, lsp_feature_snapshot_path, replace_fence,
        repo_relative_path, snapshot_caps_from_content, snapshot_comparable_feature_ids,
    };
    use color_eyre::eyre::Result;
    use perl_tdd_support::must_err;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    #[test]
    fn snapshot_caps_from_content_extracts_caps_array() -> Result<()> {
        let content = "header\n---\ncaps:\n  - lsp.hover\n  - lsp.definition\n";
        let caps = snapshot_caps_from_content(content)?;

        assert_eq!(
            caps,
            Some(BTreeSet::from(["lsp.definition".to_string(), "lsp.hover".to_string()]))
        );
        Ok(())
    }

    #[test]
    fn snapshot_caps_from_content_returns_none_without_yaml_delimiter() -> Result<()> {
        let content = "caps:\n  - lsp.hover\n";
        assert_eq!(snapshot_caps_from_content(content)?, None);
        Ok(())
    }

    #[test]
    fn repo_relative_path_keeps_catalog_test_paths_rooted_at_repo() {
        let path = repo_relative_path("crates/perl-lsp-rs/tests/lsp_completion_tests.rs");
        assert_eq!(path, PathBuf::from("crates/perl-lsp-rs/tests/lsp_completion_tests.rs"));
    }

    #[test]
    fn lsp_feature_snapshot_path_points_to_lsp_snapshot() {
        assert_eq!(
            lsp_feature_snapshot_path(),
            PathBuf::from(
                "crates/perl-lsp-rs/tests/snapshots/lsp_features_snapshot_test__advertised_vs_caps.snap"
            )
        );
    }

    #[test]
    fn snapshot_comparable_feature_ids_excludes_non_capability_rows() {
        let ids = snapshot_comparable_feature_ids();
        assert!(!ids.contains("dap.core"));
        assert!(!ids.contains("lsp.workspace_symbol_resolve"));
        assert!(!ids.contains("lsp.code_lens_refresh"));
        assert!(!ids.contains("lsp.formatting"));
        assert!(!ids.contains("lsp.type_hierarchy"));
        assert!(!ids.contains("lsp.inline_completion"));
        assert!(ids.contains("lsp.completion"));
    }

    #[test]
    fn snapshot_caps_from_content_returns_none_without_caps_key() -> Result<()> {
        let content = "header\n---\nprofiles:\n  - all\n";
        assert_eq!(snapshot_caps_from_content(content)?, None);
        Ok(())
    }

    #[test]
    fn snapshot_caps_from_content_handles_insta_two_doc_snapshot() -> Result<()> {
        let content = "\
---\n\
source: crates/perl-lsp-rs/tests/lsp_features_snapshot_test.rs\n\
expression: \"&snapshot_data\"\n\
---\n\
caps:\n\
  - lsp.hover\n\
  - lsp.definition\n";

        let caps = snapshot_caps_from_content(content)?;
        assert_eq!(
            caps,
            Some(BTreeSet::from(["lsp.definition".to_string(), "lsp.hover".to_string()]))
        );
        Ok(())
    }

    #[test]
    fn compare_snapshot_caps_reports_missing_and_extra_features() {
        let catalog = BTreeSet::from(["lsp.hover".to_string(), "lsp.definition".to_string()]);
        let snapshot = BTreeSet::from(["lsp.hover".to_string(), "lsp.rename".to_string()]);

        let (warnings, errors) = compare_snapshot_caps(&catalog, &snapshot);

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("lsp.definition"));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("lsp.rename"));
    }

    #[test]
    fn compare_snapshot_caps_accepts_perfect_match() {
        let catalog = BTreeSet::from(["lsp.hover".to_string()]);
        let snapshot = BTreeSet::from(["lsp.hover".to_string()]);

        let (warnings, errors) = compare_snapshot_caps(&catalog, &snapshot);

        assert!(warnings.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn ensure_fence_requires_both_markers() {
        let content = "<!-- BEGIN: COMPLIANCE_TABLE -->\nbody\n";
        let error = must_err(ensure_fence(content, "COMPLIANCE_TABLE"));
        assert!(error.to_string().contains("Missing documentation fence"));
    }

    #[test]
    fn replace_fence_replaces_only_tagged_section() -> Result<()> {
        let content = "before\n<!-- BEGIN: COMPLIANCE_TABLE -->\nold\n<!-- END: COMPLIANCE_TABLE -->\nafter\n";
        let replaced = replace_fence(content, "COMPLIANCE_TABLE", "new\n")?;

        assert!(replaced.contains("before"));
        assert!(replaced.contains("after"));
        assert!(
            replaced
                .contains("<!-- BEGIN: COMPLIANCE_TABLE -->\nnew\n<!-- END: COMPLIANCE_TABLE -->")
        );
        assert!(!replaced.contains("\nold\n"));
        Ok(())
    }
}
