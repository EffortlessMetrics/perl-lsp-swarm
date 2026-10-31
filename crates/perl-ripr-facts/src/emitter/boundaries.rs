//! Dynamic-boundary detection + typed verify-command candidates (Campaign 31
//! Phase B PR 8, perl-lsp-swarm#2595).
//!
//! `emit_boundaries_and_commands` emits both `dynamic_boundaries[]` and
//! `verify_commands[]` in one file walk — the issue's target shape suggested
//! a separate `verify.rs`, but the real implementation shares one scan of
//! `.pm`/`.t` files across both outputs, so splitting them would mean
//! restructuring the walk (a rewrite), not moving code. Kept fused here; see
//! the #9271 PR notes for that deviation.

use perl_parser_core::line_index::LineIndex;
use perl_parser_core::{Parser, SourceRegionIndex, SourceRegionKind};
use perl_symbol::surface::extract_symbol_decls;
use serde_json::{Value, json};

use super::discovery::{collect_pm_files, collect_t_files};
use super::ids::owner_fact_id;
use super::owners::owner_kind;

/// Patterns that indicate a dynamic boundary in Perl source. Each entry maps
/// a string-search pattern to the ripr BoundaryKind it represents.
const DYNAMIC_BOUNDARY_PATTERNS: &[(&str, &str)] = &[
    ("eval {", "eval_or_string_code"),
    ("eval(", "eval_or_string_code"),
    ("eval '", "eval_or_string_code"),
    ("eval\"", "eval_or_string_code"),
    ("->$", "dynamic_dispatch"),
    ("::->", "dynamic_dispatch"),
    ("can(", "framework_indirection"),
    ("AUTOLOAD", "framework_indirection"),
    ("@ISA", "role_composition"),
    ("use parent", "role_composition"),
    ("use base", "role_composition"),
    ("*{", "symbol_table_mutation"),
    ("no strict", "unsupported_syntax"),
    ("BEGIN {", "unsupported_syntax"),
    ("require $", "module_resolution_unknown"),
];

/// Byte offsets of statement-position `eval` keywords starting a spaced string
/// form (`eval $code`, `eval "..."`) — the spaced forms the substring table
/// cannot express (#17264).
///
/// Guards (the table's bare substrings already over-match e.g. `retrieval(`;
/// this matcher must do better, not worse):
/// - the byte before `eval` must be start-of-content or a statement char —
///   never an identifier char, a `$@%` sigil, or `>`, `-`, `:` (which would
///   make it a method call, `->eval`, `::eval`, `$eval`, or `--eval`);
/// - the byte after `eval` must be whitespace (glued forms like `eval(` stay
///   with the table, so no occurrence double-emits);
/// - the bytes after `eval` must not be exactly the table's `"eval {"` or
///   `"eval '"` prefixes (single space + `{`/`'`): those occurrences belong
///   to the table, and matching them here would emit every one twice
///   (#17283). Wider gaps (`eval  {`, tabs, newlines, `eval  '...'`) are the
///   matcher's — the table's literals cannot express them;
/// - a `sub`/`method` keyword immediately before (over whitespace) makes it a
///   definition named `eval`, not a call — skipped;
/// - the occurrence must not sit inside a comment, POD, or string/quoted
///   span. High-confidence `eval_or_string_code` facts fail closed
///   downstream, so a comment like `sub safe { # eval $code` must not
///   block guidance (#17289). Classification uses lexer token spans via
///   [`SourceRegionIndex`] — not a second full parse.
///
/// Also matched (#18541): bare `eval;` / `evalbytes;` (they evaluate `$_`),
/// `{ eval }` closing a block, `evalbytes` in every spelling, and the
/// `CORE::eval` spelling of the keyword. `$h{eval}` and `eval =>` are not
/// calls and stay unmatched.
///
/// Known miss, documented: `LABEL: eval $x` (label colon) is excluded by the
/// `:` rule. Vanishingly rare; the recall gain on the string-eval shape
/// dwarfs it.
fn statement_eval_offsets(content: &str, test_file: bool) -> Vec<usize> {
    let bytes = content.as_bytes();
    let mut offsets = Vec::new();
    let mut regions: Option<SourceRegionIndex> = None;
    for (offset, _) in content.match_indices("eval") {
        // `evalbytes` evaluates a byte string; it has no block form and no
        // table entry, so every spelling of it is ours (#18541).
        let is_evalbytes = content[offset..].starts_with("evalbytes");
        let kw_len = if is_evalbytes { "evalbytes".len() } else { "eval".len() };
        // `CORE::eval` is the same keyword spelled with its package; the
        // method/qualified `:` exclusion below must not hide it (#18541).
        let start = if offset >= 6 && &bytes[offset - 6..offset] == b"CORE::" {
            offset - 6
        } else {
            offset
        };
        let prev_byte = start.checked_sub(1).map(|i| bytes[i]);
        let prev_ok = match prev_byte {
            None => true,
            // Statement chars: everything that can legally precede a keyword
            // call, minus identifier chars, `$@%` sigils, and `>`, `-`, `:`
            // (method/qualified/label forms).
            Some(b) => b";{}()[]=,!&|+*/<? \t\r\n".contains(&b),
        };
        if !prev_ok {
            continue;
        }
        let rest = bytes.get(offset + kw_len..).unwrap_or_default();
        // `eval => 1` is a hash key before a fat comma, not a call.
        if rest.iter().copied().skip_while(u8::is_ascii_whitespace).take(2).eq(*b"=>") {
            continue;
        }
        let next_significant = rest.iter().copied().find(|b| !b.is_ascii_whitespace());
        // Bare `eval;` / `evalbytes;` evaluate `$_`. `{ eval }` ends a block
        // the same way, but `$h{eval}` is a hash key, so the `}` form needs a
        // space or statement end before the keyword (#18541).
        let bare = match (rest.first().copied(), next_significant) {
            (None, _) => true,
            (Some(b';'), _) => true,
            (Some(b'}'), _) => matches!(prev_byte, Some(b' ' | b'\t' | b'\r' | b'\n' | b';')),
            (Some(first), None) if first.is_ascii_whitespace() => true,
            (Some(first), Some(b';')) if first.is_ascii_whitespace() => true,
            (Some(first), Some(b'}')) if first.is_ascii_whitespace() => {
                matches!(prev_byte, Some(b' ' | b'\t' | b'\r' | b'\n' | b';'))
            }
            _ => false,
        };
        // `evalbytes` has no table entry, so every glued opener is ours:
        // `evalbytes(`, `evalbytes"..."`, `evalbytes'...'`, `evalbytes$code`.
        let glued_evalbytes =
            is_evalbytes && matches!(rest.first(), Some(b'(' | b'"' | b'\'' | b'$'));
        if !bare && !glued_evalbytes {
            let Some(first) = rest.first().copied() else {
                continue;
            };
            if !first.is_ascii_whitespace() {
                continue;
            }
            // Block `eval { ... }` is lexically scoped static code that only
            // traps an exception. In a test file the trapped code is what the
            // test drives, so the block is not a boundary and the oracle on
            // `$@` is graded instead (#18498). In a source file the same
            // block can hide the owner's own failure path, so it stays a
            // boundary: the table owns the exact `"eval {"` spelling, wider
            // gaps are ours.
            if !is_evalbytes
                && next_significant == Some(b'{')
                && (test_file || rest.starts_with(b" {"))
            {
                continue;
            }
            // Table overlap: `"eval '"` with a single-space gap is the
            // table's entry — skip it so one occurrence yields one boundary,
            // while wider-gap forms stay ours (#17283). `evalbytes '` is not
            // a table hit.
            if !is_evalbytes && rest.starts_with(b" '") {
                continue;
            }
        }
        // `sub eval` / `method eval`: a definition, not a call.
        let before = content[..start].trim_end();
        if before.ends_with("sub") || before.ends_with("method") {
            let keyword_len = if before.ends_with("sub") { 3 } else { 6 };
            let boundary_ok = before[..before.len() - keyword_len]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
            if boundary_ok {
                continue;
            }
        }
        let regions = regions.get_or_insert_with(|| SourceRegionIndex::build(content));
        if statement_eval_is_non_code(regions, offset) {
            continue;
        }
        offsets.push(offset);
    }
    offsets
}

/// Whether a candidate `eval` keyword sits in a non-code span that must not
/// emit a high-confidence boundary (#17289).
fn statement_eval_is_non_code(regions: &SourceRegionIndex, offset: usize) -> bool {
    matches!(
        regions.kind_at_offset(offset),
        SourceRegionKind::LineComment
            | SourceRegionKind::Pod
            | SourceRegionKind::StringLiteral
            | SourceRegionKind::QuoteLike
            | SourceRegionKind::Heredoc
    )
}

#[derive(Debug)]
struct BoundaryOwner {
    owner_id: String,
    start_byte: usize,
    end_byte: usize,
}

fn boundary_owner_index(relative_path: &str, content: &str) -> Vec<BoundaryOwner> {
    let mut parser = Parser::new(content);
    let Ok(ast) = parser.parse() else {
        return Vec::new();
    };
    extract_symbol_decls(&ast, Some("main"))
        .into_iter()
        .filter_map(|decl| {
            let kind = owner_kind(&decl.kind)?;
            let (start_byte, end_byte) = decl.full_span;
            Some(BoundaryOwner {
                owner_id: owner_fact_id(
                    relative_path,
                    kind,
                    &decl.qualified_name,
                    start_byte,
                    end_byte,
                ),
                start_byte,
                end_byte,
            })
        })
        .collect()
}

fn enclosing_boundary_owner(owners: &[BoundaryOwner], offset: usize) -> Option<&BoundaryOwner> {
    owners
        .iter()
        .filter(|owner| owner.start_byte <= offset && offset < owner.end_byte)
        .min_by_key(|owner| owner.end_byte.saturating_sub(owner.start_byte))
}

fn boundary_evidence_refs(owner_id: Option<&str>, file_id: &str) -> Vec<Value> {
    match owner_id {
        Some(owner_id) => vec![json!(owner_id)],
        None => vec![json!(file_id)],
    }
}

/// Emit one `dynamic_boundaries` fact + its limitation for a pattern hit at
/// `offset`, attributing the enclosing owner. Shared by the substring table
/// and the statement-`eval` matcher so the two cannot render divergent facts.
#[allow(clippy::too_many_arguments)]
fn push_boundary(
    boundaries: &mut Vec<Value>,
    limitations: &mut Vec<Value>,
    boundary_counter: &mut usize,
    file_path: &str,
    file_id: &str,
    owner_index: &[BoundaryOwner],
    line_index: &LineIndex,
    pattern: &str,
    boundary_kind: &str,
    offset: usize,
    len: usize,
) {
    *boundary_counter += 1;
    let boundary_id = format!("boundary:{file_path}:{boundary_kind}:{boundary_counter}");
    let owner_id =
        enclosing_boundary_owner(owner_index, offset).map(|owner| owner.owner_id.as_str());
    let ((start_line, start_column), (end_line, end_column)) =
        line_index.range(offset, offset + len);
    let evidence_refs = boundary_evidence_refs(owner_id, file_id);
    boundaries.push(json!({
        "boundary_id": boundary_id,
        "kind": boundary_kind,
        "file_id": file_id,
        "owner_id": owner_id,
        "range": {
            "start_line": start_line,
            "start_column": start_column,
            "end_line": end_line,
            "end_column": end_column,
        },
        "confidence": "high",
        "provenance_refs": []
    }));
    limitations.push(json!({
        "limitation_id": format!("limitation:{boundary_id}"),
        "kind": boundary_kind,
        "message": format!("Dynamic boundary `{pattern}` detected in {file_path}; ripr fails closed on this boundary kind."),
        "evidence_refs": evidence_refs
    }));
}

pub(crate) fn dynamic_boundaries_in_lines(
    lines: &[String],
    test_file: bool,
) -> Vec<(&'static str, &'static str)> {
    let mut seen_kinds = std::collections::HashSet::new();
    let mut boundaries = Vec::new();
    for line in lines {
        for &(pattern, boundary_kind) in DYNAMIC_BOUNDARY_PATTERNS {
            if test_file && pattern == "eval {" {
                continue;
            }
            if line.contains(pattern) && seen_kinds.insert(boundary_kind) {
                boundaries.push((pattern, boundary_kind));
            }
        }
    }
    // `#17264` / `#17283`: same spaced-`eval` recall as the file emitter, so
    // a hunk adding `eval $code` attributes the boundary its file scan would
    // find. Runs over the joined hunk text — one contiguous added-line run,
    // the same `join("\n")` reconstruction the change digest uses — so an
    // `eval` + `$code` split across hunk lines is not missed, with the shared
    // `seen_kinds` set keeping one entry per boundary kind.
    if !statement_eval_offsets(&lines.join("\n"), test_file).is_empty()
        && seen_kinds.insert("eval_or_string_code")
    {
        boundaries.push(("eval <whitespace>", "eval_or_string_code"));
    }
    boundaries
}

/// Emit dynamic-boundary facts + limitations + typed verify-command candidates.
///
/// Campaign 31 Phase B PR 8 (perl-lsp-swarm#2595). The final Phase B slice:
/// closes the producer with boundary detection, limitations, verify-command
/// candidates, and deterministic output.
///
/// - **Dynamic boundaries**: scans `.pm` + `.t` files for the patterns in `DYNAMIC_BOUNDARY_PATTERNS`. Each match emits a `dynamic_boundaries` entry + a corresponding `limitations` entry. All boundaries fail closed in ripr's strict-actionability validator.
///
/// - **Typed verify-command candidates**: derives `prove <test_path>` for each
///   `.t` file. These are candidates — ripr's typed validator (PR 13) accepts/
///   rejects them. NOT shell strings; ripr generates the receipt command.
///
/// - **Deterministic goldens**: the emitter scans files in sorted order + emits
///   arrays in a stable order (sorted by ID), so the same input always produces
///   the same packet.
pub(crate) fn emit_boundaries_and_commands(root: &str) -> (Vec<Value>, Vec<Value>, Vec<Value>) {
    let mut boundaries = Vec::new();
    let mut limitations = Vec::new();
    let mut verify_commands = Vec::new();

    // Scan .pm files for dynamic boundaries.
    let pm_files = collect_pm_files(std::path::Path::new(root));
    let t_files = collect_t_files(std::path::Path::new(root));

    let mut boundary_counter = 0usize;

    // Scan all source files (.pm + .t) for boundary patterns.
    let mut all_files: Vec<(String, String)> = Vec::new();
    for (path, content) in &pm_files {
        all_files.push((path.clone(), content.clone()));
    }
    for (_full, relative, content) in &t_files {
        all_files.push((relative.clone(), content.clone()));
    }
    all_files.sort_by(|a, b| a.0.cmp(&b.0));

    for (file_path, content) in &all_files {
        let test_file = file_path.ends_with(".t");
        let file_id = format!("file:{file_path}");
        let owner_index = boundary_owner_index(file_path, content);
        let line_index = LineIndex::new(content.clone());
        for (pattern, boundary_kind) in DYNAMIC_BOUNDARY_PATTERNS {
            if test_file && *pattern == "eval {" {
                continue;
            }
            for (offset, _) in content.match_indices(pattern) {
                push_boundary(
                    &mut boundaries,
                    &mut limitations,
                    &mut boundary_counter,
                    file_path,
                    &file_id,
                    &owner_index,
                    &line_index,
                    pattern,
                    boundary_kind,
                    offset,
                    pattern.len(),
                );
            }
        }
        // `#17264`: spaced `eval` forms (`eval $code`, `eval "..."`,
        // `eval  {`) the substring table cannot express. Runs after the table
        // over the shared counter, so pre-existing boundary ids are stable;
        // the exact-prefix skip (` {`, ` '`) guarantees no occurrence
        // double-emits with the table's `"eval {"`/`eval(`/`eval'`/`eval"`
        // entries (#17283).
        for offset in statement_eval_offsets(content, test_file) {
            push_boundary(
                &mut boundaries,
                &mut limitations,
                &mut boundary_counter,
                file_path,
                &file_id,
                &owner_index,
                &line_index,
                "eval <whitespace>",
                "eval_or_string_code",
                offset,
                "eval".len(),
            );
        }
    }

    // Emit typed verify-command candidates for each .t file.
    let mut cmd_counter = 0usize;
    let mut sorted_t: Vec<&(String, String, String)> = t_files.iter().collect();
    sorted_t.sort_by(|a, b| a.1.cmp(&b.1));
    for (_full, relative, _content) in &sorted_t {
        cmd_counter += 1;
        let command_id = format!("verify_cmd:{relative}:{cmd_counter}");
        verify_commands.push(json!({
            "command_id": command_id,
            "runner": "prove",
            "argv": ["prove", relative],
            "scope": "test",
            "test_id": format!("test:{relative}"),
            "confidence": "high",
            "preconditions": [],
            "provenance_refs": []
        }));
    }

    (boundaries, limitations, verify_commands)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use perl_tdd_support::{must, must_some};

    #[test]
    fn emit_boundaries_detects_eval() {
        let root = std::env::temp_dir().join("perl-B8-eval-root");
        let lib_dir = root.join("lib/My");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::write(
            lib_dir.join("App.pm"),
            "package My::App;\nsub run { my $c = shift; eval $c; }\n1;",
        ));

        let (boundaries, limitations, _cmds) =
            emit_boundaries_and_commands(must_some(root.to_str()));
        assert!(!boundaries.is_empty(), "string eval must produce a boundary fact");
        assert!(
            boundaries.iter().any(|b| b["kind"] == "eval_or_string_code"),
            "must have an eval_or_string_code boundary"
        );
        assert!(!limitations.is_empty(), "each boundary must have a corresponding limitation");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn statement_eval_matches_spaced_forms() {
        // `#17264`: the common string-eval shapes the table misses.
        assert_eq!(statement_eval_offsets("my $r = eval $code;", false).len(), 1);
        assert_eq!(statement_eval_offsets("my $r = eval \"$code\";", false).len(), 1);
        assert_eq!(
            statement_eval_offsets("eval $code;", false).len(),
            1,
            "statement start matches"
        );
        assert_eq!(
            statement_eval_offsets("my $r = eval ($code);", false).len(),
            1,
            "spaced paren matches (the table only has the glued form)"
        );
        assert_eq!(
            statement_eval_offsets("if ($x) {\n\teval\n$code;\n}", false).len(),
            1,
            "newline follower matches"
        );
        // Block `eval { ... }` is not a boundary in a test file in any
        // spelling (#18498): single space, wide gap, tab, newline.
        for block in [
            "my $r = eval { compute() };",
            "my $r = eval  { compute() };",
            "eval\t{ };",
            "eval\n{ };",
        ] {
            assert!(statement_eval_offsets(block, true).is_empty(), "block form `{block}`");
        }
        assert_eq!(
            statement_eval_offsets("my $r = eval  'die';", false).len(),
            1,
            "double-space quote matches (the table only has single-space)"
        );
    }

    #[test]
    fn statement_eval_rejects_glued_and_non_call_forms() {
        // Glued forms stay with the substring table (no double-emit).
        assert!(statement_eval_offsets("eval{}", false).is_empty());
        assert!(statement_eval_offsets("eval($x)", false).is_empty());
        assert!(statement_eval_offsets("eval\"$x\"", false).is_empty());
        assert!(statement_eval_offsets("eval'$x'", false).is_empty());
        // Single-space block/quote forms belong to the table's `"eval {"` /
        // `"eval '"` entries — the matcher must not double-emit them (#17283).
        assert!(statement_eval_offsets("my $r = eval { };", false).is_empty());
        assert!(statement_eval_offsets("my $r = eval { };", true).is_empty());
        assert!(statement_eval_offsets("sub run { eval '1 + 1'; }", false).is_empty());
        // Non-calls must not match (the issue's false-positive watch list).
        assert!(statement_eval_offsets("my $eval = 1;", false).is_empty(), "$eval var");
        assert!(statement_eval_offsets("$obj->eval $x;", false).is_empty(), "method call");
        assert!(statement_eval_offsets("Foo::eval $x;", false).is_empty(), "qualified call");
        assert!(statement_eval_offsets("sub eval { 1 }", false).is_empty(), "sub definition");
        assert!(statement_eval_offsets("retrieval $x;", false).is_empty(), "eval inside a word");
    }

    #[test]
    fn statement_eval_skips_comment_string_and_pod_but_keeps_calls() {
        // `#17289`: high-confidence matcher must not fire on non-code spans.
        assert!(
            statement_eval_offsets("sub safe { # eval $code\n return 1; }", false).is_empty(),
            "line comment must not count as a statement eval"
        );
        assert!(
            statement_eval_offsets(r#"my $text = "say eval $code";"#, false).is_empty(),
            "string literal must not count as a statement eval"
        );
        assert!(
            statement_eval_offsets("=pod\n\neval $code\n\n=cut\nsub f { 1 }\n", false).is_empty(),
            "POD must not count as a statement eval"
        );
        // Positive controls: real calls still count, including the wide-gap
        // block form the table cannot express, even when a comment also
        // mentions eval.
        assert_eq!(
            statement_eval_offsets("eval $code", false).len(),
            1,
            "code eval $code still counts"
        );
        assert_eq!(
            statement_eval_offsets("eval  {", false).len(),
            1,
            "wide-gap eval still counts"
        );
        assert_eq!(
            statement_eval_offsets("eval $code; # eval $other\n", false).len(),
            1,
            "code eval still counts beside a comment mention"
        );
    }

    #[test]
    fn emit_boundaries_comment_eval_is_not_a_boundary() {
        // End to end: the issue repro must not fail-closed a safe sub.
        let root = std::env::temp_dir().join("perl-B8-eval-comment-root");
        let lib_dir = root.join("lib");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::write(
            lib_dir.join("Safe.pm"),
            "package Safe;\nsub safe { # eval $code\n return 1; }\n1;",
        ));

        let (boundaries, _limitations, _cmds) =
            emit_boundaries_and_commands(must_some(root.to_str()));
        let evals: Vec<_> =
            boundaries.iter().filter(|b| b["kind"] == "eval_or_string_code").collect();
        assert!(evals.is_empty(), "comment eval must not emit a boundary: {evals:?}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dynamic_boundaries_in_lines_skips_comment_eval() {
        let lines = vec!["sub safe { # eval $code".to_string(), " return 1; }".to_string()];
        let found = dynamic_boundaries_in_lines(&lines, false);
        assert!(
            found.iter().all(|(_, kind)| *kind != "eval_or_string_code"),
            "hunk comment eval must not yield a boundary: {found:?}"
        );
    }

    #[test]
    fn emit_boundaries_detects_spaced_eval_once() {
        // End to end: one spaced `eval $code` yields exactly one boundary
        // (matcher) with no table double-emit.
        let root = std::env::temp_dir().join("perl-B8-eval-spaced-root");
        let lib_dir = root.join("lib");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::write(
            lib_dir.join("Spaced.pm"),
            "package Spaced;\nsub run { my $c = shift; my $r = eval $c; return $r; }\n1;",
        ));

        let (boundaries, limitations, _cmds) =
            emit_boundaries_and_commands(must_some(root.to_str()));
        let evals: Vec<_> =
            boundaries.iter().filter(|b| b["kind"] == "eval_or_string_code").collect();
        assert_eq!(evals.len(), 1, "spaced eval must yield exactly one boundary");
        assert!(
            limitations.iter().any(|l| l["kind"] == "eval_or_string_code"),
            "the boundary must have a matching limitation"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn statement_eval_matches_bare_evalbytes_and_core_spellings() {
        // #18541: runtime string evaluation the first matcher missed.
        for src in [
            "eval;",
            "my $r = eval ;",
            "sub run { eval }",
            "sub run {\n    eval\n}",
            "evalbytes $code;",
            "my $r = evalbytes \"$code\";",
            "evalbytes($code);",
            "evalbytes\"$code\";",
            "evalbytes'die';",
            "evalbytes$code;",
            "evalbytes;",
            "evalbytes 'die';",
            "CORE::eval $code;",
            "my $r = CORE::eval \"$code\";",
        ] {
            for test_file in [false, true] {
                assert_eq!(
                    statement_eval_offsets(src, test_file).len(),
                    1,
                    "`{src}` (test file: {test_file})"
                );
            }
        }
    }

    #[test]
    fn statement_eval_rejects_keys_names_and_qualified_lookalikes() {
        for src in [
            "$h{eval}",
            "my $v = $opts{eval};",
            "%h = (eval => 1);",
            "$obj->evalbytes $x;",
            "$obj->eval;",
            "Foo::evalbytes $x;",
            "CORE::GLOBAL::eval $c;",
            "sub evalbytes { 1 }",
            "my $evalbytes = 1;",
            "evaluate $x;",
            "eval_it;",
            "retrieval;",
            "evalbytes_helper($x);",
        ] {
            for test_file in [false, true] {
                assert!(
                    statement_eval_offsets(src, test_file).is_empty(),
                    "`{src}` (test file: {test_file})"
                );
            }
        }
    }

    #[test]
    fn bare_evalbytes_and_core_eval_fail_closed_in_source_and_test_files() {
        // #18541 runtime control: exactly one boundary per occurrence, in
        // both file kinds, and no double-emit against the substring table.
        for (name, body) in [
            ("bare", "sub run { eval; }"),
            ("bare_close", "sub run { eval }"),
            ("evalbytes_var", "sub run { my $c = shift; evalbytes $c; }"),
            ("evalbytes_paren", "sub run { my $c = shift; evalbytes($c); }"),
            ("core_var", "sub run { my $c = shift; CORE::eval $c; }"),
            ("core_paren", "sub run { my $c = shift; CORE::eval($c); }"),
            ("core_quote", "sub run { CORE::eval '1 + 1'; }"),
        ] {
            let pm = format!("package Probe;\n{body}\n1;");
            assert_eq!(eval_boundary_count(name, &pm), 1, "source `{body}`");
            let t = format!("use Test::More;\n{body}\n");
            assert_eq!(
                eval_boundary_count_in(&format!("t-{name}"), "t/probe.t", &t),
                1,
                "test `{body}`"
            );
        }
    }

    #[test]
    fn hash_key_eval_and_lookalikes_add_no_boundary() {
        for (name, body) in [
            ("key", "sub run { my %o = @_; return $o{eval}; }"),
            ("fat_comma", "sub run { return (eval => 1); }"),
            ("method", "sub run { my $o = shift; return $o->eval; }"),
        ] {
            let pm = format!("package Probe;\n{body}\n1;");
            assert_eq!(eval_boundary_count(name, &pm), 0, "source `{body}`");
        }
    }

    #[test]
    fn dynamic_boundaries_in_lines_matches_bare_evalbytes_and_core_eval() {
        for line in ["eval;", "evalbytes $c;", "CORE::eval $c;", "evalbytes($c);"] {
            for test_file in [false, true] {
                let found = dynamic_boundaries_in_lines(&[line.to_string()], test_file);
                assert!(
                    found.iter().any(|(_, kind)| *kind == "eval_or_string_code"),
                    "hunk line `{line}` (test file: {test_file}): {found:?}"
                );
            }
        }
        let found = dynamic_boundaries_in_lines(&["my $v = $o{eval};".to_string()], false);
        assert!(
            found.iter().all(|(_, kind)| *kind != "eval_or_string_code"),
            "hash key is not a boundary: {found:?}"
        );
    }

    #[test]
    fn emit_boundaries_quoted_eval_emits_once() {
        // `#17283`: `eval '...'` is covered by the table's `"eval '"` entry;
        // the matcher must not add a second boundary for the same occurrence.
        let root = std::env::temp_dir().join("perl-B8-eval-quoted-root");
        let lib_dir = root.join("lib");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::write(
            lib_dir.join("Quoted.pm"),
            "package Quoted;\nsub run { eval '1 + 1'; }\n1;",
        ));

        let (boundaries, _limitations, _cmds) =
            emit_boundaries_and_commands(must_some(root.to_str()));
        let evals: Vec<_> =
            boundaries.iter().filter(|b| b["kind"] == "eval_or_string_code").collect();
        assert_eq!(evals.len(), 1, "quoted eval must yield exactly one boundary");

        let _ = std::fs::remove_dir_all(&root);
    }

    fn eval_boundary_count(name: &str, source: &str) -> usize {
        eval_boundary_count_in(name, "lib/Probe.pm", source)
    }

    fn eval_boundary_count_in(name: &str, rel: &str, source: &str) -> usize {
        let root = std::env::temp_dir().join(format!("perl-18498-{name}"));
        let path = root.join(rel);
        must(std::fs::create_dir_all(must_some(path.parent())));
        must(std::fs::write(&path, source));
        let (boundaries, _limitations, _cmds) =
            emit_boundaries_and_commands(must_some(root.to_str()));
        let _ = std::fs::remove_dir_all(&root);
        boundaries.iter().filter(|b| b["kind"] == "eval_or_string_code").count()
    }

    #[test]
    fn block_eval_is_not_a_boundary_in_a_test_file() {
        // #18498: in a test file `eval { ... }` only traps the exception the
        // test is observing, so it must not fail the whole file closed.
        for (name, body) in [
            ("single", "eval { die };"),
            ("wide", "my $v = eval  { compute() };"),
            ("tab", "my $v = eval\t{ compute() };"),
            ("newline", "my $v = eval\n{ compute() };"),
            ("nested", "eval { eval { die }; };"),
        ] {
            let source = format!("use Test::More;\n{body}\nlike($@, qr/x/);\n");
            assert_eq!(
                eval_boundary_count_in(&format!("t-{name}"), "t/probe.t", &source),
                0,
                "block eval `{body}`"
            );
        }
    }

    #[test]
    fn block_eval_in_a_source_file_stays_a_boundary() {
        // #18498 runtime control: the same block in the owner can hide the
        // owner's own failure path, so it keeps failing closed.
        for (name, body) in [
            ("single", "sub run { eval { die }; }"),
            ("wide", "sub run { my $v = eval  { compute() }; }"),
            ("tab", "sub run { my $v = eval\t{ compute() }; }"),
            ("newline", "sub run { my $v = eval\n{ compute() }; }"),
        ] {
            let source = format!("package Probe;\n{body}\n1;");
            assert_eq!(eval_boundary_count(name, &source), 1, "block eval `{body}`");
        }
    }

    #[test]
    fn string_and_dynamic_eval_still_fail_closed_beside_block_eval() {
        // Runtime-control test for #18498: narrowing the block form must not
        // let any string-evaluated code through. Each shape yields exactly one
        // boundary, including when a block eval shares the file.
        for (name, body) in [
            ("dollar", "sub run { my $c = shift; eval $c; }"),
            ("dquote", "sub run { my $c = shift; eval \"$c\"; }"),
            ("squote", "sub run { eval '1 + 1'; }"),
            ("paren", "sub run { my $c = shift; eval($c); }"),
            ("spaced_paren", "sub run { my $c = shift; eval ($c); }"),
            ("wide_quote", "sub run { eval  'die'; }"),
        ] {
            let source = format!("package Probe;\n{body}\n1;");
            assert_eq!(eval_boundary_count(name, &source), 1, "string eval `{body}`");
            // The same string forms stay boundaries in a test file, beside a block eval.
            let t_source = format!("use Test::More;\neval {{ die }};\n{body}\n");
            assert!(
                eval_boundary_count_in(&format!("t-{name}"), "t/probe.t", &t_source) >= 1,
                "string eval `{body}` in a test file"
            );
        }
    }

    #[test]
    fn dynamic_dispatch_and_other_kinds_coexist_with_source_block_eval() {
        let root = std::env::temp_dir().join("perl-18498-other-kinds");
        let lib_dir = root.join("lib");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::write(
            lib_dir.join("Probe.pm"),
            "package Probe;\nour @ISA = ('Base');\nsub call { my $m = shift; eval { $obj->$m(); }; }\n1;",
        ));
        let (boundaries, _limitations, _cmds) =
            emit_boundaries_and_commands(must_some(root.to_str()));
        let _ = std::fs::remove_dir_all(&root);
        let kinds: Vec<&str> = boundaries.iter().filter_map(|b| b["kind"].as_str()).collect();
        assert!(kinds.contains(&"dynamic_dispatch"), "dispatch inside a block eval: {kinds:?}");
        assert!(kinds.contains(&"role_composition"), "@ISA: {kinds:?}");
        assert!(kinds.contains(&"eval_or_string_code"), "source block eval stays: {kinds:?}");
    }

    #[test]
    fn dynamic_boundaries_in_lines_matches_multiline_eval() {
        // `#17283`: `eval` + `$code` split across hunk lines must attribute
        // the boundary the file scan would find.
        let lines = vec!["my $r = eval".to_string(), "$code;".to_string()];
        let found = dynamic_boundaries_in_lines(&lines, false);
        assert_eq!(
            found.iter().filter(|(_, kind)| *kind == "eval_or_string_code").count(),
            1,
            "multiline eval across hunk lines must yield one boundary: {found:?}"
        );
    }

    #[test]
    fn dynamic_boundaries_in_lines_matches_spaced_and_wide_gap_eval() {
        // Single-line spaced forms keep working after the joined-text change,
        // and wide-gap blocks are caught in the diff scan too (#17283).
        for line in ["my $r = eval $code;", "my $r = eval  'die';", "eval ($c);"] {
            let lines = vec![line.to_string()];
            let found = dynamic_boundaries_in_lines(&lines, false);
            assert!(
                found.iter().any(|(_, kind)| *kind == "eval_or_string_code"),
                "hunk line `{line}` must yield an eval boundary: {found:?}"
            );
        }
    }

    #[test]
    fn dynamic_boundaries_in_lines_block_eval_follows_file_kind() {
        for line in ["eval { die };", "my $r = eval  { compute() };", "eval\t{ };"] {
            let in_test = dynamic_boundaries_in_lines(&[line.to_string()], true);
            assert!(
                in_test.iter().all(|(_, kind)| *kind != "eval_or_string_code"),
                "test-file block eval `{line}` is not a boundary: {in_test:?}"
            );
            let in_source = dynamic_boundaries_in_lines(&[line.to_string()], false);
            assert!(
                in_source.iter().any(|(_, kind)| *kind == "eval_or_string_code"),
                "source block eval `{line}` stays a boundary: {in_source:?}"
            );
        }
    }

    #[test]
    fn dynamic_boundaries_in_lines_dedups_table_and_matcher_eval() {
        // A table hit plus a matcher hit in one hunk still yield one entry.
        let lines = vec!["eval 'die';".to_string(), "my $r = eval $c;".to_string()];
        let found = dynamic_boundaries_in_lines(&lines, false);
        assert_eq!(
            found.iter().filter(|(_, kind)| *kind == "eval_or_string_code").count(),
            1,
            "one hunk must yield one eval entry: {found:?}"
        );
    }

    #[test]
    fn emit_boundaries_detects_dynamic_dispatch() {
        let root = std::env::temp_dir().join("perl-B8-dispatch-root");
        let lib_dir = root.join("lib");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::write(
            lib_dir.join("Dynamic.pm"),
            "package Dynamic;\nsub call { my $m = shift; $obj->$m(); }\n1;",
        ));

        let (boundaries, limitations, _cmds) =
            emit_boundaries_and_commands(must_some(root.to_str()));
        let boundary = boundaries
            .iter()
            .find(|b| b["kind"] == "dynamic_dispatch")
            .expect("->$method() must produce a dynamic_dispatch boundary");
        let owner_id = boundary["owner_id"].as_str().expect("dynamic boundary is owner-scoped");
        assert!(owner_id.contains(":call:"), "boundary owner should be the enclosing sub");
        let limitation = limitations
            .iter()
            .find(|l| l["kind"] == "dynamic_dispatch")
            .expect("dynamic boundary has a matching limitation");
        assert!(
            limitation["evidence_refs"]
                .as_array()
                .expect("evidence refs")
                .iter()
                .any(|r| r.as_str() == Some(owner_id)),
            "limitation should be scoped to the dynamic boundary owner"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enclosing_boundary_owner_treats_end_byte_as_exclusive() {
        let owners = vec![BoundaryOwner {
            owner_id: "owner:lib/App.pm:sub:App::run:10-20".to_string(),
            start_byte: 10,
            end_byte: 20,
        }];

        assert!(
            enclosing_boundary_owner(&owners, 19).is_some(),
            "offset inside [start, end) belongs to the owner"
        );
        assert!(
            enclosing_boundary_owner(&owners, 20).is_none(),
            "offset at end_byte must not be attributed to the owner"
        );
    }

    #[test]
    fn emit_verify_commands_for_t_files() {
        let root = std::env::temp_dir().join("perl-B8-cmds-root");
        let t_dir = root.join("t");
        must(std::fs::create_dir_all(&t_dir));
        must(std::fs::write(t_dir.join("alpha.t"), "use Test::More;\nok(1);\n"));
        must(std::fs::write(t_dir.join("beta.t"), "use Test::More;\nok(1);\n"));

        let (_boundaries, _limitations, verify_commands) =
            emit_boundaries_and_commands(must_some(root.to_str()));

        assert_eq!(verify_commands.len(), 2, "must emit one verify-command per .t file");
        // Verify commands use 'prove' runner.
        assert!(
            verify_commands.iter().all(|c| c["runner"] == "prove"),
            "all verify-commands must use prove runner"
        );
        // Commands are deterministic (sorted by path).
        assert!(
            verify_commands[0]["argv"][1].as_str().unwrap_or("")
                < verify_commands[1]["argv"][1].as_str().unwrap_or(""),
            "verify-commands must be sorted by path (deterministic)"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn emit_boundaries_returns_empty_when_no_source_files() {
        let root = std::env::temp_dir().join("perl-B8-empty-root");
        must(std::fs::create_dir_all(&root));
        let (boundaries, limitations, cmds) =
            emit_boundaries_and_commands(must_some(root.to_str()));
        assert!(boundaries.is_empty());
        assert!(limitations.is_empty());
        assert!(cmds.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
