//! Dynamic-boundary detection + typed verify-command candidates (Campaign 31
//! Phase B PR 8, perl-lsp-swarm#2595).
//!
//! `emit_boundaries_and_commands` emits both `dynamic_boundaries[]` and
//! `verify_commands[]` in one file walk — the issue's target shape suggested
//! a separate `verify.rs`, but the real implementation shares one scan of
//! `.pm`/`.t` files across both outputs, so splitting them would mean
//! restructuring the walk (a rewrite), not moving code. Kept fused here; see
//! the #9271 PR notes for that deviation.

use perl_parser_core::Parser;
use perl_parser_core::line_index::LineIndex;
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

pub(crate) fn dynamic_boundaries_in_lines(lines: &[String]) -> Vec<(&'static str, &'static str)> {
    let mut seen_kinds = std::collections::HashSet::new();
    let mut boundaries = Vec::new();
    for line in lines {
        for &(pattern, boundary_kind) in DYNAMIC_BOUNDARY_PATTERNS {
            if line.contains(pattern) && seen_kinds.insert(boundary_kind) {
                boundaries.push((pattern, boundary_kind));
            }
        }
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
        let file_id = format!("file:{file_path}");
        let owner_index = boundary_owner_index(file_path, content);
        let line_index = LineIndex::new(content.clone());
        for (pattern, boundary_kind) in DYNAMIC_BOUNDARY_PATTERNS {
            for (offset, _) in content.match_indices(pattern) {
                boundary_counter += 1;
                let boundary_id =
                    format!("boundary:{file_path}:{boundary_kind}:{boundary_counter}");
                let owner_id = enclosing_boundary_owner(&owner_index, offset)
                    .map(|owner| owner.owner_id.as_str());
                let ((start_line, start_column), (end_line, end_column)) =
                    line_index.range(offset, offset + pattern.len());
                let evidence_refs = boundary_evidence_refs(owner_id, &file_id);
                boundaries.push(json!({
                    "boundary_id": boundary_id,
                    "kind": boundary_kind,
                    "file_id": file_id.clone(),
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
            "package My::App;\nsub run { eval { die }; }\n1;",
        ));

        let (boundaries, limitations, _cmds) =
            emit_boundaries_and_commands(must_some(root.to_str()));
        assert!(!boundaries.is_empty(), "eval block must produce a boundary fact");
        assert!(
            boundaries.iter().any(|b| b["kind"] == "eval_or_string_code"),
            "must have an eval_or_string_code boundary"
        );
        assert!(!limitations.is_empty(), "each boundary must have a corresponding limitation");

        let _ = std::fs::remove_dir_all(&root);
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
