//! AST change impact packet validator and status generator for #5238.
//!
//! ```text
//! cargo run -p xtask --bin compiler-ast-impact -- status
//! cargo run -p xtask --bin compiler-ast-impact -- check
//! cargo run -p xtask --bin compiler-ast-impact -- explain ChainedComparison
//! cargo run -p xtask --bin compiler-ast-impact -- propose --base HEAD~1 --head HEAD
//! ```

#![allow(clippy::print_stdout)]

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use jsonschema::Validator;
use perl_ast::NodeKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: &str = "ast_change_impact.v1";
const DEFAULT_PACKET: &str = "contracts/compiler/ast_change_impact.v1.toml";
const DEFAULT_SCHEMA: &str = "schemas/ast_change_impact.v1.schema.json";
const DEFAULT_STATUS: &str = "docs/project/status/ast_change_impact.md";
const EXPECTED_INVENTORY_AUTHORITY: &str = "perl_ast::NodeKind::ALL_KIND_NAMES";

#[derive(Debug, Parser)]
#[command(name = "compiler-ast-impact")]
#[command(about = "Validate and render the AST change impact packet contract (#5238)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the generated Markdown status to stdout.
    Status {
        #[arg(long, default_value = DEFAULT_PACKET)]
        packet: PathBuf,
    },
    /// Validate the packet, schema, and checked-in status freshness.
    Check {
        #[arg(long, default_value = DEFAULT_PACKET)]
        packet: PathBuf,
        #[arg(long, default_value = DEFAULT_STATUS)]
        status: PathBuf,
        #[arg(long, default_value = DEFAULT_SCHEMA)]
        schema: PathBuf,
    },
    /// Rewrite the generated Markdown status from the packet.
    WriteStatus {
        #[arg(long, default_value = DEFAULT_PACKET)]
        packet: PathBuf,
        #[arg(long, default_value = DEFAULT_STATUS)]
        status: PathBuf,
    },
    /// Explain one AST kind row from the committed packet.
    Explain {
        ast_kind: String,
        #[arg(long, default_value = DEFAULT_PACKET)]
        packet: PathBuf,
    },
    /// Emit a review scaffold for AST changes between two commits (dispositions are manual).
    Propose {
        #[arg(long)]
        base: String,
        #[arg(long)]
        head: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ImpactPacket {
    schema_version: String,
    packet_id: String,
    controller_issue: String,
    coverage_scope: String,
    claim_boundary: String,
    complete: bool,
    #[serde(default)]
    inventory_authority: Option<String>,
    impacts: Vec<ImpactRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ImpactRow {
    ast_kind: String,
    change_kind: String,
    #[serde(default)]
    parser_contract: Option<String>,
    flat_hir_disposition: String,
    body_hir_disposition: String,
    pir_a_disposition: String,
    #[serde(default)]
    profiles_note: Option<String>,
    #[serde(default)]
    providers_note: Option<String>,
    #[serde(default)]
    gold_oracle_requirements: Vec<String>,
    #[serde(default)]
    schema_cache_effects: Vec<String>,
    owner_issue: String,
    stop_condition: String,
    claim_boundary: String,
}

impl ImpactPacket {
    fn from_str(source: &str) -> Result<Self> {
        let packet: Self = toml::from_str(source).context("parse ast change impact packet")?;
        packet.validate_semantics()?;
        Ok(packet)
    }

    fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("read ast change impact packet {}", path.display()))?;
        Self::from_str(&source)
            .with_context(|| format!("validate ast change impact packet {}", path.display()))
    }

    fn validate_semantics(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            bail!(
                "unsupported ast change impact schema {:?}; expected {:?}",
                self.schema_version,
                SCHEMA_VERSION
            );
        }

        for (name, value) in [
            ("packet_id", self.packet_id.as_str()),
            ("controller_issue", self.controller_issue.as_str()),
            ("coverage_scope", self.coverage_scope.as_str()),
            ("claim_boundary", self.claim_boundary.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("ast change impact field {name} must not be empty");
            }
        }

        validate_issue("controller_issue", &self.controller_issue)?;

        if self.impacts.is_empty() {
            bail!("ast change impact packet must contain at least one row");
        }

        let mut kinds = BTreeSet::new();
        for row in &self.impacts {
            row.validate()?;
            if !kinds.insert(row.ast_kind.as_str()) {
                bail!("duplicate ast_kind row {:?}", row.ast_kind);
            }
            if !NodeKind::ALL_KIND_NAMES.contains(&row.ast_kind.as_str()) {
                bail!("unknown ast_kind {:?}; not in NodeKind::ALL_KIND_NAMES", row.ast_kind);
            }
        }

        if self.complete {
            let authority = self.inventory_authority.as_deref().ok_or_else(|| {
                anyhow!("complete ast change impact inventory requires inventory_authority")
            })?;
            if authority != EXPECTED_INVENTORY_AUTHORITY {
                bail!(
                    "complete inventory authority must be {EXPECTED_INVENTORY_AUTHORITY:?}; got {authority:?}"
                );
            }
            let missing = missing_kinds(self);
            if !missing.is_empty() {
                bail!(
                    "complete ast change impact inventory missing rows for: {}",
                    missing.join(", ")
                );
            }
        }

        Ok(())
    }

    fn render_markdown(&self) -> Result<String> {
        self.validate_semantics()?;
        let mut normalized = self.clone();
        normalized.impacts.sort_by(|left, right| left.ast_kind.cmp(&right.ast_kind));
        for row in &mut normalized.impacts {
            row.gold_oracle_requirements.sort();
            row.schema_cache_effects.sort();
        }

        let mut output = String::new();
        line(&mut output, "# AST change impact")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "> Generated by `cargo run -p xtask --bin compiler-ast-impact -- status`.",
        )?;
        line(&mut output, "> Check with `cargo run -p xtask --bin compiler-ast-impact -- check`.")?;
        line(&mut output, "")?;
        line(&mut output, &normalized.coverage_scope)?;
        line(&mut output, "")?;
        line(&mut output, &format!("- Schema: `{}`", normalized.schema_version))?;
        line(&mut output, &format!("- Packet: `{}`", normalized.packet_id))?;
        line(&mut output, &format!("- Controller: {}", normalized.controller_issue))?;
        line(&mut output, &format!("- Inventory complete: `{}`", normalized.complete))?;
        if let Some(authority) = &normalized.inventory_authority {
            line(&mut output, &format!("- Inventory authority: `{authority}`"))?;
        } else {
            line(&mut output, "- Inventory authority: —")?;
        }

        let covered = normalized.impacts.len();
        let total = NodeKind::ALL_KIND_NAMES.len();
        line(
            &mut output,
            &format!("- Impact rows in this packet: `{covered}` / `{total}` NodeKind names"),
        )?;
        let missing = missing_kinds(&normalized);
        if missing.is_empty() {
            line(&mut output, "- Missing NodeKind rows: none")?;
        } else {
            line(&mut output, &format!("- Missing NodeKind rows: `{}` kinds", missing.len()))?;
        }
        line(&mut output, "")?;
        line(&mut output, &format!("**Claim boundary:** {}", normalized.claim_boundary))?;
        line(&mut output, "")?;

        line(&mut output, "## Impact rows")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "| AST kind | Change | Flat HIR | Body HIR | PIR-A | Owner | Stop condition |",
        )?;
        line(&mut output, "| --- | --- | --- | --- | --- | --- | --- |")?;
        for row in &normalized.impacts {
            line(
                &mut output,
                &format!(
                    "| `{}` | `{}` | `{}` | `{}` | `{}` | {} | {} |",
                    row.ast_kind,
                    row.change_kind,
                    row.flat_hir_disposition,
                    row.body_hir_disposition,
                    row.pir_a_disposition,
                    row.owner_issue,
                    escape_cell(&row.stop_condition),
                ),
            )?;
        }
        line(&mut output, "")?;

        if !missing.is_empty() && !normalized.complete {
            line(&mut output, "## Missing NodeKind rows (inventory incomplete)")?;
            line(&mut output, "")?;
            for kind in missing {
                line(&mut output, &format!("- `{kind}`"))?;
            }
            line(&mut output, "")?;
        }

        Ok(output)
    }
}

impl ImpactRow {
    fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("ast_kind", self.ast_kind.as_str()),
            ("change_kind", self.change_kind.as_str()),
            ("flat_hir_disposition", self.flat_hir_disposition.as_str()),
            ("body_hir_disposition", self.body_hir_disposition.as_str()),
            ("pir_a_disposition", self.pir_a_disposition.as_str()),
            ("stop_condition", self.stop_condition.as_str()),
            ("claim_boundary", self.claim_boundary.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("impact row field {name} must not be empty");
            }
        }
        validate_issue("owner_issue", &self.owner_issue)?;
        validate_change_kind(&self.change_kind)?;
        validate_disposition("flat_hir_disposition", &self.flat_hir_disposition)?;
        validate_disposition("body_hir_disposition", &self.body_hir_disposition)?;
        validate_disposition("pir_a_disposition", &self.pir_a_disposition)?;
        reject_bridge_overclaim(self)?;
        Ok(())
    }
}

fn missing_kinds(packet: &ImpactPacket) -> Vec<String> {
    let covered: BTreeSet<&str> = packet.impacts.iter().map(|row| row.ast_kind.as_str()).collect();
    NodeKind::ALL_KIND_NAMES
        .iter()
        .filter(|kind| !covered.contains(*kind))
        .map(|kind| (*kind).to_string())
        .collect()
}

fn reject_bridge_overclaim(row: &ImpactRow) -> Result<()> {
    let bridge_body = row.body_hir_disposition == "reviewed_bridge_debt";
    let bridge_pir = row.pir_a_disposition == "reviewed_bridge_debt";
    if bridge_body
        && matches!(row.pir_a_disposition.as_str(), "implemented" | "reviewed_non_applicability")
    {
        bail!(
            "ast_kind {}: bridge body HIR cannot pair with PIR disposition {:?} without explicit boundary",
            row.ast_kind,
            row.pir_a_disposition
        );
    }
    if bridge_pir && row.flat_hir_disposition == "implemented" {
        bail!(
            "ast_kind {}: bridge PIR cannot present flat HIR as implemented support",
            row.ast_kind
        );
    }
    if row.body_hir_disposition == "missing" && row.parser_contract.is_some() {
        bail!(
            "ast_kind {}: parser_contract is set but body_hir_disposition is missing",
            row.ast_kind
        );
    }
    Ok(())
}

fn validate_change_kind(value: &str) -> Result<()> {
    const ALLOWED: &[&str] = &["added", "semantic_change", "split", "merge", "retirement"];
    if ALLOWED.contains(&value) {
        Ok(())
    } else {
        bail!("unsupported change_kind {value:?}; expected one of {ALLOWED:?}")
    }
}

fn validate_disposition(field: &str, value: &str) -> Result<()> {
    const ALLOWED: &[&str] = &[
        "implemented",
        "explicit_typed_boundary",
        "reviewed_bridge_debt",
        "reviewed_non_applicability",
        "migration_pending",
        "missing",
    ];
    if ALLOWED.contains(&value) {
        Ok(())
    } else {
        bail!("{field} has unsupported disposition {value:?}; expected one of {ALLOWED:?}")
    }
}

fn validate_issue(name: &str, value: &str) -> Result<()> {
    let digits = value.strip_prefix('#').unwrap_or_default();
    if digits.is_empty()
        || digits.starts_with('0')
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("{name} must be a GitHub issue reference like #5238; got {value:?}");
    }
    Ok(())
}

fn validate_against_schema(schema_path: &Path, packet_path: &Path) -> Result<()> {
    let schema_source = fs::read_to_string(schema_path)
        .with_context(|| format!("read ast change impact schema {}", schema_path.display()))?;
    let schema_json: serde_json::Value =
        serde_json::from_str(&schema_source).context("parse ast change impact schema JSON")?;
    let validator = Validator::new(&schema_json).context("compile ast change impact schema")?;
    let packet_source = fs::read_to_string(packet_path)
        .with_context(|| format!("read ast change impact packet {}", packet_path.display()))?;
    let toml_value: toml::Value =
        toml::from_str(&packet_source).context("parse ast change impact packet TOML")?;
    let packet_json: serde_json::Value =
        serde_json::to_value(toml_value).context("convert ast change impact packet to JSON")?;
    let errors: Vec<String> =
        validator.iter_errors(&packet_json).map(|error| error.to_string()).collect();
    if errors.is_empty() {
        Ok(())
    } else {
        bail!("ast change impact packet failed schema validation: {}", errors.join("; "))
    }
}

fn line(output: &mut String, value: &str) -> Result<()> {
    writeln!(output, "{value}").map_err(|_| anyhow!("render ast change impact status"))
}

fn escape_cell(value: &str) -> String {
    value.replace('|', "\\|")
}

fn write_status(path: &Path, rendered: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create status directory {}", parent.display()))?;
    }
    fs::write(path, rendered).with_context(|| format!("write status {}", path.display()))
}

fn run_check(packet_path: &Path, status_path: &Path, schema_path: &Path) -> Result<()> {
    validate_against_schema(schema_path, packet_path)?;
    let packet = ImpactPacket::load(packet_path)?;
    let rendered = packet.render_markdown()?;
    let current = fs::read_to_string(status_path)
        .with_context(|| format!("read generated status {}", status_path.display()))?;
    if current != rendered {
        bail!(
            "generated ast change impact status is stale: run `cargo run -p xtask --bin compiler-ast-impact -- write-status`"
        );
    }
    println!(
        "ast change impact packet valid: {} rows; inventory complete={}; status current",
        packet.impacts.len(),
        packet.complete
    );
    Ok(())
}

fn run_explain(packet_path: &Path, ast_kind: &str) -> Result<()> {
    let packet = ImpactPacket::load(packet_path)?;
    let row = packet
        .impacts
        .iter()
        .find(|row| row.ast_kind == ast_kind)
        .ok_or_else(|| anyhow!("no impact row for ast_kind {ast_kind:?}"))?;
    let mut output = BTreeMap::new();
    output.insert("ast_kind", row.ast_kind.as_str());
    output.insert("change_kind", row.change_kind.as_str());
    output.insert("flat_hir_disposition", row.flat_hir_disposition.as_str());
    output.insert("body_hir_disposition", row.body_hir_disposition.as_str());
    output.insert("pir_a_disposition", row.pir_a_disposition.as_str());
    output.insert("owner_issue", row.owner_issue.as_str());
    output.insert("stop_condition", row.stop_condition.as_str());
    output.insert("claim_boundary", row.claim_boundary.as_str());
    for (key, value) in output {
        println!("{key}: {value}");
    }
    if let Some(parser) = &row.parser_contract {
        println!("parser_contract: {parser}");
    }
    if let Some(note) = &row.profiles_note {
        println!("profiles_note: {note}");
    }
    if let Some(note) = &row.providers_note {
        println!("providers_note: {note}");
    }
    Ok(())
}

fn run_propose(base: &str, head: &str) -> Result<()> {
    bail!(
        "propose scaffold for base={base} head={head} is manual-only: ast_change_impact.v1 dispositions must be authored and reviewed; automatic classification is forbidden (#5238 non-goals)"
    );
}

fn main() -> Result<()> {
    color_eyre::install().map_err(|error| anyhow!("install diagnostics: {error}"))?;
    let cli = Cli::parse();
    match cli.command {
        Command::Status { packet } => {
            let rendered = ImpactPacket::load(&packet)?.render_markdown()?;
            print!("{rendered}");
        }
        Command::Check { packet, status, schema } => run_check(&packet, &status, &schema)?,
        Command::WriteStatus { packet, status } => {
            let rendered = ImpactPacket::load(&packet)?.render_markdown()?;
            write_status(&status, &rendered)?;
            println!("wrote {}", status.display());
        }
        Command::Explain { ast_kind, packet } => run_explain(&packet, &ast_kind)?,
        Command::Propose { base, head } => run_propose(&base, &head)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_PACKET: &str =
        include_str!("../../../contracts/compiler/ast_change_impact.v1.toml");

    #[test]
    fn committed_migration_seed_validates() {
        ImpactPacket::from_str(FIXTURE_PACKET).expect("committed packet must validate");
    }

    #[test]
    fn complete_inventory_requires_every_nodekind_row() {
        let mut packet = ImpactPacket::from_str(FIXTURE_PACKET).expect("load seed");
        packet.complete = true;
        packet.inventory_authority = Some(EXPECTED_INVENTORY_AUTHORITY.to_string());
        let error = packet.validate_semantics().expect_err("must fail when incomplete");
        assert!(error.to_string().contains("missing rows"), "unexpected error: {error}");
    }

    #[test]
    fn parser_contract_with_missing_body_hir_fails() {
        let mut packet = ImpactPacket::from_str(FIXTURE_PACKET).expect("load seed");
        packet.impacts[0].body_hir_disposition = "missing".to_string();
        packet.impacts[0].parser_contract = Some("parsed".to_string());
        let error = packet.validate_semantics().expect_err("must reject missing body");
        assert!(
            error.to_string().contains("body_hir_disposition is missing"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn bridge_body_with_implemented_pir_fails() {
        let mut packet = ImpactPacket::from_str(FIXTURE_PACKET).expect("load seed");
        packet.impacts[0].body_hir_disposition = "reviewed_bridge_debt".to_string();
        packet.impacts[0].pir_a_disposition = "implemented".to_string();
        let error = packet.validate_semantics().expect_err("must reject overclaim");
        assert!(
            error.to_string().contains("bridge body HIR cannot pair"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn duplicate_ast_kind_fails() {
        let mut packet = ImpactPacket::from_str(FIXTURE_PACKET).expect("load seed");
        let duplicate = packet.impacts[0].clone();
        packet.impacts.push(duplicate);
        let error = packet.validate_semantics().expect_err("must reject duplicate");
        assert!(error.to_string().contains("duplicate ast_kind"), "unexpected error: {error}");
    }

    #[test]
    fn unknown_ast_kind_fails() {
        let mut packet = ImpactPacket::from_str(FIXTURE_PACKET).expect("load seed");
        packet.impacts[0].ast_kind = "NotARealKind".to_string();
        let error = packet.validate_semantics().expect_err("must reject unknown kind");
        assert!(error.to_string().contains("unknown ast_kind"), "unexpected error: {error}");
    }

    #[test]
    fn propose_is_manual_only() {
        let error = run_propose("HEAD~1", "HEAD").expect_err("propose must fail closed");
        assert!(error.to_string().contains("manual-only"), "unexpected error: {error}");
    }
}
