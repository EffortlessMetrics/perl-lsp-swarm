//! Checked inventory of production Perl source classifiers and parser-payload
//! geometry reconstructors (#7300 R1 / #13993).
//!
//! This row owns durable dispositions only. Candidate discovery and blocking CI
//! belong to #13994 and #13995 respectively.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use clap::Subcommand;

pub const LEDGER_PATH: &str = "policy/source-classifier-inventory.toml";
pub const PROJECTION_PATH: &str = "docs/architecture/source-classifier-inventory.md";
pub const JSON_PATH: &str = "target/policy/source-classifier-inventory.json";

const SCHEMA: &str = "source_classifier_inventory.v1";

const STATUSES: &[&str] = &[
    "canonical_producer",
    "thin_canonical_consumer",
    "presentation_only",
    "bounded_compatibility_adapter",
    "historical_or_test_oracle",
    "must_retire",
];

const REACHABILITY: &[&str] =
    &["production_reachable", "not_proven", "historical_only", "test_oracle_only"];

const SELF_ARTIFACTS: &[&str] = &[LEDGER_PATH, PROJECTION_PATH];

/// Issue that closed a narrow completion bug but does not retire the live scanner.
const COMPLETION_HISTORICAL_BUG_ISSUE: u64 = 4956;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    schema_version: String,
    controller_issue: u64,
    inventory_row_issue: u64,
    last_reviewed_commit: String,
    production_scan_crates: Vec<String>,
    #[serde(default)]
    site: Vec<SiteRow>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct SiteRow {
    id: String,
    path: String,
    symbol: String,
    consumer_surface: String,
    mechanism: String,
    source_classes: Vec<String>,
    reachability: String,
    reachability_evidence: String,
    canonical_dependency: String,
    status: String,
    owner_issue: u64,
    reason: String,
    exit_condition: String,
    #[serde(default)]
    review_after: String,
    #[serde(default)]
    positive_fixtures: Vec<String>,
    #[serde(default)]
    negative_fixtures: Vec<String>,
}

#[derive(Debug, Serialize)]
struct InventoryJson {
    schema: String,
    digest: String,
    row_count: usize,
    sites: Vec<SiteRow>,
}

pub fn run_inventory(check: bool, root: Option<PathBuf>) -> Result<()> {
    let root = match root {
        Some(path) => path,
        None => project_root()?,
    };
    let ledger = load_ledger(&root)?;
    validate_ledger(&ledger, &root)?;

    let json = render_json(&ledger);
    let markdown = render_markdown(&ledger);

    if check {
        let committed =
            fs::read_to_string(root.join(PROJECTION_PATH)).wrap_err_with(|| {
                format!("failed to read {PROJECTION_PATH}; run `cargo xtask source-context inventory --write`")
            })?;
        if normalize_newlines(&committed) != normalize_newlines(&markdown) {
            bail!("{PROJECTION_PATH} is stale; run `cargo xtask source-context inventory --write`");
        }
        eprintln!("source-classifier inventory: check OK ({})", ledger.site.len());
        return Ok(());
    }

    let json_path = root.join(JSON_PATH);
    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent).wrap_err("create target/policy")?;
    }
    fs::write(&json_path, serde_json::to_string_pretty(&json)?).wrap_err("write JSON inventory")?;
    fs::write(root.join(PROJECTION_PATH), &markdown).wrap_err("write Markdown projection")?;
    eprintln!("source-classifier inventory: wrote {} and {}", PROJECTION_PATH, JSON_PATH);
    Ok(())
}

pub fn run_check(root: Option<PathBuf>) -> Result<()> {
    run_inventory(true, root)
}

pub fn run_explain(query: &str, root: Option<PathBuf>) -> Result<()> {
    let root = match root {
        Some(path) => path,
        None => project_root()?,
    };
    let ledger = load_ledger(&root)?;
    validate_ledger(&ledger, &root)?;
    let query = query.trim();
    if query.is_empty() {
        bail!("explain query must be a site id or path substring");
    }

    let mut matches: Vec<&SiteRow> =
        ledger.site.iter().filter(|row| row.id == query || row.path.contains(query)).collect();
    matches.sort_by_key(|row| row.id.as_str());

    if matches.is_empty() {
        bail!("no inventory row matches {query:?}");
    }

    for row in matches {
        println!("id: {}", row.id);
        println!("path: {}", row.path);
        println!("symbol: {}", row.symbol);
        println!("status: {}", row.status);
        println!("reachability: {} ({})", row.reachability, row.reachability_evidence);
        println!("owner: #{}", row.owner_issue);
        println!("mechanism: {}", row.mechanism);
        println!("classes: {}", row.source_classes.join(", "));
        println!("canonical: {}", row.canonical_dependency);
        println!("reason: {}", row.reason);
        println!("exit: {}", row.exit_condition);
        if !row.review_after.is_empty() {
            println!("review_after: {}", row.review_after);
        }
        println!();
    }
    Ok(())
}

fn load_ledger(root: &Path) -> Result<Ledger> {
    let raw = fs::read_to_string(root.join(LEDGER_PATH))
        .wrap_err_with(|| format!("failed to read {LEDGER_PATH}"))?;
    let ledger: Ledger = toml::from_str(&raw).wrap_err("parse source-classifier inventory")?;
    Ok(ledger)
}

fn validate_ledger(ledger: &Ledger, root: &Path) -> Result<()> {
    if ledger.schema_version != SCHEMA {
        bail!("schema_version must be {SCHEMA}, got {:?}", ledger.schema_version);
    }
    if ledger.controller_issue != 7300 {
        bail!("controller_issue must be 7300");
    }
    if ledger.inventory_row_issue != 13993 {
        bail!("inventory_row_issue must be 13993 for R1 baseline");
    }
    if ledger.last_reviewed_commit.trim().is_empty() {
        bail!("last_reviewed_commit must record the reviewed main identity");
    }
    if ledger.production_scan_crates.is_empty() {
        bail!("production_scan_crates must name at least one scanned crate");
    }
    for crate_name in &ledger.production_scan_crates {
        let manifest = root.join("crates").join(crate_name).join("Cargo.toml");
        if !manifest.is_file() {
            bail!(
                "production_scan_crates entry {crate_name:?} has no crate manifest at {manifest:?}"
            );
        }
    }
    if ledger.site.is_empty() {
        bail!("inventory must contain at least one site row");
    }

    let mut ids = BTreeSet::new();
    let mut path_symbol = BTreeSet::new();
    let mut selection_raw = false;
    let mut selection_ast = false;

    for row in &ledger.site {
        if !ids.insert(row.id.clone()) {
            bail!("duplicate site id {:?}", row.id);
        }
        if row.id.trim().is_empty() {
            bail!("site id must not be empty");
        }
        if !STATUSES.contains(&row.status.as_str()) {
            bail!("site {} has invalid status {:?}", row.id, row.status);
        }
        if !REACHABILITY.contains(&row.reachability.as_str()) {
            bail!("site {} has invalid reachability {:?}", row.id, row.reachability);
        }
        if row.owner_issue == 0 {
            bail!("site {} must name a nonzero owner_issue", row.id);
        }
        if row.reason.trim().is_empty() || row.exit_condition.trim().is_empty() {
            bail!("site {} must record reason and exit_condition", row.id);
        }
        if matches!(row.status.as_str(), "must_retire" | "bounded_compatibility_adapter")
            && row.exit_condition.trim().is_empty()
        {
            bail!("site {} must record an exit_condition", row.id);
        }

        if row.status == "must_retire"
            && row.mechanism.contains("completion")
            && row.owner_issue == COMPLETION_HISTORICAL_BUG_ISSUE
        {
            bail!("site {} cannot mark completion migration complete from #4956 alone", row.id);
        }

        if row.path.contains("selection_range") && row.symbol == "selection_ranges" {
            selection_raw = true;
        }
        if row.path.contains("lsp_compat/selection_range") && row.symbol == "selection_chain" {
            selection_ast = true;
        }

        let path = root.join(&row.path);
        if !path.is_file() {
            bail!("site {} path missing: {}", row.id, row.path);
        }
        if SELF_ARTIFACTS.iter().any(|artifact| row.path == *artifact) {
            bail!("site {} cannot inventory itself: {}", row.id, row.path);
        }
        if !row.symbol.is_empty() && !symbol_present(&path, &row.symbol)? {
            bail!("site {} symbol {:?} not found in {}", row.id, row.symbol, row.path);
        }

        let key = format!("{}::{}", row.path, row.symbol);
        if !path_symbol.insert(key) {
            bail!("duplicate path/symbol row for site {}", row.id);
        }
    }

    if !selection_raw || !selection_ast {
        bail!("inventory must keep raw-text selectionRange and AST compatibility rows separate");
    }

    let has_completion_lexical = ledger.site.iter().any(|row| {
        row.path.contains("completion/completion/lexical_context.rs")
            && row.status == "must_retire"
            && row.owner_issue == 13244
    });
    if !has_completion_lexical {
        bail!(
            "inventory must include live completion lexical_context must_retire rows under #13244"
        );
    }

    let has_rename_runtime = ledger
        .site
        .iter()
        .any(|row| row.id.starts_with("rename-runtime") && row.status == "must_retire");
    if !has_rename_runtime {
        bail!("inventory must include runtime rename text-scan rows under their migration owners");
    }

    Ok(())
}

fn symbol_present(path: &Path, symbol: &str) -> Result<bool> {
    let content = fs::read_to_string(path).wrap_err("read site source")?;
    let patterns = [
        format!("fn {symbol}("),
        format!("fn {symbol}<"),
        format!("pub fn {symbol}("),
        format!("pub(super) fn {symbol}("),
        format!("pub(crate) fn {symbol}("),
        format!("enum {symbol}"),
    ];
    Ok(patterns.iter().any(|pat| content.contains(pat)))
}

fn render_json(ledger: &Ledger) -> InventoryJson {
    let mut sites = ledger.site.clone();
    sites.sort_by(|a, b| a.id.cmp(&b.id));
    let digest = inventory_digest(&sites);
    InventoryJson { schema: SCHEMA.to_string(), digest, row_count: sites.len(), sites }
}

fn inventory_digest(sites: &[SiteRow]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"source-classifier-inventory-v1");
    for row in sites {
        hasher.update(row.id.as_bytes());
        hasher.update([0]);
        hasher.update(row.path.as_bytes());
        hasher.update([0]);
        hasher.update(row.symbol.as_bytes());
        hasher.update([0]);
        hasher.update(row.status.as_bytes());
        hasher.update([0]);
        hasher.update(row.owner_issue.to_string().as_bytes());
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn render_markdown(ledger: &Ledger) -> String {
    let json = render_json(ledger);
    let mut out = String::new();
    writeln!(
        out,
        "<!-- auto-generated by `cargo xtask source-context inventory --write`; do not edit -->"
    )
    .expect("write header");
    writeln!(out).expect("newline");
    writeln!(out, "# Source-classifier inventory (#7300 / #13993)").expect("title");
    writeln!(out).expect("newline");
    writeln!(
        out,
        "Machine authority: `{LEDGER_PATH}`. Digest: `{}`. Rows: {}.",
        json.digest, json.row_count
    )
    .expect("meta");
    writeln!(
        out,
        "Reviewed commit: `{}`. Scanned crates: {}.",
        ledger.last_reviewed_commit,
        ledger.production_scan_crates.join(", ")
    )
    .expect("scan");
    writeln!(out).expect("newline");
    writeln!(out, "| id | status | path | symbol | owner | reachability |").expect("table header");
    writeln!(out, "| --- | --- | --- | --- | --- | --- |").expect("table rule");
    for row in &json.sites {
        writeln!(
            out,
            "| {} | {} | `{}` | `{}` | #{} | {} |",
            row.id, row.status, row.path, row.symbol, row.owner_issue, row.reachability
        )
        .expect("table row");
    }
    out
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[derive(Subcommand)]
pub enum SourceContextSubcommand {
    /// Validate or regenerate the checked inventory and review map (#13993).
    Inventory {
        /// Validate the ledger and require a current generated projection.
        #[arg(long)]
        check: bool,
        /// Evaluate a different repository tree (for hermetic tests).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Validate the checked inventory (`inventory --check`).
    Check {
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Print one or more accepted rows by site id or path substring.
    Explain {
        query: String,
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

pub fn run(command: SourceContextSubcommand) -> Result<()> {
    match command {
        SourceContextSubcommand::Inventory { check, root } => run_inventory(check, root),
        SourceContextSubcommand::Check { root } => run_check(root),
        SourceContextSubcommand::Explain { query, root } => run_explain(&query, root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn write_site(dir: &Path, rel: &str, symbol: &str) -> Result<()> {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(path)?;
        writeln!(file, "pub fn {symbol}() {{}}")?;
        Ok(())
    }

    fn minimal_row(id: &str, path: &str, symbol: &str, status: &str, owner: u64) -> SiteRow {
        SiteRow {
            id: id.to_string(),
            path: path.to_string(),
            symbol: symbol.to_string(),
            consumer_surface: "test".into(),
            mechanism: "test mechanism".into(),
            source_classes: vec!["Code".into()],
            reachability: "production_reachable".into(),
            reachability_evidence: "fixture".into(),
            canonical_dependency: "SourceRegionIndex".into(),
            status: status.into(),
            owner_issue: owner,
            reason: "test row".into(),
            exit_condition: "remove in test".into(),
            review_after: String::new(),
            positive_fixtures: Vec::new(),
            negative_fixtures: Vec::new(),
        }
    }

    fn write_ledger(dir: &Path, sites: Vec<SiteRow>) -> Result<()> {
        let ledger_path = dir.join(LEDGER_PATH);
        if let Some(parent) = ledger_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(ledger_path)?;
        writeln!(file, "schema_version = \"{SCHEMA}\"")?;
        writeln!(file, "controller_issue = 7300")?;
        writeln!(file, "inventory_row_issue = 13993")?;
        writeln!(file, "last_reviewed_commit = \"deadbeef\"")?;
        writeln!(file, "production_scan_crates = [\"perl-lsp-rs-core\"]")?;
        for row in sites {
            writeln!(file, "\n[[site]]")?;
            writeln!(file, "id = {:?}", row.id)?;
            writeln!(file, "path = {:?}", row.path)?;
            writeln!(file, "symbol = {:?}", row.symbol)?;
            writeln!(file, "consumer_surface = {:?}", row.consumer_surface)?;
            writeln!(file, "mechanism = {:?}", row.mechanism)?;
            writeln!(
                file,
                "source_classes = [{}]",
                row.source_classes.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>().join(", ")
            )?;
            writeln!(file, "reachability = {:?}", row.reachability)?;
            writeln!(file, "reachability_evidence = {:?}", row.reachability_evidence)?;
            writeln!(file, "canonical_dependency = {:?}", row.canonical_dependency)?;
            writeln!(file, "status = {:?}", row.status)?;
            writeln!(file, "owner_issue = {}", row.owner_issue)?;
            writeln!(file, "reason = {:?}", row.reason)?;
            writeln!(file, "exit_condition = {:?}", row.exit_condition)?;
        }
        Ok(())
    }

    #[test]
    fn refuses_completion_rows_owned_only_by_4956() -> Result<()> {
        let dir = tempfile::tempdir()?;
        write_site(
            dir.path(),
            "crates/perl-lsp-rs-core/src/providers/completion/completion/lexical_context.rs",
            "is_in_string",
        )?;
        write_site(
            dir.path(),
            "crates/perl-lsp-rs-core/src/providers/selection_range/mod.rs",
            "selection_ranges",
        )?;
        write_site(
            dir.path(),
            "crates/perl-lsp-rs-core/src/providers/lsp_compat/selection_range.rs",
            "selection_chain",
        )?;
        write_site(
            dir.path(),
            "crates/perl-lsp-rs/src/runtime/language/rename.rs",
            "add_qualified_document_rename_edits",
        )?;
        fs::create_dir_all(dir.path().join("crates/perl-lsp-rs-core"))?;
        fs::write(
            dir.path().join("crates/perl-lsp-rs-core/Cargo.toml"),
            "[package]\nname = \"perl-lsp-rs-core\"\nversion = \"0.0.0\"\n",
        )?;
        fs::create_dir_all(dir.path().join("crates/perl-lsp-rs"))?;
        fs::write(
            dir.path().join("crates/perl-lsp-rs/Cargo.toml"),
            "[package]\nname = \"perl-lsp-rs\"\nversion = \"0.0.0\"\n",
        )?;

        let mut bad = minimal_row(
            "completion-bad",
            "crates/perl-lsp-rs-core/src/providers/completion/completion/lexical_context.rs",
            "is_in_string",
            "must_retire",
            COMPLETION_HISTORICAL_BUG_ISSUE,
        );
        bad.mechanism = "completion lexical scan".into();
        let mut ast = minimal_row(
            "selection-ast",
            "crates/perl-lsp-rs-core/src/providers/lsp_compat/selection_range.rs",
            "selection_chain",
            "bounded_compatibility_adapter",
            1695,
        );
        ast.reachability = "not_proven".into();
        write_ledger(
            dir.path(),
            vec![
                bad,
                minimal_row(
                    "completion-good",
                    "crates/perl-lsp-rs-core/src/providers/completion/completion/lexical_context.rs",
                    "is_in_comment",
                    "must_retire",
                    13244,
                ),
                minimal_row(
                    "selection-raw",
                    "crates/perl-lsp-rs-core/src/providers/selection_range/mod.rs",
                    "selection_ranges",
                    "must_retire",
                    1695,
                ),
                ast,
                minimal_row(
                    "rename-runtime-qualified",
                    "crates/perl-lsp-rs/src/runtime/language/rename.rs",
                    "add_qualified_document_rename_edits",
                    "must_retire",
                    13422,
                ),
            ],
        )?;
        let ledger = load_ledger(dir.path())?;
        let err = validate_ledger(&ledger, dir.path()).unwrap_err().to_string();
        assert!(err.contains("#4956"), "{err}");
        Ok(())
    }
}
