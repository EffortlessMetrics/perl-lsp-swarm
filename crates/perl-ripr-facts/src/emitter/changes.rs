//! Diff-owned `changes[]` (#3293 PR 5): parses a caller-supplied unified diff
//! into contiguous added-line hunks and attributes each to the smallest
//! enclosing `owners[]` fact.

use serde_json::{Value, json};

use super::boundaries::dynamic_boundaries_in_lines;
use super::ids::{content_hash_to_digest, fnv1a_hash};

/// One contiguous run of added (`+`) lines within a single file's diff, tagged
/// with the head-file line range it lands on (0-based, tracked from the
/// `@@ -a,b +c,d @@` header). Pure text — no filesystem or byte offsets.
struct DiffHunkRun {
    file_path: String,
    start_line: u32,
    end_line: u32,
    lines: Vec<String>,
}

/// Parse a unified diff into contiguous added-line runs, one per uninterrupted
/// block of `+` lines, tracking the head-file line cursor from each
/// `@@ -a,b +c,d @@` header. Removed (`-`) lines do not advance the head cursor;
/// context lines do. Pure text parsing — no filesystem access, no subprocess.
fn parse_diff_hunks(diff_text: &str) -> Vec<DiffHunkRun> {
    fn flush(run: &mut Option<DiffHunkRun>, runs: &mut Vec<DiffHunkRun>) {
        if let Some(finished) = run.take() {
            runs.push(finished);
        }
    }

    let mut runs = Vec::new();
    let mut current_file: Option<String> = None;
    let mut head_line: u32 = 0;
    let mut run: Option<DiffHunkRun> = None;

    for line in diff_text.lines() {
        if let Some(rest) = line.strip_prefix("+++ b/") {
            flush(&mut run, &mut runs);
            current_file = Some(rest.trim().to_string());
            continue;
        }
        if line.starts_with("+++") || line.starts_with("---") {
            flush(&mut run, &mut runs);
            continue;
        }
        if let Some(header_rest) = line.strip_prefix("@@") {
            flush(&mut run, &mut runs);
            head_line = parse_hunk_new_start(header_rest).unwrap_or(0);
            continue;
        }
        if line.starts_with('\\') {
            // `\ No newline at end of file` — metadata about the preceding line,
            // present in neither file version. Do not flush the open run or
            // advance the head cursor (advancing it would shift every following
            // added line down by one).
            continue;
        }
        if let Some(added) = line.strip_prefix('+') {
            let file = current_file.clone().unwrap_or_default();
            match run {
                Some(ref mut open) if open.file_path == file => {
                    open.end_line = head_line;
                    open.lines.push(added.to_string());
                }
                _ => {
                    flush(&mut run, &mut runs);
                    if current_file.is_some() {
                        run = Some(DiffHunkRun {
                            file_path: file,
                            start_line: head_line,
                            end_line: head_line,
                            lines: vec![added.to_string()],
                        });
                    }
                }
            }
            head_line += 1;
        } else if line.starts_with('-') {
            // Removed line: not present in the head file, so the cursor holds.
            flush(&mut run, &mut runs);
        } else {
            // Context (or blank) line: present in the head file, advances cursor.
            flush(&mut run, &mut runs);
            head_line += 1;
        }
    }
    flush(&mut run, &mut runs);
    runs
}

/// From a hunk header body ` -a,b +c,d @@ ...`, return the new-file start line
/// `c` as a 0-based line (`c - 1`). `None` if the `+c` token is unparseable.
fn parse_hunk_new_start(header_rest: &str) -> Option<u32> {
    let plus = header_rest.split_whitespace().find(|tok| tok.starts_with('+'))?;
    let start: u32 = plus.trim_start_matches('+').split(',').next()?.parse().ok()?;
    Some(start.saturating_sub(1))
}

/// Smallest owner (by line span) in `owners` that belongs to `file_id` and whose
/// `[range.start_line, range.end_line]` inclusively contains `[start_line,
/// end_line]`. Ties (equal span) break toward `sub`/`method` over `package`, so
/// the result is deterministic and independent of `owners` order. `None` when no
/// owner contains the range (file-/script-level code, or a file with zero owners).
fn find_enclosing_owner<'a>(
    owners: &'a [Value],
    file_id: &str,
    start_line: u32,
    end_line: u32,
) -> Option<&'a Value> {
    let mut best: Option<&Value> = None;
    let mut best_span = u64::MAX;
    let mut best_is_sub = false;
    for owner in owners {
        if owner["file_id"].as_str() != Some(file_id) {
            continue;
        }
        let range = &owner["range"];
        let (Some(owner_start), Some(owner_end)) =
            (range["start_line"].as_u64(), range["end_line"].as_u64())
        else {
            continue;
        };
        let (owner_start, owner_end) = (owner_start as u32, owner_end as u32);
        if owner_start <= start_line && end_line <= owner_end {
            let span = u64::from(owner_end - owner_start);
            let is_sub = matches!(owner["kind"].as_str(), Some("sub") | Some("method"));
            if span < best_span || (span == best_span && is_sub && !best_is_sub) {
                best = Some(owner);
                best_span = span;
                best_is_sub = is_sub;
            }
        }
    }
    best
}

/// Pick a `behavior_hint` + discriminator for a hunk by scanning its added lines
/// top-to-bottom; the first line matching a known pattern (predicate boundary →
/// return value → exception path) wins. No match on any line → `"unknown"`.
fn behavior_hint_for_hunk(lines: &[String]) -> (&'static str, String) {
    for line in lines {
        // A whole-line comment is never executable, so it must never yield a
        // concrete behavior hint (e.g. `# return $x;` is not a return). This is
        // a cheap, safe filter; false positives from `die`/`return` substrings
        // *inside string literals* remain possible and are documented as a
        // limitation (a robust fix needs tokenization, out of this slice's scope).
        if line.trim_start().starts_with('#') {
            continue;
        }
        let (hint, discriminator) = infer_behavior_and_discriminator(line);
        if hint != "unknown" {
            return (hint, discriminator);
        }
    }
    ("unknown", String::new())
}

/// Normalize a `git diff` head path (repo-root-relative, e.g.
/// `crates/perl-parser/src/Foo.pm`) to a `root`-relative path matching the
/// `file_id`s [`emit_files_and_owners`] emits (e.g. `src/Foo.pm` when `root` is
/// `crates/perl-parser`). A `.` / empty root, or a path not under `root`, is
/// returned unchanged (an outside-root path stays unmatched → `diff-file-not-found`).
fn strip_root_prefix<'a>(path: &'a str, root: &str) -> &'a str {
    if root == "." || root.is_empty() {
        return path;
    }
    path.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')).unwrap_or(path)
}

/// Emit `changes[]` + `limitations[]` from a **caller-supplied** unified diff
/// (`RiprFactsRequest.diff`), resolving each contiguous added-line hunk's owner
/// by smallest-enclosing-line-range containment against `owners` (as emitted by
/// [`emit_files_and_owners`]). `files` supplies the set of parsed file ids so a
/// hunk touching a path outside `root` is surfaced as a limitation, not silently
/// dropped. `root` normalizes the diff's repo-root-relative paths to the
/// `root`-relative `file_id`s the packet uses (#3293 PR 5).
///
/// This is pure text processing — no filesystem access, no subprocess, no git.
/// Referential integrity: a `change` is emitted only when its file is a known
/// `file_id` **and** the hunk lands inside a real `owners[]` fact; otherwise a
/// limitation records the gap. Of the schema's nine `behavior_hint` values, only
/// the three syntactically-detectable ones (`predicate_boundary`,
/// `return_value`, `exception_path`) are inferred; everything else is
/// `"unknown"` and `missing_discriminator` is always `null` in this slice.
pub(crate) fn emit_changes_from_diff(
    diff_text: &str,
    root: &str,
    files: &[Value],
    owners: &[Value],
) -> (Vec<Value>, Vec<Value>) {
    let mut changes = Vec::new();
    let mut limitations = Vec::new();

    // base/head/diff are caller-asserted; this crate never runs git to confirm
    // the supplied diff is the actual base→head diff. Always surface that.
    limitations.push(json!({
        "limitation_id": "diff-provenance-unverified",
        "kind": "unverified_provenance",
        "message": "base/head/diff are caller-asserted and not verified against a repository; this packet does not confirm the supplied diff is the actual base->head diff.",
        "evidence_refs": [],
    }));

    let known_files: std::collections::HashSet<&str> =
        files.iter().filter_map(|file| file["file_id"].as_str()).collect();

    for hunk in parse_diff_hunks(diff_text) {
        // git diff paths are repo-root-relative; file_ids are root-relative.
        let rel_path = strip_root_prefix(&hunk.file_path, root);
        let file_id = format!("file:{rel_path}");

        if !known_files.contains(file_id.as_str()) {
            limitations.push(json!({
                "limitation_id": format!("diff-file-not-found:{}", hunk.file_path),
                "kind": "unresolved_diff_path",
                "message": format!(
                    "diff hunk touches `{}`, which was not parsed under the packet root; no change fact emitted",
                    hunk.file_path
                ),
                "evidence_refs": [file_id],
            }));
            continue;
        }

        let Some(owner) = find_enclosing_owner(owners, &file_id, hunk.start_line, hunk.end_line)
        else {
            limitations.push(json!({
                "limitation_id": format!("unattributable-change:{file_id}:{}", hunk.start_line),
                "kind": "unattributable_change",
                "message": format!(
                    "diff hunk at lines {}-{} of `{}` is not inside any package/sub/method owner (file- or script-level code); no change fact emitted",
                    hunk.start_line, hunk.end_line, hunk.file_path
                ),
                "evidence_refs": [file_id],
            }));
            continue;
        };

        let (behavior_hint, discriminator) = behavior_hint_for_hunk(&hunk.lines);
        let end_column = hunk.lines.last().map_or(0, |last| last.chars().count()) as u32;
        let changed_observable =
            if behavior_hint == "unknown" { Value::Null } else { Value::String(discriminator) };

        let change_id = format!("change:{file_id}:{}:{}", hunk.start_line, hunk.end_line);
        let owner_id = owner["owner_id"].clone();
        changes.push(json!({
            "change_id": change_id.clone(),
            "file_id": file_id.clone(),
            "owner_id": owner_id.clone(),
            "range": {
                "start_line": hunk.start_line,
                "start_column": 0,
                "end_line": hunk.end_line,
                "end_column": end_column,
            },
            "behavior_hint": behavior_hint,
            "changed_text_digest": content_hash_to_digest(fnv1a_hash(&hunk.lines.join("\n"))),
            "changed_observable": changed_observable,
            "missing_discriminator": Value::Null,
            "provenance_refs": [],
        }));
        for (pattern, boundary_kind) in dynamic_boundaries_in_lines(&hunk.lines) {
            limitations.push(json!({
                "limitation_id": format!("diff-dynamic-boundary:{change_id}:{boundary_kind}"),
                "kind": boundary_kind,
                "message": format!(
                    "diff-added dynamic boundary `{pattern}` detected in `{}`; ripr fails closed on this boundary kind.",
                    hunk.file_path
                ),
                "evidence_refs": [change_id, owner_id, file_id],
            }));
        }
    }

    // Packet-level honesty notes — once each, only when changes were emitted.
    if !changes.is_empty() {
        limitations.push(json!({
            "limitation_id": "change-range-imprecise",
            "kind": "range_precision",
            "message": "change ranges are line-granular with best-effort column data derived from the diff, not the byte-accurate LineIndex ranges used for owners.",
            "evidence_refs": [],
        }));
        limitations.push(json!({
            "limitation_id": "change-behavior-hint-partial",
            "kind": "partial_inference",
            "message": "only predicate_boundary / return_value / exception_path behavior_hints are inferred from added-line text; every other change resolves to \"unknown\", and missing_discriminator is always null in this slice. Whole-line comments are skipped, but a die/return/comparison token inside a string literal can still be misclassified (a robust fix needs tokenization).",
            "evidence_refs": [],
        }));
    }

    (changes, limitations)
}

/// Infer behavior kind + concrete discriminator from a changed Perl line.
///
/// Conservative: only the three alpha-supported classes produce concrete
/// discriminators. Everything else is "unknown" with an empty discriminator
/// (ripr's strict-actionability fails closed on unknown).
fn infer_behavior_and_discriminator(line: &str) -> (&'static str, String) {
    let trimmed = line.trim();

    // Predicate boundary: a LEADING conditional (if/unless/while/elsif at the
    // start of the line, after trim) with a comparison operator. A trailing
    // modifier-if is NOT a predicate boundary — `return $x if $y > 5;` is a
    // return_value and `die "x" if $y > 5;` is an exception_path, so those must
    // fall through to the branches below rather than match on the mere presence
    // of `if `.
    if (trimmed.starts_with("if ")
        || trimmed.starts_with("unless ")
        || trimmed.starts_with("while ")
        || trimmed.starts_with("elsif ")
        || trimmed.starts_with("} elsif "))
        && (trimmed.contains("==")
            || trimmed.contains("!=")
            || trimmed.contains(">=")
            || trimmed.contains("<=")
            || trimmed.contains(">")
            || trimmed.contains("<"))
    {
        // Extract the condition text as the discriminator.
        let disc = extract_condition(trimmed).unwrap_or_else(|| trimmed.to_string());
        return ("predicate_boundary", disc);
    }

    // Return value.
    if trimmed.starts_with("return") || trimmed.contains("return ") {
        let expr = trimmed
            .strip_prefix("return")
            .unwrap_or(trimmed)
            .trim()
            .trim_end_matches(';')
            .to_string();
        return ("return_value", expr);
    }

    // Exception path.
    if trimmed.contains("die ") || trimmed.contains("croak ") || trimmed.contains("confess ") {
        let msg = extract_die_message(trimmed).unwrap_or_else(|| "exception".to_string());
        return ("exception_path", msg);
    }

    ("unknown", String::new())
}

/// Extract the condition expression from a leading if/unless/while/elsif line.
fn extract_condition(line: &str) -> Option<String> {
    let after_kw = line
        .strip_prefix("if ")
        .or_else(|| line.strip_prefix("unless "))
        .or_else(|| line.strip_prefix("while "))
        .or_else(|| line.strip_prefix("elsif "))
        .or_else(|| line.strip_prefix("} elsif "))?;
    let cond = after_kw.trim_end_matches('{').trim().trim_end_matches('{').trim();
    Some(cond.to_string())
}

/// Extract the message from a die/croak/confess call.
fn extract_die_message(line: &str) -> Option<String> {
    for kw in &["die ", "croak ", "confess "] {
        if let Some(idx) = line.find(kw) {
            let rest = &line[idx + kw.len()..];
            let msg = rest
                .trim_start_matches('"')
                .trim_start_matches("'")
                .trim_end_matches(';')
                .trim_end_matches('"')
                .trim_end_matches("'");
            return Some(msg.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_predicate_boundary_from_if_condition() {
        let (kind, disc) = infer_behavior_and_discriminator("    if ($amount >= $threshold) {");
        assert_eq!(kind, "predicate_boundary");
        assert!(disc.contains(">="), "discriminator must contain the comparison: {disc}");
    }

    #[test]
    fn infer_return_value_from_return() {
        let (kind, disc) = infer_behavior_and_discriminator("    return $discounted;");
        assert_eq!(kind, "return_value");
        assert_eq!(disc, "$discounted");
    }

    #[test]
    fn infer_exception_from_die() {
        let (kind, disc) = infer_behavior_and_discriminator("    die \"Invalid amount: $amount\";");
        assert_eq!(kind, "exception_path");
        assert!(
            disc.contains("Invalid amount"),
            "discriminator must contain the die message: {disc}"
        );
    }

    #[test]
    fn infer_unknown_for_assignment() {
        let (kind, disc) = infer_behavior_and_discriminator("    my $x = 1;");
        assert_eq!(kind, "unknown");
        assert!(disc.is_empty(), "unknown must have empty discriminator");
    }

    #[test]
    fn infer_modifier_if_is_not_predicate_boundary() {
        // A trailing modifier-if/unless must NOT hijack the classification — the
        // statement's own category wins (droid P1).
        assert_eq!(infer_behavior_and_discriminator("    return $x if $y > 5;").0, "return_value");
        assert_eq!(
            infer_behavior_and_discriminator("    die \"bad\" if $y > 5;").0,
            "exception_path"
        );
        assert_eq!(
            infer_behavior_and_discriminator("    croak \"x\" unless $y >= 3;").0,
            "exception_path"
        );
        // A LEADING conditional is still a predicate boundary.
        assert_eq!(infer_behavior_and_discriminator("    if ($x > 5) {").0, "predicate_boundary");
        assert_eq!(
            infer_behavior_and_discriminator("    while ($i < 10) {").0,
            "predicate_boundary"
        );
        let (kind, disc) = infer_behavior_and_discriminator("    } elsif ($y < 3) {");
        assert_eq!(kind, "predicate_boundary");
        assert!(disc.contains("$y < 3"), "elsif condition extracted cleanly: {disc}");
    }

    /// A `lib/My/App.pm` file fact + a `sub discount` owner spanning 0-based
    /// lines 4..8, for the diff-change tests below.
    fn app_files_and_owners() -> (Vec<Value>, Vec<Value>) {
        let files = vec![json!({ "file_id": "file:lib/My/App.pm" })];
        let owners = vec![
            json!({
                "owner_id": "owner:lib/My/App.pm:package:main::App:0-200",
                "file_id": "file:lib/My/App.pm",
                "kind": "package",
                "range": {"start_line": 0, "start_column": 0, "end_line": 20, "end_column": 1},
            }),
            json!({
                "owner_id": "owner:lib/My/App.pm:sub:main::discount:60-140",
                "file_id": "file:lib/My/App.pm",
                "kind": "sub",
                "range": {"start_line": 4, "start_column": 0, "end_line": 8, "end_column": 1},
            }),
        ];
        (files, owners)
    }

    #[test]
    fn emit_changes_from_diff_emits_change_for_hunk_inside_a_sub() {
        let (files, owners) = app_files_and_owners();
        // New start line 6 (1-based) → 0-based 5; the added line lands at line 5,
        // inside the sub's 4..8 range.
        let diff = "\
--- a/lib/My/App.pm
+++ b/lib/My/App.pm
@@ -5,3 +5,4 @@
 sub discount {
     my ($amount) = @_;
+    return $amount / 2;
 }
";
        let (changes, _limitations) = emit_changes_from_diff(diff, ".", &files, &owners);
        assert_eq!(changes.len(), 1, "one added line inside a sub → one change");
        assert_eq!(changes[0]["owner_id"], "owner:lib/My/App.pm:sub:main::discount:60-140");
        assert_eq!(changes[0]["behavior_hint"], "return_value");
        assert_eq!(changes[0]["file_id"], "file:lib/My/App.pm");
        // Schema-contract parity: both nullable observation keys are present.
        assert!(changes[0].get("changed_observable").is_some());
        assert!(changes[0].get("missing_discriminator").is_some());
        assert!(
            changes[0]["changed_observable"].as_str().unwrap_or("").contains("$amount"),
            "changed_observable carries the return expression"
        );
    }

    #[test]
    fn emit_changes_from_diff_dynamic_dispatch_records_scoped_limitation() {
        let (files, owners) = app_files_and_owners();
        let diff = "\
--- a/lib/My/App.pm
+++ b/lib/My/App.pm
@@ -5,3 +5,5 @@
 sub discount {
     my ($amount) = @_;
+    my $method = 'discount';
+    return shift->$method();
 }
";
        let (changes, limitations) = emit_changes_from_diff(diff, ".", &files, &owners);
        assert_eq!(changes.len(), 1, "dynamic diff still emits the changed owner fact");
        let change_id = changes[0]["change_id"].as_str().expect("change_id");
        let owner_id = changes[0]["owner_id"].as_str().expect("owner_id");
        let limitation = limitations
            .iter()
            .find(|l| l["kind"] == "dynamic_dispatch")
            .expect("diff-added dynamic dispatch should record a blocking limitation");
        let refs = limitation["evidence_refs"].as_array().expect("evidence_refs");
        assert!(
            refs.iter().any(|r| r.as_str() == Some(change_id))
                && refs.iter().any(|r| r.as_str() == Some(owner_id)),
            "dynamic limitation should be scoped to the emitted change and owner"
        );
    }

    #[test]
    fn emit_changes_from_diff_records_each_dynamic_boundary_kind_in_hunk() {
        let (files, owners) = app_files_and_owners();
        let diff = "\
--- a/lib/My/App.pm
+++ b/lib/My/App.pm
@@ -5,3 +5,7 @@
 sub discount {
     my ($amount) = @_;
+    eval { $amount };
+    our @ISA = ('Base');
+    return shift->$method();
 }
";
        let (_changes, limitations) = emit_changes_from_diff(diff, ".", &files, &owners);
        let kinds: std::collections::HashSet<&str> =
            limitations.iter().filter_map(|limitation| limitation["kind"].as_str()).collect();

        assert!(kinds.contains("eval_or_string_code"), "eval boundary missing: {limitations:?}");
        assert!(kinds.contains("role_composition"), "role boundary missing: {limitations:?}");
        assert!(kinds.contains("dynamic_dispatch"), "dispatch boundary missing: {limitations:?}");
    }

    #[test]
    fn emit_changes_from_diff_hunk_at_file_scope_produces_no_change_but_a_limitation() {
        // File is known, but the added line is above every owner's range.
        let files = vec![json!({ "file_id": "file:lib/My/App.pm" })];
        let owners = vec![json!({
            "owner_id": "owner:lib/My/App.pm:sub:main::foo:60-140",
            "file_id": "file:lib/My/App.pm",
            "kind": "sub",
            "range": {"start_line": 10, "start_column": 0, "end_line": 14, "end_column": 1},
        })];
        let diff = "\
+++ b/lib/My/App.pm
@@ -1,0 +1,1 @@
+use strict;
";
        let (changes, limitations) = emit_changes_from_diff(diff, ".", &files, &owners);
        assert!(changes.is_empty(), "file-scope hunk (no enclosing owner) → no change fact");
        assert!(
            limitations.iter().any(|l| l["limitation_id"]
                .as_str()
                .is_some_and(|s| s.starts_with("unattributable-change:"))),
            "must record an unattributable-change limitation"
        );
    }

    #[test]
    fn emit_changes_from_diff_unknown_file_records_diff_file_not_found() {
        // The diff touches a file the packet never parsed (outside root) — no
        // change, but a diff-file-not-found limitation instead of a silent drop.
        let files = vec![json!({ "file_id": "file:lib/My/App.pm" })];
        let owners: Vec<Value> = Vec::new();
        let diff = "\
+++ b/other/Thing.pm
@@ -1,0 +1,1 @@
+return 1;
";
        let (changes, limitations) = emit_changes_from_diff(diff, ".", &files, &owners);
        assert!(changes.is_empty(), "hunk in an unknown file → no change");
        assert!(
            limitations.iter().any(|l| l["limitation_id"]
                .as_str()
                .is_some_and(|s| s.starts_with("diff-file-not-found:"))),
            "must record a diff-file-not-found limitation"
        );
    }

    #[test]
    fn emit_changes_from_diff_is_deterministic_and_stable_across_reordering() {
        let (files, owners) = app_files_and_owners();
        let hunk_a = "@@ -5,2 +5,3 @@\n sub discount {\n+    return 1;\n";
        let hunk_b = "@@ -6,2 +6,3 @@\n     my $x = 1;\n+    return 2;\n";
        let header = "+++ b/lib/My/App.pm\n";
        let ab = format!("{header}{hunk_a}{hunk_b}");
        let ba = format!("{header}{hunk_b}{hunk_a}");
        let (changes_ab, _) = emit_changes_from_diff(&ab, ".", &files, &owners);
        let (changes_ab2, _) = emit_changes_from_diff(&ab, ".", &files, &owners);
        let (changes_ba, _) = emit_changes_from_diff(&ba, ".", &files, &owners);
        // Same input → byte-identical output.
        assert_eq!(changes_ab, changes_ab2, "same diff → identical changes");
        // change_id is derived from (file_id, start_line, end_line), so each
        // hunk's id is stable no matter the order the hunks appear in the diff.
        let ids = |cs: &[Value]| {
            let mut v: Vec<String> =
                cs.iter().filter_map(|c| c["change_id"].as_str().map(str::to_owned)).collect();
            v.sort();
            v
        };
        assert_eq!(ids(&changes_ab), ids(&changes_ba), "change_ids stable across reordering");
    }

    #[test]
    fn find_enclosing_owner_picks_smallest_of_nested_package_and_sub() {
        let (_files, owners) = app_files_and_owners();
        // Line 5 is inside both the package (0..20) and the sub (4..8) → sub wins.
        let owner = find_enclosing_owner(&owners, "file:lib/My/App.pm", 5, 5).expect("an owner");
        assert_eq!(owner["kind"], "sub", "smallest enclosing owner is the sub, not the package");
    }

    #[test]
    fn find_enclosing_owner_returns_none_when_no_owner_contains_the_range() {
        let (_files, owners) = app_files_and_owners();
        assert!(
            find_enclosing_owner(&owners, "file:lib/My/App.pm", 50, 50).is_none(),
            "a range outside every owner yields None"
        );
    }

    #[test]
    fn behavior_hint_for_hunk_first_matching_line_wins() {
        assert_eq!(behavior_hint_for_hunk(&["    my $x = 1;".into()]).0, "unknown");
        assert_eq!(behavior_hint_for_hunk(&["    return $x + 1;".into()]).0, "return_value");
        assert_eq!(behavior_hint_for_hunk(&["    if ($x >= 10) {".into()]).0, "predicate_boundary");
        assert_eq!(behavior_hint_for_hunk(&["    die \"bad\";".into()]).0, "exception_path");
        // First recognized line wins over a later one.
        let lines = vec!["    my $x = 1;".into(), "    return $x;".into(), "    die \"z\";".into()];
        assert_eq!(behavior_hint_for_hunk(&lines).0, "return_value");
    }

    #[test]
    fn behavior_hint_for_hunk_skips_comment_lines() {
        // A whole-line comment is not executable → never a concrete hint.
        assert_eq!(behavior_hint_for_hunk(&["# return $x;".into()]).0, "unknown");
        assert_eq!(behavior_hint_for_hunk(&["    # die now".into()]).0, "unknown");
        // Real code after a comment still wins.
        let lines = vec!["# comment".into(), "    return $x;".into()];
        assert_eq!(behavior_hint_for_hunk(&lines).0, "return_value");
    }

    #[test]
    fn strip_root_prefix_normalizes_subdir_paths() {
        assert_eq!(strip_root_prefix("crates/p/lib/A.pm", "crates/p"), "lib/A.pm");
        assert_eq!(strip_root_prefix("lib/A.pm", "."), "lib/A.pm");
        // A path not under root is left unchanged (→ diff-file-not-found).
        assert_eq!(strip_root_prefix("other/A.pm", "crates/p"), "other/A.pm");
    }

    #[test]
    fn emit_changes_from_diff_normalizes_git_paths_against_subdir_root() {
        // git diff paths are repo-root-relative; file_ids are root-relative. A
        // subdir root must not make every hunk diff-file-not-found.
        let files = vec![json!({ "file_id": "file:lib/App.pm" })];
        let owners = vec![json!({
            "owner_id": "owner:lib/App.pm:sub:main::f:0-40",
            "file_id": "file:lib/App.pm",
            "kind": "sub",
            "range": {"start_line": 0, "start_column": 0, "end_line": 10, "end_column": 1},
        })];
        let diff =
            "+++ b/crates/perl-parser/lib/App.pm\n@@ -1,1 +1,2 @@\n sub f {\n+    return 1;\n";
        let (changes, _l) = emit_changes_from_diff(diff, "crates/perl-parser", &files, &owners);
        assert_eq!(changes.len(), 1, "subdir-root git path must normalize and match the file_id");
        assert_eq!(changes[0]["file_id"], "file:lib/App.pm");
    }

    #[test]
    fn emit_changes_from_diff_digest_uses_fnv64_prefix_not_sha256() {
        let (files, owners) = app_files_and_owners();
        let diff = "+++ b/lib/My/App.pm\n@@ -5,2 +5,3 @@\n sub discount {\n+    return 1;\n";
        let (changes, _) = emit_changes_from_diff(diff, ".", &files, &owners);
        let digest = changes[0]["changed_text_digest"].as_str().expect("digest string");
        assert!(
            digest.starts_with("fnv64:"),
            "digest must use the real fnv64: prefix, not sha256:"
        );
        assert_eq!(digest, content_hash_to_digest(fnv1a_hash("    return 1;")));
    }

    #[test]
    fn emit_changes_from_diff_missing_discriminator_is_always_null() {
        let (files, owners) = app_files_and_owners();
        let diff = "+++ b/lib/My/App.pm\n@@ -5,2 +5,3 @@\n sub discount {\n+    return 1;\n";
        let (changes, _) = emit_changes_from_diff(diff, ".", &files, &owners);
        assert!(
            changes.iter().all(|c| c["missing_discriminator"].is_null()),
            "missing_discriminator is always null in this slice"
        );
    }

    #[test]
    fn parse_diff_hunks_ignores_no_newline_marker() {
        // The `\ No newline at end of file` marker is metadata about the
        // preceding line; it must not advance the head-file cursor.
        let with_marker =
            "+++ b/f.pm\n@@ -5,1 +5,2 @@\n-old\n\\ No newline at end of file\n+new1\n+new2\n";
        let hunks = parse_diff_hunks(with_marker);
        assert_eq!(hunks.len(), 1, "one added-line run");
        // `+5` → 0-based 4; new1/new2 land at head lines 4 and 5, unshifted.
        assert_eq!(hunks[0].start_line, 4, "marker must not shift the head cursor");
        assert_eq!(hunks[0].end_line, 5);
    }

    #[test]
    fn emit_changes_from_diff_no_newline_marker_does_not_misattribute() {
        // A tight owner (lines 4..5). Without the marker fix, the added line would
        // shift to 6 and fall outside the owner → wrong result.
        let files = vec![json!({ "file_id": "file:lib/My/App.pm" })];
        let owners = vec![json!({
            "owner_id": "owner:lib/My/App.pm:sub:main::tiny:40-70",
            "file_id": "file:lib/My/App.pm",
            "kind": "sub",
            "range": {"start_line": 4, "start_column": 0, "end_line": 5, "end_column": 1},
        })];
        let diff = "+++ b/lib/My/App.pm\n@@ -5,1 +5,2 @@\n-old\n\\ No newline at end of file\n+    return 1;\n";
        let (changes, _limitations) = emit_changes_from_diff(diff, ".", &files, &owners);
        assert_eq!(changes.len(), 1, "the added line stays inside the owner");
        assert_eq!(changes[0]["owner_id"], "owner:lib/My/App.pm:sub:main::tiny:40-70");
    }

    #[test]
    fn emit_changes_empty_when_diff_has_no_added_lines() {
        // A pure-deletion diff (no `+` content) yields no hunks → no changes.
        let files = vec![json!({ "file_id": "file:lib/My/App.pm" })];
        let owners: Vec<Value> = Vec::new();
        let diff = "+++ b/lib/My/App.pm\n@@ -5,2 +5,1 @@\n sub discount {\n-    return $x;\n";
        let (changes, _limitations) = emit_changes_from_diff(diff, ".", &files, &owners);
        assert!(changes.is_empty(), "a deletion-only hunk produces no change facts");
    }
}
