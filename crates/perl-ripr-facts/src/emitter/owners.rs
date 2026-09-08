//! `files[]` + `owners[]` emission (#3293 PR 3).
//!
//! `emit_files_and_owners` produces both arrays in one parse-and-walk pass —
//! the issue's target shape suggested separate `files.rs`/`owners.rs`
//! emitters, but the real implementation derives a file's `digest`/`role` and
//! its declarations' `owner` facts from the same single parse per file, so
//! splitting them would mean re-parsing each file twice (a behavior change),
//! not moving code. Kept fused here; see the #9271 PR notes for that
//! deviation. (The file-collection *walk* helpers reused by every emitter,
//! including this one, live in [`super::discovery`].)

use perl_parser_core::Parser;
use perl_parser_core::line_index::LineIndex;
use perl_symbol::SymbolKind;
use perl_symbol::surface::extract_symbol_decls;
use serde_json::{Value, json};

use super::discovery::{collect_perl_files, file_role_from_path};
use super::ids::{content_sha256_digest, owner_fact_id};

/// Emit `files[]`, `owners[]`, per-file `provenance[]`, and parse/read
/// `limitations[]` by parsing every Perl source/test file under `root`
/// (#3293 PR 3).
///
/// For each discovered `.pm` / `.pl` / `.psgi` / `.t` file this produces:
/// - one `file` fact (repo-relative path, role, SHA-256 content digest,
///   parser-derived package names);
/// - one `owner` fact per `package` / `class` / `role` / `sub` / `method`
///   declaration, carrying the parser's real source range and a byte-span-derived
///   `owner_id` (stable, never traversal-order); and
/// - one `syntax`-sourced `provenance` fact that the file and its owners
///   reference by id.
///
/// Files that cannot be read or parsed are **not** silently dropped: a read
/// failure records a limitation and emits no file fact (a digest needs the
/// content); a parse failure still emits the file fact with zero owners plus a
/// limitation.
///
/// The `range` uses the schema's flat `{start_line, start_column, end_line,
/// end_column}` shape (0-based, UTF-16 columns from `LineIndex`), and provenance
/// uses the schema's `source` enum (`"syntax"`) — the packet contract has no
/// nested-LSP range or free-form provenance `producer`/`kind` fields.
///
/// Parser-backed via `perl-parser-core` (parse + `LineIndex` byte→line/column)
/// and `perl-symbol` (`extract_symbol_decls`) — both leaf crates with no
/// forbidden dependencies. `perl-workspace` is intentionally avoided (it pulls
/// `lsp-types`).
pub(crate) fn emit_files_and_owners(
    root: &str,
) -> (Vec<Value>, Vec<Value>, Vec<Value>, Vec<Value>) {
    let mut files = Vec::new();
    let mut owners = Vec::new();
    let mut provenance = Vec::new();
    let mut limitations = Vec::new();

    for relative_path in collect_perl_files(root) {
        let file_id = format!("file:{relative_path}");
        let absolute = std::path::Path::new(root).join(&relative_path);

        let content = match std::fs::read_to_string(&absolute) {
            Ok(content) => content,
            Err(error) => {
                // Do not silently drop: a digest needs the content, so emit no
                // file fact — just a limitation recording why.
                limitations.push(json!({
                    "limitation_id": format!("read-failed:{file_id}"),
                    "kind": "read_failure",
                    "message": format!("could not read `{relative_path}`: {error}"),
                    "evidence_refs": [file_id],
                }));
                continue;
            }
        };

        let digest = content_sha256_digest(content.as_bytes());
        let role = file_role_from_path(&relative_path);
        let provenance_id = format!("prov:syntax:{file_id}");
        let mut package_names: Vec<String> = Vec::new();

        // Parse and project declarations into owner facts. Scope the parser's
        // borrow of `content` to the parse so `content` can move into
        // `LineIndex` afterwards (no per-file clone).
        let parsed = {
            let mut parser = Parser::new(&content);
            parser.parse()
        };
        match parsed {
            Ok(ast) => {
                let line_index = LineIndex::new(content);
                for decl in extract_symbol_decls(&ast, Some("main")) {
                    let Some(kind) = owner_kind(&decl.kind) else {
                        continue;
                    };
                    if matches!(
                        decl.kind,
                        SymbolKind::Package | SymbolKind::Class | SymbolKind::Role
                    ) && !package_names.contains(&decl.qualified_name)
                    {
                        package_names.push(decl.qualified_name.clone());
                    }

                    let (start_byte, end_byte) = decl.full_span;
                    let ((start_line, start_column), (end_line, end_column)) =
                        line_index.range(start_byte, end_byte);

                    // Byte-span-derived id: stable and independent of traversal
                    // order (a decl is uniquely located by its span).
                    owners.push(json!({
                        "owner_id": owner_fact_id(
                            &relative_path,
                            kind,
                            &decl.qualified_name,
                            start_byte,
                            end_byte,
                        ),
                        "file_id": file_id,
                        "kind": kind,
                        "package": decl.container,
                        "name": decl.name,
                        "range": {
                            "start_line": start_line,
                            "start_column": start_column,
                            "end_line": end_line,
                            "end_column": end_column,
                        },
                        "confidence": "high",
                        "provenance_refs": [provenance_id.clone()],
                    }));
                }
            }
            Err(_error) => {
                // Fail soft: still emit the file fact (below) with zero owners,
                // and record why parsing yielded none.
                limitations.push(json!({
                    "limitation_id": format!("parse-failed:{file_id}"),
                    "kind": "parse_failure",
                    "message": format!(
                        "could not parse `{relative_path}` as Perl; emitted the file fact with no owners"
                    ),
                    "evidence_refs": [file_id.clone()],
                }));
            }
        }

        package_names.sort();
        package_names.dedup();

        provenance.push(json!({
            "provenance_id": provenance_id.clone(),
            "source": "syntax",
            "file_id": file_id.clone(),
            "confidence": "high",
        }));

        files.push(json!({
            "file_id": file_id,
            "path": relative_path,
            "role": [role],
            "digest": digest,
            "package_names": package_names,
            "provenance_refs": [provenance_id],
        }));
    }

    (files, owners, provenance, limitations)
}

/// Map a `perl-symbol` [`SymbolKind`] to the ripr `owner.kind` vocabulary.
///
/// Only namespace and callable declarations are owners; variables, constants,
/// and imports are not. `Class` / `Role` are namespace declarations, so they map
/// to `package`.
pub(crate) fn owner_kind(kind: &SymbolKind) -> Option<&'static str> {
    match kind {
        SymbolKind::Package | SymbolKind::Class | SymbolKind::Role => Some("package"),
        SymbolKind::Subroutine => Some("sub"),
        SymbolKind::Method => Some("method"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::{must, must_some};

    #[test]
    fn emit_files_and_owners_extracts_packages_and_subs() {
        let root = std::env::temp_dir().join("perl-P3-files-owners-root");
        let lib_dir = root.join("lib/My");
        must(std::fs::create_dir_all(&lib_dir));
        must(std::fs::write(
            lib_dir.join("App.pm"),
            "package My::App;\nsub discount { return 42; }\nsub total { }\n1;\n",
        ));

        let (files, owners, provenance, limitations) =
            emit_files_and_owners(must_some(root.to_str()));

        // One file fact for the .pm — source role, a SHA-256 digest, the package name.
        assert_eq!(files.len(), 1, "one .pm file → one file fact");
        let file = &files[0];
        assert_eq!(file["path"], "lib/My/App.pm");
        assert_eq!(file["role"], json!(["source"]));
        assert!(
            must_some(file["digest"].as_str()).starts_with("sha256:"),
            "digest is a SHA-256 hex string, got {:?}",
            file["digest"]
        );
        assert_eq!(file["package_names"], json!(["My::App"]));
        assert_eq!(file["file_id"], "file:lib/My/App.pm");
        assert_eq!(file["provenance_refs"], json!(["prov:syntax:file:lib/My/App.pm"]));

        // Owners: the package + both subs, with parser-derived kinds.
        let kinds: Vec<&str> = owners.iter().filter_map(|o| o["kind"].as_str()).collect();
        assert!(kinds.contains(&"package"), "package My::App must be an owner, got {kinds:?}");
        assert_eq!(
            kinds.iter().filter(|k| **k == "sub").count(),
            2,
            "both subs must be owners, got {kinds:?}"
        );

        // A sub owner carries a real range + the per-file syntax provenance ref.
        let sub = owners.iter().find(|o| o["name"] == "discount").expect("discount owner");
        assert_eq!(sub["kind"], "sub");
        assert_eq!(sub["file_id"], "file:lib/My/App.pm");
        assert_eq!(sub["confidence"], "high");
        assert_eq!(sub["provenance_refs"], json!(["prov:syntax:file:lib/My/App.pm"]));
        // `sub discount` is declared on the second line (0-based line 1).
        assert_eq!(sub["range"]["start_line"], 1, "discount is on the second line (0-based)");
        // The owner id is byte-span-derived, not traversal-order (no trailing `:N`).
        let owner_id = must_some(sub["owner_id"].as_str());
        assert!(owner_id.contains("discount"), "owner id names the decl: {owner_id}");

        // A per-file `syntax` provenance fact exists without file-digest limitations.
        assert!(
            provenance
                .iter()
                .any(|p| p["source"] == "syntax" && p["file_id"] == "file:lib/My/App.pm"),
            "a per-file syntax provenance entry must exist"
        );
        assert!(limitations.is_empty(), "valid file digesting should add no limitations");

        let _ = std::fs::remove_dir_all(&root);
    }
}
