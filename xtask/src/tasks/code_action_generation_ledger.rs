//! Validate the machine-readable code-action provider-generation ledger.
//!
//! The ledger (#9188) freezes, per `(generation, family)`, what production code
//! actions do before #9189 changes routing and #9190 deletes a generation. This
//! check keeps the frozen record honest in both directions: a ledger row cannot
//! survive the code it describes, and a new code-action module cannot land
//! without an exact family disposition.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

const POLICY_PATH: &str = "policy/code-action-generation-ledger.toml";
const HUMAN_LEDGER: &str = "docs/project/status/code_action_generation_ledger.md";
const POLICY_NAME: &str = "code-action-generation-ledger";
const ORCHESTRATOR: &str = "crates/perl-lsp-rs/src/runtime/language/code_actions.rs";
const PARITY_CORPUS: &str = "crates/perl-lsp-rs/tests/code_action_generation_parity_corpus.rs";

/// The six-way disposition vocabulary fixed by #9188.
const REQUIRED_DISPOSITIONS: &[&str] = &[
    "canonical_candidate",
    "unique_behavior",
    "redundant_behavior",
    "compatibility_only",
    "shadow_only_candidate",
    "retire_candidate",
];

const REQUIRED_REACHABILITY: &[&str] = &["production", "unreachable_stub"];

/// Prefix every parity-corpus fixture id shares, so the corpus can be scanned
/// for fixture identities without parsing Rust.
const FIXTURE_PREFIX: &str = "cac-parity-";

const EMPTY_CELL: &str = "—";

#[derive(Debug, Deserialize)]
struct Ledger {
    schema_version: u32,
    policy: String,
    human_ledger: String,
    orchestrator: String,
    parity_corpus: String,
    disposition_states: Vec<String>,
    family_registry: Vec<String>,
    retirement_blocker_registry: Vec<String>,
    owned_module_roots: Vec<String>,
    #[serde(default)]
    generation: Vec<Generation>,
    #[serde(default)]
    route: Vec<Route>,
}

#[derive(Debug, Deserialize)]
struct Generation {
    id: String,
    stage: u32,
    path: String,
    reachability: String,
    #[serde(default)]
    production_anchor: Option<String>,
    modules: Vec<String>,
    summary: String,
}

#[derive(Debug, Deserialize)]
struct Route {
    generation: String,
    family: String,
    disposition: String,
    notes: String,
    retirement_blockers: Vec<String>,
    parity_fixtures: Vec<String>,
    /// Why this row has no executable parity evidence.
    ///
    /// A row is either covered by the corpus or honestly declared uncovered;
    /// #9188 records `NOT_PROVEN` rather than letting a gap read as coverage.
    #[serde(default)]
    proof_gap: Option<String>,
}

#[derive(Debug)]
struct MarkdownTable {
    header: Vec<String>,
    rows: Vec<TableRow>,
}

#[derive(Debug)]
struct TableRow {
    line_number: usize,
    cells: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LedgerRow {
    key: String,
    disposition: String,
    retirement_blockers: String,
}

#[derive(Debug)]
struct ValidationStats {
    generations: usize,
    routes: usize,
    families: usize,
    fixtures: usize,
    proof_gaps: usize,
}

pub fn run() -> Result<()> {
    let root = project_root()?;
    let stats = validate(&root)?;
    println!(
        "code-action generation ledger check passed: {} generations, {} (generation, family) routes, {} families, {} parity fixtures, {} declared proof gaps",
        stats.generations, stats.routes, stats.families, stats.fixtures, stats.proof_gaps
    );
    Ok(())
}

fn validate(root: &Path) -> Result<ValidationStats> {
    let ledger = read_ledger(root, POLICY_PATH)?;
    let human_text = read_text(root, HUMAN_LEDGER)?;
    let orchestrator_text = read_text(root, ORCHESTRATOR)?;
    let corpus_text = read_text(root, PARITY_CORPUS)?;
    let human_table = parse_table(&human_text, "Generation").ok_or_else(|| {
        color_eyre::eyre::eyre!("{HUMAN_LEDGER}: disposition ledger table not found")
    })?;

    let mut violations = Vec::new();
    validate_ledger_shape(&ledger, &mut violations);
    validate_generations(root, &ledger, &orchestrator_text, &mut violations);
    validate_routes(&ledger, &mut violations);
    validate_module_coverage(root, &ledger, &mut violations);
    validate_parity_fixtures(&ledger, &corpus_text, &mut violations);
    validate_human_table(&human_table, &mut violations);
    validate_ledger_matches_human(&ledger, &human_table, &mut violations);

    if !violations.is_empty() {
        eprintln!("code-action generation ledger violations:");
        for violation in &violations {
            eprintln!("  - {violation}");
        }
        bail!("code-action generation ledger check failed with {} violation(s)", violations.len());
    }

    let families = ledger.route.iter().map(|row| row.family.as_str()).collect::<BTreeSet<_>>();
    let fixtures = ledger_fixture_ids(&ledger);

    Ok(ValidationStats {
        generations: ledger.generation.len(),
        routes: ledger.route.len(),
        families: families.len(),
        fixtures: fixtures.len(),
        proof_gaps: ledger
            .route
            .iter()
            .filter(|route| {
                route.proof_gap.as_deref().map(str::trim).is_some_and(|gap| !gap.is_empty())
            })
            .count(),
    })
}

fn read_ledger(root: &Path, rel: &str) -> Result<Ledger> {
    let text = read_text(root, rel)?;
    toml::from_str(&text).with_context(|| format!("failed to parse {rel}"))
}

fn read_text(root: &Path, rel: &str) -> Result<String> {
    let path = root.join(rel);
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

fn validate_ledger_shape(ledger: &Ledger, violations: &mut Vec<String>) {
    if ledger.schema_version != 1 {
        violations.push(format!(
            "{POLICY_PATH}: schema_version is {}; expected 1",
            ledger.schema_version
        ));
    }
    require_field(POLICY_PATH, "policy", &ledger.policy, POLICY_NAME, violations);
    require_field(POLICY_PATH, "human_ledger", &ledger.human_ledger, HUMAN_LEDGER, violations);
    require_field(POLICY_PATH, "orchestrator", &ledger.orchestrator, ORCHESTRATOR, violations);
    require_field(POLICY_PATH, "parity_corpus", &ledger.parity_corpus, PARITY_CORPUS, violations);

    require_exact_set(
        POLICY_PATH,
        "disposition_states",
        &ledger.disposition_states,
        REQUIRED_DISPOSITIONS,
        violations,
    );

    require_unique_non_empty(POLICY_PATH, "family_registry", &ledger.family_registry, violations);
    require_unique_non_empty(
        POLICY_PATH,
        "retirement_blocker_registry",
        &ledger.retirement_blocker_registry,
        violations,
    );
    require_unique_non_empty(
        POLICY_PATH,
        "owned_module_roots",
        &ledger.owned_module_roots,
        violations,
    );

    if ledger.generation.is_empty() {
        violations.push(format!("{POLICY_PATH}: generation must not be empty"));
    }
    if ledger.route.is_empty() {
        violations.push(format!("{POLICY_PATH}: route must not be empty"));
    }
}

fn validate_generations(
    root: &Path,
    ledger: &Ledger,
    orchestrator_text: &str,
    violations: &mut Vec<String>,
) {
    let reachability_states = REQUIRED_REACHABILITY.iter().copied().collect::<BTreeSet<_>>();
    let mut seen_ids = BTreeSet::new();
    let mut seen_stages: BTreeMap<u32, String> = BTreeMap::new();

    for generation in &ledger.generation {
        let id = generation.id.trim();
        if id.is_empty() {
            violations.push(format!("{POLICY_PATH}: generation id must not be empty"));
            continue;
        }
        if !seen_ids.insert(id.to_string()) {
            violations.push(format!("{POLICY_PATH}: duplicate generation id {id:?}"));
        }
        if generation.summary.trim().is_empty() {
            violations.push(format!("{POLICY_PATH}: generation {id} summary must not be empty"));
        }
        if generation.path.trim().is_empty() {
            violations.push(format!("{POLICY_PATH}: generation {id} path must not be empty"));
        }
        if !reachability_states.contains(generation.reachability.as_str()) {
            violations.push(format!(
                "{POLICY_PATH}: generation {id} reachability {:?} is not one of {REQUIRED_REACHABILITY:?}",
                generation.reachability
            ));
        }
        if generation.modules.is_empty() {
            violations.push(format!("{POLICY_PATH}: generation {id} modules must not be empty"));
        }
        for module in &generation.modules {
            if !root.join(module).is_file() {
                violations.push(format!(
                    "{POLICY_PATH}: generation {id} names a module that does not exist: {module}"
                ));
            }
        }

        match generation.reachability.as_str() {
            "production" => {
                if let Some(previous) = seen_stages.insert(generation.stage, id.to_string()) {
                    violations.push(format!(
                        "{POLICY_PATH}: generations {previous} and {id} both claim production stage {}",
                        generation.stage
                    ));
                }
                match generation.production_anchor.as_deref().map(str::trim) {
                    None | Some("") => violations.push(format!(
                        "{POLICY_PATH}: generation {id} is production-reachable but has no production_anchor"
                    )),
                    Some(anchor) => {
                        if !orchestrator_text.contains(anchor) {
                            violations.push(format!(
                                "{POLICY_PATH}: generation {id} production_anchor {anchor:?} no longer occurs in {ORCHESTRATOR}"
                            ));
                        }
                    }
                }
            }
            "unreachable_stub" if generation.production_anchor.is_some() => {
                violations.push(format!(
                    "{POLICY_PATH}: generation {id} is an unreachable stub but declares a production_anchor"
                ));
            }
            _ => {}
        }
    }
}

fn validate_routes(ledger: &Ledger, violations: &mut Vec<String>) {
    let dispositions =
        ledger.disposition_states.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let families = ledger.family_registry.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let blockers =
        ledger.retirement_blocker_registry.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let generation_ids =
        ledger.generation.iter().map(|row| row.id.as_str()).collect::<BTreeSet<_>>();

    let mut seen_rows = BTreeSet::new();
    let mut canonical_by_family: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut generations_with_routes = BTreeSet::new();

    for route in &ledger.route {
        let key = row_key(&route.generation, &route.family);
        if !seen_rows.insert(key.clone()) {
            violations.push(format!("{POLICY_PATH}: duplicate route row {key}"));
        }
        generations_with_routes.insert(route.generation.as_str());

        if !generation_ids.contains(route.generation.as_str()) {
            violations.push(format!(
                "{POLICY_PATH}: route {key} names generation {:?}, which has no generation row",
                route.generation
            ));
        }
        if !families.contains(route.family.as_str()) {
            violations.push(format!(
                "{POLICY_PATH}: route {key} family {:?} is missing from family_registry",
                route.family
            ));
        }
        if !dispositions.contains(route.disposition.as_str()) {
            violations.push(format!(
                "{POLICY_PATH}: route {key} disposition {:?} is not listed in disposition_states",
                route.disposition
            ));
        }
        if route.notes.trim().is_empty() {
            violations.push(format!("{POLICY_PATH}: route {key} notes must not be empty"));
        }

        for blocker in &route.retirement_blockers {
            if !blockers.contains(blocker.as_str()) {
                violations.push(format!(
                    "{POLICY_PATH}: route {key} retirement blocker {blocker:?} is missing from retirement_blocker_registry"
                ));
            }
        }

        if route.disposition == "canonical_candidate" {
            canonical_by_family
                .entry(route.family.as_str())
                .or_default()
                .push(route.generation.as_str());
        }

        // A generation kept for behavior nothing else reproduces must say what
        // reproducing it would take, or #9190 cannot tell when it is safe.
        if route.disposition == "unique_behavior" && route.retirement_blockers.is_empty() {
            violations.push(format!(
                "{POLICY_PATH}: route {key} is unique_behavior but cites no retirement blocker"
            ));
        }

        // Everything except a row with no production behavior needs evidence
        // that survives the generation it describes — or an explicit statement
        // that it has none, so #9189 cannot mistake a gap for coverage.
        let declared_gap = route.proof_gap.as_deref().map(str::trim).unwrap_or_default();
        if route.disposition != "retire_candidate"
            && route.parity_fixtures.is_empty()
            && declared_gap.is_empty()
        {
            violations.push(format!(
                "{POLICY_PATH}: route {key} has disposition {:?} but names neither a parity fixture nor a proof_gap",
                route.disposition
            ));
        }
        if !declared_gap.is_empty() && !route.parity_fixtures.is_empty() {
            violations.push(format!(
                "{POLICY_PATH}: route {key} declares a proof_gap and also names parity fixtures; a covered row has no gap"
            ));
        }

        for fixture in &route.parity_fixtures {
            if !fixture.starts_with(FIXTURE_PREFIX) {
                violations.push(format!(
                    "{POLICY_PATH}: route {key} parity fixture {fixture:?} does not start with {FIXTURE_PREFIX:?}"
                ));
            }
        }
    }

    // The invariant the whole train exists to reach: one canonical production
    // publisher per exact action family.
    for (family, generations) in &canonical_by_family {
        if generations.len() > 1 {
            violations.push(format!(
                "{POLICY_PATH}: duplicate production authority — family {family:?} has {} canonical_candidate generations: {}",
                generations.len(),
                generations.join(", ")
            ));
        }
    }

    for generation in &ledger.generation {
        if !generations_with_routes.contains(generation.id.as_str()) {
            violations.push(format!(
                "{POLICY_PATH}: generation {} has no route row, so it carries no family disposition",
                generation.id
            ));
        }
    }
}

fn validate_module_coverage(root: &Path, ledger: &Ledger, violations: &mut Vec<String>) {
    let claimed = ledger
        .generation
        .iter()
        .flat_map(|generation| generation.modules.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();

    for module_root in &ledger.owned_module_roots {
        let absolute = root.join(module_root);
        if !absolute.is_dir() {
            violations.push(format!(
                "{POLICY_PATH}: owned_module_roots entry is not a directory: {module_root}"
            ));
            continue;
        }

        for entry in WalkDir::new(&absolute).sort_by_file_name() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    violations
                        .push(format!("{POLICY_PATH}: failed to walk {module_root}: {error}"));
                    continue;
                }
            };
            if !entry.file_type().is_file()
                || entry.path().extension().is_none_or(|ext| ext != "rs")
            {
                continue;
            }
            let Ok(relative) = entry.path().strip_prefix(root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            if !claimed.contains(relative.as_str()) {
                violations.push(format!(
                    "{POLICY_PATH}: {relative} is under owned module root {module_root} but is claimed by no generation"
                ));
            }
        }
    }
}

fn validate_parity_fixtures(ledger: &Ledger, corpus_text: &str, violations: &mut Vec<String>) {
    let declared = ledger_fixture_ids(ledger);
    let present = corpus_fixture_ids(corpus_text);

    for missing in declared.difference(&present) {
        violations.push(format!(
            "{POLICY_PATH}: parity fixture {missing:?} is named by a route but absent from {PARITY_CORPUS}"
        ));
    }
    for unclaimed in present.difference(&declared) {
        violations.push(format!(
            "{PARITY_CORPUS}: parity fixture {unclaimed:?} is not claimed by any ledger route"
        ));
    }
}

fn ledger_fixture_ids(ledger: &Ledger) -> BTreeSet<String> {
    ledger
        .route
        .iter()
        .flat_map(|route| route.parity_fixtures.iter().cloned())
        .collect::<BTreeSet<_>>()
}

/// Collect every `cac-parity-*` identity literal in the corpus source.
///
/// The corpus declares its fixture ids as ordinary string literals, so this
/// stays a text scan rather than a Rust parse. Ids are restricted to the
/// characters the prefix convention allows, which is what terminates a match.
fn corpus_fixture_ids(corpus_text: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut rest = corpus_text;

    while let Some(index) = rest.find(FIXTURE_PREFIX) {
        let candidate = &rest[index..];
        let end = candidate
            .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
            .unwrap_or(candidate.len());
        let id = &candidate[..end];
        if id.len() > FIXTURE_PREFIX.len() && !id.ends_with('-') {
            ids.insert(id.to_string());
        }
        rest = &candidate[end.max(1)..];
    }

    ids
}

fn validate_human_table(table: &MarkdownTable, violations: &mut Vec<String>) {
    let expected = ["Generation", "Family", "Disposition", "Retirement blockers"];
    if table.header.len() != expected.len() {
        violations.push(format!(
            "{HUMAN_LEDGER}: disposition table has {} columns; expected {}",
            table.header.len(),
            expected.len()
        ));
        return;
    }

    for (index, (actual, expected_cell)) in table.header.iter().zip(expected.iter()).enumerate() {
        if actual != expected_cell {
            violations.push(format!(
                "{HUMAN_LEDGER}: disposition table column {} is {actual:?}; expected {expected_cell:?}",
                index + 1
            ));
        }
    }
}

fn validate_ledger_matches_human(
    ledger: &Ledger,
    table: &MarkdownTable,
    violations: &mut Vec<String>,
) {
    let mut human_rows = BTreeSet::new();
    for row in &table.rows {
        if row.cells.len() != table.header.len() {
            violations.push(format!(
                "{HUMAN_LEDGER}:{}: row has {} columns; expected {}",
                row.line_number,
                row.cells.len(),
                table.header.len()
            ));
            continue;
        }
        human_rows.insert(LedgerRow {
            key: row_key(&row.cells[0], &row.cells[1]),
            disposition: normalize_inline_code(&row.cells[2]),
            retirement_blockers: normalize_inline_code(&row.cells[3]),
        });
    }

    let policy_rows = ledger
        .route
        .iter()
        .map(|route| LedgerRow {
            key: row_key(&route.generation, &route.family),
            disposition: route.disposition.clone(),
            retirement_blockers: render_blockers(&route.retirement_blockers),
        })
        .collect::<BTreeSet<_>>();

    for row in human_rows.difference(&policy_rows) {
        violations.push(format!(
            "{POLICY_PATH}: no TOML route matches human ledger row {} ({} / {})",
            row.key, row.disposition, row.retirement_blockers
        ));
    }
    for row in policy_rows.difference(&human_rows) {
        violations.push(format!(
            "{HUMAN_LEDGER}: no table row matches TOML route {} ({} / {})",
            row.key, row.disposition, row.retirement_blockers
        ));
    }
}

fn render_blockers(blockers: &[String]) -> String {
    if blockers.is_empty() {
        return EMPTY_CELL.to_string();
    }
    blockers.join(", ")
}

fn require_field(
    doc: &str,
    field: &str,
    actual: &str,
    expected: &str,
    violations: &mut Vec<String>,
) {
    if actual != expected {
        violations.push(format!("{doc}: {field} is {actual:?}; expected {expected:?}"));
    }
}

fn require_exact_set(
    doc: &str,
    field: &str,
    actual: &[String],
    expected: &[&str],
    violations: &mut Vec<String>,
) {
    let actual_set = actual.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let expected_set = expected.iter().copied().collect::<BTreeSet<_>>();

    for missing in expected_set.difference(&actual_set) {
        violations.push(format!("{doc}: {field} missing required entry {missing:?}"));
    }
    for unexpected in actual_set.difference(&expected_set) {
        violations.push(format!("{doc}: {field} contains unsupported entry {unexpected:?}"));
    }
}

fn require_unique_non_empty(
    doc: &str,
    field: &str,
    values: &[String],
    violations: &mut Vec<String>,
) {
    if values.is_empty() {
        violations.push(format!("{doc}: {field} must not be empty"));
        return;
    }
    let mut seen = BTreeSet::new();
    for value in values {
        if value.trim().is_empty() {
            violations.push(format!("{doc}: {field} contains an empty item"));
            continue;
        }
        if !seen.insert(value.as_str()) {
            violations.push(format!("{doc}: {field} contains duplicate entry {value:?}"));
        }
    }
}

fn row_key(generation: &str, family: &str) -> String {
    format!("{}::{}", generation.trim(), family.trim())
}

fn normalize_inline_code(value: &str) -> String {
    value.trim().trim_matches('`').trim().to_string()
}

fn parse_table(text: &str, first_header_cell: &str) -> Option<MarkdownTable> {
    let lines = text.lines().collect::<Vec<_>>();
    let header_index = lines.iter().position(|line| {
        split_table_row(line)
            .and_then(|cells| cells.first().cloned())
            .is_some_and(|cell| cell == first_header_cell)
    })?;
    let header = split_table_row(lines[header_index])?;
    let mut rows = Vec::new();

    for (offset, line) in lines.iter().enumerate().skip(header_index + 1) {
        let Some(cells) = split_table_row(line) else {
            break;
        };
        if is_separator_row(&cells) {
            continue;
        }
        rows.push(TableRow { line_number: offset + 1, cells });
    }

    Some(MarkdownTable { header, rows })
}

fn split_table_row(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') {
        return None;
    }
    Some(trimmed.trim_matches('|').split('|').map(|cell| cell.trim().to_string()).collect())
}

fn is_separator_row(cells: &[String]) -> bool {
    cells.iter().all(|cell| {
        let trimmed = cell.trim();
        !trimmed.is_empty()
            && trimmed.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
            && trimmed.contains('-')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation(id: &str, anchor: Option<&str>) -> Generation {
        Generation {
            id: id.to_string(),
            stage: 1,
            path: "ast".to_string(),
            reachability: "production".to_string(),
            production_anchor: anchor.map(str::to_string),
            modules: vec![ORCHESTRATOR.to_string()],
            summary: "summary".to_string(),
        }
    }

    fn route(generation: &str, family: &str, disposition: &str) -> Route {
        Route {
            generation: generation.to_string(),
            family: family.to_string(),
            disposition: disposition.to_string(),
            notes: "notes".to_string(),
            retirement_blockers: Vec::new(),
            parity_fixtures: vec!["cac-parity-example".to_string()],
            proof_gap: None,
        }
    }

    fn ledger(generations: Vec<Generation>, routes: Vec<Route>) -> Ledger {
        Ledger {
            schema_version: 1,
            policy: POLICY_NAME.to_string(),
            human_ledger: HUMAN_LEDGER.to_string(),
            orchestrator: ORCHESTRATOR.to_string(),
            parity_corpus: PARITY_CORPUS.to_string(),
            disposition_states: REQUIRED_DISPOSITIONS
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            family_registry: vec!["quickfix:diagnostic_routed".to_string()],
            retirement_blocker_registry: vec![
                "canonical_route_omits_diagnostic_association".to_string(),
            ],
            owned_module_roots: vec![
                "crates/perl-lsp-rs-core/src/providers/code_actions".to_string(),
            ],
            generation: generations,
            route: routes,
        }
    }

    /// The invariant #8393 exists to reach: two production answers for one
    /// exact action family must fail the contract.
    #[test]
    fn rejects_two_canonical_candidates_for_one_family() {
        let ledger = ledger(
            vec![generation("provider_original", Some("a")), generation("provider_v2", Some("b"))],
            vec![
                route("provider_original", "quickfix:diagnostic_routed", "canonical_candidate"),
                route("provider_v2", "quickfix:diagnostic_routed", "canonical_candidate"),
            ],
        );

        let mut violations = Vec::new();
        validate_routes(&ledger, &mut violations);

        assert!(
            violations.iter().any(|violation| violation.contains("duplicate production authority")
                && violation.contains("quickfix:diagnostic_routed")),
            "expected duplicate production authority violation, got {violations:?}"
        );
    }

    /// Negative control for the rule above: the same two generations answering
    /// one family are accepted once exactly one of them is canonical.
    #[test]
    fn accepts_one_canonical_candidate_beside_a_unique_behavior_row() {
        let mut unique = route("provider_v2", "quickfix:diagnostic_routed", "unique_behavior");
        unique.retirement_blockers =
            vec!["canonical_route_omits_diagnostic_association".to_string()];

        let ledger = ledger(
            vec![generation("provider_original", Some("a")), generation("provider_v2", Some("b"))],
            vec![
                route("provider_original", "quickfix:diagnostic_routed", "canonical_candidate"),
                unique,
            ],
        );

        let mut violations = Vec::new();
        validate_routes(&ledger, &mut violations);

        assert!(violations.is_empty(), "expected no violations, got {violations:?}");
    }

    #[test]
    fn rejects_unique_behavior_without_a_retirement_blocker() {
        let ledger = ledger(
            vec![generation("provider_v2", Some("a"))],
            vec![route("provider_v2", "quickfix:diagnostic_routed", "unique_behavior")],
        );

        let mut violations = Vec::new();
        validate_routes(&ledger, &mut violations);

        assert!(
            violations.iter().any(|violation| violation.contains("cites no retirement blocker")),
            "expected missing retirement blocker violation, got {violations:?}"
        );
    }

    /// A row with neither evidence nor a declared gap would read as covered.
    #[test]
    fn rejects_a_row_with_neither_parity_evidence_nor_a_declared_gap() {
        let mut uncovered =
            route("text_fallback", "quickfix:diagnostic_routed", "compatibility_only");
        uncovered.parity_fixtures = Vec::new();

        let ledger = ledger(vec![generation("text_fallback", Some("a"))], vec![uncovered]);

        let mut violations = Vec::new();
        validate_routes(&ledger, &mut violations);

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("neither a parity fixture nor a proof_gap")),
            "expected missing evidence violation, got {violations:?}"
        );
    }

    #[test]
    fn accepts_an_uncovered_row_that_declares_its_proof_gap() {
        let mut uncovered =
            route("text_fallback", "quickfix:diagnostic_routed", "compatibility_only");
        uncovered.parity_fixtures = Vec::new();
        uncovered.proof_gap = Some("no deterministic route from the LSP surface".to_string());

        let ledger = ledger(vec![generation("text_fallback", Some("a"))], vec![uncovered]);

        let mut violations = Vec::new();
        validate_routes(&ledger, &mut violations);

        assert!(violations.is_empty(), "expected no violations, got {violations:?}");
    }

    /// A gap and evidence together is a contradiction, not extra information.
    #[test]
    fn rejects_a_row_that_declares_both_a_gap_and_parity_evidence() {
        let mut contradictory =
            route("text_fallback", "quickfix:diagnostic_routed", "compatibility_only");
        contradictory.proof_gap = Some("uncovered".to_string());

        let ledger = ledger(vec![generation("text_fallback", Some("a"))], vec![contradictory]);

        let mut violations = Vec::new();
        validate_routes(&ledger, &mut violations);

        assert!(
            violations.iter().any(|violation| violation.contains("a covered row has no gap")),
            "expected contradictory evidence violation, got {violations:?}"
        );
    }

    #[test]
    fn rejects_a_route_whose_generation_has_no_row() {
        let ledger = ledger(
            vec![generation("provider_original", Some("a"))],
            vec![
                route("provider_original", "quickfix:diagnostic_routed", "canonical_candidate"),
                route("ghost_generation", "quickfix:diagnostic_routed", "redundant_behavior"),
            ],
        );

        let mut violations = Vec::new();
        validate_routes(&ledger, &mut violations);

        assert!(
            violations.iter().any(|violation| violation.contains("which has no generation row")),
            "expected unknown generation violation, got {violations:?}"
        );
    }

    #[test]
    fn rejects_a_generation_with_no_family_disposition() {
        let ledger = ledger(
            vec![generation("provider_original", Some("a")), generation("orphan", Some("b"))],
            vec![route("provider_original", "quickfix:diagnostic_routed", "canonical_candidate")],
        );

        let mut violations = Vec::new();
        validate_routes(&ledger, &mut violations);

        assert!(
            violations.iter().any(|violation| violation.contains("has no route row")),
            "expected missing disposition violation, got {violations:?}"
        );
    }

    #[test]
    fn rejects_a_stale_production_anchor() {
        let ledger = ledger(
            vec![generation("provider_original", Some("NoSuchCall::new("))],
            vec![route("provider_original", "quickfix:diagnostic_routed", "canonical_candidate")],
        );

        let mut violations = Vec::new();
        let root = std::path::PathBuf::from(".");
        validate_generations(&root, &ledger, "fn handle_code_action() {}", &mut violations);

        assert!(
            violations.iter().any(|violation| violation.contains("no longer occurs in")),
            "expected stale anchor violation, got {violations:?}"
        );
    }

    #[test]
    fn rejects_a_production_generation_without_an_anchor() {
        let ledger = ledger(
            vec![generation("provider_original", None)],
            vec![route("provider_original", "quickfix:diagnostic_routed", "canonical_candidate")],
        );

        let mut violations = Vec::new();
        let root = std::path::PathBuf::from(".");
        validate_generations(&root, &ledger, "anything", &mut violations);

        assert!(
            violations.iter().any(|violation| violation.contains("has no production_anchor")),
            "expected missing anchor violation, got {violations:?}"
        );
    }

    #[test]
    fn collects_corpus_fixture_identities() {
        let ids = corpus_fixture_ids(
            r#"
            const A: &str = "cac-parity-success-case";
            // cac-parity-disabled-case is also declared
            let _ = "cac-parity-success-case";
            "#,
        );

        assert_eq!(
            ids.into_iter().collect::<Vec<_>>(),
            vec!["cac-parity-disabled-case", "cac-parity-success-case"]
        );
    }

    #[test]
    fn reports_fixture_ids_missing_from_the_corpus() {
        let ledger = ledger(
            vec![generation("provider_original", Some("a"))],
            vec![route("provider_original", "quickfix:diagnostic_routed", "canonical_candidate")],
        );

        let mut violations = Vec::new();
        validate_parity_fixtures(&ledger, "no fixtures here", &mut violations);

        assert!(
            violations.iter().any(|violation| violation.contains("absent from")),
            "expected missing fixture violation, got {violations:?}"
        );
    }

    #[test]
    fn reports_corpus_fixtures_no_route_claims() {
        let ledger = ledger(
            vec![generation("provider_original", Some("a"))],
            vec![route("provider_original", "quickfix:diagnostic_routed", "canonical_candidate")],
        );

        let mut violations = Vec::new();
        validate_parity_fixtures(
            &ledger,
            r#""cac-parity-example" and "cac-parity-orphan""#,
            &mut violations,
        );

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("is not claimed by any ledger route")),
            "expected unclaimed fixture violation, got {violations:?}"
        );
    }

    #[test]
    fn parses_the_human_disposition_table() -> Result<()> {
        let table = parse_table(
            "\
| Generation | Family | Disposition | Retirement blockers |
| --- | --- | --- | --- |
| provider_v2 | quickfix:diagnostic_routed | unique_behavior | canonical_route_omits_diagnostic_association |
",
            "Generation",
        )
        .ok_or_else(|| color_eyre::eyre::eyre!("expected disposition table"))?;

        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].cells[0], "provider_v2");
        assert_eq!(table.rows[0].cells[2], "unique_behavior");
        Ok(())
    }

    #[test]
    fn renders_an_empty_blocker_list_as_the_table_placeholder() {
        assert_eq!(render_blockers(&[]), EMPTY_CELL);
        assert_eq!(render_blockers(&["one".to_string()]), "one");
    }

    /// The tracked ledger, human page, orchestrator, and corpus must agree.
    #[test]
    fn tracked_ledger_is_valid() -> Result<()> {
        let root = project_root()?;
        validate(&root)?;
        Ok(())
    }
}
