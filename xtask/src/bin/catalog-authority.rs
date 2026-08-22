//! Catalog authority gate for #6731.
//!
//! `features.toml` at the repository root is the single catalog authority.
//! The crate-local vendored copies (`crates/*/features_sot.toml`) are
//! generated projections of it. This bin:
//!
//! - `check-catalog`  — validates the authority against
//!   `policy/ga-evidence-policy.toml`: every evidence citation names an
//!   existing file and an existing assertion-bearing test function, and no
//!   row claims more than its class policy allows;
//! - `check-drift`    — fails when any vendored projection diverges from the
//!   authority (semantic comparison, declaration order independent);
//! - `sync-vendored`  — regenerates the vendored projections (`--write`) or
//!   verifies they are current (default);
//! - `sync-status`    — regenerates `docs/project/status/lsp.md` from claim
//!   counts (`--write`) or verifies it is current (default).
#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail};

#[derive(Debug, Parser)]
#[command(name = "catalog-authority")]
struct Args {
    #[command(subcommand)]
    command: Command,
    /// Repository root; defaults to the nearest directory containing
    /// `features.toml`.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    CheckCatalog,
    CheckDrift,
    SyncVendored {
        #[arg(long)]
        write: bool,
    },
    SyncStatus {
        #[arg(long)]
        write: bool,
    },
}

const VENDORED_CATALOGS: &[&str] = &[
    "crates/perl-lsp-rs/features_sot.toml",
    "crates/perl-lsp-rs-core/features_sot.toml",
    "crates/perl-parser/features_sot.toml",
    "crates/perl-dap/features_sot.toml",
];

const STATUS_DOC: &str = "docs/project/status/lsp.md";

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let root = resolve_root(args.root.as_deref())?;
    match args.command {
        Command::CheckCatalog => check_catalog(&root),
        Command::CheckDrift => check_drift(&root),
        Command::SyncVendored { write } => sync_vendored(&root, write),
        Command::SyncStatus { write } => sync_status(&root, write),
    }
}

fn resolve_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = explicit {
        return Ok(root.to_path_buf());
    }
    let cwd = std::env::current_dir().context("resolving current directory")?;
    perl_lsp_rs_core::feature_evidence::find_repo_root(&cwd)
        .ok_or_else(|| color_eyre::Report::msg(format!("no features.toml found above {}", cwd.display())))
}

fn load_authority(root: &Path) -> Result<perl_lsp_rs_core::feature_catalog::Catalog> {
    let path = root.join("features.toml");
    perl_lsp_rs_core::feature_catalog::read_catalog(&path)
        .with_context(|| format!("loading authority catalog {}", path.display()))
}

fn load_policy(
    root: &Path,
) -> Result<perl_lsp_rs_core::feature_evidence::GaEvidencePolicy> {
    let path = root.join("policy/ga-evidence-policy.toml");
    perl_lsp_rs_core::feature_evidence::GaEvidencePolicy::load(&path)
        .map_err(|e| color_eyre::Report::msg(e))
        .with_context(|| format!("loading GA evidence policy {}", path.display()))
}

fn check_catalog(root: &Path) -> Result<()> {
    let catalog = load_authority(root)?;
    let policy = load_policy(root)?;
    if let Err(violations) =
        perl_lsp_rs_core::feature_evidence::validate_catalog_evidence(root, &catalog, &policy)
    {
        eprintln!("CATALOG EVIDENCE VIOLATIONS ({}):", violations.len());
        for violation in &violations {
            eprintln!("  {}: {}", violation.feature_id, violation.detail);
        }
        bail!(
            "catalog evidence validation failed with {} violation(s)",
            violations.len()
        );
    }

    let areas = perl_lsp_rs_core::feature_evidence::claim_counts_by_area(&catalog, &policy);
    let mut proven = 0usize;
    let mut preview = 0usize;
    let mut planned = 0usize;
    let mut not_proven = 0usize;
    let mut unsupported = 0usize;
    for counts in areas.values() {
        proven += counts.proven;
        preview += counts.preview;
        planned += counts.planned;
        not_proven += counts.not_proven;
        unsupported += counts.unsupported;
    }
    println!(
        "catalog evidence OK: {} rows — {proven} proven, {preview} preview, {planned} planned, {not_proven} not_proven, {unsupported} unsupported",
        catalog.feature.len()
    );
    Ok(())
}

fn sync_vendored(root: &Path, write: bool) -> Result<()> {
    let catalog = load_authority(root)?;
    let rendered = perl_lsp_rs_core::feature_evidence::render_vendored_projection(&catalog);

    let mut stale: Vec<String> = Vec::new();
    for rel in VENDORED_CATALOGS {
        let path = root.join(rel);
        let current = match fs::read_to_string(&path) {
            Ok(current) => current,
            Err(_) if write => String::new(),
            Err(e) => {
                stale.push(format!("{rel}: missing or unreadable ({e})"));
                continue;
            }
        };
        if current == rendered {
            continue;
        }
        if write {
            atomic_write(&path, &rendered)?;
            println!("regenerated {rel}");
        } else {
            // Distinguish actionable drift classes in the failure output.
            let detail = match perl_lsp_rs_core::feature_catalog::read_catalog(&path) {
                Ok(candidate) => {
                    perl_lsp_rs_core::feature_evidence::semantic_divergence(&catalog, &candidate)
                        .err()
                        .unwrap_or_else(|| "stale rendering".to_string())
                }
                Err(e) => format!("unparseable vendored copy: {e}"),
            };
            stale.push(format!("{rel}: {detail}"));
        }
    }
    if !stale.is_empty() {
        eprintln!("VENDORED CATALOG DRIFT:");
        for line in &stale {
            eprintln!("  {line}");
        }
        bail!(
            "{} vendored catalog(s) diverge from features.toml; run `cargo xtask catalog-authority sync-vendored --write`",
            stale.len()
        );
    }
    println!("vendored catalogs are projections of features.toml ({} files)", VENDORED_CATALOGS.len());
    Ok(())
}

fn check_drift(root: &Path) -> Result<()> {
    // Drift checking is the read-only half of sync_vendored.
    sync_vendored(root, false)
}

fn render_status_doc(root: &Path) -> Result<String> {
    use perl_lsp_rs_core::feature_evidence::claim_counts_by_area;

    let catalog = load_authority(root)?;
    let policy = load_policy(root)?;
    let table =
        perl_lsp_rs_core::feature_evidence::render_claim_status_table(&catalog, &policy)
            .map_err(color_eyre::Report::msg)?;

    let counts = claim_counts_by_area(&catalog, &policy);
    let (mut proven, mut preview, mut planned, mut not_proven, mut unsupported) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    for c in counts.values() {
        proven += c.proven;
        preview += c.preview;
        planned += c.planned;
        not_proven += c.not_proven;
        unsupported += c.unsupported;
    }
    let total = catalog.feature.len();

    Ok(format!(
        "# LSP Status

> Generated by `cargo xtask catalog-authority sync-status --write`. Do not hand-edit between markers.

## Claim Status

<!-- BEGIN: LSP_CLAIM_TABLE -->
{table}
<!-- END: LSP_CLAIM_TABLE -->

## Claim Basis

<!-- BEGIN: LSP_CLAIM_BASIS -->
- **Denominator**: all {total} rows of `features.toml`, the single catalog authority.
- **proven** ({proven} rows): the row's feature class is GA-eligible and every required evidence class is cited — capability/dispatch proof, positive behavior, response shape/schema proof, a real-process wire exchange, and a negative control (`policy/ga-evidence-policy.toml`).
- **preview** ({preview} rows): shipped and advertised, but the evidence-backed claim is not yet earned; missing evidence classes are recorded per row as the residual sweep of #6731.
- **planned** ({planned} rows): acknowledged protocol surface, not implemented.
- **not_proven** ({not_proven} rows): present in the catalog but not advertised, so there is no capability claim to evaluate.
- **unsupported** ({unsupported} rows): explicitly withdrawn or not applicable.
- Server-to-client requests, document/workspace retained-state lifecycle, cancellation/progress, transport/lifecycle substrate, DAP, and custom-extension feature classes are not GA-eligible yet; their owner issues are declared in the policy file.
- No percentage is published over these statuses: a ratio would re-create the advertised-denominator defect this surface retired (#6731).
<!-- END: LSP_CLAIM_BASIS -->
"
    ))
}

fn sync_status(root: &Path, write: bool) -> Result<()> {
    let rendered = render_status_doc(root)?;
    let path = root.join(STATUS_DOC);
    let current = fs::read_to_string(&path).context("reading docs/project/status/lsp.md")?;
    if current == rendered {
        println!("{STATUS_DOC} is current with the catalog authority");
        return Ok(());
    }
    if write {
        atomic_write(&path, &rendered)?;
        println!("regenerated {STATUS_DOC}");
    } else {
        bail!(
            "{STATUS_DOC} diverges from the catalog authority; run `cargo xtask catalog-authority sync-status --write`"
        );
    }
    Ok(())
}

fn atomic_write(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)?;
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_lsp_rs_core::feature_evidence::{
        DeclaredClaim, EvidenceCitation, EvidenceClass, render_vendored_projection,
    };

    /// A fixture workspace: authority catalog + policy + one vendored copy.
    fn fixture_workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let mut catalog_toml = String::from(
            "[meta]\nversion = '1.0.0'\nlsp_version = '3.18'\n\n[[feature]]\nid = \"lsp.a\"\nspec = \"LSP 3.18\"\narea = \"text_document\"\nmaturity = \"ga\"\nadvertised = true\nclaim = \"preview\"\ntests = [\"crates/x/tests/a.rs\"]\ndescription = \"row a\"\n\n",
        );
        // A proven row whose citations live inside the fixture workspace.
        std::fs::create_dir_all(dir.path().join("crates/x/tests")).unwrap();
        std::fs::write(
            dir.path().join("crates/x/tests/a.rs"),
            "#[test]\nfn receipt() {\n    assert_eq!(2 + 2, 4);\n}\n",
        )
        .unwrap();
        catalog_toml.push_str(
            "[[feature]]\nid = \"lsp.proven\"\nspec = \"LSP 3.18\"\narea = \"text_document\"\nmaturity = \"ga\"\nadvertised = true\nclass = \"request_response\"\nclaim = \"proven\"\nevidence = [\n  { proves = \"capability_dispatch\", test = \"crates/x/tests/a.rs::receipt\" },\n  { proves = \"positive_behavior\", test = \"crates/x/tests/a.rs::receipt\" },\n  { proves = \"shape_schema\", test = \"crates/x/tests/a.rs::receipt\" },\n  { proves = \"real_process_wire\", test = \"crates/x/tests/a.rs::receipt\" },\n  { proves = \"negative_control\", test = \"crates/x/tests/a.rs::receipt\" },\n]\ndescription = \"fully cited\"\n\n",
        );
        catalog_toml.push_str("[[feature]]\nid = \"lsp.planned\"\narea = \"workspace\"\nmaturity = \"planned\"\nadvertised = false\ndescription = \"later\"\n\n");
        std::fs::write(dir.path().join("features.toml"), catalog_toml).unwrap();

        std::fs::create_dir_all(dir.path().join("policy")).unwrap();
        std::fs::write(
            dir.path().join("policy/ga-evidence-policy.toml"),
            "schema_version = 1\npolicy = \"ga-evidence-policy\"\ndefault_class = \"request_response\"\n\n[[class]]\nid = \"request_response\"\nga_eligible = true\nrequired_evidence = [\"capability_dispatch\", \"positive_behavior\", \"shape_schema\", \"real_process_wire\", \"negative_control\"]\n",
        )
        .unwrap();
        dir
    }

    fn write_stale_vendored(root: &Path) {
        std::fs::create_dir_all(root.join("crates/perl-parser")).unwrap();
        std::fs::write(
            root.join("crates/perl-parser/features_sot.toml"),
            "[meta]\nversion = '0.0.1'\nlsp_version = '3.17'\n",
        )
        .unwrap();
    }

    #[test]
    fn check_catalog_accepts_the_consistent_fixture() {
        let dir = fixture_workspace();
        check_catalog(dir.path()).unwrap();
    }

    #[test]
    fn check_catalog_rejects_a_proven_row_without_citations() {
        let dir = fixture_workspace();
        let path = dir.path().join("features.toml");
        let text = fs::read_to_string(&path).unwrap();
        let stripped = text.replace("claim = \"proven\"", "claim = \"proven\" # now uncited")
            .replace("evidence = [", "evidence_disabled = [");
        fs::write(&path, stripped).unwrap();
        assert!(check_catalog(dir.path()).is_err());
    }

    #[test]
    fn sync_status_writes_then_verifies_clean() {
        let dir = fixture_workspace();
        std::fs::create_dir_all(dir.path().join("docs/project/status")).unwrap();
        fs::write(dir.path().join(STATUS_DOC), "stale").unwrap();

        sync_status(dir.path(), true).unwrap();
        // Second run in check mode must pass — determinism.
        sync_status(dir.path(), false).unwrap();

        let rendered = fs::read_to_string(dir.path().join(STATUS_DOC)).unwrap();
        assert!(rendered.contains("| **Overall** | **1** |"));
        assert!(!rendered.contains('%'), "no percentage may be published");
    }

    #[test]
    fn sync_status_check_fails_on_drift() {
        let dir = fixture_workspace();
        std::fs::create_dir_all(dir.path().join("docs/project/status")).unwrap();
        fs::write(dir.path().join(STATUS_DOC), "stale content").unwrap();
        assert!(sync_status(dir.path(), false).is_err());
    }

    #[test]
    fn sync_vendored_repairs_drift_and_then_passes() {
        let dir = fixture_workspace();
        write_stale_vendored(dir.path());

        // Check mode fails while stale.
        assert!(sync_vendored(dir.path(), false).is_err());

        // Write mode repairs every vendored copy to the projection bytes.
        sync_vendored(dir.path(), true).unwrap();

        let catalog = load_authority(dir.path()).unwrap();
        let expected = render_vendored_projection(&catalog);
        for rel in VENDORED_CATALOGS {
            let on_disk = fs::read_to_string(dir.path().join(rel)).unwrap();
            assert_eq!(on_disk, expected, "{rel} must equal the authority projection");
            assert!(!on_disk.contains("compliance_percent"));
        }
        assert!(sync_vendored(dir.path(), false).is_ok());
    }

    #[test]
    fn citation_parsing_rejects_assertion_free_receipts_via_the_validator() {
        let dir = fixture_workspace();
        let path = dir.path().join("features.toml");
        let text = fs::read_to_string(&path).unwrap();
        // Point one citation at the file WITHOUT naming the function: only
        // static_mapping may cite bare paths, so this must fail.
        let broken =
            text.replace("{ proves = \"capability_dispatch\", test = \"crates/x/tests/a.rs::receipt\" }", "{ proves = \"capability_dispatch\", test = \"crates/x/tests/a.rs\" }");
        fs::write(&path, broken).unwrap();
        assert!(check_catalog(dir.path()).is_err());

        // And an assertion-free function is rejected by name.
        fs::write(
            dir.path().join("crates/x/tests/doc.rs"),
            "#[test]\nfn documents_only() {\n    println!(\"contract\");\n}\n",
        )
        .unwrap();
        let policy = load_policy(dir.path()).unwrap();
        let catalog = load_authority(dir.path()).unwrap();
        let mut feature = catalog.features()[0].clone();
        feature.evidence = vec![EvidenceCitation {
            proves: EvidenceClass::PositiveBehavior,
            test: String::from("crates/x/tests/doc.rs::documents_only"),
        }];
        feature.claim = Some(DeclaredClaim::Proven);
        let violations = perl_lsp_rs_core::feature_evidence::validate_catalog_evidence(
            dir.path(),
            &perl_lsp_rs_core::feature_catalog::Catalog {
                meta: catalog.meta.clone(),
                feature: vec![feature],
            },
            &policy,
        )
        .err()
        .unwrap();
        assert!(
            violations
                .iter()
                .any(|v| v.detail.contains("asserts nothing")),
            "{violations:?}"
        );
    }
}
