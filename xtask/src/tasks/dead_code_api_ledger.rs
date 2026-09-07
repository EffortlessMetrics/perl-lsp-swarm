//! Validate the `perl_parser::dead_code` API disposition ledger (#9777).
//!
//! The ledger (`policy/dead-code-api-ledger.toml`) freezes, item by item, what
//! the bounded compatibility surface does today, what it cannot represent, and
//! which authority owns the replacement. This check keeps that record honest in
//! both directions: a ledger row cannot survive the item it describes, and a new
//! public item cannot land without a disposition.
//!
//! The check is offline and read-only. It never runs the analyzer, never
//! consults GitHub, and never rewrites the ledger or its projection.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

const POLICY_PATH: &str = "policy/dead-code-api-ledger.toml";

/// Trait methods produced by `#[derive(...)]`. They appear in the public-API
/// baseline but are consequences of a type's own disposition rather than
/// separately decided API, so the ledger dispositions the type, not the derive.
const DERIVED_METHODS: &[&str] = &[
    "clone",
    "fmt",
    "eq",
    "ne",
    "default",
    "serialize",
    "deserialize",
    "hash",
    "cmp",
    "partial_cmp",
];

/// Distinctive references that mean a file consumes this surface. Deliberately
/// excludes the bare word `dead_code`, which collides with `#[allow(dead_code)]`
/// across the workspace and would make the scan meaningless.
const CONSUMER_NEEDLES: &[&str] = &[
    "perl_parser::dead_code",
    "dead_code_detector",
    "crate::dead_code",
    "DeadCodeDetector",
    "DeadCodeAnalysis",
    "DeadCodeStats",
    "DeadCodeType",
    // The prelude route, which reaches the types without naming the module.
    // The bare identifier `DeadCode` is deliberately NOT a needle:
    // `perl-tdd-support` has an unrelated `CodeSmell::DeadCode` variant, so a
    // bare match reports files that do not touch this surface at all. Every
    // realistic consumer of `DeadCode` also names one of the needles above,
    // because the only way to obtain one is through `DeadCodeDetector` or
    // `DeadCodeAnalysis`.
    "prelude::DeadCode",
];

/// Directories scanned for consumers, relative to the repository root.
const CONSUMER_SCAN_ROOTS: &[&str] = &["crates", "xtask/src"];

/// The twelve result states the reachability programme distinguishes. Every one
/// must be dispositioned; the set is fixed here so a state cannot quietly
/// disappear from the ledger.
const REQUIRED_RESULT_STATES: &[&str] = &[
    "complete_canonical_local",
    "complete_canonical_workspace",
    "complete_local_only_workspace_deferred",
    "partial_or_degraded",
    "cancelled",
    "deadline_exceeded",
    "resource_exhausted",
    "stale_or_superseded",
    "product_failure",
    "instrument_failure",
    "complete_semantics_bounded_view",
    "incomplete_semantic_computation",
];

const REQUIRED_ITEM_KINDS: &[&str] =
    &["module", "enum", "variant", "struct", "field", "method", "function"];

#[derive(Debug, Deserialize)]
struct Ledger {
    schema_version: u32,
    policy: String,
    human_ledger: String,
    module_source: String,
    public_api_baseline: String,
    compat_corpus: String,
    canonical_path: String,
    semantic_classes: Vec<String>,
    dispositions: Vec<String>,
    producer_states: Vec<String>,
    proof_ceilings: Vec<String>,
    representations: Vec<String>,
    defect_representations: Vec<String>,
    consumer_classes: Vec<String>,
    governance_paths: Vec<String>,
    #[serde(default)]
    export_path: Vec<ExportPath>,
    #[serde(default)]
    item: Vec<Item>,
    #[serde(default)]
    result_state: Vec<ResultState>,
    #[serde(default)]
    consumer: Vec<Consumer>,
    #[serde(default)]
    not_proven: Vec<NotProven>,
}

#[derive(Debug, Deserialize)]
struct ExportPath {
    path: String,
    role: String,
    declared_at: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct Item {
    id: String,
    kind: String,
    semantic_class: String,
    disposition: String,
    producer: String,
    proof_ceiling: String,
    replacement_owner: String,
    edit_authority: String,
    lossiness: String,
    removal_condition: String,
    #[serde(default)]
    fixtures: Vec<String>,
    /// Only meaningful for the entry-point API; `false` records that a path
    /// argument is not canonical root identity.
    #[serde(default)]
    root_identity: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ResultState {
    id: String,
    analyze_file: String,
    analyze_workspace: String,
    #[serde(default)]
    defect: String,
    #[serde(default)]
    owner: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct Consumer {
    path: String,
    class: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct NotProven {
    id: String,
    claim: String,
    reason: String,
}

#[derive(Debug)]
struct Stats {
    items: usize,
    never_produced: usize,
    inert: usize,
    defect_states: usize,
    consumers: usize,
    fixtures: usize,
    not_proven: usize,
}

pub fn run() -> Result<()> {
    let root = project_root()?;
    let stats = validate(&root)?;
    println!(
        "dead-code API ledger check passed: {} dispositioned items ({} never produced, {} inert), \
         {} result states recorded as defects, {} declared consumers, {} bound fixtures, \
         {} declared NOT_PROVEN boundaries",
        stats.items,
        stats.never_produced,
        stats.inert,
        stats.defect_states,
        stats.consumers,
        stats.fixtures,
        stats.not_proven
    );
    Ok(())
}

fn validate(root: &Path) -> Result<Stats> {
    let ledger = read_ledger(root, POLICY_PATH)?;
    let module_text = read_text(root, &ledger.module_source)?;
    let baseline_text = read_text(root, &ledger.public_api_baseline)?;
    let corpus_text = read_text(root, &ledger.compat_corpus)?;
    let human_text = read_text(root, &ledger.human_ledger)?;

    let mut violations = Vec::new();

    validate_shape(&ledger, &mut violations);
    let source_items = parse_module_surface(&module_text)
        .with_context(|| format!("failed to parse {}", ledger.module_source))?;
    validate_source_coverage(&ledger, &source_items, &mut violations);
    validate_baseline_coverage(&ledger, &baseline_text, &mut violations);
    validate_item_laws(&ledger, &mut violations);
    validate_result_states(&ledger, &mut violations);
    validate_fixtures(&ledger, &corpus_text, &mut violations);
    validate_consumers(root, &ledger, &mut violations);
    validate_human_projection(&ledger, &human_text, &mut violations);

    if !violations.is_empty() {
        eprintln!("dead-code API ledger violations:");
        for violation in &violations {
            eprintln!("  - {violation}");
        }
        bail!("dead-code API ledger check failed with {} violation(s)", violations.len());
    }

    Ok(Stats {
        items: ledger.item.len(),
        never_produced: ledger.item.iter().filter(|i| i.producer == "never_produced").count(),
        inert: ledger.item.iter().filter(|i| i.producer == "inert").count(),
        defect_states: ledger
            .result_state
            .iter()
            .filter(|s| {
                ledger.defect_representations.contains(&s.analyze_file)
                    || ledger.defect_representations.contains(&s.analyze_workspace)
            })
            .count(),
        consumers: ledger.consumer.len(),
        fixtures: ledger.item.iter().flat_map(|i| i.fixtures.iter()).collect::<BTreeSet<_>>().len(),
        not_proven: ledger.not_proven.len(),
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

// ---------------------------------------------------------------------------
// Shape
// ---------------------------------------------------------------------------

fn validate_shape(ledger: &Ledger, violations: &mut Vec<String>) {
    if ledger.schema_version != 1 {
        violations.push(format!(
            "schema_version must be 1 while this contract is current, found {}",
            ledger.schema_version
        ));
    }
    if ledger.policy != POLICY_PATH {
        violations
            .push(format!("policy self-reference must be {POLICY_PATH}, found {}", ledger.policy));
    }
    if ledger.canonical_path != "perl_parser::dead_code" {
        violations.push(format!(
            "canonical_path must be perl_parser::dead_code, found {}",
            ledger.canonical_path
        ));
    }
    for representation in &ledger.defect_representations {
        if !ledger.representations.contains(representation) {
            violations.push(format!(
                "defect_representations contains {representation}, which is not a declared representation"
            ));
        }
    }
    if ledger.export_path.is_empty() {
        violations.push("no export paths declared; the surface is reachable somehow".to_string());
    }
    let mut roles = BTreeSet::new();
    for export in &ledger.export_path {
        if export.declared_at.is_empty() {
            violations.push(format!("export path {} has no declared_at", export.path));
        }
        if export.note.trim().is_empty() {
            violations.push(format!(
                "export path {} has no note; each path must say what distinguishes it from the \
                 canonical one",
                export.path
            ));
        }
        if !roles.insert(export.role.clone()) && export.role == "canonical" {
            violations.push("more than one canonical export path declared".to_string());
        }
    }
    if !roles.contains("canonical") {
        violations.push("no canonical export path declared".to_string());
    }
    for np in &ledger.not_proven {
        if np.claim.trim().is_empty() || np.reason.trim().is_empty() {
            violations
                .push(format!("NOT_PROVEN row {} must carry both a claim and a reason", np.id));
        }
    }
}

// ---------------------------------------------------------------------------
// L1 — source coverage
// ---------------------------------------------------------------------------

/// One public item discovered in the module source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SourceItem {
    id: String,
    kind: String,
}

/// Extract the public surface of the module with `syn`, so the item inventory
/// comes from the code rather than from a hand-kept list.
fn parse_module_surface(text: &str) -> Result<BTreeSet<SourceItem>> {
    let file: syn::File = syn::parse_str(text).context("module source is not parseable Rust")?;
    let mut items = BTreeSet::new();
    // The module itself is always dispositioned.
    items.insert(SourceItem { id: "dead_code".to_string(), kind: "module".to_string() });

    for item in &file.items {
        match item {
            syn::Item::Enum(node) if is_public(&node.vis) => {
                let name = node.ident.to_string();
                items.insert(SourceItem { id: name.clone(), kind: "enum".to_string() });
                for variant in &node.variants {
                    items.insert(SourceItem {
                        id: format!("{name}::{}", variant.ident),
                        kind: "variant".to_string(),
                    });
                }
            }
            syn::Item::Struct(node) if is_public(&node.vis) => {
                let name = node.ident.to_string();
                items.insert(SourceItem { id: name.clone(), kind: "struct".to_string() });
                for field in &node.fields {
                    if !is_public(&field.vis) {
                        continue;
                    }
                    if let Some(ident) = field.ident.as_ref() {
                        items.insert(SourceItem {
                            id: format!("{name}::{ident}"),
                            kind: "field".to_string(),
                        });
                    }
                }
            }
            syn::Item::Fn(node) if is_public(&node.vis) => {
                items.insert(SourceItem {
                    id: node.sig.ident.to_string(),
                    kind: "function".to_string(),
                });
            }
            syn::Item::Impl(node) if node.trait_.is_none() => {
                let Some(self_name) = type_ident(&node.self_ty) else {
                    continue;
                };
                for inner in &node.items {
                    if let syn::ImplItem::Fn(method) = inner
                        && is_public(&method.vis)
                    {
                        items.insert(SourceItem {
                            id: format!("{self_name}::{}", method.sig.ident),
                            kind: "method".to_string(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(items)
}

fn is_public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

fn type_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

fn validate_source_coverage(
    ledger: &Ledger,
    source_items: &BTreeSet<SourceItem>,
    violations: &mut Vec<String>,
) {
    let ledger_kinds: BTreeMap<&str, &str> =
        ledger.item.iter().map(|i| (i.id.as_str(), i.kind.as_str())).collect();

    if ledger_kinds.len() != ledger.item.len() {
        violations.push("ledger contains duplicate item ids".to_string());
    }

    for source in source_items {
        match ledger_kinds.get(source.id.as_str()) {
            None => violations.push(format!(
                "L1: public item `{}` ({}) has no ledger disposition; every public item must be classified",
                source.id, source.kind
            )),
            Some(kind) if *kind != source.kind => violations.push(format!(
                "L1: ledger classifies `{}` as {} but the source declares a {}",
                source.id, kind, source.kind
            )),
            Some(_) => {}
        }
    }

    let source_ids: BTreeSet<&str> = source_items.iter().map(|s| s.id.as_str()).collect();
    for item in &ledger.item {
        if !source_ids.contains(item.id.as_str()) {
            violations.push(format!(
                "L1: ledger row `{}` names no public item in the module source; a retired item must \
                 leave the ledger with its removal, not outlive it",
                item.id
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// L2 — public-API baseline coverage
// ---------------------------------------------------------------------------

/// Reduce a public-API baseline line to the ledger item id it corresponds to,
/// or `None` when the line does not describe an item this ledger owns.
fn baseline_item_id(line: &str, canonical_path: &str) -> Option<String> {
    let rest = line.strip_prefix("pub ")?;
    // Strip the optional item keyword so what remains starts with the subject path.
    let rest = ["mod ", "enum ", "struct ", "fn ", "trait ", "type ", "const ", "static "]
        .iter()
        .find_map(|kw| rest.strip_prefix(*kw))
        .unwrap_or(rest);

    // The subject path ends at the first `(`, `<`, `:` (field type) or space.
    let subject: String =
        rest.chars().take_while(|c| !matches!(c, '(' | '<' | ' ')).collect::<String>();
    // A field row is `path::name: Type`; trim a trailing `:`.
    let subject = subject.trim_end_matches(':');

    if subject == canonical_path {
        return Some("dead_code".to_string());
    }
    // Require a real path boundary so `perl_parser::dead_code_detector` — the
    // compatibility alias, which duplicates every row — is not matched here.
    let tail = subject.strip_prefix(canonical_path)?.strip_prefix("::")?;
    if tail.is_empty() {
        return None;
    }
    // Drop derive-generated trait methods.
    if let Some((_, last)) = tail.rsplit_once("::")
        && DERIVED_METHODS.contains(&last)
    {
        return None;
    }
    Some(tail.to_string())
}

fn validate_baseline_coverage(ledger: &Ledger, baseline_text: &str, violations: &mut Vec<String>) {
    let ledger_ids: BTreeSet<&str> = ledger.item.iter().map(|i| i.id.as_str()).collect();
    let mut missing = BTreeSet::new();
    let mut matched = BTreeSet::new();
    for line in baseline_text.lines() {
        if let Some(id) = baseline_item_id(line.trim(), &ledger.canonical_path) {
            if !ledger_ids.contains(id.as_str()) {
                missing.insert(id.clone());
            }
            matched.insert(id);
        }
    }
    // Vacuity guard: if the baseline format moves and nothing matches any more,
    // L2 would pass by checking nothing. The surface always has at least the
    // module, the four types and the two free/associated entry points.
    if matched.len() < 6 {
        violations.push(format!(
            "L2: only {} item(s) recognised in {}; the baseline format changed and this law is no \
             longer checking anything",
            matched.len(),
            ledger.public_api_baseline
        ));
    }
    for id in missing {
        violations.push(format!(
            "L2: `{id}` is exported per {} but has no ledger disposition",
            ledger.public_api_baseline
        ));
    }
}

// ---------------------------------------------------------------------------
// L3, L4, L6 — per-item laws
// ---------------------------------------------------------------------------

fn validate_item_laws(ledger: &Ledger, violations: &mut Vec<String>) {
    for item in &ledger.item {
        let id = &item.id;
        require_member(id, "kind", &item.kind, REQUIRED_ITEM_KINDS, violations);
        require_member_owned(
            id,
            "semantic_class",
            &item.semantic_class,
            &ledger.semantic_classes,
            violations,
        );
        require_member_owned(
            id,
            "disposition",
            &item.disposition,
            &ledger.dispositions,
            violations,
        );
        require_member_owned(id, "producer", &item.producer, &ledger.producer_states, violations);
        require_member_owned(
            id,
            "proof_ceiling",
            &item.proof_ceiling,
            &ledger.proof_ceilings,
            violations,
        );

        if item.lossiness.trim().is_empty() {
            violations
                .push(format!("`{id}` declares no lossiness; \"none\" must be stated explicitly"));
        }
        if item.removal_condition.trim().is_empty() {
            violations.push(format!("`{id}` declares no removal condition or exit"));
        }
        if item.replacement_owner.trim().is_empty() {
            violations.push(format!("`{id}` names no replacement owner"));
        }

        // L4: no item on this surface authorizes an edit. #4205/#4208 own edit
        // authorization and nothing here may acquire it.
        if item.edit_authority != "none" {
            violations.push(format!(
                "L4: `{id}` claims edit_authority `{}`; this compatibility surface authorizes no edit",
                item.edit_authority
            ));
        }

        // L3: a variant nothing constructs may not be presented as working, and
        // must be held down by an executable negative control.
        if item.producer == "never_produced" {
            if !matches!(item.disposition.as_str(), "deprecate" | "remove") {
                violations.push(format!(
                    "L3: `{id}` is never produced but is dispositioned `{}`; an unimplemented item \
                     may only be deprecated or removed, never advertised as retained behavior",
                    item.disposition
                ));
            }
            if item.proof_ceiling != "none" {
                violations.push(format!(
                    "L3: `{id}` is never produced but claims proof ceiling `{}`",
                    item.proof_ceiling
                ));
            }
            if item.fixtures.is_empty() {
                violations.push(format!(
                    "L3: `{id}` is never produced but has no negative control; the claim would be \
                     unfalsifiable"
                ));
            }
        }

        // L6: inert configuration must say so, and must not be mapped to root identity.
        if item.producer == "inert" {
            if item.disposition != "deprecate" && item.disposition != "remove" {
                violations.push(format!(
                    "L6: `{id}` is inert but dispositioned `{}`; configuration nothing reads may \
                     not be retained as if it worked",
                    item.disposition
                ));
            }
            if item.fixtures.is_empty() {
                violations
                    .push(format!("L6: `{id}` is inert but has no fixture proving inertness"));
            }
        }
        if item.root_identity == Some(true) {
            violations.push(format!(
                "L6: `{id}` claims root identity; a path is a configuration candidate, not canonical \
                 root identity (#10871)"
            ));
        }

        // A produced item must be reachable through a proof class.
        if item.producer == "produced" && item.kind == "variant" && item.proof_ceiling == "none" {
            violations.push(format!(
                "`{id}` is produced but claims no proof ceiling; a produced finding class must \
                 record what its value can support"
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// L5 — result-state mapping
// ---------------------------------------------------------------------------

fn validate_result_states(ledger: &Ledger, violations: &mut Vec<String>) {
    let present: BTreeSet<&str> = ledger.result_state.iter().map(|s| s.id.as_str()).collect();
    if present.len() != ledger.result_state.len() {
        violations.push("duplicate result-state rows".to_string());
    }
    for required in REQUIRED_RESULT_STATES {
        if !present.contains(required) {
            violations.push(format!(
                "L5: result state `{required}` has no disposition; a state the surface cannot \
                 represent must be recorded, not omitted"
            ));
        }
    }
    for state in &ledger.result_state {
        if !REQUIRED_RESULT_STATES.contains(&state.id.as_str()) {
            violations.push(format!("L5: unknown result state `{}`", state.id));
        }
        require_member_owned(
            &state.id,
            "analyze_file",
            &state.analyze_file,
            &ledger.representations,
            violations,
        );
        require_member_owned(
            &state.id,
            "analyze_workspace",
            &state.analyze_workspace,
            &ledger.representations,
            violations,
        );

        let collapses = ledger.defect_representations.contains(&state.analyze_file)
            || ledger.defect_representations.contains(&state.analyze_workspace);
        if collapses {
            if state.defect.trim().is_empty() {
                violations.push(format!(
                    "L5: `{}` collapses into an ordinary-looking result but names no defect; a \
                     hidden state must be recorded as a defect, never accepted silently",
                    state.id
                ));
            }
            if state.owner.trim().is_empty() {
                violations.push(format!(
                    "L5: `{}` records a defect with no owner to route it to",
                    state.id
                ));
            }
            // A collapse is explained by its defect; `note` stays optional there.
        } else if state.note.trim().is_empty() {
            violations.push(format!(
                "L5: `{}` neither records a defect nor explains itself; a state disposition must \
                 say why it is representable, lossy or unreachable",
                state.id
            ));
        }
        if !collapses && !state.defect.trim().is_empty() {
            violations.push(format!(
                "L5: `{}` names a defect but neither entry point collapses; the defect field would \
                 be unreadable",
                state.id
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// L9, L10 — fixture binding
// ---------------------------------------------------------------------------

/// Fixture identities declared in the compatibility corpus, written as
/// `dcapi-<name>` in its section banners and doc comments.
fn corpus_fixture_ids(corpus_text: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for line in corpus_text.lines() {
        let mut rest = line;
        while let Some(idx) = rest.find("dcapi-") {
            let tail = &rest[idx..];
            let id: String =
                tail.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
            let id = id.trim_end_matches('-').to_string();
            if id.len() > "dcapi-".len() {
                ids.insert(id);
            }
            rest = &tail[1..];
        }
    }
    ids
}

fn validate_fixtures(ledger: &Ledger, corpus_text: &str, violations: &mut Vec<String>) {
    let corpus = corpus_fixture_ids(corpus_text);
    if corpus.is_empty() {
        violations.push(format!(
            "L9: no `dcapi-` fixture identities found in {}; the ledger would be unbound",
            ledger.compat_corpus
        ));
    }

    let mut cited = BTreeSet::new();
    for item in &ledger.item {
        for fixture in &item.fixtures {
            cited.insert(fixture.clone());
            if !corpus.contains(fixture) {
                violations.push(format!(
                    "L9: `{}` cites fixture `{fixture}`, which does not exist in {}",
                    item.id, ledger.compat_corpus
                ));
            }
        }
    }
    for fixture in &corpus {
        if !cited.contains(fixture) {
            violations.push(format!(
                "L10: fixture `{fixture}` exists in the corpus but no ledger row cites it; proof \
                 that supports no recorded claim is either mis-filed or the claim is missing"
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// L7 — consumer inventory
// ---------------------------------------------------------------------------

fn validate_consumers(root: &Path, ledger: &Ledger, violations: &mut Vec<String>) {
    let declared: BTreeMap<&str, &str> =
        ledger.consumer.iter().map(|c| (c.path.as_str(), c.class.as_str())).collect();
    if declared.len() != ledger.consumer.len() {
        violations.push("duplicate consumer rows".to_string());
    }

    for consumer in &ledger.consumer {
        require_member_owned(
            &consumer.path,
            "class",
            &consumer.class,
            &ledger.consumer_classes,
            violations,
        );
        if !root.join(&consumer.path).exists() {
            violations.push(format!(
                "L7: declared consumer {} does not exist; a stale inventory reads as evidence it is not",
                consumer.path
            ));
        }
        if consumer.note.trim().is_empty() {
            violations
                .push(format!("L7: consumer {} has no note explaining its class", consumer.path));
        }
    }

    let mut found = BTreeSet::new();
    for scan_root in CONSUMER_SCAN_ROOTS {
        let dir = root.join(scan_root);
        if !dir.exists() {
            violations.push(format!("L7: consumer scan root {scan_root} is missing"));
            continue;
        }
        for entry in WalkDir::new(&dir).into_iter().filter_map(std::result::Result::ok) {
            let path = entry.path();
            if !path.is_file() || path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            // Skip the target directory if a nested one exists.
            if path.components().any(|c| c.as_os_str() == "target") {
                continue;
            }
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            if CONSUMER_NEEDLES.iter().any(|needle| contains_word(&text, needle)) {
                let Ok(rel) = path.strip_prefix(root) else {
                    continue;
                };
                found.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    for path in &found {
        // The subject module and this ledger's own tooling name the surface in
        // order to govern it. The exemption list lives in the ledger so it stays
        // reviewable; it is checked for staleness below.
        if ledger.governance_paths.iter().any(|g| g == path) {
            continue;
        }
        if !declared.contains_key(path.as_str()) {
            violations.push(format!(
                "L7: {path} references the dead-code surface but is not in the consumer inventory; \
                 an undeclared consumer — production or otherwise — must be dispositioned before it lands"
            ));
        }
    }
    for governance in &ledger.governance_paths {
        if !root.join(governance).exists() {
            violations.push(format!("L7: governance path {governance} does not exist"));
        } else if !found.contains(governance.as_str()) {
            violations.push(format!(
                "L7: governance path {governance} no longer references the surface; a stale \
                 exemption can hide a real consumer"
            ));
        }
        if declared.contains_key(governance.as_str()) {
            violations.push(format!(
                "L7: {governance} is both a governance path and a declared consumer; it must be one \
                 or the other"
            ));
        }
    }

    for consumer in &ledger.consumer {
        // Re-export and dormant rows may live in files the needle scan also
        // finds; a declared row that the scan cannot see is a stale record.
        if !found.contains(consumer.path.as_str()) && consumer.class != "independent_implementation"
        {
            violations.push(format!(
                "L7: declared consumer {} no longer references the surface; retire the row",
                consumer.path
            ));
        }
        if consumer.class == "independent_implementation" && found.contains(consumer.path.as_str())
        {
            violations.push(format!(
                "L7: {} is declared an independent implementation but now references this surface; \
                 that is a duplicate-authority change, not a documentation change",
                consumer.path
            ));
        }
    }
}

/// Substring search with identifier boundaries, so `DeadCodeType` does not match
/// inside a longer identifier and `crate::dead_code` does not match
/// `crate::dead_code_detector`.
fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(idx, _)| {
        let before = haystack[..idx].chars().next_back();
        let after = haystack[idx + needle.len()..].chars().next();
        before.is_none_or(|c| !is_ident_char(c)) && after.is_none_or(|c| !is_ident_char(c))
    })
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

// ---------------------------------------------------------------------------
// L8 — Markdown projection agreement
// ---------------------------------------------------------------------------

fn validate_human_projection(ledger: &Ledger, human_text: &str, violations: &mut Vec<String>) {
    let Some(item_table) = parse_table(human_text, "Item") else {
        violations.push(format!("L8: {} has no item disposition table", ledger.human_ledger));
        return;
    };
    let Some(state_table) = parse_table(human_text, "Result state") else {
        violations.push(format!("L8: {} has no result-state table", ledger.human_ledger));
        return;
    };

    let expected_items: Vec<Vec<String>> = ledger
        .item
        .iter()
        .map(|i| {
            vec![
                format!("`{}`", i.id),
                i.kind.clone(),
                i.semantic_class.clone(),
                i.disposition.clone(),
                i.producer.clone(),
                i.proof_ceiling.clone(),
                i.replacement_owner.clone(),
            ]
        })
        .collect();
    compare_table("item", &expected_items, &item_table, &ledger.human_ledger, violations);

    let expected_states: Vec<Vec<String>> = ledger
        .result_state
        .iter()
        .map(|s| {
            vec![
                format!("`{}`", s.id),
                s.analyze_file.clone(),
                s.analyze_workspace.clone(),
                if s.owner.is_empty() { "—".to_string() } else { s.owner.clone() },
            ]
        })
        .collect();
    compare_table("result-state", &expected_states, &state_table, &ledger.human_ledger, violations);
}

fn compare_table(
    label: &str,
    expected: &[Vec<String>],
    actual: &[Vec<String>],
    human_ledger: &str,
    violations: &mut Vec<String>,
) {
    if expected.len() != actual.len() {
        violations.push(format!(
            "L8: {human_ledger} {label} table has {} rows, the ledger has {}; the projection is not \
             hand-edited and must be regenerated from the ledger",
            actual.len(),
            expected.len()
        ));
        return;
    }
    for (index, (want, got)) in expected.iter().zip(actual.iter()).enumerate() {
        if want != got {
            violations.push(format!(
                "L8: {human_ledger} {label} row {} disagrees with the ledger:\n      ledger: {:?}\n      \
                 markdown: {:?}",
                index + 1,
                want,
                got
            ));
        }
    }
}

/// Parse the first Markdown table whose header begins with `first_header_cell`.
fn parse_table(text: &str, first_header_cell: &str) -> Option<Vec<Vec<String>>> {
    let lines: Vec<&str> = text.lines().collect();
    let header_index = lines.iter().position(|line| {
        split_row(line)
            .is_some_and(|cells| cells.first().map(String::as_str) == Some(first_header_cell))
    })?;
    let mut rows = Vec::new();
    for line in lines.iter().skip(header_index + 1) {
        let Some(cells) = split_row(line) else {
            break;
        };
        if is_separator(&cells) {
            continue;
        }
        rows.push(cells);
    }
    Some(rows)
}

fn split_row(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') || trimmed.len() < 2 {
        return None;
    }
    Some(trimmed[1..trimmed.len() - 1].split('|').map(|cell| cell.trim().to_string()).collect())
}

fn is_separator(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells
            .iter()
            .all(|cell| !cell.is_empty() && cell.chars().all(|c| matches!(c, '-' | ':' | ' ')))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_member(
    subject: &str,
    field: &str,
    value: &str,
    allowed: &[&str],
    violations: &mut Vec<String>,
) {
    if !allowed.contains(&value) {
        violations.push(format!("`{subject}` has unknown {field} `{value}`"));
    }
}

fn require_member_owned(
    subject: &str,
    field: &str,
    value: &str,
    allowed: &[String],
    violations: &mut Vec<String>,
) {
    if !allowed.iter().any(|candidate| candidate == value) {
        violations.push(format!("`{subject}` has unknown {field} `{value}`"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tracked ledger, projection, module source, baseline and corpus agree
    /// on the current tree. This is the gate: it fails when any of the five
    /// drifts away from the others.
    #[test]
    fn tracked_ledger_is_valid() -> Result<()> {
        let root = project_root()?;
        validate(&root)?;
        Ok(())
    }

    fn sample_source() -> &'static str {
        r#"
        pub enum Kind { A, B }
        pub struct Item { pub field: usize, private: usize }
        pub fn free() {}
        impl Item { pub fn method(&self) {} fn hidden(&self) {} }
        "#
    }

    #[test]
    fn source_surface_finds_public_items_and_skips_private_ones() -> Result<()> {
        let items = parse_module_surface(sample_source())?;
        let ids: BTreeSet<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.contains("dead_code"), "the module itself is always dispositioned");
        assert!(ids.contains("Kind"));
        assert!(ids.contains("Kind::A"));
        assert!(ids.contains("Item::field"));
        assert!(ids.contains("Item::method"));
        assert!(ids.contains("free"));
        assert!(!ids.contains("Item::private"), "private fields are not public API");
        assert!(!ids.contains("Item::hidden"), "private methods are not public API");
        Ok(())
    }

    #[test]
    fn baseline_ids_reject_the_alias_and_derive_noise() {
        let canonical = "perl_parser::dead_code";
        assert_eq!(
            baseline_item_id("pub mod perl_parser::dead_code", canonical).as_deref(),
            Some("dead_code")
        );
        assert_eq!(
            baseline_item_id("pub enum perl_parser::dead_code::DeadCodeType", canonical).as_deref(),
            Some("DeadCodeType")
        );
        assert_eq!(
            baseline_item_id("pub perl_parser::dead_code::DeadCode::confidence: f32", canonical)
                .as_deref(),
            Some("DeadCode::confidence")
        );
        assert_eq!(
            baseline_item_id(
                "pub fn perl_parser::dead_code::DeadCodeDetector::analyze_file(&self) -> ()",
                canonical
            )
            .as_deref(),
            Some("DeadCodeDetector::analyze_file")
        );
        // The compatibility alias duplicates every row; matching it would double
        // the inventory and make L2 unfalsifiable.
        assert_eq!(
            baseline_item_id(
                "pub fn perl_parser::dead_code_detector::generate_report(x: u8)",
                canonical
            ),
            None
        );
        // Derive-generated trait methods are consequences of the type's row.
        assert_eq!(
            baseline_item_id(
                "pub fn perl_parser::dead_code::DeadCode::clone(&self) -> perl_parser::dead_code::DeadCode",
                canonical
            ),
            None
        );
        // A prelude row whose *type* mentions the module is not a row of it.
        assert_eq!(
            baseline_item_id(
                "pub perl_parser::prelude::DeadCode::code_type: perl_parser::dead_code::DeadCodeType",
                canonical
            ),
            None
        );
    }

    #[test]
    fn contains_word_respects_identifier_boundaries() {
        assert!(contains_word("use crate::dead_code;", "crate::dead_code"));
        assert!(!contains_word("use crate::dead_code_detector;", "crate::dead_code"));
        assert!(contains_word("let x: DeadCodeType = y;", "DeadCodeType"));
        assert!(!contains_word("struct MyDeadCodeTypeThing;", "DeadCodeType"));
        // The bare attribute must never register as a consumer.
        assert!(!contains_word("#[allow(dead_code)]", "crate::dead_code"));
        assert!(!contains_word("#[allow(dead_code)]", "perl_parser::dead_code"));
    }

    /// `perl-tdd-support` has its own unrelated `CodeSmell::DeadCode` variant.
    /// Scanning for the bare identifier reported it as a consumer of this
    /// surface, which it is not, so the needle set uses the prelude path
    /// instead. This pins the decision so the broad needle is not reintroduced.
    #[test]
    fn consumer_needles_do_not_match_an_unrelated_dead_code_identifier() {
        let unrelated = "    /// Unreachable or unused code\n    DeadCode,\n";
        assert!(
            !CONSUMER_NEEDLES.iter().any(|needle| contains_word(unrelated, needle)),
            "an unrelated `DeadCode` enum variant must not register as a consumer"
        );
        // The prelude route still registers.
        let prelude_use = "use perl_parser::prelude::DeadCode;";
        assert!(CONSUMER_NEEDLES.iter().any(|needle| contains_word(prelude_use, needle)));
    }

    #[test]
    fn corpus_fixture_ids_are_extracted() {
        let text = "// dcapi-one proves a thing\n//! see `dcapi-two-three`.\nlet x = 1;\n";
        let ids = corpus_fixture_ids(text);
        assert!(ids.contains("dcapi-one"));
        assert!(ids.contains("dcapi-two-three"));
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn markdown_tables_parse_and_skip_separators() {
        let text = "| Item | Kind |\n| --- | --- |\n| `a` | enum |\n| `b` | field |\n\ntext\n";
        let table = parse_table(text, "Item").expect("table");
        assert_eq!(table, vec![vec!["`a`", "enum"], vec!["`b`", "field"]]);
    }

    // ---- Falsifiers: each mutates a valid ledger and asserts the named law bites ----

    fn law_violations(mutate: impl FnOnce(&mut Ledger)) -> Result<Vec<String>> {
        let root = project_root()?;
        let mut ledger = read_ledger(&root, POLICY_PATH)?;
        mutate(&mut ledger);
        let mut violations = Vec::new();
        validate_item_laws(&ledger, &mut violations);
        validate_result_states(&ledger, &mut violations);
        Ok(violations)
    }

    #[test]
    fn falsifier_l3_never_produced_item_cannot_be_retained() -> Result<()> {
        let violations = law_violations(|ledger| {
            for item in &mut ledger.item {
                if item.producer == "never_produced" {
                    item.disposition = "retain_projection".to_string();
                }
            }
        })?;
        assert!(
            violations.iter().any(|v| v.starts_with("L3:")),
            "presenting an unimplemented variant as retained behavior must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn falsifier_l3_never_produced_item_needs_a_negative_control() -> Result<()> {
        let violations = law_violations(|ledger| {
            for item in &mut ledger.item {
                if item.producer == "never_produced" {
                    item.fixtures.clear();
                }
            }
        })?;
        assert!(
            violations.iter().any(|v| v.contains("unfalsifiable")),
            "an unimplemented variant with no negative control must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn falsifier_l4_no_item_may_claim_edit_authority() -> Result<()> {
        let violations = law_violations(|ledger| {
            if let Some(item) = ledger.item.iter_mut().find(|i| i.id == "generate_report") {
                item.edit_authority = "safe_delete".to_string();
            }
        })?;
        assert!(
            violations.iter().any(|v| v.starts_with("L4:")),
            "a report gaining deletion authority must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn falsifier_l5_a_collapsed_state_cannot_be_accepted_silently() -> Result<()> {
        let violations = law_violations(|ledger| {
            for state in &mut ledger.result_state {
                state.defect.clear();
                state.owner.clear();
            }
        })?;
        assert!(
            violations.iter().any(|v| v.contains("names no defect")),
            "hiding a collapse must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn falsifier_l5_every_state_must_be_dispositioned() -> Result<()> {
        let violations = law_violations(|ledger| {
            ledger.result_state.retain(|s| s.id != "incomplete_semantic_computation");
        })?;
        assert!(
            violations.iter().any(|v| v.contains("incomplete_semantic_computation")),
            "dropping a state must fail rather than read as absent: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn falsifier_l6_entry_point_cannot_claim_root_identity() -> Result<()> {
        let violations = law_violations(|ledger| {
            if let Some(item) =
                ledger.item.iter_mut().find(|i| i.id == "DeadCodeDetector::add_entry_point")
            {
                item.root_identity = Some(true);
            }
        })?;
        assert!(
            violations.iter().any(|v| v.starts_with("L6:") && v.contains("root identity")),
            "a path masquerading as root identity must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn falsifier_l1_a_new_public_item_is_unclassified() -> Result<()> {
        let root = project_root()?;
        let ledger = read_ledger(&root, POLICY_PATH)?;
        let mut source = parse_module_surface(&read_text(&root, &ledger.module_source)?)?;
        source.insert(SourceItem {
            id: "DeadCodeType::UnusedLabel".to_string(),
            kind: "variant".to_string(),
        });
        let mut violations = Vec::new();
        validate_source_coverage(&ledger, &source, &mut violations);
        assert!(
            violations.iter().any(|v| v.starts_with("L1:") && v.contains("UnusedLabel")),
            "a new public item must fail until it is dispositioned: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn falsifier_l7_an_undeclared_consumer_fails() -> Result<()> {
        let root = project_root()?;
        let mut ledger = read_ledger(&root, POLICY_PATH)?;
        let dropped = ledger.consumer.remove(0);
        let mut violations = Vec::new();
        validate_consumers(&root, &ledger, &mut violations);
        assert!(
            violations.iter().any(|v| v.starts_with("L7:") && v.contains(dropped.path.as_str())),
            "a consumer that exists but is not inventoried must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn falsifier_l9_a_ledger_row_cannot_cite_a_missing_fixture() -> Result<()> {
        let root = project_root()?;
        let mut ledger = read_ledger(&root, POLICY_PATH)?;
        let corpus = read_text(&root, &ledger.compat_corpus)?;
        if let Some(item) = ledger.item.first_mut() {
            item.fixtures.push("dcapi-does-not-exist".to_string());
        }
        let mut violations = Vec::new();
        validate_fixtures(&ledger, &corpus, &mut violations);
        assert!(
            violations.iter().any(|v| v.starts_with("L9:")),
            "citing proof that does not exist must fail: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn falsifier_l8_markdown_drift_is_rejected() -> Result<()> {
        let root = project_root()?;
        let ledger = read_ledger(&root, POLICY_PATH)?;
        let human = read_text(&root, &ledger.human_ledger)?;
        let drifted = human.replacen("retain_projection", "remove", 1);
        let mut violations = Vec::new();
        validate_human_projection(&ledger, &drifted, &mut violations);
        assert!(
            violations.iter().any(|v| v.starts_with("L8:")),
            "a hand-edited projection must fail: {violations:?}"
        );
        Ok(())
    }
}
