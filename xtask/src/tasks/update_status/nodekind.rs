//! NodeKind inventory receipt + status dashboard generator.
//!
//! Builds an inventory of all 69 NodeKind variants using corpus coverage data
//! and classification metadata from `perl_ast::classification`, then:
//!
//! - Writes a receipt JSON to `target/receipts/nodekind_inventory.json`
//! - Renders `docs/project/status/nodekind.md` with BEGIN/END marker blocks

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::eyre::{Context, Result, eyre};
use perl_ast::classification::{NodeKindCategory, NodeKindFlags};
use perl_parser::ast::NodeKind;

use crate::tasks::corpus_audit::compute_status_summary;

use super::replace_block;

// ---------------------------------------------------------------------------
// Classification bridge
// ---------------------------------------------------------------------------

/// Returns `(category, flags)` for a NodeKind identified by its canonical name string.
///
/// Constructs a minimal stub instance per variant and delegates to
/// `NodeKind::category()` / `NodeKind::flags()` from `perl_ast::classification`.
/// The match is exhaustive over all 69 variants in `ALL_KIND_NAMES`.
fn classify(kind_name: &str) -> Option<(NodeKindCategory, NodeKindFlags)> {
    // Helper stubs for field-carrying variants.  Classification is variant-level
    // only — field values are ignored by `category()` and `flags()`.
    let stub_node = || -> Box<perl_ast::ast::Node> {
        Box::new(perl_ast::ast::Node::new(
            NodeKind::Undef,
            perl_ast::SourceLocation { start: 0, end: 0 },
        ))
    };
    let stub_loc = perl_ast::SourceLocation { start: 0, end: 0 };

    let kind: NodeKind = match kind_name {
        "ArrayLiteral" => NodeKind::ArrayLiteral { elements: vec![] },
        "Assignment" => {
            NodeKind::Assignment { lhs: stub_node(), rhs: stub_node(), op: "=".to_string() }
        }
        "Binary" => NodeKind::Binary { op: "+".to_string(), left: stub_node(), right: stub_node() },
        "Block" => NodeKind::Block { statements: vec![] },
        "Class" => NodeKind::Class { name: String::new(), parents: vec![], body: stub_node() },
        "DataSection" => NodeKind::DataSection { marker: String::new(), body: None },
        "Default" => NodeKind::Default { body: stub_node() },
        "Defer" => NodeKind::Defer { block: stub_node() },
        "Diamond" => NodeKind::Diamond,
        "Do" => NodeKind::Do { block: stub_node() },
        "Ellipsis" => NodeKind::Ellipsis,
        "Error" => {
            NodeKind::Error { message: String::new(), expected: vec![], found: None, partial: None }
        }
        "Eval" => NodeKind::Eval { block: stub_node() },
        "ExpressionStatement" => NodeKind::ExpressionStatement { expression: stub_node() },
        "For" => NodeKind::For {
            init: None,
            condition: None,
            update: None,
            body: stub_node(),
            continue_block: None,
        },
        "Foreach" => NodeKind::Foreach {
            variable: stub_node(),
            list: stub_node(),
            body: stub_node(),
            continue_block: None,
        },
        "Format" => NodeKind::Format { name: String::new(), body: String::new() },
        "FunctionCall" => NodeKind::FunctionCall { name: String::new(), args: vec![] },
        "Given" => NodeKind::Given { expr: stub_node(), body: stub_node() },
        "Glob" => NodeKind::Glob { pattern: String::new() },
        "Goto" => NodeKind::Goto { target: stub_node() },
        "HashLiteral" => NodeKind::HashLiteral { pairs: vec![] },
        "Heredoc" => NodeKind::Heredoc {
            delimiter: String::new(),
            content: String::new(),
            interpolated: false,
            indented: false,
            command: false,
            body_span: None,
        },
        "Identifier" => NodeKind::Identifier { name: String::new() },
        "If" => NodeKind::If {
            condition: stub_node(),
            then_branch: stub_node(),
            elsif_branches: vec![],
            else_branch: None,
        },
        "IndirectCall" => {
            NodeKind::IndirectCall { method: String::new(), object: stub_node(), args: vec![] }
        }
        "LabeledStatement" => {
            NodeKind::LabeledStatement { label: String::new(), statement: stub_node() }
        }
        "LoopControl" => NodeKind::LoopControl { op: String::new(), label: None },
        "MandatoryParameter" => NodeKind::MandatoryParameter { variable: stub_node() },
        "Match" => NodeKind::Match {
            expr: stub_node(),
            pattern: String::new(),
            modifiers: String::new(),
            has_embedded_code: false,
            negated: false,
        },
        "Method" => NodeKind::Method {
            name: String::new(),
            attributes: vec![],
            signature: None,
            body: stub_node(),
        },
        "MethodCall" => {
            NodeKind::MethodCall { object: stub_node(), method: String::new(), args: vec![] }
        }
        "MissingBlock" => NodeKind::MissingBlock,
        "MissingExpression" => NodeKind::MissingExpression,
        "MissingIdentifier" => NodeKind::MissingIdentifier,
        "MissingStatement" => NodeKind::MissingStatement,
        "NamedParameter" => NodeKind::NamedParameter { variable: stub_node() },
        "No" => NodeKind::No { module: String::new(), args: vec![], has_filter_risk: false },
        "Number" => NodeKind::Number { value: String::new() },
        "OptionalParameter" => {
            NodeKind::OptionalParameter { variable: stub_node(), default_value: stub_node() }
        }
        "Package" => NodeKind::Package { name: String::new(), name_span: stub_loc, block: None },
        "PhaseBlock" => {
            NodeKind::PhaseBlock { phase: String::new(), phase_span: None, block: stub_node() }
        }
        "Program" => NodeKind::Program { statements: vec![] },
        "Prototype" => NodeKind::Prototype { content: String::new() },
        "Readline" => NodeKind::Readline { filehandle: None },
        "Regex" => NodeKind::Regex {
            pattern: String::new(),
            replacement: None,
            modifiers: String::new(),
            has_embedded_code: false,
        },
        "Return" => NodeKind::Return { value: None },
        "Signature" => NodeKind::Signature { parameters: vec![] },
        "SlurpyParameter" => NodeKind::SlurpyParameter { variable: stub_node() },
        "StatementModifier" => NodeKind::StatementModifier {
            statement: stub_node(),
            modifier: String::new(),
            condition: stub_node(),
        },
        "String" => NodeKind::String { value: String::new(), interpolated: false },
        "Subroutine" => NodeKind::Subroutine {
            name: None,
            name_span: None,
            prototype: None,
            signature: None,
            attributes: vec![],
            body: stub_node(),
        },
        "Substitution" => NodeKind::Substitution {
            expr: stub_node(),
            pattern: String::new(),
            replacement: String::new(),
            modifiers: String::new(),
            has_embedded_code: false,
            negated: false,
        },
        "Ternary" => NodeKind::Ternary {
            condition: stub_node(),
            then_expr: stub_node(),
            else_expr: stub_node(),
        },
        "Tie" => NodeKind::Tie { variable: stub_node(), package: stub_node(), args: vec![] },
        "Transliteration" => NodeKind::Transliteration {
            expr: stub_node(),
            search: String::new(),
            replace: String::new(),
            modifiers: String::new(),
            negated: false,
        },
        "Try" => NodeKind::Try { body: stub_node(), catch_blocks: vec![], finally_block: None },
        "Typeglob" => NodeKind::Typeglob { name: String::new() },
        "Unary" => NodeKind::Unary { op: String::new(), operand: stub_node() },
        "Undef" => NodeKind::Undef,
        "UnknownRest" => NodeKind::UnknownRest,
        "Untie" => NodeKind::Untie { variable: stub_node() },
        "Use" => NodeKind::Use { module: String::new(), args: vec![], has_filter_risk: false },
        "Variable" => NodeKind::Variable { sigil: String::new(), name: String::new() },
        "VariableDeclaration" => NodeKind::VariableDeclaration {
            declarator: String::new(),
            variable: stub_node(),
            initializer: None,
            attributes: vec![],
        },
        "VariableListDeclaration" => NodeKind::VariableListDeclaration {
            declarator: String::new(),
            variables: vec![],
            attributes: vec![],
            initializer: None,
        },
        "VariableWithAttributes" => {
            NodeKind::VariableWithAttributes { variable: stub_node(), attributes: vec![] }
        }
        "When" => NodeKind::When { condition: stub_node(), body: stub_node() },
        "While" => {
            NodeKind::While { condition: stub_node(), body: stub_node(), continue_block: None }
        }
        _ => return None,
    };
    Some((kind.category(), kind.flags()))
}

// ---------------------------------------------------------------------------
// Receipt helpers
// ---------------------------------------------------------------------------

/// Returns the list of active flag names for a [`NodeKindFlags`] struct.
fn flags_to_list(flags: &NodeKindFlags) -> Vec<&'static str> {
    let mut active = Vec::new();
    if flags.executable {
        active.push("executable");
    }
    if flags.introduces_scope {
        active.push("introduces_scope");
    }
    if flags.declares_symbol {
        active.push("declares_symbol");
    }
    if flags.references_symbol {
        active.push("references_symbol");
    }
    if flags.contains_children {
        active.push("contains_children");
    }
    if flags.recovery_artifact {
        active.push("recovery_artifact");
    }
    if flags.safe_for_breakpoint {
        active.push("safe_for_breakpoint");
    }
    active
}

/// Returns current git commit short SHA, or `"unknown"` on failure.
fn current_commit(root: &Path) -> String {
    std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy(), "rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

// ---------------------------------------------------------------------------
// Per-variant frequency map
// ---------------------------------------------------------------------------

/// Parse the corpus and return per-variant frequency counts.
///
/// Returns an empty map when the corpus path does not exist or parsing fails.
fn corpus_frequency_map(corpus_path: &Path) -> HashMap<String, usize> {
    use crate::tasks::corpus_audit::{
        analyze_nodekind_coverage_pub, parse_corpus_files_pub, parse_corpus_pub,
    };

    let corpus_files = match parse_corpus_files_pub(corpus_path) {
        Ok(files) => files,
        Err(_) => return HashMap::new(),
    };

    let timeout = Duration::from_secs(30);
    let parse_results = match parse_corpus_pub(&corpus_files, timeout) {
        Ok(r) => r,
        Err(_) => return HashMap::new(),
    };
    let stats = analyze_nodekind_coverage_pub(&parse_results);
    stats.frequency
}

// ---------------------------------------------------------------------------
// Receipt generation
// ---------------------------------------------------------------------------

/// Generate the JSON receipt and return it as a String.
///
/// The receipt is written to `target/receipts/nodekind_inventory.json` by
/// [`run_nodekind_subsystem`] — this function only builds the JSON text.
pub fn generate_nodekind_inventory_receipt(root: &Path) -> Result<String> {
    // Discover the corpus from the repo root (test_corpus/, tree-sitter corpus,
    // etc.) — same entry point parser.rs uses. Pointing at a single crate dir
    // finds no .pl files and yields a false 0/69 coverage.
    let corpus_path = root.to_path_buf();
    let timeout = Duration::from_secs(30);

    // Use StatusSummary for aggregate counts (already cached in corpus_audit)
    let summary = compute_status_summary(&corpus_path, timeout).unwrap_or_else(|_| {
        // If corpus isn't available fall back to synthetic totals
        crate::tasks::corpus_audit::StatusSummary {
            total_files: 0,
            ok_files: 0,
            error_files: 0,
            timeout_files: 0,
            panic_files: 0,
            test_corpus_files: 0,
            perl_corpus_files: 0,
            nodekind_covered: 0,
            nodekind_total: NodeKind::ALL_KIND_NAMES.len(),
            nodekind_never_seen: NodeKind::ALL_KIND_NAMES.len(),
            nodekind_allowlisted_never_seen: 0,
            nodekind_actionable_never_seen: 0,
            nodekind_allowlisted_names: vec![],
            ga_covered: 0,
            ga_total: 0,
        }
    });

    // Per-variant frequency for the covered column
    let freq_map = corpus_frequency_map(&corpus_path);

    let commit = current_commit(root);
    let total_count = NodeKind::ALL_KIND_NAMES.len();
    let covered_count = summary.nodekind_covered;
    let never_seen_count = total_count.saturating_sub(covered_count);
    let coverage_pct =
        if total_count == 0 { 0.0_f64 } else { 100.0 * covered_count as f64 / total_count as f64 };
    let coverage_pct_rounded = (coverage_pct * 10.0).round() / 10.0;

    let recovery_set: std::collections::HashSet<&'static str> =
        NodeKind::RECOVERY_KIND_NAMES.iter().copied().collect();

    let mut variants = Vec::with_capacity(total_count);
    for &name in NodeKind::ALL_KIND_NAMES {
        let corpus_frequency = freq_map.get(name).copied().unwrap_or(0);
        let covered = corpus_frequency > 0;
        let (category, flags) =
            classify(name).ok_or_else(|| eyre!("unknown NodeKind name: {name}"))?;
        let allowlisted_reason = if !covered && recovery_set.contains(name) {
            Some("Synthetic recovery node; only produced on malformed input")
        } else {
            None
        };
        variants.push(serde_json::json!({
            "name": name,
            "category": format!("{category:?}").to_lowercase(),
            "flags": flags_to_list(&flags),
            "covered": covered,
            "corpus_frequency": corpus_frequency,
            "allowlisted_reason": allowlisted_reason,
            "doc_status": "missing",
            "example_snippet": null
        }));
    }

    let receipt = serde_json::json!({
        "schema_version": 1,
        "commit": commit,
        "generated_by": "cargo xtask update-status --write --only nodekind",
        "total_count": total_count,
        "covered_count": covered_count,
        "never_seen_count": never_seen_count,
        "coverage_percentage": coverage_pct_rounded,
        "actionable_never_seen_count": summary.nodekind_actionable_never_seen,
        "allowlisted_never_seen_count": summary.nodekind_allowlisted_never_seen,
        "variants": variants,
    });

    serde_json::to_string_pretty(&receipt).context("serializing nodekind inventory receipt")
}

// ---------------------------------------------------------------------------
// Dashboard rendering
// ---------------------------------------------------------------------------

/// Update all marker blocks in `original_md` and return the new file text.
pub fn generate_nodekind_status(receipt_json: &str, original_md: &str) -> Result<String> {
    let receipt: serde_json::Value =
        serde_json::from_str(receipt_json).context("parsing nodekind receipt")?;
    let mut text = original_md.to_string();

    text = replace_block(
        &text,
        "<!-- BEGIN: NODEKIND_COVERAGE_SUMMARY -->",
        "<!-- END: NODEKIND_COVERAGE_SUMMARY -->",
        &render_coverage_summary(&receipt),
    )?;

    text = replace_block(
        &text,
        "<!-- BEGIN: NODEKIND_CATEGORY_TABLE -->",
        "<!-- END: NODEKIND_CATEGORY_TABLE -->",
        &render_category_table(&receipt),
    )?;

    text = replace_block(
        &text,
        "<!-- BEGIN: NODEKIND_VARIANT_TABLE -->",
        "<!-- END: NODEKIND_VARIANT_TABLE -->",
        &render_variant_table(&receipt),
    )?;

    text = replace_block(
        &text,
        "<!-- BEGIN: NODEKIND_ALLOWLIST -->",
        "<!-- END: NODEKIND_ALLOWLIST -->",
        &render_allowlist_table(&receipt),
    )?;

    text = replace_block(
        &text,
        "<!-- BEGIN: NODEKIND_GAPS -->",
        "<!-- END: NODEKIND_GAPS -->",
        &render_gaps_table(&receipt),
    )?;

    Ok(text)
}

fn render_coverage_summary(receipt: &serde_json::Value) -> String {
    let total = receipt["total_count"].as_u64().unwrap_or(0);
    let covered = receipt["covered_count"].as_u64().unwrap_or(0);
    let never_seen = receipt["never_seen_count"].as_u64().unwrap_or(0);
    let actionable = receipt["actionable_never_seen_count"].as_u64().unwrap_or(0);
    let pct = receipt["coverage_percentage"].as_f64().unwrap_or(0.0);

    format!(
        "| Metric | Value |\n\
         |--------|-------|\n\
         | Total variants | {total} |\n\
         | Covered | {covered} |\n\
         | Never-seen | {never_seen} |\n\
         | Actionable gaps | {actionable} |\n\
         | Coverage | {pct:.1}% |"
    )
}

fn render_category_table(receipt: &serde_json::Value) -> String {
    let variants = match receipt["variants"].as_array() {
        Some(v) => v,
        None => return String::new(),
    };

    // Collect totals and covered counts per category
    let mut category_total: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let mut category_covered: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();

    for v in variants {
        let cat = v["category"].as_str().unwrap_or("unknown").to_string();
        *category_total.entry(cat.clone()).or_default() += 1;
        if v["covered"].as_bool().unwrap_or(false) {
            *category_covered.entry(cat).or_default() += 1;
        }
    }

    let mut rows = String::from(
        "| Category | Variants | Covered | % |\n\
         |----------|----------|---------|---|\n",
    );
    for (cat, total) in &category_total {
        let cov = category_covered.get(cat).copied().unwrap_or(0);
        let pct = if *total == 0 { 0.0 } else { 100.0 * cov as f64 / *total as f64 };
        rows.push_str(&format!("| {cat} | {total} | {cov} | {pct:.0}% |\n"));
    }
    // Strip trailing newline for clean block insertion
    rows.trim_end_matches('\n').to_string()
}

fn render_variant_table(receipt: &serde_json::Value) -> String {
    let variants = match receipt["variants"].as_array() {
        Some(v) => v,
        None => return String::new(),
    };

    let mut table = String::from(
        "| Variant | Category | executable | introduces_scope | declares_symbol | \
         references_symbol | contains_children | recovery_artifact | safe_for_breakpoint | Covered |\n\
         |---------|----------|-----------|-----------------|----------------|--------------------|-----------------|------------------|--------------------|---------|\n",
    );

    for v in variants {
        let name = v["name"].as_str().unwrap_or("");
        let cat = v["category"].as_str().unwrap_or("");
        let flags = v["flags"]
            .as_array()
            .map(|f| f.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        let flag_cell = |f: &str| if flags.contains(&f) { "yes" } else { "" };
        let covered = if v["covered"].as_bool().unwrap_or(false) { "yes" } else { "no" };

        table.push_str(&format!(
            "| {name} | {cat} | {} | {} | {} | {} | {} | {} | {} | {covered} |\n",
            flag_cell("executable"),
            flag_cell("introduces_scope"),
            flag_cell("declares_symbol"),
            flag_cell("references_symbol"),
            flag_cell("contains_children"),
            flag_cell("recovery_artifact"),
            flag_cell("safe_for_breakpoint"),
        ));
    }
    table.trim_end_matches('\n').to_string()
}

fn render_allowlist_table(receipt: &serde_json::Value) -> String {
    let variants = match receipt["variants"].as_array() {
        Some(v) => v,
        None => return String::new(),
    };

    let allowlisted: Vec<_> =
        variants.iter().filter(|v| v["allowlisted_reason"].is_string()).collect();

    if allowlisted.is_empty() {
        return "_No allowlisted variants._".to_string();
    }

    let mut table = String::from(
        "| Variant | Reason |\n\
         |---------|--------|\n",
    );
    for v in allowlisted {
        let name = v["name"].as_str().unwrap_or("");
        let reason = v["allowlisted_reason"].as_str().unwrap_or("");
        table.push_str(&format!("| {name} | {reason} |\n"));
    }
    table.trim_end_matches('\n').to_string()
}

fn render_gaps_table(receipt: &serde_json::Value) -> String {
    let variants = match receipt["variants"].as_array() {
        Some(v) => v,
        None => return String::new(),
    };

    let actionable: Vec<_> = variants
        .iter()
        .filter(|v| !v["covered"].as_bool().unwrap_or(false) && v["allowlisted_reason"].is_null())
        .collect();

    if actionable.is_empty() {
        return "_No actionable never-seen NodeKinds. \
                All uncovered variants are intentionally allowlisted._"
            .to_string();
    }

    let mut table = String::from(
        "| Variant | Category | Corpus Frequency |\n\
         |---------|----------|------------------|\n",
    );
    for v in actionable {
        let name = v["name"].as_str().unwrap_or("");
        let cat = v["category"].as_str().unwrap_or("");
        let freq = v["corpus_frequency"].as_u64().unwrap_or(0);
        table.push_str(&format!("| {name} | {cat} | {freq} |\n"));
    }
    table.trim_end_matches('\n').to_string()
}

// ---------------------------------------------------------------------------
// Entry point called from mod.rs
// ---------------------------------------------------------------------------

pub fn run_nodekind_subsystem(
    root: &Path,
    files_to_update: &mut Vec<(&'static str, PathBuf, String)>,
) -> Result<()> {
    let receipt_json = generate_nodekind_inventory_receipt(root)?;

    // Write receipt to target/receipts/nodekind_inventory.json (not committed)
    let receipt_path = root.join("target/receipts/nodekind_inventory.json");
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent).context("creating target/receipts/")?;
    }
    fs::write(&receipt_path, &receipt_json).context("writing nodekind_inventory.json")?;

    // Update nodekind.md
    let nodekind_path = root.join("docs/project/status/nodekind.md");
    let original_md =
        fs::read_to_string(&nodekind_path).context("reading docs/project/status/nodekind.md")?;
    let updated_md = generate_nodekind_status(&receipt_json, &original_md)?;
    if updated_md != original_md {
        files_to_update.push(("docs/project/status/nodekind.md", nodekind_path, updated_md));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// All 69 NodeKind names must be classifiable (no arm is missing from `classify()`).
    #[test]
    fn test_nodekind_classify_covers_all_69_variants() {
        let mut missing = Vec::new();
        for &name in NodeKind::ALL_KIND_NAMES {
            if classify(name).is_none() {
                missing.push(name);
            }
        }
        assert!(
            missing.is_empty(),
            "classify() is missing arms for: {missing:?} — add them to nodekind.rs"
        );
        assert_eq!(
            NodeKind::ALL_KIND_NAMES.len(),
            69,
            "expected 69 NodeKind variants in ALL_KIND_NAMES"
        );
    }

    /// For "Error": category must be Recovery, flags must include recovery_artifact,
    /// must NOT include safe_for_breakpoint, and flags must pass invariant validation.
    #[test]
    fn test_nodekind_classify_uses_real_classification() {
        let (cat, flags) = classify("Error").expect("Error must be classifiable");
        assert_eq!(cat, NodeKindCategory::Recovery, "Error must be Recovery category");
        assert!(flags.recovery_artifact, "Error must have recovery_artifact flag");
        assert!(!flags.safe_for_breakpoint, "Error must not have safe_for_breakpoint flag");
        assert!(flags.validate().is_ok(), "Error flags must satisfy invariant");
    }

    /// Every recovery variant must have recovery_artifact=true and safe_for_breakpoint=false.
    #[test]
    fn test_recovery_variants_never_safe_for_breakpoint() {
        for &name in NodeKind::RECOVERY_KIND_NAMES {
            let (_, flags) = classify(name)
                .unwrap_or_else(|| panic!("recovery kind {name} must be classifiable"));
            assert!(flags.recovery_artifact, "{name} is recovery but recovery_artifact is false");
            assert!(
                !flags.safe_for_breakpoint,
                "{name} is recovery but safe_for_breakpoint is true"
            );
        }
    }

    /// All NodeKind flags must satisfy the recovery_artifact / safe_for_breakpoint invariant.
    #[test]
    fn test_all_variants_flags_pass_invariant() {
        for &name in NodeKind::ALL_KIND_NAMES {
            let (_, flags) =
                classify(name).unwrap_or_else(|| panic!("variant {name} must be classifiable"));
            assert!(
                flags.validate().is_ok(),
                "flags for {name} fail invariant: {:?}",
                flags.validate().unwrap_err()
            );
        }
    }

    /// Marker blocks must be present in the committed nodekind.md file.
    #[test]
    fn test_nodekind_md_has_all_marker_blocks() -> Result<()> {
        let root = crate::utils::project_root()?;
        let path = root.join("docs/project/status/nodekind.md");
        let content =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        for marker in &[
            "NODEKIND_COVERAGE_SUMMARY",
            "NODEKIND_CATEGORY_TABLE",
            "NODEKIND_VARIANT_TABLE",
            "NODEKIND_ALLOWLIST",
            "NODEKIND_GAPS",
        ] {
            assert!(
                content.contains(&format!("<!-- BEGIN: {marker} -->")),
                "nodekind.md missing BEGIN marker for {marker}"
            );
            assert!(
                content.contains(&format!("<!-- END: {marker} -->")),
                "nodekind.md missing END marker for {marker}"
            );
        }
        Ok(())
    }

    /// `generate_nodekind_status` is idempotent: applying it twice produces
    /// the same markdown output.
    #[test]
    fn test_nodekind_status_generation_is_idempotent() -> Result<()> {
        // Build a minimal mock receipt
        let receipt = serde_json::json!({
            "schema_version": 1,
            "commit": "abc1234",
            "generated_by": "test",
            "total_count": 2,
            "covered_count": 1,
            "never_seen_count": 1,
            "coverage_percentage": 50.0,
            "actionable_never_seen_count": 0,
            "allowlisted_never_seen_count": 1,
            "variants": [
                {
                    "name": "Program",
                    "category": "program",
                    "flags": ["contains_children", "introduces_scope"],
                    "covered": true,
                    "corpus_frequency": 42,
                    "allowlisted_reason": null,
                    "doc_status": "missing",
                    "example_snippet": null
                },
                {
                    "name": "Error",
                    "category": "recovery",
                    "flags": ["recovery_artifact"],
                    "covered": false,
                    "corpus_frequency": 0,
                    "allowlisted_reason": "Synthetic recovery node; only produced on malformed input",
                    "doc_status": "missing",
                    "example_snippet": null
                }
            ]
        });
        let receipt_json = serde_json::to_string_pretty(&receipt)?;

        let seed = "# NodeKind Coverage Dashboard\n\
\n\
<!-- BEGIN: NODEKIND_COVERAGE_SUMMARY -->\n\
<!-- END: NODEKIND_COVERAGE_SUMMARY -->\n\
\n\
## Category Breakdown\n\
\n\
<!-- BEGIN: NODEKIND_CATEGORY_TABLE -->\n\
<!-- END: NODEKIND_CATEGORY_TABLE -->\n\
\n\
## Per-Variant Table\n\
\n\
<!-- BEGIN: NODEKIND_VARIANT_TABLE -->\n\
<!-- END: NODEKIND_VARIANT_TABLE -->\n\
\n\
## Allowlisted Never-Seen\n\
\n\
<!-- BEGIN: NODEKIND_ALLOWLIST -->\n\
<!-- END: NODEKIND_ALLOWLIST -->\n\
\n\
## Actionable Gaps\n\
\n\
<!-- BEGIN: NODEKIND_GAPS -->\n\
<!-- END: NODEKIND_GAPS -->\n";

        let first_pass = generate_nodekind_status(&receipt_json, seed)?;
        let second_pass = generate_nodekind_status(&receipt_json, &first_pass)?;
        assert_eq!(first_pass, second_pass, "generate_nodekind_status must be idempotent");
        Ok(())
    }
}
