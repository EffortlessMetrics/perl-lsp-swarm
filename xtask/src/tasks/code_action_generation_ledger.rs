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

/// Which branch of `handle_code_action` a generation is invoked from.
/// `both` means the anchor has a call site on each side of the no-AST boundary.
const REQUIRED_PATHS: &[&str] = &["ast", "no_ast", "both", "none"];

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
    handler_start: String,
    handler_end: String,
    no_ast_boundary: String,
    disposition_states: Vec<String>,
    family_registry: Vec<String>,
    retirement_blocker_registry: Vec<String>,
    unreachable_kinds: Vec<String>,
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
    validate_kind_coverage(&ledger, &orchestrator_text, &mut violations);
    validate_parity_fixtures(&ledger, &corpus_text, &mut violations);
    validate_human_fixture_mentions(&ledger, &human_text, &mut violations);
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
    for (field, value) in [
        ("handler_start", &ledger.handler_start),
        ("handler_end", &ledger.handler_end),
        ("no_ast_boundary", &ledger.no_ast_boundary),
    ] {
        if value.trim().is_empty() {
            violations.push(format!("{POLICY_PATH}: {field} must not be empty"));
        }
    }

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
        "unreachable_kinds",
        &ledger.unreachable_kinds,
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
    let path_states = REQUIRED_PATHS.iter().copied().collect::<BTreeSet<_>>();
    let mut seen_ids = BTreeSet::new();
    let mut seen_stages: BTreeMap<u32, String> = BTreeMap::new();
    // Anchors only count inside `handle_code_action` itself. An occurrence in a
    // helper, a test-only handler, or the test module is not production routing.
    let handler = handler_range(ledger, orchestrator_text, violations);
    let boundary = orchestrator_text.find(ledger.no_ast_boundary.as_str());
    // Anchor search runs over a copy with comments blanked out, so a retired
    // producer whose call text survives only in a comment no longer satisfies
    // its row. Offsets are preserved, and the boundary/handler markers are
    // resolved from the raw text above because the boundary IS a comment.
    let executable_text = blank_comments(orchestrator_text);
    if boundary.is_none() {
        violations.push(format!(
            "{POLICY_PATH}: no_ast_boundary {:?} no longer occurs in {ORCHESTRATOR}",
            ledger.no_ast_boundary
        ));
    }
    if let (Some((start, end)), Some(boundary)) = (handler, boundary)
        && !(start < boundary && boundary < end)
    {
        violations.push(format!(
            "{POLICY_PATH}: no_ast_boundary does not lie inside the handler range [{start}, {end})"
        ));
    }

    // (stage, id, primary offset) for production rows, used for the order check.
    let mut ordered: Vec<(u32, String, usize)> = Vec::new();

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
        if !path_states.contains(generation.path.as_str()) {
            violations.push(format!(
                "{POLICY_PATH}: generation {id} path {:?} is not one of {REQUIRED_PATHS:?}",
                generation.path
            ));
        }

        // Reachability and path are not independent: a production generation
        // runs on a real branch, and a stub runs nowhere. Validating them
        // separately let a row claim `production` with `path = "none"`, or
        // `unreachable_stub` with an active branch.
        match (generation.reachability.as_str(), generation.path.as_str()) {
            ("production", "none") => violations.push(format!(
                "{POLICY_PATH}: generation {id} is production-reachable but declares path \"none\""
            )),
            ("unreachable_stub", path) if path != "none" => violations.push(format!(
                "{POLICY_PATH}: generation {id} is an unreachable stub but declares path {path:?}; expected \"none\""
            )),
            _ => {}
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
                        let all = occurrences(&executable_text, anchor);
                        let offsets = match handler {
                            Some((start, end)) => all
                                .iter()
                                .copied()
                                .filter(|offset| *offset >= start && *offset < end)
                                .collect::<Vec<_>>(),
                            None => all.clone(),
                        };
                        if offsets.is_empty() {
                            let outside = all.len();
                            violations.push(format!(
                                "{POLICY_PATH}: generation {id} production_anchor {anchor:?} does not occur inside handle_code_action in {ORCHESTRATOR}{}",
                                if outside > 0 {
                                    format!(" (it occurs {outside} time(s) elsewhere in the file, which is not production routing)")
                                } else {
                                    String::new()
                                }
                            ));
                        } else {
                            if let Some(boundary) = boundary {
                                check_branch(id, generation, &offsets, boundary, violations);
                            }
                            let primary = if generation.path == "no_ast" {
                                offsets.iter().copied().max().unwrap_or_default()
                            } else {
                                offsets[0]
                            };
                            ordered.push((generation.stage, id.to_string(), primary));
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

    // Declared stage order must match the order the anchors actually appear in
    // the orchestrator. Presence alone would let a reordering leave every stage
    // number stale while the check stayed green.
    ordered.sort_by_key(|(stage, _, _)| *stage);
    for window in ordered.windows(2) {
        let (earlier_stage, earlier_id, earlier_offset) = &window[0];
        let (later_stage, later_id, later_offset) = &window[1];
        if earlier_offset >= later_offset {
            violations.push(format!(
                "{POLICY_PATH}: stage {earlier_stage} ({earlier_id}) is declared before stage {later_stage} ({later_id}), but its anchor occurs later in {ORCHESTRATOR}; the declared invocation order is stale"
            ));
        }
    }
}

/// Replace every `//` and `/* */` comment with spaces, preserving length and
/// therefore every byte offset.
///
/// String literals are deliberately left intact: one production anchor is a
/// format string (`"Generate test for '{}'"`), which is executable code.
fn blank_comments(source: &str) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Code,
        LineComment,
        BlockComment,
        Str,
        StrEscape,
        Char,
        CharEscape,
    }

    let bytes = source.as_bytes();
    // Only ASCII comment bytes are overwritten with ASCII spaces, so the result
    // stays valid UTF-8 and every byte offset is preserved.
    let mut out_bytes = bytes.to_vec();
    let mut state = State::Code;
    // Rust block comments nest: `/* /* */ still a comment */`. Tracking depth
    // stops an anchor after an inner `*/` from looking like executable code.
    let mut block_depth = 0usize;
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        let next = bytes.get(index + 1).copied();
        match state {
            State::Code => match (byte, next) {
                (b'/', Some(b'/')) => {
                    state = State::LineComment;
                    out_bytes[index] = b' ';
                }
                (b'/', Some(b'*')) => {
                    state = State::BlockComment;
                    block_depth = 1;
                    out_bytes[index] = b' ';
                }
                (b'"', _) => state = State::Str,
                (b'\'', _) => state = State::Char,
                _ => {}
            },
            State::LineComment => {
                if byte == b'\n' {
                    state = State::Code;
                } else {
                    out_bytes[index] = b' ';
                }
            }
            State::BlockComment => {
                if byte == b'/' && next == Some(b'*') {
                    block_depth += 1;
                    out_bytes[index] = b' ';
                    out_bytes[index + 1] = b' ';
                    index += 2;
                    continue;
                }
                if byte == b'*' && next == Some(b'/') {
                    out_bytes[index] = b' ';
                    if index + 1 < out_bytes.len() {
                        out_bytes[index + 1] = b' ';
                    }
                    index += 2;
                    block_depth = block_depth.saturating_sub(1);
                    if block_depth == 0 {
                        state = State::Code;
                    }
                    continue;
                }
                if byte != b'\n' {
                    out_bytes[index] = b' ';
                }
            }
            State::Str => match byte {
                b'\\' => state = State::StrEscape,
                b'"' => state = State::Code,
                _ => {}
            },
            State::StrEscape => state = State::Str,
            State::Char => match byte {
                b'\\' => state = State::CharEscape,
                b'\'' => state = State::Code,
                _ => {}
            },
            State::CharEscape => state = State::Char,
        }
        index += 1;
    }

    String::from_utf8(out_bytes).unwrap_or_else(|_| source.to_string())
}

/// Byte range of `handle_code_action` in the orchestrator source.
fn handler_range(
    ledger: &Ledger,
    orchestrator_text: &str,
    violations: &mut Vec<String>,
) -> Option<(usize, usize)> {
    let start = orchestrator_text.find(ledger.handler_start.as_str());
    let end = orchestrator_text.find(ledger.handler_end.as_str());
    match (start, end) {
        (Some(start), Some(end)) if start < end => Some((start, end)),
        (Some(start), Some(end)) => {
            violations.push(format!(
                "{POLICY_PATH}: handler_start occurs at {start} but handler_end occurs at {end}; the handler range is empty or inverted"
            ));
            None
        }
        _ => {
            if start.is_none() {
                violations.push(format!(
                    "{POLICY_PATH}: handler_start {:?} no longer occurs in {ORCHESTRATOR}",
                    ledger.handler_start
                ));
            }
            if end.is_none() {
                violations.push(format!(
                    "{POLICY_PATH}: handler_end {:?} no longer occurs in {ORCHESTRATOR}",
                    ledger.handler_end
                ));
            }
            None
        }
    }
}

/// Every byte offset at which `needle` occurs in `haystack`.
fn occurrences(haystack: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut offsets = Vec::new();
    let mut from = 0;
    while let Some(index) = haystack[from..].find(needle) {
        let at = from + index;
        offsets.push(at);
        from = at + needle.len();
    }
    offsets
}

/// A generation's declared branch must match where its anchor actually sits
/// relative to the no-AST boundary.
fn check_branch(
    id: &str,
    generation: &Generation,
    offsets: &[usize],
    boundary: usize,
    violations: &mut Vec<String>,
) {
    let before = offsets.iter().any(|offset| *offset < boundary);
    let after = offsets.iter().any(|offset| *offset > boundary);

    match generation.path.as_str() {
        "ast" if after => violations.push(format!(
            "{POLICY_PATH}: generation {id} declares path \"ast\" but its anchor also occurs after the no-AST boundary; declare path \"both\" or split the row"
        )),
        "no_ast" if before => violations.push(format!(
            "{POLICY_PATH}: generation {id} declares path \"no_ast\" but its anchor also occurs before the no-AST boundary; declare path \"both\" or split the row"
        )),
        "both" if !(before && after) => violations.push(format!(
            "{POLICY_PATH}: generation {id} declares path \"both\" but its anchor occurs on only one side of the no-AST boundary"
        )),
        _ => {}
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

/// Every LSP `CodeActionKind` literal the handler can serialize must be either
/// a registered family's kind or an explicitly recorded unreachable kind.
///
/// This is the mechanically checkable half of family coverage. It catches a new
/// kind appearing with no disposition. It cannot catch a new *family* added
/// within a kind that already has one — that needs the semantic judgment this
/// ledger exists to record, and the human page says so.
fn validate_kind_coverage(ledger: &Ledger, orchestrator_text: &str, violations: &mut Vec<String>) {
    let Some((start, end)) = (match (
        orchestrator_text.find(ledger.handler_start.as_str()),
        orchestrator_text.find(ledger.handler_end.as_str()),
    ) {
        (Some(start), Some(end)) if start < end => Some((start, end)),
        _ => None,
    }) else {
        return;
    };
    let handler = blank_comments(&orchestrator_text[start..end]);

    let family_kinds = ledger
        .family_registry
        .iter()
        .filter_map(|family| family.split(':').next())
        .filter(|kind| *kind != "none")
        .collect::<BTreeSet<_>>();
    let unreachable = ledger.unreachable_kinds.iter().map(String::as_str).collect::<BTreeSet<_>>();

    for kind in &unreachable {
        if family_kinds.contains(kind) {
            violations.push(format!(
                "{POLICY_PATH}: kind {kind:?} is listed in unreachable_kinds but a family also claims it"
            ));
        }
        if !handler.contains(&format!("\"{kind}\"")) {
            violations.push(format!(
                "{POLICY_PATH}: unreachable_kinds entry {kind:?} no longer occurs in the handler"
            ));
        }
    }

    for kind in emitted_kinds(&handler) {
        if !family_kinds.contains(kind.as_str()) && !unreachable.contains(kind.as_str()) {
            violations.push(format!(
                "{POLICY_PATH}: the handler can publish CodeActionKind {kind:?}, which no family claims and unreachable_kinds does not record"
            ));
        }
    }
}

/// LSP kind literals appearing in the handler.
fn emitted_kinds(handler: &str) -> BTreeSet<String> {
    const ROOTS: [&str; 3] = ["quickfix", "refactor", "source"];
    let mut kinds = BTreeSet::new();
    for raw in handler.split('"').skip(1).step_by(2) {
        let is_kind = ROOTS.iter().any(|root| {
            raw == *root || raw.strip_prefix(root).is_some_and(|rest| rest.starts_with('.'))
        }) && raw.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.');
        if is_kind {
            kinds.insert(raw.to_string());
        }
    }
    kinds
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

    // Surface a syntax error directly. Without this, an unparseable corpus
    // binds nothing and the operator sees a wall of "missing fixture"
    // violations instead of the one line that explains them.
    if let Err(error) = syn::parse_file(corpus_text) {
        violations.push(format!(
            "{PARITY_CORPUS}: could not be parsed as Rust ({error}); no fixture binding is possible until it parses"
        ));
    }

    let fixtures = parse_corpus_fixtures(corpus_text);

    for fixture in &fixtures {
        if !fixture.bound_to_a_test {
            violations.push(format!(
                "{PARITY_CORPUS}: fixture {:?} is declared by const {} but that constant is not referenced by any test, so it is not executable evidence",
                fixture.id, fixture.const_name
            ));
        }
    }

    let present = fixtures
        .iter()
        .filter(|fixture| fixture.bound_to_a_test)
        .map(|fixture| fixture.id.clone())
        .collect::<BTreeSet<_>>();

    for missing in declared.difference(&present) {
        violations.push(format!(
            "{POLICY_PATH}: parity fixture {missing:?} is named by a route but is not declared by a test-referenced constant in {PARITY_CORPUS}"
        ));
    }
    for unclaimed in present.difference(&declared) {
        violations.push(format!(
            "{PARITY_CORPUS}: parity fixture {unclaimed:?} is not claimed by any ledger route"
        ));
    }
}

/// Every `cac-parity-*` id named anywhere in the human page must be a real
/// ledger fixture.
///
/// The page carries a by-outcome-class fixture table that nothing validated,
/// so renaming a fixture left it pointing at an identity that no longer
/// existed — which review caught rather than the check.
fn validate_human_fixture_mentions(
    ledger: &Ledger,
    human_text: &str,
    violations: &mut Vec<String>,
) {
    let declared = ledger_fixture_ids(ledger);
    for mentioned in scan_fixture_ids(human_text) {
        if !declared.contains(&mentioned) {
            violations.push(format!(
                "{HUMAN_LEDGER}: names parity fixture {mentioned:?}, which no ledger route claims"
            ));
        }
    }
}

/// Loose scan for `cac-parity-*` identities in prose.
fn scan_fixture_ids(text: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut rest = text;
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

fn ledger_fixture_ids(ledger: &Ledger) -> BTreeSet<String> {
    ledger
        .route
        .iter()
        .flat_map(|route| route.parity_fixtures.iter().cloned())
        .collect::<BTreeSet<_>>()
}

/// One `cac-parity-*` identity declared by the corpus.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CorpusFixture {
    id: String,
    const_name: String,
    /// Whether `const_name` is referenced anywhere at or after the first
    /// `#[test]` in the file.
    bound_to_a_test: bool,
}

/// Parse the corpus's fixture identities from their `const` declarations and
/// record whether each one is actually referenced by a test.
///
/// An earlier version scanned the whole file for `cac-parity-` literals, which
/// meant deleting a test while leaving its id in a comment or an unused
/// constant still satisfied the ledger's coverage claim. Identity now comes
/// only from a `const NAME: &str = "cac-parity-…";` declaration, and coverage
/// additionally requires `NAME` to appear in the file's test region.
fn parse_corpus_fixtures(corpus_text: &str) -> Vec<CorpusFixture> {
    let Ok(file) = syn::parse_file(corpus_text) else {
        // A corpus that does not parse cannot bind anything; the caller turns
        // an empty result into "named by a route but absent" violations rather
        // than silently accepting every claim.
        return Vec::new();
    };

    let mut declarations = Vec::new();
    let mut referenced = BTreeSet::new();
    collect_fixture_bindings(&file.items, &mut declarations, &mut referenced);

    declarations
        .into_iter()
        .map(|(const_name, id)| CorpusFixture {
            bound_to_a_test: referenced.contains(const_name.as_str()),
            id,
            const_name,
        })
        .collect()
}

/// Walk items, collecting `const NAME: &str = "cac-parity-…";` declarations and
/// every identifier mentioned inside a `#[test]` function body.
///
/// Using a real parse rather than scanning text is what makes the binding
/// trustworthy: a comment, a doc string, or a helper defined beside the tests
/// contributes no identifier here, and tests nested in a module are still
/// found because recursion follows the module tree instead of guessing where a
/// body ends from brace indentation.
fn collect_fixture_bindings(
    items: &[syn::Item],
    declarations: &mut Vec<(String, String)>,
    referenced: &mut BTreeSet<String>,
) {
    for item in items {
        match item {
            syn::Item::Const(constant) => {
                if let syn::Expr::Lit(literal) = constant.expr.as_ref()
                    && let syn::Lit::Str(value) = &literal.lit
                {
                    let id = value.value();
                    if id.starts_with(FIXTURE_PREFIX) && id.len() > FIXTURE_PREFIX.len() {
                        declarations.push((constant.ident.to_string(), id));
                    }
                }
            }
            syn::Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    collect_fixture_bindings(nested, declarations, referenced);
                }
            }
            syn::Item::Fn(function) => {
                let is_test =
                    function.attrs.iter().any(|attribute| attribute.path().is_ident("test"));
                if is_test {
                    collect_identifiers(&function.block, referenced);
                }
            }
            _ => {}
        }
    }
}

/// Every identifier appearing in a test body, including those inside
/// `format!`/`assert!` macro arguments and inline format captures, which is
/// where fixture constants are actually used.
fn collect_identifiers(block: &syn::Block, referenced: &mut BTreeSet<String>) {
    struct Collector<'a> {
        referenced: &'a mut BTreeSet<String>,
    }

    impl Collector<'_> {
        fn walk_tokens(&mut self, stream: proc_macro2::TokenStream) {
            for token in stream {
                match token {
                    proc_macro2::TokenTree::Ident(ident) => {
                        self.referenced.insert(ident.to_string());
                    }
                    proc_macro2::TokenTree::Group(group) => self.walk_tokens(group.stream()),
                    proc_macro2::TokenTree::Literal(literal) => {
                        self.collect_format_captures(&literal.to_string());
                    }
                    proc_macro2::TokenTree::Punct(_) => {}
                }
            }
        }

        /// `"{SUCCESS_PRAGMA}: ..."` names its constant inside a string
        /// literal, so the capture has to be recovered from the literal text.
        fn collect_format_captures(&mut self, text: &str) {
            let mut rest = text;
            while let Some(open) = rest.find('{') {
                rest = &rest[open + 1..];
                let Some(close) = rest.find('}') else {
                    break;
                };
                let capture =
                    rest[..close].split([':', '?']).next().unwrap_or_default().trim().to_string();
                if !capture.is_empty()
                    && capture.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                {
                    self.referenced.insert(capture);
                }
                rest = &rest[close + 1..];
            }
        }
    }

    impl<'ast> syn::visit::Visit<'ast> for Collector<'_> {
        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            self.referenced.insert(ident.to_string());
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            self.walk_tokens(mac.tokens.clone());
            syn::visit::visit_macro(self, mac);
        }

        fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
            self.collect_format_captures(&literal.value());
            syn::visit::visit_lit_str(self, literal);
        }
    }

    let mut collector = Collector { referenced };
    syn::visit::Visit::visit_block(&mut collector, block);
}

/// Whole-identifier containment, so `FOO` does not match `FOO_BAR`.
fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(offset) = haystack[from..].find(needle) {
        let start = from + offset;
        let end = start + needle.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + needle.len();
    }
    false
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
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

    const NO_AST_MARKER: &str = "// No AST (parse error), but we can still offer some actions";
    const HANDLER_START: &str = "pub(crate) fn handle_code_action(";
    const HANDLER_END: &str = "/// Cancellation-aware wrapper";

    /// Wrap a handler body in the markers the validator uses to bound it.
    fn handler(body: &str) -> String {
        format!("{HANDLER_START}\n{body}\n{HANDLER_END}\n")
    }

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
            handler_start: HANDLER_START.to_string(),
            handler_end: HANDLER_END.to_string(),
            no_ast_boundary: NO_AST_MARKER.to_string(),
            disposition_states: REQUIRED_DISPOSITIONS
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            family_registry: vec!["quickfix:diagnostic_routed".to_string()],
            retirement_blocker_registry: vec![
                "canonical_route_omits_diagnostic_association".to_string(),
            ],
            unreachable_kinds: vec!["refactor".to_string()],
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
        validate_generations(&root, &ledger, &handler("NOTHING_HERE"), &mut violations);

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
        validate_generations(&root, &ledger, &handler("anything"), &mut violations);

        assert!(
            violations.iter().any(|violation| violation.contains("has no production_anchor")),
            "expected missing anchor violation, got {violations:?}"
        );
    }

    #[test]
    fn collects_fixture_identities_bound_to_a_test() {
        let fixtures = parse_corpus_fixtures(
            r#"
            const A: &str = "cac-parity-success-case";
            const B: &str = "cac-parity-orphan-case";

            #[test]
            fn uses_a() {
                assert!(true, "{A}");
            }
            "#,
        );

        let bound = fixtures
            .iter()
            .filter(|fixture| fixture.bound_to_a_test)
            .map(|fixture| fixture.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(bound, vec!["cac-parity-success-case"]);

        let unbound = fixtures
            .iter()
            .filter(|fixture| !fixture.bound_to_a_test)
            .map(|fixture| fixture.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(unbound, vec!["cac-parity-orphan-case"]);
    }

    /// Tests nested in a module are still found, and a helper sitting beside
    /// them inside that module still does not count. A brace-scanning
    /// heuristic got this wrong because rustfmt indents nested bodies.
    #[test]
    fn finds_tests_inside_a_nested_module_without_crediting_its_helpers() {
        let fixtures = parse_corpus_fixtures(
            r#"
            const USED: &str = "cac-parity-used-in-nested-test";
            const HELPER_ONLY: &str = "cac-parity-helper-inside-module";

            mod inner {
                use super::*;

                fn helper() {
                    let _ = HELPER_ONLY;
                }

                #[test]
                fn nested() {
                    assert!(true, "{USED}");
                }
            }
            "#,
        );

        let bound = fixtures
            .iter()
            .filter(|fixture| fixture.bound_to_a_test)
            .map(|fixture| fixture.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            bound,
            vec!["cac-parity-used-in-nested-test"],
            "nested test binding wrong: {fixtures:?}"
        );
    }

    /// A corpus that does not parse binds nothing, so every ledger claim
    /// surfaces as a missing-fixture violation rather than passing silently.
    #[test]
    fn an_unparseable_corpus_binds_nothing() {
        let fixtures = parse_corpus_fixtures("fn broken( {");
        assert!(fixtures.is_empty(), "expected no bindings, got {fixtures:?}");
    }

    /// A reference from a helper — or any code outside a `#[test]` body — is
    /// not executable evidence either.
    #[test]
    fn a_reference_outside_a_test_body_is_not_coverage() {
        let fixtures = parse_corpus_fixtures(
            "const A: &str = \"cac-parity-helper-only\";\n\nfn helper() {\n    let _ = A;\n}\n\n#[test]\nfn unrelated() {\n    assert!(true);\n}\n",
        );

        assert_eq!(fixtures.len(), 1);
        assert!(
            !fixtures[0].bound_to_a_test,
            "a helper reference must not bind the fixture, got {fixtures:?}"
        );
    }

    /// Deleting a test but leaving its id behind in a comment must not keep the
    /// route's coverage claim alive.
    #[test]
    fn a_fixture_id_in_a_comment_alone_is_not_coverage() {
        let fixtures = parse_corpus_fixtures(
            r#"
            // cac-parity-deleted-case used to live here.
            #[test]
            fn unrelated() {}
            "#,
        );

        assert!(
            fixtures.is_empty(),
            "a bare comment mention must not declare a fixture, got {fixtures:?}"
        );
    }

    #[test]
    fn whole_identifier_matching_does_not_accept_a_prefix() {
        assert!(contains_word("let _ = FOO;", "FOO"));
        assert!(!contains_word("let _ = FOO_BAR;", "FOO"));
        assert!(!contains_word("let _ = XFOO;", "FOO"));
    }

    /// The declared invocation order must match the order the anchors actually
    /// occur in, or every stage number can go stale unnoticed.
    #[test]
    fn rejects_stage_order_that_contradicts_source_order() {
        let mut first = generation("later_in_file", Some("SECOND_CALL"));
        first.stage = 1;
        let mut second = generation("earlier_in_file", Some("FIRST_CALL"));
        second.stage = 2;

        let ledger = ledger(
            vec![first, second],
            vec![
                route("later_in_file", "quickfix:diagnostic_routed", "canonical_candidate"),
                route("earlier_in_file", "quickfix:diagnostic_routed", "redundant_behavior"),
            ],
        );

        let mut violations = Vec::new();
        let source = handler(&format!("FIRST_CALL\nSECOND_CALL\n{NO_AST_MARKER}"));
        validate_generations(&std::path::PathBuf::from("."), &ledger, &source, &mut violations);

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("declared invocation order is stale")),
            "expected stage-order violation, got {violations:?}"
        );
    }

    #[test]
    fn rejects_a_production_generation_that_runs_nowhere() {
        let mut nowhere = generation("ghost", Some("THE_CALL"));
        nowhere.path = "none".to_string();

        let ledger = ledger(
            vec![nowhere],
            vec![route("ghost", "quickfix:diagnostic_routed", "canonical_candidate")],
        );

        let mut violations = Vec::new();
        validate_generations(
            &std::path::PathBuf::from("."),
            &ledger,
            &handler(&format!("THE_CALL\n{NO_AST_MARKER}")),
            &mut violations,
        );

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("production-reachable but declares path")),
            "expected production/none contradiction, got {violations:?}"
        );
    }

    #[test]
    fn rejects_an_unreachable_stub_that_claims_a_branch() {
        let mut stub = generation("stub", None);
        stub.reachability = "unreachable_stub".to_string();
        stub.path = "ast".to_string();

        let ledger =
            ledger(vec![stub], vec![route("stub", "none:unreachable_stub", "retire_candidate")]);

        let mut violations = Vec::new();
        validate_generations(
            &std::path::PathBuf::from("."),
            &ledger,
            &handler(NO_AST_MARKER),
            &mut violations,
        );

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("unreachable stub but declares path")),
            "expected stub/branch contradiction, got {violations:?}"
        );
    }

    /// A retired producer whose call text survives only in a comment must not
    /// keep its row alive.
    #[test]
    fn rejects_an_anchor_that_survives_only_in_a_comment() {
        let ledger = ledger(
            vec![generation("retired", Some("RETIRED_CALL"))],
            vec![route("retired", "quickfix:diagnostic_routed", "canonical_candidate")],
        );

        let mut violations = Vec::new();
        let source = handler(&format!("// RETIRED_CALL was removed here\n{NO_AST_MARKER}"));
        validate_generations(&std::path::PathBuf::from("."), &ledger, &source, &mut violations);

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("does not occur inside handle_code_action")),
            "expected comment-only anchor to be rejected, got {violations:?}"
        );
    }

    /// Rust block comments nest, so an inner `*/` must not end the outer one.
    #[test]
    fn blanks_nested_block_comments_entirely() {
        let source = "/* outer /* inner */ STALE_CALL() */ let a = LIVE_CALL();\n";
        let blanked = blank_comments(source);
        assert_eq!(blanked.len(), source.len(), "offsets must be preserved");
        assert!(
            occurrences(&blanked, "STALE_CALL()").is_empty(),
            "an anchor after an inner */ must stay commented out: {blanked:?}"
        );
        assert_eq!(occurrences(&blanked, "LIVE_CALL()").len(), 1, "live code must survive");
    }

    #[test]
    fn blanks_comments_while_preserving_offsets_and_strings() {
        let source = "let a = CALL(); // CALL() in a comment\nlet s = \"CALL()\";\n";
        let blanked = blank_comments(source);
        assert_eq!(blanked.len(), source.len(), "offsets must be preserved");
        assert_eq!(
            occurrences(&blanked, "CALL()").len(),
            2,
            "code and string survive; comment does not"
        );
    }

    /// A new CodeActionKind must be classified, not silently uninventoried.
    #[test]
    fn rejects_an_emitted_kind_that_no_family_or_exception_claims() {
        let mut ledger = ledger(
            vec![generation("gen", Some("THE_CALL"))],
            vec![route("gen", "quickfix:diagnostic_routed", "canonical_candidate")],
        );
        ledger.unreachable_kinds = Vec::new();

        let mut violations = Vec::new();
        let source = handler(&format!(
            "let _ = \"quickfix\"; let _ = \"source.brandNew\"; THE_CALL\n{NO_AST_MARKER}"
        ));
        validate_kind_coverage(&ledger, &source, &mut violations);

        assert!(
            violations.iter().any(|violation| violation.contains("source.brandNew")),
            "expected unclassified kind violation, got {violations:?}"
        );
    }

    /// A recorded unreachable kind that no longer appears is stale.
    #[test]
    fn rejects_a_stale_unreachable_kind() {
        let mut ledger = ledger(
            vec![generation("gen", Some("THE_CALL"))],
            vec![route("gen", "quickfix:diagnostic_routed", "canonical_candidate")],
        );
        ledger.unreachable_kinds = vec!["refactor.inline".to_string()];

        let mut violations = Vec::new();
        let source = handler(&format!("let _ = \"quickfix\"; THE_CALL\n{NO_AST_MARKER}"));
        validate_kind_coverage(&ledger, &source, &mut violations);

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("no longer occurs in the handler")),
            "expected stale unreachable kind violation, got {violations:?}"
        );
    }

    /// The exact defect review caught on the tracked ledger: an anchor whose
    /// only occurrence lives in a different function is not production routing,
    /// even though it sits after the no-AST boundary in the file.
    #[test]
    fn rejects_an_anchor_that_occurs_only_outside_the_handler() {
        let ledger = ledger(
            vec![generation("elsewhere", Some("HELPER_ONLY_CALL"))],
            vec![route("elsewhere", "quickfix:diagnostic_routed", "canonical_candidate")],
        );

        let mut violations = Vec::new();
        let source = format!(
            "{}\nfn some_test_only_helper() {{ HELPER_ONLY_CALL }}\n",
            handler(&NO_AST_MARKER.to_string())
        );
        validate_generations(&std::path::PathBuf::from("."), &ledger, &source, &mut violations);

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("does not occur inside handle_code_action")
                    && violation.contains("elsewhere in the file")),
            "expected out-of-handler violation, got {violations:?}"
        );
    }

    /// A generation that moved into the degraded branch must not keep an `ast`
    /// declaration.
    #[test]
    fn rejects_an_ast_generation_whose_anchor_moved_past_the_no_ast_boundary() {
        let ledger = ledger(
            vec![generation("moved", Some("THE_CALL"))],
            vec![route("moved", "quickfix:diagnostic_routed", "canonical_candidate")],
        );

        let mut violations = Vec::new();
        let source = handler(&format!("{NO_AST_MARKER}\nTHE_CALL"));
        validate_generations(&std::path::PathBuf::from("."), &ledger, &source, &mut violations);

        assert!(
            violations.iter().any(|violation| violation.contains("declare path")),
            "expected branch violation, got {violations:?}"
        );
    }

    /// `both` means exactly that: one call site on each side.
    #[test]
    fn rejects_a_both_generation_present_on_only_one_branch() {
        let mut only_ast = generation("half", Some("THE_CALL"));
        only_ast.path = "both".to_string();

        let ledger = ledger(
            vec![only_ast],
            vec![route("half", "quickfix:diagnostic_routed", "canonical_candidate")],
        );

        let mut violations = Vec::new();
        let source = handler(&format!("THE_CALL\n{NO_AST_MARKER}"));
        validate_generations(&std::path::PathBuf::from("."), &ledger, &source, &mut violations);

        assert!(
            violations.iter().any(|violation| violation.contains("occurs on only one side")),
            "expected one-sided `both` violation, got {violations:?}"
        );
    }

    #[test]
    fn reports_fixture_ids_missing_from_the_corpus() {
        let ledger = ledger(
            vec![generation("provider_original", Some("a"))],
            vec![route("provider_original", "quickfix:diagnostic_routed", "canonical_candidate")],
        );

        let mut violations = Vec::new();
        validate_parity_fixtures(&ledger, "#[test]\nfn nothing() {}", &mut violations);

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("not declared by a test-referenced constant")),
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
            r#"
            const EXAMPLE: &str = "cac-parity-example";
            const ORPHAN: &str = "cac-parity-orphan";

            #[test]
            fn uses_both() {
                let _ = (EXAMPLE, ORPHAN);
            }
            "#,
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
