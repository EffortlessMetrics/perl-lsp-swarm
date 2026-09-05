//! Checked ownership matrix for server-initiated LSP requests (#13223).
//!
//! `perllsp` sends a handful of requests to the client. Truth about them is
//! spread across the direction registry, the runtime emitters, the feature
//! catalog, the response registry, and the test clients, and nothing currently
//! proves those surfaces agree. This task binds each server-initiated request
//! to one row naming its emitter, capability gate, response policy, decoder,
//! terminal-state owner, and proof disposition, then fails closed when the row
//! and the real surfaces drift.
//!
//! The matrix is an ownership and proof map, not a second implementation
//! registry: `crates/perl-lsp-rs/src/protocol/method_direction.rs` remains the
//! only classifier of direction and envelope kind, and this task reads it.

mod check;
mod discover;
mod model;

#[cfg(test)]
mod tests;

use clap::{Parser, Subcommand};
use color_eyre::eyre::{Result, WrapErr, bail};
use model::{Matrix, Violation};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Default location of the matrix relative to the repository root.
const DEFAULT_MATRIX: &str = "policy/server-request-ownership.v1.toml";

#[derive(Debug, Parser)]
#[command(
    name = "server-request-ownership",
    about = "Check the server-initiated request ownership matrix against real surfaces"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Join the matrix against the direction registry, production emitters, and
    /// feature catalog, and fail closed on any drift.
    Check {
        /// Matrix path, relative to the repository root.
        #[arg(long, default_value = DEFAULT_MATRIX)]
        matrix: PathBuf,
        /// Repository root used to resolve every cited path.
        #[arg(long, default_value = ".")]
        repo_root: PathBuf,
    },
    /// Print the deterministic ownership view without failing on findings.
    Explain {
        #[arg(long, default_value = DEFAULT_MATRIX)]
        matrix: PathBuf,
        #[arg(long, default_value = ".")]
        repo_root: PathBuf,
        /// Restrict the view to one method.
        #[arg(long)]
        method: Option<String>,
    },
}

/// Resolve `.` to the real project root so the task works from any directory.
fn resolve_root(repo_root: PathBuf) -> Result<PathBuf> {
    if repo_root.as_path() == Path::new(".") { crate::utils::project_root() } else { Ok(repo_root) }
}

fn load(repo_root: &Path, matrix_path: &Path) -> Result<Matrix> {
    let absolute = repo_root.join(matrix_path);
    let source = std::fs::read_to_string(&absolute)
        .wrap_err_with(|| format!("reading matrix {}", absolute.display()))?;
    // `deny_unknown_fields` on the model rejects an unknown cell rather than
    // silently dropping it.
    toml::from_str(&source).wrap_err_with(|| format!("parsing matrix {}", absolute.display()))
}

/// Stable content fingerprint over the rendered view.
fn fingerprint(rendered: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"server_request_ownership.v1\0");
    hasher.update(rendered.as_bytes());
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Render the matrix in a deterministic, host-independent order.
fn render(matrix: &Matrix, method_filter: Option<&str>) -> String {
    let mut rows: Vec<_> = matrix
        .request
        .iter()
        .filter(|row| method_filter.is_none_or(|method| row.method == method))
        .collect();
    rows.sort_by(|left, right| left.id.cmp(&right.id));

    let mut out = String::new();
    for row in rows {
        out.push_str(&format!(
            "{id}\n  method={method}\n  spec={spec} baseline={baseline}\n  emission={emission} \
             emitters=[{emitters}]\n  catalog_row={catalog}\n  capability_gate={gate} \
             owner={gate_owner}\n  ux_default_response_owner={ux}\n  \
             programmable_actions_owner={prog}\n  response_decoder={decoder}\n  \
             terminal_state_owner={terminal}\n  timeout_cleanup_policy={timeout}\n  \
             exact_process_proof={proof}\n  schema_evidence={schema}\n  \
             disposition={disposition}\n  limitations={limitations}\n",
            id = row.id,
            method = row.method,
            spec = row.spec,
            baseline = row.protocol_baseline,
            emission = row.emission,
            emitters = row.emitters.join(", "),
            catalog = row.feature_catalog_row,
            gate = row.capability_gate,
            gate_owner = row.capability_gate_owner,
            ux = row.ux_default_response_owner,
            prog = row.programmable_actions_owner,
            decoder = row.response_decoder,
            terminal = row.terminal_state_owner,
            timeout = row.timeout_cleanup_policy,
            proof = row.exact_process_proof,
            schema = row.schema_evidence,
            disposition = row.disposition,
            limitations = row.limitations,
        ));
    }
    out
}

/// Run the joined check and return every finding.
fn evaluate(repo_root: &Path, matrix: &Matrix) -> Result<Vec<Violation>> {
    let (discovered, discovery_findings) = discover::discover(
        repo_root,
        &matrix.meta.direction_registry,
        &matrix.meta.feature_catalog,
        &matrix.meta.emission_scan_root,
    )?;
    Ok(check::check(repo_root, matrix, &discovered, discovery_findings))
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Check { matrix, repo_root } => {
            let root = resolve_root(repo_root)?;
            let parsed = load(&root, &matrix)?;
            let violations = evaluate(&root, &parsed)?;

            if violations.is_empty() {
                println!(
                    "server-request-ownership: {} rows current against #{} (fingerprint {})",
                    parsed.request.len(),
                    parsed.meta.owner_issue,
                    fingerprint(&render(&parsed, None))
                );
                return Ok(());
            }

            for violation in &violations {
                println!("{}: {} — {}", violation.rule, violation.subject, violation.detail);
            }
            bail!(
                "server-request-ownership: {} finding(s); the matrix and the current surfaces \
                 disagree",
                violations.len()
            );
        }
        Command::Explain { matrix, repo_root, method } => {
            let root = resolve_root(repo_root)?;
            let parsed = load(&root, &matrix)?;
            let rendered = render(&parsed, method.as_deref());
            if rendered.is_empty() {
                bail!("server-request-ownership: no row matched");
            }
            print!("{rendered}");
            // `explain` renders; it does not validate. Printing a bare
            // fingerprint here invites it to be pasted as evidence the matrix
            // is current, which only `check` establishes — and a filtered
            // render covers one row, so its digest is not the check digest.
            let scope = match method.as_deref() {
                Some(method) => format!("`{method}` only"),
                None => "all rows".to_string(),
            };
            println!(
                "fingerprint {} ({scope}, rendered without validation; run `check` for currency)",
                fingerprint(&rendered)
            );
            Ok(())
        }
    }
}
