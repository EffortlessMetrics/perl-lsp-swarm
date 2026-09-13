//! Recurrence guard for live runtime-proof producer labels (#14303 / #12102).
//!
//! The four live runtime proofs (references, definition, document_symbols,
//! workspace_symbols) answer through `WorkspaceIndex` / `SemanticQueries` /
//! parser-syntax source-backed paths. A nominal `compiler_receipt` /
//! compiler-PIR producer label on those routes creates a false before-state
//! for the #12075 cutover.
//!
//! The guard scans only those four live route sources. Sibling live proofs
//! that still own a different claim (semantic tokens, refactor blockers) are
//! intentionally out of scope.
//!
//! Unlock requires an actual generation-owned compiler contribution joined in
//! an executable position (`FilePirLexicalContribution` via #9284/#8669).
//! Mention, import, or comment of that type is not lineage.

const REFERENCES_LIVE_ROUTE: &str = include_str!("references.rs");
const DEFINITION_LIVE_ROUTE: &str = include_str!("navigation.rs");
const DOCUMENT_SYMBOLS_LIVE_ROUTE: &str = include_str!("symbols.rs");
const WORKSPACE_SYMBOLS_LIVE_ROUTE: &str = include_str!("../workspace.rs");

const COMPILER_LINEAGE_MARKER: &str = "FilePirLexicalContribution";

/// Executable positions that count as joining the envelope into the route's
/// lineage. An import path (`use crate::compiler::FilePirLexicalContribution;`)
/// matches none of these.
const LINEAGE_USE_PATTERNS: &[&str] = &[
    ": FilePirLexicalContribution",
    "-> FilePirLexicalContribution",
    "as FilePirLexicalContribution",
    "<FilePirLexicalContribution",
    "(FilePirLexicalContribution",
];

/// Nominal producer labels forbidden without real contribution lineage.
const NOMINAL_PRODUCER_LABELS: &[&str] = &[
    "\"compiler_receipt\"",
    "ENABLE_PIR",
    "PIR_A_LEXICAL_REFERENCES",
    "live compiler source-backed",
    "source-backed compiler facts",
    "compiler_fact_candidates=",
    "compiler_result_count=",
    "source_backed_compiler_symbols=",
    "document_symbols_empty_compiler_receipt",
];

struct LiveCohort {
    name: &'static str,
    source: &'static str,
}

const LIVE_COHORTS: &[LiveCohort] = &[
    LiveCohort { name: "references", source: REFERENCES_LIVE_ROUTE },
    LiveCohort { name: "definition", source: DEFINITION_LIVE_ROUTE },
    LiveCohort { name: "document_symbols", source: DOCUMENT_SYMBOLS_LIVE_ROUTE },
    LiveCohort { name: "workspace_symbols", source: WORKSPACE_SYMBOLS_LIVE_ROUTE },
];

fn envelope_joined_in_executable_position(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("use ") {
            return false;
        }
        LINEAGE_USE_PATTERNS.iter().any(|pattern| line.contains(pattern))
    })
}

fn labels_in(source: &str) -> Vec<&'static str> {
    NOMINAL_PRODUCER_LABELS.iter().copied().filter(|label| source.contains(label)).collect()
}

/// Split a source file into function bodies plus leftover module text.
/// Lineage in one function must not unlock nominal labels in another.
fn source_units(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut units = Vec::new();
    let mut occupied = vec![false; bytes.len()];
    let mut i = 0;
    while i < bytes.len() {
        if let Some(fn_start) = find_fn_keyword(source, i) {
            if let Some((_, body_end)) = function_body_span(source, fn_start) {
                units.push(source[fn_start..body_end].to_string());
                occupied[fn_start..body_end].fill(true);
                i = body_end;
                continue;
            }
            i = fn_start + 2;
            continue;
        }
        break;
    }
    let rest: String = source
        .char_indices()
        .filter_map(|(idx, ch)| {
            let span = ch.len_utf8();
            if occupied[idx..idx + span].iter().any(|flag| *flag) { None } else { Some(ch) }
        })
        .collect();
    if rest.chars().any(|ch| !ch.is_whitespace()) {
        units.push(rest);
    }
    if units.is_empty() {
        units.push(source.to_string());
    }
    units
}

fn find_fn_keyword(source: &str, from: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = from;
    while i + 2 <= bytes.len() {
        if bytes[i] == b'f' && bytes[i + 1] == b'n' && fn_keyword_at(source, i) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn fn_keyword_at(source: &str, idx: usize) -> bool {
    let bytes = source.as_bytes();
    let before_ok = idx == 0 || {
        let prev = bytes[idx - 1];
        !(prev.is_ascii_alphanumeric() || prev == b'_')
    };
    let after = idx + 2;
    let after_ok = after >= bytes.len() || {
        let next = bytes[after];
        !(next.is_ascii_alphanumeric() || next == b'_')
    };
    before_ok && after_ok
}

fn function_body_span(source: &str, fn_start: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut i = fn_start;
    while i < bytes.len() {
        match bytes[i] {
            b';' => return None,
            b'{' => {
                let end = matching_brace_end(source, i)?;
                return Some((fn_start, end));
            }
            _ => i += 1,
        }
    }
    None
}

fn matching_brace_end(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    let mut in_string = false;
    let mut in_char = false;
    let mut escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if in_char {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'\'' {
                in_char = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                in_string = true;
                i += 1;
            }
            b'\'' => {
                in_char = true;
                i += 1;
            }
            b'{' => {
                depth = depth.saturating_add(1);
                i += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => i += 1,
        }
    }
    None
}

fn nominal_labels_without_lineage(source: &str) -> Vec<&'static str> {
    let mut violations = Vec::new();
    for unit in source_units(source) {
        if envelope_joined_in_executable_position(&unit) {
            continue;
        }
        for label in labels_in(&unit) {
            if !violations.contains(&label) {
                violations.push(label);
            }
        }
    }
    violations
}

#[test]
fn each_live_runtime_proof_reports_no_stronger_producer_than_its_semantic_source() {
    let mut dirty = Vec::new();
    for cohort in LIVE_COHORTS {
        let violations = nominal_labels_without_lineage(cohort.source);
        if !violations.is_empty() {
            dirty.push((cohort.name, violations));
        }
    }
    assert!(
        dirty.is_empty(),
        "live runtime proofs carry nominal compiler/PIR producer labels without an actual \
         {COMPILER_LINEAGE_MARKER} contribution lineage (#14303): {dirty:?}; restore truthful \
         semantic naming or join a real compiler contribution"
    );
}

#[test]
fn guard_scans_exactly_the_four_live_runtime_proof_cohorts() {
    let names: Vec<&str> = LIVE_COHORTS.iter().map(|cohort| cohort.name).collect();
    assert_eq!(
        names,
        vec!["references", "definition", "document_symbols", "workspace_symbols"],
        "producer_truth_guard must cover the four live runtime proofs named by #14303"
    );
}

#[test]
fn guard_does_not_treat_semantic_tokens_as_a_live_runtime_proof_cohort() {
    let names: Vec<&str> = LIVE_COHORTS.iter().map(|cohort| cohort.name).collect();
    assert!(
        !names.contains(&"semantic_tokens"),
        "semantic_tokens remains a separate claim; folding it into this guard would absorb work #14303 excluded"
    );
}

#[test]
fn guard_unlocks_when_an_actual_compiler_envelope_joins_the_route() {
    let with_real_lineage = "fn join(c: FilePirLexicalContribution) {\n\
         tracing::debug!(\"References: returned live compiler facts\");\n\
         let _receipt = json!({\"compiler_receipt\": c});\n\
         let _note = \"compiler_fact_candidates=1; compiler_result_count=1\";\n}";
    let violations = nominal_labels_without_lineage(with_real_lineage);
    assert!(
        violations.is_empty(),
        "the guard must permit genuine compiler contribution lineage; unexpectedly flagged {violations:?}"
    );
}

#[test]
fn guard_stays_red_when_the_envelope_is_only_imported() {
    let mention_only_import = "use crate::compiler::FilePirLexicalContribution;\n\
         let _receipt = json!({\"compiler_receipt\": null});\n";
    let violations = nominal_labels_without_lineage(mention_only_import);
    assert_eq!(
        violations,
        vec!["\"compiler_receipt\""],
        "a bare import mention must not unlock nominal producer labels"
    );
}

#[test]
fn guard_stays_red_when_the_envelope_is_only_commented() {
    let mention_only_comment = "// TODO(#9284): thread FilePirLexicalContribution through here.\n\
         tracing::debug!(\"References: returned source-backed compiler facts\");\n";
    let violations = nominal_labels_without_lineage(mention_only_comment);
    assert!(!violations.is_empty(), "a comment mention must not unlock nominal producer labels");
}

#[test]
fn guard_stays_red_when_a_lineage_pattern_appears_only_on_a_comment_line() {
    let commented_join = "// fn join(c: FilePirLexicalContribution) {}\n\
         let _receipt = json!({\"compiler_receipt\": null});\n";
    let violations = nominal_labels_without_lineage(commented_join);
    assert_eq!(
        violations,
        vec!["\"compiler_receipt\""],
        "a commented type annotation must not count as executable lineage"
    );
}

#[test]
fn guard_stays_red_when_a_lineage_pattern_appears_only_on_a_doc_comment() {
    let doc_comment_join = "/// Accepts `: FilePirLexicalContribution` once #9284 lands.\n\
         let _note = \"compiler_fact_candidates=4\";\n";
    let violations = nominal_labels_without_lineage(doc_comment_join);
    assert_eq!(
        violations,
        vec!["compiler_fact_candidates="],
        "a doc-comment type annotation must not count as executable lineage"
    );
}

#[test]
fn guard_catches_definition_shaped_compiler_fact_candidate_notes() {
    let definition_shaped = "receipt.notes.push(format!(\n\
         \"definition runtime proof: compiler_fact_candidates={}; compiler_result_count={}\",\n\
         1, 1));\n";
    let violations = nominal_labels_without_lineage(definition_shaped);
    assert!(
        violations.contains(&"compiler_fact_candidates=")
            && violations.contains(&"compiler_result_count="),
        "definition-shaped note keys must fail the guard without lineage: {violations:?}"
    );
}

#[test]
fn guard_catches_workspace_shaped_source_backed_compiler_symbol_notes() {
    let workspace_shaped = "format!(\"workspace-symbol runtime quality receipt: source_backed_compiler_symbols={}\", 3)\n";
    let violations = nominal_labels_without_lineage(workspace_shaped);
    assert_eq!(
        violations,
        vec!["source_backed_compiler_symbols="],
        "workspace/document-symbol note keys must fail the guard without lineage: {violations:?}"
    );
}

#[test]
fn guard_catches_a_single_dirty_cohort_without_requiring_the_other_three() {
    let dirty_definition = "let _ = json!({\"compiler_receipt\": null});\n";
    let clean_references = "let _ = json!({\"source_backed_receipt\": null});\n";
    assert!(
        !nominal_labels_without_lineage(dirty_definition).is_empty(),
        "one dirty cohort must be visible on its own"
    );
    assert!(
        nominal_labels_without_lineage(clean_references).is_empty(),
        "a clean sibling must not be required to fail before a dirty cohort is reported"
    );
}

#[test]
fn guard_does_not_flag_the_intentional_not_compiler_fact_comment() {
    let honest_split = "// It is intentionally not labeled `compiler_fact`;\n\
         Self::SemanticSourceBacked => \"semantic_fact\",\n";
    let violations = nominal_labels_without_lineage(honest_split);
    assert!(
        violations.is_empty(),
        "the #3046 tracking comment that refuses `compiler_fact` must not itself trip the guard: {violations:?}"
    );
}

#[test]
fn guard_stays_red_when_lineage_joins_an_unrelated_function() {
    let unrelated_join = "fn unused(c: FilePirLexicalContribution) {}\n\
         fn proof() {\n\
         let _receipt = json!({\"compiler_receipt\": null});\n\
         }\n";
    let violations = nominal_labels_without_lineage(unrelated_join);
    assert_eq!(
        violations,
        vec!["\"compiler_receipt\""],
        "lineage in an unrelated function must not unlock nominal producer labels elsewhere"
    );
}
