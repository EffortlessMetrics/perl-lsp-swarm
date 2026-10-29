//! A2 guidance-actionability bench (#17154).
//!
//! Agent guidance in the packet must be machine-actionable, verified over a
//! produced packet (source fixture + tests + one boundary + one diff change):
//!
//! - every `verify_commands[]` entry must argv-parse: `argv` is a non-empty
//!   string vector (never a shell string), `argv[0]` names a concrete runner,
//!   no element smuggles shell metacharacters, `test_id` resolves to a real
//!   test, and path arguments exist on disk under the packet root;
//! - every annotation anchor must resolve to real lines: each `range` in
//!   `owners[]`/`tests[]`/`oracles[]`/`dynamic_boundaries[]`/`changes[]` (and
//!   each string `range` on a file-scoped `provenance[]` entry) must satisfy
//!   `0 <= start_line <= end_line <= newline_count` of its referenced file.
//!
//! Thresholds (A2): verify-command argv-parse rate >= 0.95, anchor resolution
//! 100% (every anchor resolves; a dangling anchor misdirects the agent).

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598); see src/lib.rs.

use std::collections::{HashMap, HashSet};

use perl_ripr_facts::{RiprFactsRequest, build_ripr_facts_packet};
use perl_tdd_support::{must, must_some, must_some_with, must_with};

const ALL_FACT_CLASSES: &str = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,verify_commands,limitations,provenance";

/// A2 bars from #17154: verify-command argv-parse >= 95%, anchors 100%.
const MIN_ARGV_PARSE_RATE: f64 = 0.95;

const APP_PM: &str = "package App;\nuse strict;\nuse warnings;\n\nsub discount {\n    my ($amount) = @_;\n    if ($amount > 100) {\n        return $amount * 0.9;\n    }\n    return $amount;\n}\n\nsub risky {\n    eval { die \"boom\" };\n    return 1;\n}\n\n1;\n";

const OTHER_PM: &str = "package Other;\n\nsub other_sub {\n    return 1;\n}\n\n1;\n";

const APP_T: &str = "use strict;\nuse warnings;\nuse Test::More;\n\nuse App;\n\nis(App::discount(50), 50, 'no discount under threshold');\nok(App::risky(), 'risky returns true');\n\ndone_testing;\n";

const OTHER_T: &str = "use strict;\nuse warnings;\nuse Test::More;\n\nuse Other;\n\nok(other_sub(), 'bare call after use');\n\ndone_testing;\n";

/// One hunk inside `sub discount` (lines 8-8 of `lib/App.pm`) so `changes[]`
/// anchors are exercised too.
const DIFF: &str = "+++ b/lib/App.pm\n@@ -8,1 +9,2 @@\n         return $amount * 0.9;\n+    if ($amount >= 500) {\n";

/// Characters that must never appear in an argv element: their presence means
/// the "parsed argv vector" would change meaning under a shell join, i.e. it
/// is not safely executable guidance.
const SHELL_METACHARS: &[char] = &[';', '|', '&', '`', '$', '<', '>', '\n', '\r', '\0'];

/// Concrete runners an agent can exec (schema also permits `unknown`, which is
/// honest but not actionable guidance).
const ACTIONABLE_RUNNERS: &[&str] = &["prove", "yath", "carton", "dzil"];

fn check_verify_command(
    entry: &serde_json::Value,
    test_ids: &HashSet<String>,
    root: &str,
) -> Vec<String> {
    let mut failures = Vec::new();
    let command_id = entry["command_id"].as_str().unwrap_or("<missing command_id>");
    let argv = must_some(entry["argv"].as_array());
    if argv.is_empty() {
        failures.push(format!("{command_id}: argv must be non-empty"));
        return failures;
    }
    let argv_strs: Vec<&str> = argv.iter().filter_map(|a| a.as_str()).collect();
    if argv_strs.len() != argv.len() {
        failures.push(format!("{command_id}: every argv element must be a string"));
    }
    let runner = entry["runner"].as_str().unwrap_or("<missing runner>");
    if !ACTIONABLE_RUNNERS.contains(&runner) {
        failures.push(format!("{command_id}: runner `{runner}` is not an actionable runner"));
    }
    if argv_strs.first() != Some(&runner) {
        failures.push(format!(
            "{command_id}: argv[0] must name the runner `{runner}`, got {:?}",
            argv_strs.first()
        ));
    }
    for arg in &argv_strs {
        if arg.trim().is_empty() {
            failures.push(format!("{command_id}: argv elements must be non-blank"));
        }
        if let Some(metachar) = arg.chars().find(|c| SHELL_METACHARS.contains(c)) {
            failures.push(format!(
                "{command_id}: argv element `{arg}` smuggles shell metacharacter `{metachar}`"
            ));
        }
    }
    if let Some(test_id) = entry["test_id"].as_str()
        && !test_ids.contains(test_id)
    {
        failures.push(format!("{command_id}: test_id `{test_id}` resolves to no tests[] entry"));
    }
    // Path arguments (relative test/source paths) must exist under the root.
    for arg in argv_strs.iter().skip(1) {
        let looks_like_path = arg.contains('/')
            || arg.ends_with(".t")
            || arg.ends_with(".pm")
            || arg.ends_with(".pl");
        if looks_like_path && !std::path::Path::new(root).join(arg).is_file() {
            failures
                .push(format!("{command_id}: referenced file `{arg}` does not exist under root"));
        }
    }
    failures
}

/// Check one schema `range` object against its file's newline count; pushes a
/// failure description on any violation. Returns whether it resolved.
fn check_range(
    range: &serde_json::Value,
    newlines: usize,
    what: &str,
    failures: &mut Vec<String>,
) -> bool {
    let start_line = range["start_line"].as_u64();
    let end_line = range["end_line"].as_u64();
    let (Some(start), Some(end)) = (start_line, end_line) else {
        failures.push(format!("{what}: range lacks integer start_line/end_line: {range}"));
        return false;
    };
    if range["start_column"].as_u64().is_none() || range["end_column"].as_u64().is_none() {
        failures.push(format!("{what}: range lacks integer columns: {range}"));
        return false;
    }
    #[allow(clippy::cast_possible_truncation)]
    let start_usize = start as usize;
    #[allow(clippy::cast_possible_truncation)]
    let end_usize = end as usize;
    if start_usize > end_usize {
        failures.push(format!("{what}: start_line {start} > end_line {end}"));
        return false;
    }
    // 0-based lines: the last addressable line is the count of `\n` (EOF sits
    // on it when the file ends with a newline, else the final partial line).
    if end_usize > newlines {
        failures.push(format!("{what}: end_line {end} exceeds file line bound {newlines}"));
        return false;
    }
    true
}

/// Parse a provenance `range` string (`sl:sc-el:ec`) into line bounds.
fn parse_provenance_range(text: &str) -> Option<(u64, u64)> {
    let (start, end) = text.split_once('-')?;
    let (sl, _) = start.split_once(':')?;
    let (el, _) = end.split_once(':')?;
    Some((sl.parse().ok()?, el.parse().ok()?))
}

#[test]
fn packet_guidance_is_actionable() {
    let root = "target/ripr-a2/guidance";
    let _ = std::fs::remove_dir_all(root);
    for (rel, content) in [
        ("lib/App.pm", APP_PM),
        ("lib/Other.pm", OTHER_PM),
        ("t/app.t", APP_T),
        ("t/other.t", OTHER_T),
    ] {
        let path = format!("{root}/{rel}");
        if let Some(parent) = std::path::Path::new(&path).parent() {
            must_with(std::fs::create_dir_all(parent), format!("mkdir for {rel}"));
        }
        must_with(std::fs::write(&path, content), format!("stage A2 file {rel}"));
    }
    let packet = must(build_ripr_facts_packet(&RiprFactsRequest {
        schema: "ripr-perl-facts-v1",
        root,
        base: Some("origin/main"),
        head: Some("HEAD"),
        fact_classes: ALL_FACT_CLASSES,
        diff: Some(DIFF),
    }));

    // file_id -> newline count, read back from the staged files on disk.
    let mut newlines_by_file: HashMap<String, usize> = HashMap::new();
    for file in must_some(packet["files"].as_array()) {
        let file_id = must_some_with(file["file_id"].as_str(), "file fact carries file_id");
        let path = must_some_with(file["path"].as_str(), "file fact carries path");
        let content = must_with(
            std::fs::read_to_string(format!("{root}/{path}")),
            format!("packet file {path} exists on disk under root"),
        );
        newlines_by_file.insert(file_id.to_string(), content.matches('\n').count());
    }

    let test_ids: HashSet<String> = must_some(packet["tests"].as_array())
        .iter()
        .filter_map(|t| t["test_id"].as_str().map(String::from))
        .collect();
    // Oracles anchor via `test_id` (the schema gives them no `file_id`), so
    // resolve test -> file for anchor checking.
    let file_by_test: HashMap<String, String> = must_some(packet["tests"].as_array())
        .iter()
        .filter_map(|t| {
            Some((t["test_id"].as_str()?.to_string(), t["file_id"].as_str()?.to_string()))
        })
        .collect();

    // Part 1: verify-command argv-parse rate.
    let commands = must_some(packet["verify_commands"].as_array());
    assert!(!commands.is_empty(), "A2 needs at least one verify command to judge");
    let mut argv_failures = Vec::new();
    let mut argv_passed = 0usize;
    for entry in commands {
        let failures = check_verify_command(entry, &test_ids, root);
        if failures.is_empty() {
            argv_passed += 1;
        } else {
            argv_failures.extend(failures);
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let argv_rate = argv_passed as f64 / commands.len() as f64;
    assert!(
        argv_rate >= MIN_ARGV_PARSE_RATE,
        "A2 argv-parse rate {argv_rate:.2} ({argv_passed}/{}) below bar {MIN_ARGV_PARSE_RATE}: {argv_failures:?}",
        commands.len()
    );

    // Part 2: every annotation anchor resolves to real lines (100%).
    let mut anchor_failures = Vec::new();
    let mut anchors_total = 0usize;
    let mut anchors_resolved = 0usize;
    let mut check_fact_range = |fact: &serde_json::Value, id_key: &str, array: &str| {
        let id = fact[id_key].as_str().unwrap_or("<missing id>");
        let range = &fact["range"];
        if range.is_null() {
            return; // Optional on some facts (e.g. boundaries); nothing to resolve.
        }
        anchors_total += 1;
        let file_id = fact["file_id"].as_str().map(String::from).or_else(|| {
            fact["test_id"].as_str().and_then(|test_id| file_by_test.get(test_id).cloned())
        });
        let Some(file_id) = file_id else {
            anchor_failures.push(format!("{array} {id}: no file_id and no resolvable test_id"));
            return;
        };
        let Some(newlines) = newlines_by_file.get(&file_id) else {
            anchor_failures
                .push(format!("{array} {id}: file_id `{file_id}` resolves to no files[] entry"));
            return;
        };
        if check_range(range, *newlines, &format!("{array} {id}"), &mut anchor_failures) {
            anchors_resolved += 1;
        }
    };
    for (array, id_key) in [
        ("owners", "owner_id"),
        ("tests", "test_id"),
        ("oracles", "oracle_id"),
        ("dynamic_boundaries", "boundary_id"),
        ("changes", "change_id"),
    ] {
        for fact in must_some(packet[array].as_array()) {
            check_fact_range(fact, id_key, array);
        }
    }
    // Provenance string anchors (`sl:sc-el:ec`) on file-scoped entries.
    for prov in must_some(packet["provenance"].as_array()) {
        let (Some(range_text), Some(file_id)) = (prov["range"].as_str(), prov["file_id"].as_str())
        else {
            continue; // Whole-packet provenance (e.g. cli-surface) carries no anchor.
        };
        anchors_total += 1;
        let prov_id = prov["provenance_id"].as_str().unwrap_or("<missing provenance_id>");
        let (Some((start, end)), Some(newlines)) =
            (parse_provenance_range(range_text), newlines_by_file.get(file_id))
        else {
            anchor_failures.push(format!(
                "provenance {prov_id}: range `{range_text}` unparsable or file_id `{file_id}` unresolvable"
            ));
            continue;
        };
        #[allow(clippy::cast_possible_truncation)]
        let (start_usize, end_usize) = (start as usize, end as usize);
        if start_usize <= end_usize && end_usize <= *newlines {
            anchors_resolved += 1;
        } else {
            anchor_failures.push(format!(
                "provenance {prov_id}: range `{range_text}` outside file line bound {newlines}"
            ));
        }
    }

    let _ = std::fs::remove_dir_all(root);
    assert!(anchors_total > 0, "A2 needs at least one anchor to judge");
    assert_eq!(
        anchors_resolved, anchors_total,
        "A2 anchor resolution must be 100%; failures: {anchor_failures:?}"
    );
}
