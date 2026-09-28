//! Disk-index hover for files that are indexed but not open (#16647).
//!
//! Hover was the one navigation-adjacent feature that required an open editor:
//! references, definition, and workspace/symbol already answer for
//! never-opened files from the same disk index. When the hovered URI is
//! absent from the open-documents map but present in the workspace index, the
//! file is read from disk, parsed ephemerally, and run through the same
//! extraction pipeline as an open document.

use super::hover_extracted::HoverExtracted;
use super::live_compiler_hover::LiveHoverCompilerContext;
use super::{LspServer, Value, json};
use crate::state::ParsedSnapshot;
use std::sync::Arc;

#[cfg(feature = "workspace")]
use crate::runtime::readiness::IndexReadinessPolicy;

/// Everything [`LspServer::closed_file_hover_parts`] extracted for a not-open
/// file, shaped exactly like the open-document Phase 1 output so Phase 2 can
/// resolve it unchanged.
pub(super) struct ClosedFileHover {
    pub(super) extracted: HoverExtracted,
    pub(super) live_compiler_context: Option<LiveHoverCompilerContext>,
    pub(super) hover_range: Option<Value>,
}

impl LspServer {
    /// Disk-index hover facts for a workspace file that is not open (#16647).
    ///
    /// The workspace index must already hold `uri` — the same proof
    /// references, definition, and workspace/symbol use for never-opened
    /// files. The disk text is the file's current bytes, so the local symbol
    /// facts parsed from it are never staler than the index entry that proved
    /// the file; every cross-file lookup on the extracted value keeps its own
    /// existing freshness gate.
    ///
    /// The parsed snapshot is ephemeral: it never enters the open-documents
    /// map and never publishes parse side effects. Publication re-checks that
    /// the file is still not open, so a `didOpen` racing the request fails
    /// closed instead of answering disk text over a live buffer.
    ///
    /// Returns `None` — the pre-#16647 null hover — when the index does not
    /// hold the URI, the file is unreadable from disk, the position is out of
    /// bounds, or the parse produced no AST.
    pub(super) fn closed_file_hover_parts(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<ClosedFileHover> {
        #[cfg(not(feature = "workspace"))]
        {
            let _ = (uri, line, character);
            return None;
        }

        #[cfg(feature = "workspace")]
        {
            // Give a still-building index the same brief chance the other
            // disk-index consumers get before declaring the URI unknown
            // (mirrors references #3069 / workspace-symbol #1514).
            let _ = self.check_index_readiness(IndexReadinessPolicy::WaitBriefly);
            let workspace_index = self.workspace_index()?;
            // The index must hold the URI — this is the same proof the other
            // disk-index consumers answer from; the FileId itself is unused.
            workspace_index.file_id_for_uri(uri)?;

            let path = url::Url::parse(uri).ok()?.to_file_path().ok()?;
            let text = std::fs::read_to_string(path).ok()?;

            // The canonical parse the parse worker runs (code-slice input,
            // full text as the snapshot source), minus the publish step: this
            // snapshot belongs to no document generation.
            let code_text = crate::util::code_slice(&text);
            let regex_session = perl_parser_core::RetainedRegexSession::begin(code_text);
            let mut parser = perl_parser::Parser::new(code_text);
            let (ast, parse_errors, regex_analysis) = match parser.parse() {
                Ok(mut ast) => {
                    let table = regex_session.finish(Some(&mut ast));
                    let parse_errors = parser.errors().to_vec();
                    (Some(Arc::new(ast)), parse_errors, Arc::new(table))
                }
                Err(error) => (None, vec![error], Arc::new(regex_session.finish(None))),
            };
            let snapshot = Arc::new(
                ParsedSnapshot::from_parse_result(0, &text, ast, parse_errors)
                    .with_regex_analysis(regex_analysis),
            );
            let ast = snapshot.ast()?;

            let offset = crate::util::position_to_offset(&text, line, character)?;
            let (token_start, token_end) = Self::token_byte_bounds_of(&text, offset);
            let hover_range = if token_end > token_start && token_end <= text.len() {
                let start = crate::util::offset_to_position(&text, token_start);
                let end = crate::util::offset_to_position(&text, token_end);
                Some(json!({
                    "start": { "line": start.line, "character": start.character },
                    "end": { "line": end.line, "character": end.character }
                }))
            } else {
                None
            };

            // Same generation-bound source-region evidence the open path
            // records (#5003): the proven-code gates in the extraction and in
            // the live-compiler cutover read this index.
            let source_region = snapshot.source_region_index();
            let source_region_kind = source_region.kind_at_offset(offset).as_str().to_string();
            super::set_hover_trace_source_region_kind(Some(source_region_kind.clone()));
            let live_compiler_context = Self::live_hover_compiler_context(
                uri,
                &text,
                offset,
                Some(source_region_kind),
                Some(source_region.as_ref()),
            );

            let parsed = Some(Arc::clone(&snapshot));
            let extracted = if let Some(module_name) = Self::find_use_module_at_offset(ast, offset)
            {
                if let Some(pragma_hover) = Self::build_pragma_hover(&module_name) {
                    HoverExtracted::Complete(pragma_hover)
                } else {
                    HoverExtracted::UseModule(module_name, text.clone(), uri.to_string(), offset)
                }
            } else if let Some(module_name) = Self::find_require_module_at_offset(&text, offset) {
                HoverExtracted::UseModule(module_name, text.clone(), uri.to_string(), offset)
            } else if let Some(module_name) = Self::find_with_module_at_offset(ast, offset) {
                HoverExtracted::UseModule(module_name, text.clone(), uri.to_string(), offset)
            } else {
                self.extract_symbol_hover(uri, ast, &text, offset, &parsed)
            };

            Some(ClosedFileHover { extracted, live_compiler_context, hover_range })
        }
    }
}

#[cfg(all(test, feature = "workspace"))]
mod tests {
    use super::*;
    use serde_json::Value;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const ANIMAL: &str = "package ClosedAnimal;\nsub speak {\n    my $self = shift;\n    my $n = $self->name;\n    return $n;\n}\nsub name { return 'generic' }\n1;\n";

    fn file_uri(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
        Ok(url::Url::from_file_path(path).map_err(|()| "invalid file path")?.to_string())
    }

    /// A server whose disk index holds one real on-disk file that no open
    /// document backs — the #16647 fixture.
    fn closed_file_server(
        dir: &std::path::Path,
        text: &str,
    ) -> Result<(LspServer, String), Box<dyn std::error::Error>> {
        let path = dir.join("lib").join("ClosedAnimal.pm");
        std::fs::create_dir_all(path.parent().ok_or("fixture path has no parent")?)
            .map_err(std::io::Error::other)?;
        std::fs::write(&path, text).map_err(std::io::Error::other)?;
        let uri = file_uri(&path)?;

        let server = LspServer::default();
        let folder_uri = url::Url::from_directory_path(dir)
            .map_err(|()| "invalid workspace directory path")?
            .to_string();
        server.test_set_root_path(dir.to_path_buf());
        server.test_set_workspace_folder_uris(&[folder_uri.as_str()]);
        server
            .test_index_file_in_building_state(&uri, text)
            .map_err(|error| format!("index the closed fixture file: {error}"))?;
        server.test_simulate_indexing_complete();
        assert!(
            !server.test_has_document(&uri),
            "fixture must not be open: the closed-file path is the claim"
        );
        Ok((server, uri))
    }

    fn hover_markdown(value: &Value) -> Option<&str> {
        value.get("contents")?.get("value")?.as_str()
    }

    fn hover_at(
        server: &LspServer,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<Value>, Box<dyn std::error::Error>> {
        Ok(server
            .test_handle_hover(Some(json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            })))
            .map_err(|error| format!("hover request must not error: {error}"))?)
    }

    /// The issue's case: hover on a lexical variable in an indexed-but-not-open
    /// file must answer from the disk index with the same scalar-variable card
    /// the open document gets.
    #[test]
    fn closed_file_hover_returns_index_facts_for_lexical_variable() -> TestResult {
        let dir = tempfile::tempdir()?;
        let (server, uri) = closed_file_server(dir.path(), ANIMAL)?;

        // Line 3 (0-based): `    my $n = $self->name;` — cursor on the `n`
        // (byte 8: four spaces, `my`, space, `$`).
        let hover =
            hover_at(&server, &uri, 3, 8)?.ok_or("closed-file $n hover must not be null")?;
        let value = hover_markdown(&hover).ok_or("hover must carry markdown contents")?;

        assert!(
            value.contains("**Scalar Variable**") && value.contains("`$n`"),
            "closed-file $n hover must be the scalar-variable card, got: {value}"
        );
        assert!(
            value.contains("**Declared at**: line 4"),
            "closed-file hover must carry the index-file declaration line, got: {value}"
        );
        assert!(
            value.contains("lexical in subroutine `speak`"),
            "closed-file hover must carry the lexical scope context, got: {value}"
        );
        assert!(
            hover.get("range").is_some(),
            "closed-file hover must carry the token range, got: {hover}"
        );
        Ok(())
    }

    /// Hover on a `sub` declaration token in the same never-opened file must
    /// return the subroutine card, not null.
    #[test]
    fn closed_file_hover_returns_subroutine_card_for_sub_declaration() -> TestResult {
        let dir = tempfile::tempdir()?;
        let (server, uri) = closed_file_server(dir.path(), ANIMAL)?;

        // Line 1 (0-based): `sub speak {` — cursor on `speak`.
        let hover =
            hover_at(&server, &uri, 1, 4)?.ok_or("closed-file sub hover must not be null")?;
        let value = hover_markdown(&hover).ok_or("hover must carry markdown contents")?;

        assert!(
            value.contains("**Subroutine**") && value.contains("speak"),
            "closed-file sub hover must be the subroutine card, got: {value}"
        );
        assert!(
            !value.contains("**Scalar Variable**"),
            "the sub declaration must not be answered with a variable card, got: {value}"
        );
        Ok(())
    }

    /// An unindexed URI — even a well-formed workspace URI — still fails closed
    /// to null: the index-presence gate is what bounds the fallback.
    #[test]
    fn closed_file_hover_stays_null_for_unindexed_uri() -> TestResult {
        let dir = tempfile::tempdir()?;
        let (server, _) = closed_file_server(dir.path(), ANIMAL)?;

        let stray = dir.path().join("stray.pl");
        std::fs::write(&stray, "my $unindexed = 1;\n")?;
        let stray_uri = file_uri(&stray)?;
        let hover = hover_at(&server, &stray_uri, 0, 3)?;

        assert!(
            hover.as_ref().is_none_or(Value::is_null),
            "hover on an unindexed file must stay null, got: {hover:?}"
        );
        Ok(())
    }

    /// The index proves the file existed at scan time, not that it is still
    /// readable: an unreadable closed file fails closed to null.
    #[test]
    fn closed_file_hover_stays_null_when_disk_file_is_missing() -> TestResult {
        let dir = tempfile::tempdir()?;
        let (server, uri) = closed_file_server(dir.path(), ANIMAL)?;
        let path = url::Url::parse(&uri)?.to_file_path().map_err(|()| "not a file uri")?;
        std::fs::remove_file(&path)?;

        let hover = hover_at(&server, &uri, 3, 8)?;
        assert!(
            hover.as_ref().is_none_or(Value::is_null),
            "hover on an index entry whose file vanished must stay null, got: {hover:?}"
        );
        Ok(())
    }

    /// Publication guard: a `didOpen` that lands while the disk answer is being
    /// computed supersedes disk text — the DiskSnapshot publication must fail
    /// closed, while the open-document generation publication still answers.
    #[test]
    fn disk_snapshot_publication_fails_closed_once_the_file_is_open() -> TestResult {
        let dir = tempfile::tempdir()?;
        let (server, uri) = closed_file_server(dir.path(), ANIMAL)?;

        let answer = json!({
            "contents": { "kind": "markdown", "value": "**Scalar Variable**\n\n`my $n`" }
        });

        server.test_apply_did_open(&uri, ANIMAL, 1)?;

        let stale_disk_answer = server.publish_hover_answer(
            &uri,
            super::super::HoverPublication::DiskSnapshot,
            Some(answer.clone()),
        )?;
        assert!(
            stale_disk_answer.as_ref().is_none_or(Value::is_null),
            "a disk answer must not publish over a document opened meanwhile, got: {stale_disk_answer:?}"
        );

        let open_answer = server.publish_hover_answer(
            &uri,
            super::super::HoverPublication::OpenGeneration(1),
            Some(answer),
        )?;
        assert!(
            open_answer.as_ref().is_some_and(|value| !value.is_null()),
            "the open document's own generation must still publish, got: {open_answer:?}"
        );
        Ok(())
    }
}
