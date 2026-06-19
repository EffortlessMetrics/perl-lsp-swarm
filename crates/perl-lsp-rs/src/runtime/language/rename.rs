//! Rename handlers for symbol renaming
//!
//! Handles textDocument/prepareRename and textDocument/rename requests.
//! Supports both single-file and workspace-wide renaming.
//!
//! # Lifecycle-Aware Behavior
//!
//! Uses the routing module for state-aware dispatch:
//! - **Ready state**: Full workspace rename across all indexed files
//! - **Building state**: Bounded readiness wait before falling back
//! - **Degraded state**: Same-file rename only for local symbols; package-scoped edits are refused

use super::super::*;
use crate::protocol::{req_position, req_uri};
#[cfg(feature = "workspace")]
use crate::runtime::routing::{IndexAccessMode, route_index_access};
#[cfg(feature = "workspace")]
use perl_lsp_rs_core::providers::navigation::rename_shadow::{
    RenamePackagePilotIneligibleReason, RenamePackagePilotResult, rename_package_pilot_proof,
};
use perl_lsp_rs_core::providers::rename::{RenameOptions, RenameProvider, TextEdit as RenameEdit};
#[cfg(feature = "workspace")]
use perl_semantic_facts::{EntityId, FileId, PlannedEdit};
#[cfg(feature = "workspace")]
use perl_workspace::semantic::queries::{QueryContext, SemanticQueries};
#[cfg(feature = "workspace")]
use std::collections::BTreeMap;
#[cfg(feature = "workspace")]
use std::sync::Arc;
#[cfg(feature = "workspace")]
use std::time::{Duration, Instant};

/// Returns true if `c` is a Perl variable sigil (`$`, `@`, or `%`).
fn is_perl_sigil(c: char) -> bool {
    matches!(c, '$' | '@' | '%')
}

fn strip_perl_sigil(name: &str) -> &str {
    match name.chars().next() {
        Some(c) if is_perl_sigil(c) => &name[c.len_utf8()..],
        _ => name,
    }
}

fn lexical_declaration_keyword_before(source: &str, symbol_start: usize) -> bool {
    let line_start =
        if symbol_start == 0 { 0 } else { source[..symbol_start].rfind('\n').map_or(0, |p| p + 1) };
    let prefix = source[line_start..symbol_start].trim_end();
    let previous_word =
        prefix.split(|c: char| !c.is_alphanumeric() && c != '_').rfind(|word| !word.is_empty());
    matches!(previous_word, Some("my" | "state"))
}

impl LspServer {
    #[cfg(feature = "workspace")]
    fn package_name_before_offset(source: &str, offset: usize) -> Option<&str> {
        let prefix = source.get(..offset.min(source.len()))?;
        let mut current_pkg = None;

        for raw_line in prefix.lines() {
            let trimmed = raw_line.trim_start();
            if !trimmed.starts_with("package ") {
                continue;
            }

            let package_decl = trimmed.trim_start_matches("package ").trim_start();
            let package_name = package_decl
                .split(|ch: char| ch.is_whitespace() || ch == ';')
                .next()
                .unwrap_or_default();

            if !package_name.is_empty() {
                current_pkg = Some(package_name);
            }
        }

        current_pkg
    }

    #[cfg(feature = "workspace")]
    fn offset_is_sub_declaration_name(source: &str, offset: usize, symbol: &str) -> bool {
        let Some(prefix) = source.get(..offset.min(source.len())) else {
            return false;
        };
        let line_start = prefix.rfind('\n').map_or(0, |idx| idx + 1);
        let line_end = source
            .get(offset.min(source.len())..)
            .and_then(|suffix| suffix.find('\n').map(|idx| offset.min(source.len()) + idx))
            .unwrap_or(source.len());
        let Some(line_text) = source.get(line_start..line_end) else {
            return false;
        };

        let leading_ws = line_text.len() - line_text.trim_start().len();
        let trimmed = &line_text[leading_ws..];
        let Some(after_sub) = trimmed.strip_prefix("sub") else {
            return false;
        };
        if !after_sub.chars().next().is_some_and(char::is_whitespace) {
            return false;
        }

        let whitespace_bytes: usize =
            after_sub.chars().take_while(|ch| ch.is_whitespace()).map(char::len_utf8).sum();
        let name_start = line_start + leading_ws + "sub".len() + whitespace_bytes;
        let Some(tail) = source.get(name_start..) else {
            return false;
        };
        let declared_name = tail
            .split(|ch: char| ch.is_whitespace() || ch == '(' || ch == '{' || ch == ';')
            .next()
            .unwrap_or_default();
        if declared_name.is_empty() {
            return false;
        }

        let name_end = name_start + declared_name.len();
        let bare_declared_name =
            declared_name.rsplit_once("::").map_or(declared_name, |(_, bare)| bare);

        bare_declared_name == symbol && name_start <= offset && offset <= name_end
    }

    #[cfg(feature = "workspace")]
    fn qualified_sub_key_at_offset(
        source: &str,
        offset: usize,
    ) -> Option<crate::workspace_index::SymbolKey> {
        if !source.is_char_boundary(offset) {
            return None;
        }

        let bytes = source.as_bytes();
        if offset >= bytes.len() {
            return None;
        }

        fn is_qualified_name_byte(byte: u8) -> bool {
            byte.is_ascii_alphanumeric() || byte == b'_' || byte == b':'
        }

        if !is_qualified_name_byte(bytes[offset]) {
            return None;
        }

        let mut start = offset;
        while start > 0 && is_qualified_name_byte(bytes[start - 1]) {
            start -= 1;
        }

        let mut end = offset + 1;
        while end < bytes.len() && is_qualified_name_byte(bytes[end]) {
            end += 1;
        }

        let token = source.get(start..end)?;
        let (pkg, name) = token.rsplit_once("::")?;
        if pkg.is_empty() || name.is_empty() {
            return None;
        }

        Some(crate::workspace_index::SymbolKey {
            pkg: Arc::from(pkg),
            name: Arc::from(name),
            sigil: None,
            kind: crate::workspace_index::SymKind::Sub,
        })
    }

    #[cfg(feature = "workspace")]
    fn is_package_sub_workspace_rename_key(key: &crate::workspace_index::SymbolKey) -> bool {
        key.kind == crate::workspace_index::SymKind::Sub && key.pkg.as_ref() != "main"
    }

    #[cfg(feature = "workspace")]
    fn package_rename_pilot_entity_id<Q: SemanticQueries>(
        queries: &Q,
        file_id: FileId,
        byte_offset: u32,
        symbol: &str,
    ) -> Option<EntityId> {
        queries
            .symbol_at(file_id, byte_offset)
            .and_then(|(_, occurrence)| occurrence.entity_id)
            .or_else(|| {
                let context = QueryContext::new(file_id, None, Some(byte_offset));
                queries.definitions(symbol, &context).first().map(|candidate| candidate.entity_id)
            })
    }

    #[cfg(feature = "workspace")]
    fn package_rename_pilot_edits_to_workspace_edit(
        workspace_index: &crate::workspace_index::WorkspaceIndex,
        edits: Vec<PlannedEdit>,
    ) -> Option<(Value, usize)> {
        if edits.is_empty() {
            return None;
        }

        let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let mut edit_count = 0_usize;

        for edit in edits {
            // GA promotion (#1386): all PlannedEditCategory variants (Definition,
            // Reference, ImportList, ExportList, and any future #[non_exhaustive]
            // additions) are accepted here.  The anchor byte-range guard below
            // (start >= end || old_text mismatch) is the correctness safety valve
            // for every category — if the anchor resolves to the wrong text the
            // whole workspace edit is aborted rather than emitting a corrupt edit.
            let location = workspace_index
                .semantic_anchor_wire_location_for_file(edit.file_id, edit.anchor_id)?;
            let doc = workspace_index.document_store().get(&location.uri)?;
            let start = location.range.start.to_byte_offset(&doc.text);
            let end = location.range.end.to_byte_offset(&doc.text);
            if start >= end || doc.text.get(start..end)? != edit.old_text {
                return None;
            }

            grouped.entry(location.uri).or_default().push(json!({
                "range": {
                    "start": {
                        "line": location.range.start.line,
                        "character": location.range.start.character
                    },
                    "end": {
                        "line": location.range.end.line,
                        "character": location.range.end.character
                    }
                },
                "newText": edit.new_text
            }));
            edit_count += 1;
        }

        Some((json!({ "changes": grouped }), edit_count))
    }

    #[cfg(feature = "workspace")]
    fn package_rename_live_pilot_workspace_edit(
        &self,
        workspace_index: &crate::workspace_index::WorkspaceIndex,
        uri: &str,
        byte_offset: usize,
        symbol: &str,
        new_name_bare: &str,
    ) -> Option<Result<(Value, usize), ()>> {
        let byte_offset = u32::try_from(byte_offset).ok()?;
        let planned_edits = workspace_index
            .with_semantic_queries_for_uri(uri, |file_id, queries| {
                let entity_id =
                    Self::package_rename_pilot_entity_id(&queries, file_id, byte_offset, symbol)?;
                let outcome = rename_package_pilot_proof(true, &queries, entity_id, new_name_bare);
                match outcome.result {
                    RenamePackagePilotResult::Eligible { edits } => Some(Ok(edits)),
                    RenamePackagePilotResult::Ineligible {
                        reason: RenamePackagePilotIneligibleReason::EmptyPlan,
                        ..
                    } => None,
                    RenamePackagePilotResult::Ineligible { .. } => Some(Err(())),
                    _ => None,
                }
            })
            .flatten();

        match planned_edits {
            Some(Ok(edits)) => {
                Self::package_rename_pilot_edits_to_workspace_edit(workspace_index, edits).map(Ok)
            }
            Some(Err(())) => Some(Err(())),
            None => None,
        }
    }

    fn record_rename_provider_decision_trace(
        &self,
        uri: Option<&str>,
        symbol: Option<&str>,
        reason: &'static str,
        edit_count: usize,
        fallback_state: &'static str,
    ) {
        self.record_provider_decision_trace(
            "rename",
            &json!({
                "provider": "rename",
                "provider_action": "textDocument/rename",
                "decision": if edit_count > 0 { "acted" } else { "fallback" },
                "reason": reason,
                "uri": uri,
                "symbol": symbol,
                "live_provider_edit_count": edit_count,
                "fallback_state": fallback_state,
                "claim_boundary": "package-local compiler facts require exact live guardrails; broader compiler-backed refactor facts remain gated by receipt proof"
            }),
        );
    }

    fn token_byte_span_in_line(line: &str, offset: usize) -> Option<(usize, usize)> {
        let is_ident_char = |ch: char| ch.is_alphanumeric() || ch == '_';
        let clamped_offset = offset.min(line.len());

        let mut probe = None;
        let mut previous = None;
        for (idx, ch) in line.char_indices() {
            let end = idx + ch.len_utf8();
            if idx <= clamped_offset && clamped_offset < end {
                probe = Some((idx, ch));
                break;
            }
            if idx >= clamped_offset {
                break;
            }
            previous = Some((idx, ch));
        }

        if probe.is_none()
            && let Some((idx, ch)) = previous
            && idx + ch.len_utf8() == clamped_offset
        {
            probe = Some((idx, ch));
        }

        let (probe_idx, probe_char) = probe?;
        if !is_ident_char(probe_char) {
            return None;
        }

        let mut start = probe_idx;
        while start > 0 {
            let prefix = &line[..start];
            let Some((prev_idx, prev_char)) = prefix.char_indices().next_back() else {
                break;
            };
            if !is_ident_char(prev_char) {
                break;
            }
            start = prev_idx;
        }

        let mut end = probe_idx + probe_char.len_utf8();
        while end < line.len() {
            let suffix = &line[end..];
            let Some(next_char) = suffix.chars().next() else {
                break;
            };
            if !is_ident_char(next_char) {
                break;
            }
            end += next_char.len_utf8();
        }

        Some((start, end))
    }

    fn generated_accessor_prefix_matches(prefix: &str) -> bool {
        let mut rest = prefix.trim_start();

        if let Some(after_paren) = rest.strip_prefix('(') {
            rest = after_paren.trim_start();
        }

        if let Some(after_quote) = rest.strip_prefix(['\'', '"']) {
            rest = after_quote.trim_start();
        }

        if let Some(after_plus) = rest.strip_prefix('+') {
            rest = after_plus.trim_start();
        }

        if let Some(after_quote) = rest.strip_prefix(['\'', '"']) {
            rest = after_quote.trim_start();
        }

        rest.is_empty()
    }

    fn generated_accessor_arrow_follows(suffix: &str) -> bool {
        let mut rest = suffix.trim_start();
        if let Some(quote) = rest.chars().next().filter(|ch| matches!(ch, '\'' | '"')) {
            rest = rest[quote.len_utf8()..].trim_start();
        }
        if let Some(after_paren) = rest.strip_prefix(')') {
            rest = after_paren.trim_start();
        }
        rest.starts_with("=>")
    }

    fn has_generated_accessor_marker_before(line: &str, token_start: usize) -> bool {
        let mut search_from = 0;

        while let Some(relative_idx) = line[search_from..token_start].find("has") {
            let idx = search_from + relative_idx;
            let after_idx = idx + "has".len();
            let before_is_boundary = idx == 0
                || line[..idx]
                    .chars()
                    .next_back()
                    .is_none_or(|ch| !ch.is_alphanumeric() && ch != '_');
            if before_is_boundary
                && Self::generated_accessor_prefix_matches(&line[after_idx..token_start])
            {
                return true;
            }
            search_from = after_idx;
        }

        false
    }

    fn offset_is_generated_accessor_declaration(source: &str, offset: usize) -> bool {
        let clamped_offset = offset.min(source.len());
        let line_start = if clamped_offset == 0 {
            0
        } else {
            source[..clamped_offset].rfind('\n').map_or(0, |p| p + 1)
        };
        let line_end =
            source[clamped_offset..].find('\n').map_or(source.len(), |p| clamped_offset + p);
        let Some(line) = source.get(line_start..line_end) else {
            return false;
        };
        let relative_offset = clamped_offset.saturating_sub(line_start);
        let Some((token_start, token_end)) = Self::token_byte_span_in_line(line, relative_offset)
        else {
            return false;
        };

        if !Self::has_generated_accessor_marker_before(line, token_start) {
            return false;
        }

        Self::generated_accessor_arrow_follows(&line[token_end..])
    }

    fn rename_blocked_at(doc: &crate::state::DocumentState, offset: usize) -> bool {
        Self::offset_is_generated_accessor_declaration(&doc.text, offset)
            || Self::offset_is_inside_quoted_string(&doc.text, offset)
    }

    fn scoped_lexical_rename_edits(
        &self,
        doc: &crate::state::DocumentState,
        ast: &perl_parser_core::Node,
        offset: usize,
        normalized_name: &str,
    ) -> Option<Vec<Value>> {
        if normalized_name.chars().next().is_none_or(|c| !is_perl_sigil(c)) {
            return None;
        }

        let provider = RenameProvider::new(ast, doc.text.clone());
        let result = provider.scoped_rename(
            offset,
            strip_perl_sigil(normalized_name),
            &RenameOptions::default(),
        );
        if !result.is_valid || result.edits.is_empty() {
            return None;
        }
        let lexical_declaration_edit_count =
            result.edits.iter().filter(|edit| self.is_lexical_declaration_edit(doc, edit)).count();
        if lexical_declaration_edit_count != 1 {
            return None;
        }

        Some(
            result
                .edits
                .iter()
                .map(|edit| self.rename_edit_to_lsp_text_edit(doc, edit, normalized_name))
                .collect(),
        )
    }

    fn is_lexical_declaration_edit(
        &self,
        doc: &crate::state::DocumentState,
        edit: &RenameEdit,
    ) -> bool {
        if edit.location.start == 0 || edit.location.start > doc.text.len() {
            return false;
        }
        let Some(prefix) = doc.text.get(..edit.location.start) else {
            return false;
        };
        let Some(previous) = prefix.chars().next_back() else {
            return false;
        };
        if !is_perl_sigil(previous) {
            return false;
        }
        lexical_declaration_keyword_before(&doc.text, edit.location.start - previous.len_utf8())
    }

    fn rename_edit_to_lsp_text_edit(
        &self,
        doc: &crate::state::DocumentState,
        edit: &RenameEdit,
        normalized_name: &str,
    ) -> Value {
        let mut start = edit.location.start;
        let mut new_text = edit.new_text.clone();

        if start > 0
            && let Some(prefix) = doc.text.get(..start)
            && let Some(previous) = prefix.chars().next_back()
            && is_perl_sigil(previous)
        {
            start = start.saturating_sub(previous.len_utf8());
            new_text = normalized_name.to_string();
        }

        let (start_line, start_char) = self.offset_to_pos16(doc, start);
        let (end_line, end_char) = self.offset_to_pos16(doc, edit.location.end);

        json!({
            "range": {
                "start": { "line": start_line, "character": start_char },
                "end": { "line": end_line, "character": end_char }
            },
            "newText": new_text
        })
    }

    fn token_span_at(content: &str, offset: usize) -> Option<(usize, usize)> {
        // Build (byte_offset, char) pairs so all index arithmetic stays in byte space.
        // Callers receive a byte offset from pos16_to_offset and the returned
        // (start, end) are passed to offset_to_pos16, which also expects byte offsets.
        let pairs: Vec<(usize, char)> = content.char_indices().collect();
        if pairs.is_empty() {
            return None;
        }

        let is_ident_char = |ch: char| ch.is_alphanumeric() || ch == '_';
        let is_sigil = |ch: char| ch == '$' || ch == '@' || ch == '%';

        // Map the byte offset to a char-index in pairs via binary search.
        // partition_point returns the first index where byte_offset >= offset.
        let ci = pairs.partition_point(|(b, _)| *b < offset);
        // Clamp to last valid index.
        let ci = ci.min(pairs.len().saturating_sub(1));

        // Allow cursor-at-end and cursor-next-to-token positions by probing the
        // previous character when needed.
        let mut probe = ci;
        let at_logical_end = offset >= content.len()
            || (ci == pairs.len().saturating_sub(1) && offset > pairs[ci].0);
        if at_logical_end
            || (!is_ident_char(pairs[probe].1)
                && !is_sigil(pairs[probe].1)
                && probe > 0
                && (is_ident_char(pairs[probe - 1].1) || is_sigil(pairs[probe - 1].1)))
        {
            probe = probe.saturating_sub(1);
        }

        if !is_ident_char(pairs[probe].1) && !is_sigil(pairs[probe].1) {
            return None;
        }

        // Skip from sigil to identifier body when the cursor is on sigil.
        let mut start = probe;
        if is_sigil(pairs[start].1) && start + 1 < pairs.len() && is_ident_char(pairs[start + 1].1)
        {
            start += 1;
        }

        while start > 0 && is_ident_char(pairs[start - 1].1) {
            start -= 1;
        }
        if start > 0 && is_sigil(pairs[start - 1].1) {
            start -= 1;
        }

        let mut end = start;
        if is_sigil(pairs[end].1) {
            end += 1;
        }
        while end < pairs.len() && is_ident_char(pairs[end].1) {
            end += 1;
        }

        // Require at least one identifier character so we don't rename standalone sigils.
        let body_start = if is_sigil(pairs[start].1) { start + 1 } else { start };
        if body_start >= end {
            return None;
        }

        // Convert char indices back to byte offsets for the return value.
        let start_byte = pairs[start].0;
        let end_byte = if end < pairs.len() { pairs[end].0 } else { content.len() };

        Some((start_byte, end_byte))
    }

    fn offset_is_inside_quoted_string(content: &str, offset: usize) -> bool {
        let mut in_single = false;
        let mut in_double = false;
        let mut escaped = false;
        let mut in_comment = false;

        for (byte_offset, ch) in content.char_indices() {
            if byte_offset >= offset {
                break;
            }

            if in_comment {
                if ch == '\n' {
                    in_comment = false;
                }
                continue;
            }

            if escaped {
                escaped = false;
                continue;
            }

            if in_single {
                match ch {
                    '\\' => escaped = true,
                    '\'' => in_single = false,
                    _ => {}
                }
                continue;
            }

            if in_double {
                match ch {
                    '\\' => escaped = true,
                    '"' => in_double = false,
                    _ => {}
                }
                continue;
            }

            match ch {
                '#' => in_comment = true,
                '\'' => in_single = true,
                '"' => in_double = true,
                _ => {}
            }
        }

        in_single || in_double
    }

    /// Normalize a rename target against the current symbol, validating the sigil and identifier.
    ///
    /// If `current_symbol` starts with a sigil, the returned name is sigil-prefixed.
    /// If `requested_name` is missing its sigil, the current symbol's sigil is applied.
    /// If `requested_name` has a mismatching sigil, this returns an error.
    fn normalize_rename_target(
        &self,
        current_symbol: Option<&str>,
        requested_name: &str,
    ) -> Result<String, JsonRpcError> {
        if requested_name.is_empty() {
            return Err(JsonRpcError {
                code: -32602,
                message: "Invalid identifier: empty rename target".to_string(),
                data: None,
            });
        }

        let current_sigil =
            current_symbol.and_then(|symbol| symbol.chars().next()).filter(|c| is_perl_sigil(*c));

        match current_sigil {
            Some(sigil) => {
                let mut requested_chars = requested_name.chars();
                let requested_first = requested_chars.next();
                let bare_name = if let Some(first) = requested_first {
                    if is_perl_sigil(first) {
                        if first != sigil {
                            return Err(JsonRpcError {
                                code: -32602,
                                message: format!(
                                    "Invalid identifier: sigil '{}' does not match '{}'",
                                    first, sigil
                                ),
                                data: None,
                            });
                        }
                        requested_chars.collect::<String>()
                    } else {
                        requested_name.to_string()
                    }
                } else {
                    String::new()
                };

                if !self.is_valid_identifier(&bare_name) {
                    return Err(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid identifier: {}", requested_name),
                        data: None,
                    });
                }

                Ok(format!("{}{}", sigil, bare_name))
            }
            None => {
                if !self.is_valid_identifier(requested_name) {
                    return Err(JsonRpcError {
                        code: -32602,
                        message: format!("Invalid identifier: {}", requested_name),
                        data: None,
                    });
                }
                Ok(requested_name.to_string())
            }
        }
    }

    /// Handle textDocument/prepareRename request
    pub(crate) fn handle_prepare_rename(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(params) = params {
            let uri = req_uri(&params)?;
            let (line, character) = req_position(&params)?;

            let documents = self.documents_guard();
            if let Some(doc) = self.get_document(&documents, uri) {
                if let Some(_ast) = &doc.ast {
                    let offset = self.pos16_to_offset(doc, line, character);
                    if Self::rename_blocked_at(doc, offset) {
                        return Ok(Some(json!(null)));
                    }

                    // Get the token at the current position
                    let token = self.get_token_at_position(&doc.text, offset);
                    if !token.is_empty()
                        && (token.starts_with('$')
                            || token.starts_with('@')
                            || token.starts_with('%')
                            || token.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_'))
                    {
                        // Find the token bounds
                        let (start_offset, end_offset) = self.get_token_bounds(&doc.text, offset);
                        let (start_line, start_char) = self.offset_to_pos16(doc, start_offset);
                        let (end_line, end_char) = self.offset_to_pos16(doc, end_offset);

                        // Return the range and placeholder text
                        return Ok(Some(json!({
                            "range": {
                                "start": {
                                    "line": start_line,
                                    "character": start_char
                                },
                                "end": {
                                    "line": end_line,
                                    "character": end_char
                                }
                            },
                            "placeholder": token
                        })));
                    }
                }
            }
        }

        // Return null if rename is not possible at this position
        Ok(Some(json!(null)))
    }

    /// Handle textDocument/rename request with workspace support
    ///
    /// Uses routing helper for lifecycle-aware behavior:
    /// - **Ready state**: Full workspace rename across all indexed files
    /// - **Building/Degraded state**: Same-file rename only with warning log
    pub(crate) fn handle_rename_workspace(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_rename_workspace_inner(params, true)
    }

    pub(crate) fn handle_rename_workspace_for_receipt_noise(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        self.handle_rename_workspace_inner(params, false)
    }

    fn workspace_edit_change_count(workspace_edit: &Value) -> usize {
        workspace_edit
            .get("changes")
            .and_then(Value::as_object)
            .map(|changes| changes.values().filter_map(Value::as_array).map(Vec::len).sum())
            .unwrap_or(0)
    }

    #[cfg(feature = "workspace")]
    fn overlay_open_documents_on_workspace_index(
        &self,
        workspace_index: &crate::workspace_index::WorkspaceIndex,
        open_documents: &[(String, i32, String)],
    ) {
        for (uri, version, text) in open_documents {
            workspace_index.document_store().open(uri.clone(), *version, text.clone());

            if let Ok(url) = url::Url::parse(uri)
                && let Err(error) = workspace_index.index_file(url, text.clone())
            {
                tracing::debug!(
                    uri,
                    error,
                    "Rename: open document overlay kept text but could not refresh index facts"
                );
            }
        }
    }

    #[cfg(feature = "workspace")]
    fn wait_for_ready_index(
        coordinator: &Arc<crate::workspace_index::IndexCoordinator>,
        budget: Duration,
    ) -> bool {
        let start = Instant::now();
        while start.elapsed() < budget {
            match coordinator.state() {
                crate::workspace_index::IndexState::Ready { .. } => return true,
                crate::workspace_index::IndexState::Building { .. } => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                crate::workspace_index::IndexState::Degraded { .. } => return false,
            }
        }

        matches!(coordinator.state(), crate::workspace_index::IndexState::Ready { .. })
    }

    fn package_rename_guard_accepts_workspace_edit(
        guard_workspace_edit: &Value,
        semantic_workspace_edit: &Value,
        semantic_edit_count: usize,
    ) -> bool {
        Self::workspace_edit_change_count(guard_workspace_edit) == semantic_edit_count
            && guard_workspace_edit == semantic_workspace_edit
    }

    fn handle_rename_workspace_inner(
        &self,
        params: Option<Value>,
        package_local_live_pilot_enabled: bool,
    ) -> Result<Option<Value>, JsonRpcError> {
        if let Some(p) = params {
            if let (Some(uri), Some(line), Some(ch), Some(new_name)) = (
                p.get("textDocument").and_then(|t| t.get("uri")).and_then(|s| s.as_str()),
                p.get("position").and_then(|p| p.get("line")).and_then(|n| n.as_u64()),
                p.get("position").and_then(|p| p.get("character")).and_then(|n| n.as_u64()),
                p.get("newName").and_then(|s| s.as_str()),
            ) {
                let rename_starts_in_blocked_context = {
                    let documents = self.documents_guard();
                    self.get_document(&documents, uri)
                        .map(|doc| {
                            let offset = self.pos16_to_offset(doc, line as u32, ch as u32);
                            Self::rename_blocked_at(doc, offset)
                        })
                        .unwrap_or(false)
                };
                if rename_starts_in_blocked_context {
                    self.record_rename_provider_decision_trace(
                        Some(uri),
                        None,
                        "blocked_context",
                        0,
                        "no_edit",
                    );
                    return Ok(Some(json!({"changes": {}})));
                }

                // Check index access mode using routing helper
                #[cfg(feature = "workspace")]
                {
                    let mut access_mode = route_index_access(self.coordinator());
                    if matches!(
                        access_mode,
                        IndexAccessMode::Partial(reason) if reason.starts_with("index building")
                    ) && let Some(coordinator) = self.coordinator()
                        && Self::wait_for_ready_index(coordinator, Duration::from_secs(2))
                    {
                        access_mode = route_index_access(self.coordinator());
                    }

                    let (symbol_key, rename_byte_offset, rename_is_package_scoped, package_scope) =
                        {
                            let documents = self.documents_guard();
                            self.get_document(&documents, uri).and_then(|doc| {
                                doc.ast.as_ref().map(|ast| {
                                    let offset = self.pos16_to_offset(doc, line as u32, ch as u32);
                                    let current_pkg =
                                        crate::declaration::current_package_at(ast, offset);
                                    let source_pkg =
                                        Self::package_name_before_offset(&doc.text, offset);
                                    let package_scope =
                                        source_pkg.unwrap_or(current_pkg).to_string();
                                    let key = crate::declaration::symbol_at_cursor_with_source(
                                        ast,
                                        offset,
                                        current_pkg,
                                        &doc.text,
                                    );
                                    (key, offset, !package_scope.is_empty(), package_scope)
                                })
                            })
                        }
                        .map_or(
                            (None, None, false, None),
                            |(key, offset, package_scoped, package_scope)| {
                                (key, Some(offset), package_scoped, Some(package_scope))
                            },
                        );
                    let current_symbol = {
                        let documents = self.documents_guard();
                        self.get_document(&documents, uri).map(|doc| {
                            let offset = self.pos16_to_offset(doc, line as u32, ch as u32);
                            self.get_token_at_position(&doc.text, offset)
                        })
                    };
                    let normalized_name =
                        self.normalize_rename_target(current_symbol.as_deref(), new_name)?;
                    let normalized_bare = strip_perl_sigil(&normalized_name);
                    let workspace_symbol_key = symbol_key
                        .as_ref()
                        .map(super::to_workspace_symbol_key)
                        .or_else(|| {
                            let offset = rename_byte_offset?;
                            let documents = self.documents_guard();
                            let doc = self.get_document(&documents, uri)?;
                            Self::qualified_sub_key_at_offset(&doc.text, offset)
                        })
                        .or_else(|| {
                            let symbol = current_symbol.as_deref()?;
                            if symbol.is_empty() || symbol.chars().next().is_some_and(is_perl_sigil)
                            {
                                return None;
                            }
                            let package = package_scope.as_deref()?;
                            let offset = rename_byte_offset?;
                            let documents = self.documents_guard();
                            let doc = self.get_document(&documents, uri)?;
                            if !Self::offset_is_sub_declaration_name(&doc.text, offset, symbol) {
                                return None;
                            }

                            Some(crate::workspace_index::SymbolKey {
                                pkg: Arc::from(package),
                                name: Arc::from(symbol),
                                sigil: None,
                                kind: crate::workspace_index::SymKind::Sub,
                            })
                        });

                    match access_mode {
                        IndexAccessMode::Partial(reason) => {
                            tracing::debug!(
                                reason,
                                "Rename: partial-index workspace facts cannot authorize package-local live edits, using same-file only"
                            );
                            self.record_rename_provider_decision_trace(
                                Some(uri),
                                current_symbol.as_deref(),
                                "partial_index_package_local_live_pilot_blocked",
                                0,
                                "same_file",
                            );
                            if workspace_symbol_key
                                .as_ref()
                                .is_some_and(Self::is_package_sub_workspace_rename_key)
                            {
                                self.record_rename_provider_decision_trace(
                                    Some(uri),
                                    current_symbol.as_deref(),
                                    "partial_index_workspace_rename_blocked",
                                    0,
                                    "partial_index",
                                );
                                return Ok(Some(json!({"changes": {}})));
                            }
                            // Fall through to same-file rename
                        }
                        IndexAccessMode::None => {
                            tracing::debug!("Rename: no workspace feature, using same-file only");
                            // Fall through to same-file rename
                        }
                        IndexAccessMode::Full(coordinator) => {
                            let idx = coordinator.index();
                            let open_document_snapshot: Vec<(String, i32, String)> = {
                                let documents = self.documents_guard();
                                documents
                                    .iter()
                                    .map(|(uri, doc)| (uri.clone(), doc.version, doc.text.clone()))
                                    .collect()
                            };
                            let open_documents: Vec<(String, String)> = open_document_snapshot
                                .iter()
                                .map(|(uri, _, text)| (uri.clone(), text.clone()))
                                .collect();
                            self.overlay_open_documents_on_workspace_index(
                                idx.as_ref(),
                                &open_document_snapshot,
                            );
                            if let Some(key) = workspace_symbol_key.as_ref() {
                                let edits = match crate::features::workspace_rename::build_rename_edit_with_open_documents(
                                        idx.as_ref(),
                                        key,
                                        normalized_bare,
                                        &open_documents,
                                    ) {
                                        Ok(edits) => edits,
                                        Err(refusal) => {
                                            self.record_rename_provider_decision_trace(
                                                Some(uri),
                                                current_symbol.as_deref(),
                                                "package_local_live_pilot_ambiguous",
                                                0,
                                                "ambiguous_identity",
                                            );
                                            return Err(JsonRpcError {
                                                code: -32602,
                                                message: refusal.to_string(),
                                                data: None,
                                            });
                                        }
                                    };
                                if !edits.is_empty() {
                                    let ws_edit =
                                        crate::features::workspace_rename::to_workspace_edit(edits);
                                    let edit_count = Self::workspace_edit_change_count(&ws_edit);
                                    if edit_count > 1 {
                                        self.record_rename_provider_decision_trace(
                                            Some(uri),
                                            current_symbol.as_deref(),
                                            "full_index_workspace_edit",
                                            edit_count,
                                            "workspace_index",
                                        );
                                        return Ok(Some(ws_edit));
                                    }
                                }
                            }

                            if package_local_live_pilot_enabled {
                                if let (Some(offset), Some(symbol)) =
                                    (rename_byte_offset, current_symbol.as_deref())
                                    && !symbol.is_empty()
                                    && symbol.chars().next().is_some_and(|c| !is_perl_sigil(c))
                                    && rename_is_package_scoped
                                {
                                    match self.package_rename_live_pilot_workspace_edit(
                                        idx.as_ref(),
                                        uri,
                                        offset,
                                        symbol,
                                        normalized_bare,
                                    ) {
                                        Some(Ok((semantic_ws_edit, semantic_edit_count))) => {
                                            let Some(key) = workspace_symbol_key.as_ref() else {
                                                self.record_rename_provider_decision_trace(
                                                    Some(uri),
                                                    Some(symbol),
                                                    "package_local_live_pilot_blocked",
                                                    0,
                                                    "no_edit",
                                                );
                                                return Ok(Some(json!({"changes": {}})));
                                            };

                                            let guard_edits =
                                                crate::features::workspace_rename::build_rename_edit_with_open_documents(
                                                    idx.as_ref(),
                                                    key,
                                                    normalized_bare,
                                                    &open_documents,
                                                )
                                                .map_err(|refusal| {
                                                    self.record_rename_provider_decision_trace(
                                                        Some(uri),
                                                        Some(symbol),
                                                        "package_local_live_pilot_ambiguous",
                                                        0,
                                                        "ambiguous_identity",
                                                    );
                                                    JsonRpcError {
                                                        code: -32602,
                                                        message: refusal.to_string(),
                                                        data: None,
                                                    }
                                                })?;

                                            if !guard_edits.is_empty() {
                                                let guard_ws_edit =
                                                    crate::features::workspace_rename::to_workspace_edit(
                                                        guard_edits,
                                                    );
                                                if Self::package_rename_guard_accepts_workspace_edit(
                                                    &guard_ws_edit,
                                                    &semantic_ws_edit,
                                                    semantic_edit_count,
                                                ) {
                                                    self.record_rename_provider_decision_trace(
                                                        Some(uri),
                                                        Some(symbol),
                                                        "package_local_live_pilot",
                                                        semantic_edit_count,
                                                        "none",
                                                    );
                                                    return Ok(Some(semantic_ws_edit));
                                                }

                                                let guard_edit_count =
                                                    Self::workspace_edit_change_count(
                                                        &guard_ws_edit,
                                                    );
                                                self.record_rename_provider_decision_trace(
                                                    Some(uri),
                                                    Some(symbol),
                                                    "full_index_workspace_edit",
                                                    guard_edit_count,
                                                    "workspace_index",
                                                );
                                                return Ok(Some(guard_ws_edit));
                                            }

                                            self.record_rename_provider_decision_trace(
                                                Some(uri),
                                                Some(symbol),
                                                "package_local_live_pilot_guard_mismatch",
                                                0,
                                                "no_edit",
                                            );
                                            return Ok(Some(json!({"changes": {}})));
                                        }
                                        Some(Err(())) => {
                                            self.record_rename_provider_decision_trace(
                                                Some(uri),
                                                Some(symbol),
                                                "package_local_live_pilot_blocked",
                                                0,
                                                "no_edit",
                                            );
                                            return Ok(Some(json!({"changes": {}})));
                                        }
                                        None => {}
                                    }
                                }
                            }

                            if let Some(key) = workspace_symbol_key.as_ref() {
                                let edits = crate::features::workspace_rename::build_rename_edit_with_open_documents(
                                    idx.as_ref(),
                                    key,
                                    normalized_bare,
                                    &open_documents,
                                )
                                .map_err(|refusal| {
                                    JsonRpcError {
                                        code: -32602,
                                        message: refusal.to_string(),
                                        data: None,
                                    }
                                })?;
                                if edits.is_empty() {
                                    // Fall through to same-file rename.
                                } else {
                                    let ws_edit =
                                        crate::features::workspace_rename::to_workspace_edit(edits);
                                    let edit_count = Self::workspace_edit_change_count(&ws_edit);
                                    self.record_rename_provider_decision_trace(
                                        Some(uri),
                                        current_symbol.as_deref(),
                                        "full_index_workspace_edit",
                                        edit_count,
                                        "workspace_index",
                                    );
                                    return Ok(Some(ws_edit));
                                }
                            }
                        }
                    }
                }

                // Same-file fallback for degraded/partial modes
                let documents = self.documents_guard();
                if let Some(doc) = self.get_document(&documents, uri) {
                    if let Some(ref ast) = doc.ast {
                        let offset = self.pos16_to_offset(doc, line as u32, ch as u32);
                        let current_symbol = self.get_token_at_position(&doc.text, offset);
                        let normalized_name =
                            self.normalize_rename_target(Some(current_symbol.as_str()), new_name)?;

                        if let Some(edits) =
                            self.scoped_lexical_rename_edits(doc, ast, offset, &normalized_name)
                        {
                            let edit_count = edits.len();
                            self.record_rename_provider_decision_trace(
                                Some(uri),
                                Some(current_symbol.as_str()),
                                "same_file_lexical",
                                edit_count,
                                "none",
                            );
                            return Ok(Some(json!({
                                "changes": {
                                    uri: edits
                                }
                            })));
                        }

                        // Create semantic analyzer for same-file rename
                        let analyzer = crate::semantic::SemanticAnalyzer::analyze(ast);

                        // Find all references (including definition)
                        let references = analyzer.find_all_references(offset, true);

                        if !references.is_empty() {
                            let edit_count = references.len();
                            // Create text edits for all references
                            let mut edits = Vec::new();
                            for location in references {
                                let (start_line, start_char) =
                                    self.offset_to_pos16(doc, location.start);
                                let (end_line, end_char) = self.offset_to_pos16(doc, location.end);

                                edits.push(json!({
                                    "range": {
                                        "start": { "line": start_line, "character": start_char },
                                        "end": { "line": end_line, "character": end_char }
                                    },
                                    "newText": normalized_name
                                }));
                            }

                            // Return WorkspaceEdit with same-file changes only
                            self.record_rename_provider_decision_trace(
                                Some(uri),
                                Some(current_symbol.as_str()),
                                "same_file_semantic",
                                edit_count,
                                "none",
                            );
                            return Ok(Some(json!({
                                "changes": {
                                    uri: edits
                                }
                            })));
                        }
                    }
                }
            }
        }
        // Explicit blocker paths return empty edits above. If no safe edit path
        // resolved, return null so clients can treat this as unavailable rather
        // than as an empty successful refactor.
        Ok(None)
    }

    /// Validate if a string is a valid Perl identifier
    pub(crate) fn is_valid_identifier(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }

        let chars: Vec<char> = name.chars().collect();

        // First character must be letter or underscore
        let first_char = match chars.first() {
            Some(c) => c,
            None => return false, // Empty string is not a valid identifier
        };
        if !first_char.is_alphabetic() && *first_char != '_' {
            return false;
        }

        // Rest must be alphanumeric or underscore
        for ch in &chars[1..] {
            if !ch.is_alphanumeric() && *ch != '_' {
                return false;
            }
        }

        true
    }

    /// Get token at position (simple implementation)
    pub(crate) fn get_token_at_position(&self, content: &str, offset: usize) -> String {
        if content.is_empty() || offset > content.len() {
            return String::new();
        }
        match Self::token_span_at(content, offset) {
            Some((start, end)) => content[start..end].to_string(),
            None => String::new(),
        }
    }

    /// Get the bounds of the token at the given position.
    ///
    /// Returns byte offsets `(start, end)` into `content`, suitable for
    /// passing directly to `offset_to_pos16`.
    pub(crate) fn get_token_bounds(&self, content: &str, offset: usize) -> (usize, usize) {
        if content.is_empty() || offset > content.len() {
            return (offset, offset);
        }
        Self::token_span_at(content, offset).unwrap_or((offset, offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "workspace")]
    use std::sync::Arc;

    #[test]
    fn token_helpers_support_cursor_on_sigil() -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let text = "my $value = 1;";
        let offset = text.find('$').ok_or("missing sigil")?;

        let token = server.get_token_at_position(text, offset);
        let (start, end) = server.get_token_bounds(text, offset);

        assert_eq!(token, "$value");
        // bounds are byte offsets; slice with &text[start..end]
        assert_eq!(&text[start..end], "$value");
        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn offset_is_sub_declaration_name_accepts_offset_eq_name_end_boundary()
    -> Result<(), Box<dyn std::error::Error>> {
        for (label, text) in [
            ("space-brace", "package Foo;\nsub process_data { 1 }\n"),
            ("paren", "package Foo;\nsub process_data() { 1 }\n"),
            ("semicolon", "package Foo;\nsub process_data;\n"),
            ("newline", "package Foo;\nsub process_data\n"),
            ("qualified", "package Foo;\nsub Foo::process_data { 1 }\n"),
        ] {
            let declared_name =
                if label == "qualified" { "Foo::process_data" } else { "process_data" };
            let declared_start = text.find(declared_name).ok_or("missing sub name")?;
            let bare_start = text.find("process_data").ok_or("missing bare sub name")?;
            let bare_end = bare_start + "process_data".len();
            let before_name = declared_start.checked_sub(1).ok_or("name starts at byte zero")?;

            assert_eq!(
                LspServer::offset_is_sub_declaration_name(text, before_name, "process_data"),
                false,
                "{label}: byte before sub name must not match"
            );
            assert_eq!(
                LspServer::offset_is_sub_declaration_name(text, bare_start, "process_data"),
                true,
                "{label}: start byte must match"
            );
            assert_eq!(
                LspServer::offset_is_sub_declaration_name(text, bare_end, "process_data"),
                true,
                "{label}: end byte boundary must match"
            );
            assert_eq!(
                LspServer::offset_is_sub_declaration_name(text, bare_end + 1, "process_data"),
                false,
                "{label}: byte after delimiter must not match"
            );
        }

        let longer_name = "package Foo;\nsub process_data_extra { 1 }\n";
        let process_data_end = longer_name.find("process_data").ok_or("missing longer sub name")?
            + "process_data".len();
        assert_eq!(
            LspServer::offset_is_sub_declaration_name(
                longer_name,
                process_data_end,
                "process_data"
            ),
            false,
            "partial prefix of a longer sub declaration must not match"
        );
        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn qualified_sub_key_at_offset_accepts_colon_and_underscore_boundaries()
    -> Result<(), Box<dyn std::error::Error>> {
        let text = "my $value = Foo::process_data();";
        let colon_offset = text.find("::").ok_or("missing package separator")?;
        let underscore_offset = text.find('_').ok_or("missing underscore")?;

        let colon_key = LspServer::qualified_sub_key_at_offset(text, colon_offset)
            .ok_or("colon should still resolve the qualified sub token")?;
        let underscore_key = LspServer::qualified_sub_key_at_offset(text, underscore_offset)
            .ok_or("underscore should still resolve the qualified sub token")?;

        for key in [colon_key, underscore_key] {
            assert_eq!(key.pkg.as_ref(), "Foo");
            assert_eq!(key.name.as_ref(), "process_data");
            assert!(matches!(key.kind, crate::workspace_index::SymKind::Sub));
        }
        Ok(())
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn package_sub_workspace_rename_key_requires_sub_kind_outside_main() {
        let package_sub = crate::workspace_index::SymbolKey {
            pkg: Arc::from("Foo"),
            name: Arc::from("process_data"),
            sigil: None,
            kind: crate::workspace_index::SymKind::Sub,
        };
        let main_sub = crate::workspace_index::SymbolKey {
            pkg: Arc::from("main"),
            name: Arc::from("process_data"),
            sigil: None,
            kind: crate::workspace_index::SymKind::Sub,
        };
        let package_var = crate::workspace_index::SymbolKey {
            pkg: Arc::from("Foo"),
            name: Arc::from("process_data"),
            sigil: Some('$'),
            kind: crate::workspace_index::SymKind::Var,
        };

        assert_eq!(LspServer::is_package_sub_workspace_rename_key(&package_sub), true);
        assert_eq!(LspServer::is_package_sub_workspace_rename_key(&main_sub), false);
        assert_eq!(LspServer::is_package_sub_workspace_rename_key(&package_var), false);
    }

    #[test]
    fn token_helpers_support_cursor_after_identifier() -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        let text = "my $value = 1;";
        let offset = text.find("$value").ok_or("missing variable")? + "$value".len();

        let token = server.get_token_at_position(text, offset);
        let (start, end) = server.get_token_bounds(text, offset);

        assert_eq!(token, "$value");
        // bounds are byte offsets; slice with &text[start..end]
        assert_eq!(&text[start..end], "$value");
        Ok(())
    }

    #[test]
    fn token_helpers_work_with_non_ascii_prefix() -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::default();
        // "# café\n" — 'é' (U+00E9) is 2 UTF-8 bytes; the line is 9 bytes, 8 chars.
        // Byte offset of '$' on line 2 = 9 + 3 = 12 (after "# café\nmy ").
        let text = "# café\nmy $foo = 1;";
        let dollar_offset = text.find('$').ok_or("missing sigil")?; // byte 12

        let token = server.get_token_at_position(text, dollar_offset);
        assert_eq!(token, "$foo", "byte offset must not be treated as char index");

        let (start, end) = server.get_token_bounds(text, dollar_offset);
        assert_eq!(&text[start..end], "$foo");
        Ok(())
    }

    #[test]
    fn rename_guard_detects_dynamic_typeglob_string_positions()
    -> Result<(), Box<dyn std::error::Error>> {
        let text = r#"*{"Mojolicious::Routes::Route::$name"} = sub { $cb->(@_) };"#;
        let string_offset = text.find("Routes::Route").ok_or("missing dynamic package")? + 2;
        let code_offset = text.find("$cb").ok_or("missing callback")? + 1;

        assert!(LspServer::offset_is_inside_quoted_string(text, string_offset));
        assert!(!LspServer::offset_is_inside_quoted_string(text, code_offset));
        Ok(())
    }

    #[test]
    fn rename_guard_uses_byte_offsets_with_unicode_prefix() -> Result<(), Box<dyn std::error::Error>>
    {
        let text = "my $emoji = \"🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀\";\n*{\"Mojolicious::Routes::Route::$name\"} = sub { $cb->(@_) };";
        let string_offset = text.find("Routes::Route").ok_or("missing dynamic package")? + 2;
        let code_offset = text.find("$cb").ok_or("missing callback")? + 1;

        assert!(LspServer::offset_is_inside_quoted_string(text, string_offset));
        assert!(!LspServer::offset_is_inside_quoted_string(text, code_offset));
        Ok(())
    }

    #[test]
    fn rename_guard_detects_unquoted_generated_accessors() -> Result<(), Box<dyn std::error::Error>>
    {
        let text = "use Moo;\nhas routes      => (is => 'ro');\nhas title => 1; has slug => 1;\nsub routes { 1 }\n";
        let accessor_offset = text.find("routes      =>").ok_or("missing has accessor")? + 2;
        let same_line_accessor_offset =
            text.find("slug =>").ok_or("missing same-line has accessor")? + 2;
        let sub_offset = text.rfind("routes").ok_or("missing sub name")? + 2;

        assert!(LspServer::offset_is_generated_accessor_declaration(text, accessor_offset));
        assert!(LspServer::offset_is_generated_accessor_declaration(
            text,
            same_line_accessor_offset
        ));
        assert!(!LspServer::offset_is_generated_accessor_declaration(text, sub_offset));
        Ok(())
    }

    #[test]
    fn rename_guard_detects_quoted_generated_accessors() -> Result<(), Box<dyn std::error::Error>> {
        let text = "use Moose;\nhas 'name' => (is => 'rw');\nhas'compact' => 1;\nhas '+extended' => 1;\nhas + 'spaced' => 1;\nhas ('wrapped') => 1;\nhash name => 1;\n";
        let accessor_offset = text.find("name' =>").ok_or("missing quoted accessor")? + 1;
        let compact_accessor_offset =
            text.find("compact' =>").ok_or("missing compact accessor")? + 1;
        let extended_accessor_offset =
            text.find("extended' =>").ok_or("missing extended accessor")? + 1;
        let spaced_accessor_offset = text.find("spaced' =>").ok_or("missing spaced accessor")? + 1;
        let wrapped_accessor_offset =
            text.find("wrapped') =>").ok_or("missing wrapped accessor")? + 1;
        let hash_offset = text.rfind("name").ok_or("missing hash key")? + 1;

        assert!(LspServer::offset_is_generated_accessor_declaration(text, accessor_offset));
        assert!(LspServer::offset_is_generated_accessor_declaration(text, compact_accessor_offset));
        assert!(LspServer::offset_is_generated_accessor_declaration(
            text,
            extended_accessor_offset
        ));
        assert!(LspServer::offset_is_generated_accessor_declaration(text, spaced_accessor_offset));
        assert!(LspServer::offset_is_generated_accessor_declaration(text, wrapped_accessor_offset));
        assert!(!LspServer::offset_is_generated_accessor_declaration(text, hash_offset));
        Ok(())
    }
}
