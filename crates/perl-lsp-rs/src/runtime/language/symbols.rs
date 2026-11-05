//! Symbol and folding handlers for document outline features
//!
//! Handles textDocument/documentSymbol and textDocument/foldingRange requests.

use super::super::{
    GLOBAL_CANCELLATION_REGISTRY, JsonRpcError, JsonRpcId, LspServer, PerlLspCancellationToken,
    Value, byte_to_utf16_col, json,
};
use crate::cancellation::RequestCleanupGuard;
use crate::fallback::text::folding_ranges_from_text;
use crate::protocol::{REQUEST_CANCELLED, invalid_params, req_uri};
#[cfg(test)]
use crate::protocol::{REQUEST_FAILED, internal_error};
use crate::state::document_symbol_cap;
#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::sync::OnceLock;

#[cfg(test)]
#[derive(Clone, Copy)]
enum FoldingRangeTestFault {
    Cancelled,
    ProviderFailure,
    Unavailable,
    InstrumentFailure,
}

#[cfg(test)]
thread_local! {
    static FOLDING_RANGE_TEST_FAULT: Cell<Option<FoldingRangeTestFault>> =
        const { Cell::new(None) };
    // Observe the actual scanner input, not the capture operation. A test
    // retains the document Arc independently so a full-source copy cannot
    // reuse its address. Never dereferenced or emitted in production.
    static FOLDING_RANGE_TEST_SOURCE: Cell<Option<(*const u8, usize)>> =
        const { Cell::new(None) };
}

#[cfg(test)]
#[must_use]
struct FoldingRangeTestFaultGuard;

#[cfg(test)]
impl Drop for FoldingRangeTestFaultGuard {
    fn drop(&mut self) {
        FOLDING_RANGE_TEST_FAULT.with(|cell| cell.set(None));
    }
}

#[cfg(test)]
impl FoldingRangeTestFault {
    fn into_error(self) -> JsonRpcError {
        match self {
            Self::Cancelled => JsonRpcError {
                code: REQUEST_CANCELLED,
                message: "Request cancelled - folding range provider".to_string(),
                data: None,
            },
            Self::ProviderFailure => internal_error("folding range provider failure"),
            Self::Unavailable => JsonRpcError {
                code: REQUEST_FAILED,
                message: "folding range analysis unavailable".to_string(),
                data: None,
            },
            Self::InstrumentFailure => internal_error("folding range instrument failure"),
        }
    }
}

static SUB_REGEX: OnceLock<Result<regex::Regex, regex::Error>> = OnceLock::new();
static PACKAGE_REGEX: OnceLock<Result<regex::Regex, regex::Error>> = OnceLock::new();

fn get_sub_regex() -> Option<&'static regex::Regex> {
    SUB_REGEX.get_or_init(|| regex::Regex::new(r"^\s*sub\s+([a-zA-Z_]\w*)\b")).as_ref().ok()
}

fn get_package_regex() -> Option<&'static regex::Regex> {
    PACKAGE_REGEX
        .get_or_init(|| regex::Regex::new(r"^\s*package\s+([a-zA-Z_][\w:]*)\b"))
        .as_ref()
        .ok()
}

impl LspServer {
    fn dancer2_document_symbols_structured(
        &self,
        uri: &str,
        snapshot: &crate::state::ParsedSnapshot,
        text: &str,
        ast: &perl_parser::ast::Node,
    ) -> Vec<perl_lsp_rs_core::providers::document_symbols::DocumentSymbol> {
        use perl_lsp_rs_core::providers::dancer2::{
            dancer2_document_symbols, dancer2_request_kind,
        };
        use perl_lsp_rs_core::providers::document_symbols::leaf_document_symbol;
        use perl_position_tracking::WireRange;

        let context = self.dancer2_request_context(uri, text, snapshot.content_hash(), ast);
        if !context.activations.has_exact() {
            return Vec::new();
        }
        dancer2_document_symbols(&context.facts)
            .into_iter()
            .map(|symbol| {
                let start = usize::try_from(symbol.start).unwrap_or(0);
                let end = usize::try_from(symbol.end).unwrap_or(0);
                let range = WireRange::from_byte_offsets(text, start, end);
                leaf_document_symbol(
                    symbol.name,
                    symbol.detail,
                    dancer2_request_kind(symbol.is_route),
                    range,
                )
            })
            .collect()
    }

    /// Cancellation-aware wrapper for `textDocument/documentSymbol`.
    ///
    /// Polls the cancellation token before the symbol-extraction pipeline
    /// (AST-backed source symbols, subtest discovery, POD section scan) so a
    /// `$/cancelRequest` issued while the handler is waiting on the documents
    /// lock is observed promptly. Returns `REQUEST_CANCELLED` (code -32800)
    /// when cancelled.
    pub(crate) fn handle_document_symbol_cancellable(
        &self,
        params: Option<Value>,
        request_id: Option<&Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let typed_id = request_id.and_then(JsonRpcId::try_from_value);
        let _cleanup_guard = RequestCleanupGuard::from_ref(typed_id.as_ref());

        if let Some(ref tid) = typed_id {
            let token = GLOBAL_CANCELLATION_REGISTRY.get_token(tid).unwrap_or_else(|| {
                let token = PerlLspCancellationToken::new(
                    tid.clone(),
                    "textDocument/documentSymbol".into(),
                );
                let _ = GLOBAL_CANCELLATION_REGISTRY.register_token(token.clone());
                token
            });
            if token.is_cancelled_relaxed() {
                return Err(JsonRpcError {
                    code: REQUEST_CANCELLED,
                    message: "Request cancelled - document symbol provider".to_string(),
                    data: None,
                });
            }
        }

        self.handle_document_symbol(params)
    }

    /// Handle textDocument/documentSymbol request
    pub(crate) fn handle_document_symbol(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        // Gate unadvertised feature
        if !self.advertised_features.lock().document_symbol {
            return Err(crate::protocol::method_not_advertised());
        }

        let cap = document_symbol_cap();
        clear_document_symbol_projection_receipt();

        if let Some(params) = params {
            let uri = req_uri(&params)?;

            enum DocumentSymbolCapture {
                Missing,
                FullSyncPending,
                Ast {
                    snapshot: std::sync::Arc<crate::state::ParsedSnapshot>,
                    text: String,
                    ast: std::sync::Arc<perl_parser::ast::Node>,
                },
                Fallback {
                    text: String,
                },
            }

            let capture = {
                let documents = self.documents_guard();
                match self.get_document(&documents, uri) {
                    None => DocumentSymbolCapture::Missing,
                    Some(doc) => {
                        if doc.text_for_user_answers().is_none() {
                            DocumentSymbolCapture::FullSyncPending
                        } else {
                            let text = doc.text.clone();
                            match doc.current_parsed() {
                                Some(snapshot) => match snapshot.ast().cloned() {
                                    Some(ast) => DocumentSymbolCapture::Ast { snapshot, text, ast },
                                    None => DocumentSymbolCapture::Fallback { text },
                                },
                                None => DocumentSymbolCapture::Fallback { text },
                            }
                        }
                    }
                }
            };

            let finish_outline = |outline: Vec<
                perl_lsp_rs_core::providers::document_symbols::DocumentSymbol,
            >| {
                let budget_outcome =
                        perl_lsp_rs_core::providers::document_symbols::apply_document_symbol_node_budget(
                            outline,
                            cap,
                        );
                record_document_symbol_projection_receipt(cap, &budget_outcome, None);
                let symbols = document_symbols_to_json(budget_outcome.symbols);
                Ok(Some(json!(symbols)))
            };

            match capture {
                DocumentSymbolCapture::Missing | DocumentSymbolCapture::FullSyncPending => {
                    return Ok(Some(json!([])));
                }
                DocumentSymbolCapture::Fallback { text } => {
                    tracing::debug!(uri, "Using fallback symbol extraction");
                    let mut outline = fallback_document_symbols(&text);
                    let pod_headings =
                        perl_lsp_rs_core::providers::document_symbols::pod_heading_document_symbols(
                            &text,
                        );
                    perl_lsp_rs_core::providers::document_symbols::merge_document_symbol_roots_in_source_order(
                        &mut outline,
                        pod_headings,
                    );
                    return finish_outline(outline);
                }
                DocumentSymbolCapture::Ast { snapshot, text, ast } => {
                    let live_result =
                        perl_lsp_rs_core::providers::document_symbols::source_backed_document_symbols_from_ast(
                            &ast,
                            &text,
                        );

                    let mut outline = live_result.symbols;
                    let subtests = perl_lsp_rs_core::providers::testing::subtest::discover_subtests(
                        &ast, &text,
                    );
                    if !subtests.is_empty() {
                        perl_lsp_rs_core::providers::testing::subtest::nest_subtest_symbols_in_outline(
                            &mut outline,
                            &subtests,
                            &text,
                        );
                    }

                    let dancer2_outline =
                        self.dancer2_document_symbols_structured(uri, &snapshot, &text, &ast);
                    perl_lsp_rs_core::providers::document_symbols::merge_document_symbol_roots_in_source_order(
                        &mut outline,
                        dancer2_outline,
                    );
                    let pod_headings =
                        perl_lsp_rs_core::providers::document_symbols::pod_heading_document_symbols(
                            &text,
                        );
                    // AST-path roots span their package extents, so headings
                    // nest under the enclosing package instead of cluttering
                    // the top level and duplicating nested subs (#18769).
                    perl_lsp_rs_core::providers::document_symbols::nest_pod_headings_into_outline(
                        &mut outline,
                        pod_headings,
                    );

                    return finish_outline(outline);
                }
            }
        }

        Ok(Some(json!([])))
    }

    /// Admit a foldingRange request before fold computation.
    ///
    /// Production dispatch is a transparent adapter over this result (#13981):
    /// malformed, stale, unavailable, and injected provider faults stay errors
    /// rather than being rewritten as an empty fold list.
    fn admit_folding_range_request<'a>(
        &self,
        params: Option<&'a Value>,
    ) -> Result<&'a str, JsonRpcError> {
        if !self.advertised_features.lock().folding_range {
            return Err(crate::protocol::method_not_advertised());
        }

        let params =
            params.ok_or_else(|| invalid_params("Missing required parameter: textDocument.uri"))?;
        let uri = req_uri(params)?;
        let req_version =
            params["textDocument"]["version"].as_i64().and_then(|n| i32::try_from(n).ok());
        self.ensure_latest(uri, req_version)?;

        #[cfg(test)]
        if let Some(fault) = FOLDING_RANGE_TEST_FAULT.with(Cell::take) {
            return Err(fault.into_error());
        }

        Ok(uri)
    }

    #[cfg(test)]
    fn inject_folding_range_test_fault(
        &self,
        fault: FoldingRangeTestFault,
    ) -> FoldingRangeTestFaultGuard {
        let _ = self;
        FOLDING_RANGE_TEST_FAULT.with(|cell| cell.set(Some(fault)));
        FoldingRangeTestFaultGuard
    }

    /// Handle textDocument/foldingRange request
    pub(crate) fn handle_folding_range(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let uri = self.admit_folding_range_request(params.as_ref())?;

        // Snapshot current user-answer text and parsed AST under the
        // documents lock, then drop the guard so the expensive scanning,
        // AST walk, and deduplication run off-lock (#4966). Predecessor
        // `text_arc` remains stored as evidence and must not publish folds
        // while Full-sync is outstanding.
        //
        // Prefer the generation-current snapshot; fall back to the latest
        // published one when the current generation has no snapshot yet —
        // the workspace indexer bumps the generation after didOpen, which
        // would otherwise make every AST-derived fold vanish behind a
        // keyword-free empty response (#11858 pattern, #15430).
        let (text, parsed, captured_generation) = {
            let documents = self.documents_guard();
            match self.get_document(&documents, uri) {
                Some(doc) => match doc.text_for_user_answers() {
                    // Clone the Arc (O(1)) under the lock, then borrow its
                    // immutable source after release without a full text copy.
                    // The latest_parsed fallback is gated on the snapshot's
                    // content hash matching the current text: a stale AST's
                    // offsets paired with shifted text would fold the wrong
                    // lines (#15776 review). It only runs for synchronized
                    // text — when Full-sync is outstanding, the arm above
                    // returns empty before any predecessor snapshot is read.
                    Some(_) => {
                        let current_or_matching = doc.current_parsed().or_else(|| {
                            let latest = doc.latest_parsed()?;
                            let matches =
                                perl_lsp_rs_core::tooling::perl_critic::hash_content(&doc.text)
                                    == latest.content_hash();
                            matches.then_some(latest)
                        });
                        (doc.text_arc.clone(), current_or_matching, doc.current_generation())
                    }
                    None => return Ok(Some(json!([]))),
                },
                None => return Ok(Some(json!([]))),
            }
        };
        // Keep the captured Arc alive while every scanner borrows its buffer.
        let doc_text: &str = &text;
        #[cfg(test)]
        FOLDING_RANGE_TEST_SOURCE.with(|cell| cell.set(Some((doc_text.as_ptr(), doc_text.len()))));
        let mut lsp_ranges = Vec::new();

        // Add text-based data section folding
        if let Some(marker_offset) = crate::util::find_data_marker_byte_lexed(doc_text) {
            let marker_line = offset_to_line(doc_text, marker_offset);
            let total_lines = doc_text.lines().count();

            // Add fold for data section body if it exists
            let start_line = marker_line + 1;
            let end_line = total_lines.saturating_sub(1);
            push_multiline_folding_range(&mut lsp_ranges, start_line, end_line, "comment");
        }

        // NOTE: Heredoc folding is handled by the AST NodeKind::Heredoc arm
        // in FoldingRangeExtractor::extract. The previous lexer-based
        // extract_heredoc_ranges produced overlapping-but-non-identical
        // ranges that caused double-fold chevrons (#5072).

        // Add POD folding ranges (POD is parser trivia — no NodeKind::Pod — so the
        // AST path cannot fold it).  This scan runs only when the AST is available,
        // complementing the existing fallback that runs when it is not.  (#5071)
        for (pod_start_line, pod_end_line) in extract_pod_ranges(doc_text) {
            push_multiline_folding_range(&mut lsp_ranges, pod_start_line, pod_end_line, "comment");
        }

        // Add #region/#endregion folding ranges
        let region_ranges = crate::folding::FoldingRangeExtractor::extract_region_markers(doc_text);
        for range in region_ranges {
            let start_line = offset_to_line(doc_text, range.start_offset);
            let end_line = offset_to_line(doc_text, range.end_offset);
            push_multiline_folding_range(&mut lsp_ranges, start_line, end_line, "region");
        }

        if let Some(ast) = parsed.as_ref().and_then(|p| p.ast()) {
            // Extract folding ranges from AST
            let mut extractor = crate::folding::FoldingRangeExtractor::new();
            let ranges = extractor.extract(ast);

            // Convert to LSP JSON format with proper line offsets
            for range in ranges {
                // Calculate actual line numbers from document content
                let start_line = offset_to_line(doc_text, range.start_offset);
                let end_line = offset_to_line(doc_text, range.end_offset);
                // Imports spans end at the last use/no statement, so their
                // end line is content to keep inside the fold, not a closing
                // brace line to keep visible (#18113).
                let is_imports =
                    matches!(range.kind, Some(crate::folding::FoldingRangeKind::Imports));
                let lsp_end_line = if range.end_offset_on_content_line {
                    // Content-terminated spans (heredoc bodies) end at the
                    // last content byte of their final body line, so the
                    // line containing the end offset is already the
                    // inclusive last folded line; decrementing it left the
                    // last body line unfolded for every body of 3+ lines
                    // (#17299).
                    lsp_inclusive_content_end_line(start_line, end_line)
                } else if is_imports && end_line > start_line {
                    Some(end_line)
                } else {
                    match lsp_inclusive_multiline_end_line(start_line, end_line) {
                        Some(end) => Some(end),
                        None => {
                            // A span rejected by the inclusive filter may still
                            // genuinely cover multiple lines (e.g. a closing
                            // delimiter that shares its line with content, or
                            // an end offset at the start of its last line).
                            // Count the newlines inside the span: any newline
                            // means real multiline content that must not be
                            // dropped (#15430 residue).
                            let newlines = doc_text
                                .get(range.start_offset..range.end_offset)
                                .map(|span| span.matches('\n').count())
                                .unwrap_or(0);
                            (newlines >= 1).then_some(start_line + newlines)
                        }
                    }
                };
                if let Some(lsp_end_line) = lsp_end_line {
                    let mut lsp_range = json!({
                        "startLine": start_line,
                        "endLine": lsp_end_line,  // LSP folding ranges are inclusive
                    });

                    if let Some(ref kind) = range.kind {
                        lsp_range["kind"] = match kind {
                            crate::folding::FoldingRangeKind::Comment => json!("comment"),
                            crate::folding::FoldingRangeKind::Imports => json!("imports"),
                            crate::folding::FoldingRangeKind::Region => json!("region"),
                        };
                    }

                    lsp_ranges.push(lsp_range);
                }
            }

            // Dedup identical ranges (start+end+kind) that arise when both a
            // Subroutine node and its inner Block node map to the same line span.
            lsp_ranges.sort_by_key(|r| {
                (
                    r["startLine"].as_u64().unwrap_or(0),
                    r["endLine"].as_u64().unwrap_or(0),
                    r.get("kind").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                )
            });
            lsp_ranges.dedup_by_key(|r| {
                (
                    r["startLine"].as_u64().unwrap_or(0),
                    r["endLine"].as_u64().unwrap_or(0),
                    r.get("kind").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                )
            });

            // If no ranges from AST, try fallback
            if lsp_ranges.is_empty() {
                lsp_ranges = folding_ranges_from_text(doc_text, 1000);
            }
        } else {
            lsp_ranges = folding_ranges_from_text(doc_text, 1000);
        }

        if !self.user_answer_text_is_current(uri, captured_generation) {
            return Ok(Some(json!([])));
        }
        Ok(Some(json!(lsp_ranges)))
    }

    /// Non-blocking folding range handler with text-based fallback.
    ///
    /// Test-only (#4628 / #13981): compiled out of production builds so this
    /// path cannot satisfy production foldingRange acceptance.
    #[cfg(any(test, feature = "test-fallbacks"))]
    pub(crate) fn on_folding_range(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let uri = params.pointer("/textDocument/uri").and_then(|v| v.as_str()).unwrap_or("");
        let text = self.buffer_text(uri).unwrap_or_default();
        let ranges = folding_ranges_from_text(&text, 128);
        Ok(serde_json::to_value(ranges).unwrap_or(serde_json::json!([])))
    }
}

thread_local! {
    static LAST_DOCUMENT_SYMBOL_PROJECTION_RECEIPT: RefCell<
        Option<perl_lsp_rs_core::providers::document_symbols::DocumentSymbolProjectionReceipt>,
    > = const { RefCell::new(None) };
}

fn record_document_symbol_projection_receipt(
    configured_cap: usize,
    outcome: &perl_lsp_rs_core::providers::document_symbols::DocumentSymbolBudgetOutcome,
    operation_result_identity: Option<String>,
) {
    let receipt = perl_lsp_rs_core::providers::document_symbols::DocumentSymbolProjectionReceipt::from_budget_outcome(
        configured_cap,
        outcome,
        operation_result_identity,
    );
    LAST_DOCUMENT_SYMBOL_PROJECTION_RECEIPT.with(|cell| {
        *cell.borrow_mut() = Some(receipt);
    });
}

fn clear_document_symbol_projection_receipt() {
    LAST_DOCUMENT_SYMBOL_PROJECTION_RECEIPT.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
pub(crate) fn last_document_symbol_projection_receipt()
-> Option<perl_lsp_rs_core::providers::document_symbols::DocumentSymbolProjectionReceipt> {
    LAST_DOCUMENT_SYMBOL_PROJECTION_RECEIPT.with(|cell| cell.borrow().clone())
}

fn fallback_document_symbols(
    content: &str,
) -> Vec<perl_lsp_rs_core::providers::document_symbols::DocumentSymbol> {
    use perl_lsp_rs_core::providers::document_symbols::leaf_document_symbol;
    use perl_position_tracking::{WirePosition, WireRange};

    let mut symbols = Vec::new();
    let Some(sub_regex) = get_sub_regex() else {
        return symbols;
    };
    let Some(package_regex) = get_package_regex() else {
        return symbols;
    };

    for (line_num, line) in content.lines().enumerate() {
        let line_index = u32::try_from(line_num).unwrap_or(u32::MAX);
        let line_end = u32::try_from(byte_to_utf16_col(line, line.len())).unwrap_or(0);

        if let Some(captures) = sub_regex.captures(line)
            && let Some(name_match) = captures.get(1)
        {
            let range = WireRange {
                start: WirePosition::new(line_index, 0),
                end: WirePosition::new(line_index, line_end),
            };
            let start_char =
                u32::try_from(byte_to_utf16_col(line, name_match.start())).unwrap_or(0);
            let end_char = u32::try_from(byte_to_utf16_col(line, name_match.end())).unwrap_or(0);
            let mut symbol = leaf_document_symbol(name_match.as_str(), "", 12, range);
            symbol.selection_range = WireRange {
                start: WirePosition::new(line_index, start_char),
                end: WirePosition::new(line_index, end_char),
            };
            symbols.push(symbol);
        }

        if let Some(captures) = package_regex.captures(line)
            && let Some(name_match) = captures.get(1)
        {
            let range = WireRange {
                start: WirePosition::new(line_index, 0),
                end: WirePosition::new(line_index, line_end),
            };
            let start_char =
                u32::try_from(byte_to_utf16_col(line, name_match.start())).unwrap_or(0);
            let end_char = u32::try_from(byte_to_utf16_col(line, name_match.end())).unwrap_or(0);
            let mut symbol = leaf_document_symbol(name_match.as_str(), "", 4, range);
            symbol.selection_range = WireRange {
                start: WirePosition::new(line_index, start_char),
                end: WirePosition::new(line_index, end_char),
            };
            symbols.push(symbol);
        }
    }

    symbols
}

fn document_symbols_to_json(
    symbols: Vec<perl_lsp_rs_core::providers::document_symbols::DocumentSymbol>,
) -> Vec<Value> {
    match serde_json::to_value(symbols) {
        Ok(Value::Array(items)) => items,
        Ok(_) | Err(_) => Vec::new(),
    }
}

/// Helper function to convert offset to line number
fn offset_to_line(content: &str, offset: usize) -> usize {
    // Floor to a char boundary: an externally-derived offset can land
    // mid-codepoint, and slicing there would panic (#18097).
    let mut offset = offset.min(content.len());
    while offset > 0 && !content.is_char_boundary(offset) {
        offset -= 1;
    }
    content[..offset].chars().filter(|&c| c == '\n').count()
}

/// Scan for POD blocks (`=pod`/`=head*`/`=begin` ... `=cut`/`=end`) and return
/// `(start_line, end_line)` pairs suitable for folding.  POD is parser trivia —
/// no `NodeKind::Pod` — so the AST folding path cannot cover it.
fn extract_pod_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut pod_start: Option<usize> = None;
    for (line_no, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if pod_start.is_none() {
            if trimmed.starts_with("=pod")
                || trimmed.starts_with("=head")
                || trimmed.starts_with("=begin")
                || trimmed.starts_with("=over")
                || trimmed.starts_with("=item")
                || trimmed.starts_with("=encoding")
                || trimmed.starts_with("=for")
            {
                pod_start = Some(line_no);
            }
        } else if trimmed.starts_with("=cut") || trimmed.starts_with("=end") {
            // `pod_start` is necessarily `Some` in this branch (the `if` above
            // covers the `None` case), but take it by pattern match rather than
            // `unwrap()` so a future edit to the branch condition degrades to a
            // skipped range instead of panicking the server on user input.
            if let Some(start) = pod_start.take() {
                ranges.push((start, line_no));
            }
        }
    }
    // Unclosed POD block extends to end of file
    if let Some(start) = pod_start {
        let last_line = text.lines().count().saturating_sub(1);
        if last_line > start {
            ranges.push((start, last_line));
        }
    }
    ranges
}

fn push_multiline_folding_range<T>(
    lsp_ranges: &mut Vec<Value>,
    start_line: T,
    end_line: T,
    kind: &str,
) where
    T: Copy + Ord + serde::Serialize,
{
    if end_line > start_line {
        lsp_ranges.push(json!({
            "startLine": start_line,
            "endLine": end_line,
            "kind": kind
        }));
    }
}

/// Inclusive LSP end line for a delimiter-terminated span whose end offset
/// lands on a closing line (`}` of a block): closing lines stay visible in
/// editors, so the raw end line steps back one. Spans that collapse onto
/// their start line return `None` and may fall back to newline counting.
fn lsp_inclusive_multiline_end_line(start_line: usize, raw_end_line: usize) -> Option<usize> {
    let lsp_end_line = raw_end_line.saturating_sub(1);
    (lsp_end_line > start_line).then_some(lsp_end_line)
}

/// Inclusive LSP end line for a content-terminated span whose end offset
/// lands at the last content byte of its final line (heredoc bodies):
/// the raw end line is already the last folded line and is kept as-is,
/// while single-line spans drop out of the multiline-only filter (#17299).
fn lsp_inclusive_content_end_line(start_line: usize, raw_end_line: usize) -> Option<usize> {
    (raw_end_line > start_line).then_some(raw_end_line)
}

impl LspServer {
    /// Document symbol runtime quality receipt for the source-backed live slice.
    ///
    /// Calls the live `textDocument/documentSymbol` handler and wraps the result
    /// in a typed receipt that records the fresh parser-syntax symbols promoted
    /// live. Astless, generated/no-source, stale, dynamic, low-confidence, and
    /// ambiguous cases keep fallback or gated behavior.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub(crate) fn document_symbols_runtime_quality_receipt(
        &self,
        params: Option<Value>,
    ) -> Result<Option<Value>, JsonRpcError> {
        let (source_backed_receipt, source_backed_count) =
            self.document_symbols_source_backed_receipt(params.as_ref())?;
        let live_provider_result = self.handle_document_symbol(params)?;
        let live_provider_count = match live_provider_result.as_ref() {
            Some(Value::Array(items)) => items.len(),
            _ => 0,
        };
        let no_live_behavior_change = source_backed_count == 0;

        Ok(Some(json!({
            "provider": "document_symbols",
            "live_provider_result": live_provider_result,
            "live_provider_count": live_provider_count,
            "shadow_state": "partial_live_source_backed",
            "source_backed_receipt": source_backed_receipt,
            "projection_receipt": last_document_symbol_projection_receipt(),
            "no_live_behavior_change": no_live_behavior_change,
            "notes": [
                format!(
                    "document-symbol runtime quality receipt: live_provider_count={}; \
                     source_backed_candidates={}; \
                     source-backed parser syntax document symbols are live; \
                     astless, stale, dynamic, generated/no-source, and ambiguous cases keep fallback",
                    live_provider_count,
                    source_backed_count
                )
            ]
        })))
    }

    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    fn document_symbols_source_backed_receipt(
        &self,
        params: Option<&Value>,
    ) -> Result<(Value, usize), JsonRpcError> {
        let Some(params) = params else {
            return Ok((document_symbols_empty_source_backed_receipt("missing_params"), 0));
        };
        let uri = req_uri(params)?;
        let documents = self.documents_guard();
        let Some(doc) = self.get_document(&documents, uri) else {
            return Ok((document_symbols_empty_source_backed_receipt("unknown_uri"), 0));
        };
        let parsed = doc.current_parsed();
        let Some(ast) = parsed.as_ref().and_then(|p| p.ast()) else {
            return Ok((document_symbols_empty_source_backed_receipt("ast_unavailable"), 0));
        };

        let live_result =
            perl_lsp_rs_core::providers::document_symbols::source_backed_document_symbols_from_ast(
                ast, &doc.text,
            );
        let source_backed_count = live_result.fact_traces.len();
        Ok((
            json!({
                "source": "ParserSyntax",
                "provenance": "ExactAst",
                "confidence": "High",
                "freshness": "Fresh",
                "fallback_state": "Primary",
                "source_backed_count": source_backed_count,
                "fact_source_traces": live_result.fact_traces,
            }),
            source_backed_count,
        ))
    }
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
fn document_symbols_empty_source_backed_receipt(reason: &str) -> Value {
    json!({
        "source_backed_count": 0,
        "fallback_state": "Fallback",
        "reason": reason,
        "fact_source_traces": [],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        CONTENT_MODIFIED, INTERNAL_ERROR, INVALID_PARAMS, JsonRpcId, JsonRpcRequest,
        JsonRpcResponse, METHOD_NOT_FOUND, REQUEST_FAILED,
    };

    /// A mid-codepoint offset must floor instead of panicking (#18097).
    #[test]
    fn offset_to_line_floors_mid_codepoint_offsets() {
        // "é" is 2 bytes (0..2), "\n" at 2, "x" at 3.
        let content = "é\nx";
        assert_eq!(offset_to_line(content, 1), 0, "mid-codepoint floors to line 0");
        assert_eq!(offset_to_line(content, 2), 0);
        assert_eq!(offset_to_line(content, 3), 1);
        assert_eq!(offset_to_line(content, usize::MAX), 1, "length clamp still applies");
    }

    #[test]
    fn push_multiline_folding_range_boundary_discriminator_end_line_gt_start_line_rejects_equal_input()
     {
        let mut ranges = Vec::new();

        push_multiline_folding_range(&mut ranges, 4, 4, "region");

        assert!(ranges.is_empty());
    }

    #[test]
    fn push_multiline_folding_range_boundary_discriminator_input_that_hits_the_boundary_end_line_gt_start_line_accepts_multiline_input()
     {
        let mut ranges = Vec::new();

        push_multiline_folding_range(&mut ranges, 4, 6, "comment");

        assert_eq!(ranges.len(), 1, "input that hits the boundary: end_line > start_line");
        assert_eq!(ranges[0]["startLine"], json!(4));
        assert_eq!(ranges[0]["endLine"], json!(6));
        assert_eq!(ranges[0]["kind"], json!("comment"));
    }

    #[test]
    fn lsp_inclusive_multiline_end_line_boundary_discriminator_lsp_end_line_gt_start_line_rejects_short_span()
     {
        assert_eq!(lsp_inclusive_multiline_end_line(4, 5), None);
        assert_eq!(lsp_inclusive_multiline_end_line(4, 0), None);
    }

    #[test]
    fn lsp_inclusive_multiline_end_line_boundary_discriminator_input_that_hits_the_boundary_lsp_end_line_gt_start_line_accepts_multiline_span()
     {
        assert_eq!(
            lsp_inclusive_multiline_end_line(4, 6),
            Some(5),
            "input that hits the boundary: lsp_end_line > start_line"
        );
    }

    #[test]
    fn lsp_inclusive_content_end_line_boundary_discriminator_raw_end_line_gt_start_line_rejects_single_line_span()
     {
        assert_eq!(lsp_inclusive_content_end_line(4, 4), None);
        assert_eq!(lsp_inclusive_content_end_line(4, 0), None);
    }

    #[test]
    fn lsp_inclusive_content_end_line_boundary_discriminator_input_that_hits_the_boundary_raw_end_line_gt_start_line_keeps_raw_end_line()
     {
        assert_eq!(
            lsp_inclusive_content_end_line(4, 5),
            Some(5),
            "input that hits the boundary: raw_end_line > start_line"
        );
        assert_eq!(
            lsp_inclusive_content_end_line(4, 6),
            Some(6),
            "input that hits the boundary: raw_end_line > start_line"
        );
    }

    fn folding_ranges_for_source(source: &str) -> Result<Vec<Value>, JsonRpcError> {
        let server = LspServer::new();
        let uri = "file:///folding-observer.pl";
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": source,
            }
        })))?;

        let response = server.handle_folding_range(Some(json!({
            "textDocument": { "uri": uri }
        })))?;

        Ok(response.and_then(|value| value.as_array().cloned()).unwrap_or_default())
    }

    #[test]
    fn handle_folding_range_call_presence_observer_push_multiline_folding_range_data_section_comment()
    -> Result<(), Box<dyn std::error::Error>> {
        let ranges = folding_ranges_for_source("print \"ok\\n\";\n__DATA__\nalpha\nbeta\n")?;

        assert!(
            ranges.iter().any(|range| {
                range.get("kind") == Some(&json!("comment"))
                    && range.get("startLine") == Some(&json!(2))
                    && range.get("endLine") == Some(&json!(3))
            }),
            "input that reaches call push_multiline_folding_range(&mut lsp_ranges, start_line, end_line, \"comment\")"
        );

        Ok(())
    }

    #[test]
    fn handle_folding_range_heredoc_returns_well_formed_parser_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let ranges = folding_ranges_for_source("my $text = <<'TXT';\nalpha\nbeta\nTXT\n")?;

        assert!(
            ranges.iter().all(|range| {
                let Some(start_line) = range.get("startLine").and_then(Value::as_u64) else {
                    return false;
                };
                let Some(end_line) = range.get("endLine").and_then(Value::as_u64) else {
                    return false;
                };
                end_line > start_line
                    && range.get("kind").and_then(Value::as_str).is_none_or(|kind| kind == "region")
            }),
            "heredoc folding output must contain only valid multiline ranges: {ranges:?}"
        );

        Ok(())
    }

    #[test]
    fn handle_folding_range_heredoc_body_length_sweep_covers_last_body_line()
    -> Result<(), Box<dyn std::error::Error>> {
        // Bodies of 2, 3, and 4 lines in one document: every fold must cover
        // the whole body with the terminator line left visible, regardless of
        // body length (#17299: 3+ line bodies folded one line short while
        // 2-line bodies were correct, because the two lengths took different
        // endLine conversion paths).
        let source = concat!(
            "my $a = <<'A';\n", // line 0  (declaration)
            "a1\n",             // line 1  (body start)
            "a2\n",             // line 2  (body end)
            "A\n",              // line 3  (terminator)
            "my $b = <<'B';\n", // line 4  (declaration)
            "b1\n",             // line 5  (body start)
            "b2\n",             // line 6
            "b3\n",             // line 7  (body end)
            "B\n",              // line 8  (terminator)
            "my $c = <<'C';\n", // line 9  (declaration)
            "c1\n",             // line 10 (body start)
            "c2\n",             // line 11
            "c3\n",             // line 12
            "c4\n",             // line 13 (body end)
            "C\n",              // line 14 (terminator)
        );
        let ranges = folding_ranges_for_source(source)?;

        for (start, end) in [(1, 2), (5, 7), (10, 13)] {
            assert!(
                ranges.iter().any(|range| {
                    range.get("kind") == Some(&json!("region"))
                        && range.get("startLine") == Some(&json!(start))
                        && range.get("endLine") == Some(&json!(end))
                }),
                "heredoc body must fold as [{start}..{end}] (terminator visible): {ranges:?}"
            );
        }

        Ok(())
    }

    #[test]
    fn handle_folding_range_pod_block_produces_comment_fold() {
        let source = "=pod\n\nThis is documentation.\n\n=head1 SYNOPSIS\n\n    use Foo;\n\n=cut\nmy $x = 1;\n";
        let ranges = folding_ranges_for_source(source).unwrap_or_default();
        assert!(
            ranges.iter().any(|range| {
                range.get("kind") == Some(&json!("comment"))
                    && range.get("startLine") == Some(&json!(0))
            }),
            "POD block starting at line 0 should produce a comment fold: {ranges:?}"
        );
    }

    #[test]
    fn handle_folding_range_call_presence_observer_ast_lsp_end_line_gt_start_line()
    -> Result<(), Box<dyn std::error::Error>> {
        let ranges = folding_ranges_for_source("sub full {\n    my $value = 1;\n}\n")?;

        assert!(ranges.iter().any(|range| {
            range.get("startLine") == Some(&json!(0))
                && range.get("endLine").and_then(Value::as_u64).is_some_and(|end| end > 0)
        }));

        Ok(())
    }

    fn folding_source_arc(
        server: &LspServer,
        uri: &str,
    ) -> Result<std::sync::Arc<str>, Box<dyn std::error::Error>> {
        let documents = server.documents_guard();
        let doc = server.get_document(&documents, uri).ok_or("folding source document")?;
        Ok(std::sync::Arc::clone(&doc.text_arc))
    }

    fn assert_folding_source_shared(source: &str) {
        assert_eq!(
            FOLDING_RANGE_TEST_SOURCE.with(Cell::take),
            Some((source.as_ptr(), source.len())),
            "folding scans must borrow the document buffer without copying its bytes"
        );
    }

    #[test]
    fn folding_range_borrows_source_for_repeated_ast_requests()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///folding-source-ast.pl";
        let source = "sub full {\n    my $value = '😀';\n    return $value;\n}\n";
        server.test_apply_did_open(uri, source, 1)?;
        let shared = folding_source_arc(&server, uri)?;
        {
            let documents = server.documents_guard();
            let doc = server.get_document(&documents, uri).ok_or("parsed folding document")?;
            assert!(
                doc.latest_parsed().is_some_and(|snapshot| snapshot.ast().is_some()),
                "fixture must exercise AST-backed folding"
            );
        }

        // Equal content is insufficient: this deliberately copied source must
        // fail the buffer-identity oracle while the independent Arc is alive.
        let copied = shared.to_string();
        assert_eq!(copied.as_str(), shared.as_ref());
        assert_ne!(copied.as_ptr(), shared.as_ptr());

        for _ in 0..2 {
            FOLDING_RANGE_TEST_SOURCE.with(|cell| cell.set(None));
            let result = server.handle_folding_range(Some(json!({
                "textDocument": { "uri": uri }
            })))?;
            assert_eq!(result, Some(json!([{ "startLine": 0, "endLine": 2 }])));
            assert_folding_source_shared(&shared);
        }
        Ok(())
    }

    #[test]
    fn folding_range_borrows_current_fallback_source_and_refuses_desync()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///folding-source-fallback.pl";
        let source = "=pod\r\n😀\r\n=cut\r\nmy $broken = ;\r\n";
        let doc = crate::state::DocumentState::new(source, 1);
        assert!(doc.latest_parsed().is_none(), "fixture must exercise text fallback");
        server.documents_guard().insert(server.normalize_uri_key(uri), doc);
        let predecessor = folding_source_arc(&server, uri)?;

        FOLDING_RANGE_TEST_SOURCE.with(|cell| cell.set(None));
        let first = server.handle_folding_range(Some(json!({
            "textDocument": { "uri": uri }
        })))?;
        assert_eq!(first, Some(json!([{ "startLine": 0, "endLine": 2, "kind": "comment" }])));
        assert_folding_source_shared(&predecessor);

        {
            let mut documents = server.documents_guard();
            let doc =
                server.get_document_mut(&mut documents, uri).ok_or("editable folding document")?;
            doc.update_content("my $broken = ;\n=pod\n文 😀\nmore\n=cut\n", 2);
            assert!(doc.current_parsed().is_none(), "edited fixture must still use text fallback");
        }
        let current = folding_source_arc(&server, uri)?;
        assert_ne!(predecessor.as_ptr(), current.as_ptr());
        FOLDING_RANGE_TEST_SOURCE.with(|cell| cell.set(None));
        let edited = server.handle_folding_range(Some(json!({
            "textDocument": { "uri": uri, "version": 2 }
        })))?;
        assert_eq!(edited, Some(json!([{ "startLine": 1, "endLine": 4, "kind": "comment" }])));
        assert_folding_source_shared(&current);

        FOLDING_RANGE_TEST_SOURCE.with(|cell| cell.set(None));
        let stale = server.handle_folding_range(Some(json!({
            "textDocument": { "uri": uri, "version": 1 }
        })));
        assert!(matches!(stale, Err(error) if error.code == CONTENT_MODIFIED));
        assert_eq!(FOLDING_RANGE_TEST_SOURCE.with(Cell::take), None);

        {
            let mut documents = server.documents_guard();
            let doc =
                server.get_document_mut(&mut documents, uri).ok_or("desync folding document")?;
            doc.mark_full_sync_required();
        }
        let unavailable = server.handle_folding_range(Some(json!({
            "textDocument": { "uri": uri }
        })))?;
        assert_eq!(unavailable, Some(json!([])));
        assert_eq!(
            FOLDING_RANGE_TEST_SOURCE.with(Cell::take),
            None,
            "full-sync refusal must happen before scanning predecessor source"
        );
        Ok(())
    }

    fn production_folding_range_tests_blocked_by_test_fallbacks() -> bool {
        if std::env::var("LSP_TEST_FALLBACKS").is_ok() {
            eprintln!("Skipping production foldingRange dispatch test: LSP_TEST_FALLBACKS is set");
            true
        } else {
            false
        }
    }

    fn folding_range_rpc(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            id: Some(JsonRpcId::Integer(id)),
            method: method.to_string(),
            params,
        }
    }

    fn initialized_folding_range_server() -> LspServer {
        let server = LspServer::new();
        let init = server.handle_request(folding_range_rpc(1, "initialize", Some(json!({}))));
        assert!(
            init.as_ref().is_some_and(|response| response.error.is_none()),
            "initialize must succeed before production foldingRange dispatch tests"
        );
        server
    }

    fn request_folding_range(
        server: &LspServer,
        id: i64,
        params: Option<Value>,
    ) -> Result<JsonRpcResponse, Box<dyn std::error::Error>> {
        server
            .handle_request(folding_range_rpc(id, "textDocument/foldingRange", params))
            .ok_or_else(|| "textDocument/foldingRange must produce a JSON-RPC response".into())
    }

    fn assert_folding_range_error(
        response: &JsonRpcResponse,
        expected_code: i32,
        message_needle: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if response.result.is_some() {
            return Err(format!(
                "non-success foldingRange terminal must not become empty success, got result {:?}",
                response.result
            )
            .into());
        }
        let error = response.error.as_ref().ok_or("expected JsonRpcError, not empty success")?;
        if error.code != expected_code {
            return Err(format!(
                "foldingRange must preserve the exact JsonRpcError code {expected_code}, got {}; message={}",
                error.code, error.message
            )
            .into());
        }
        if !error.message.contains(message_needle) {
            return Err(format!(
                "foldingRange error message {:?} must contain {message_needle:?}",
                error.message
            )
            .into());
        }
        Ok(())
    }

    fn foldable_sub_source() -> &'static str {
        "sub full {\n    my $value = 1;\n    return $value;\n}\n"
    }

    fn open_foldable_document(
        server: &LspServer,
        uri: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 2,
                "text": foldable_sub_source(),
            }
        })))?;
        Ok(())
    }

    #[test]
    fn folding_range_dispatch_returns_legitimate_empty_for_known_empty_document()
    -> Result<(), Box<dyn std::error::Error>> {
        if production_folding_range_tests_blocked_by_test_fallbacks() {
            return Ok(());
        }

        let server = initialized_folding_range_server();
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": "file:///empty-folds.pl",
                "languageId": "perl",
                "version": 1,
                "text": "",
            }
        })))?;

        let response = request_folding_range(
            &server,
            2,
            Some(json!({ "textDocument": { "uri": "file:///empty-folds.pl" } })),
        )?;
        if let Some(error) = response.error.as_ref() {
            return Err(format!(
                "legitimate empty must come from a completed handler, not an error: {error:?}"
            )
            .into());
        }
        if response.result.as_ref() != Some(&json!([])) {
            return Err(format!(
                "known empty document must return a successful empty fold list, got {:?}",
                response.result
            )
            .into());
        }
        Ok(())
    }

    #[test]
    fn folding_range_dispatch_returns_known_folds_for_completed_handler()
    -> Result<(), Box<dyn std::error::Error>> {
        if production_folding_range_tests_blocked_by_test_fallbacks() {
            return Ok(());
        }

        let server = initialized_folding_range_server();
        open_foldable_document(&server, "file:///known-folds.pl")?;

        let response = request_folding_range(
            &server,
            2,
            Some(json!({ "textDocument": { "uri": "file:///known-folds.pl" } })),
        )?;
        if let Some(error) = response.error.as_ref() {
            return Err(format!("known folds must succeed: {error:?}").into());
        }
        let ranges = response
            .result
            .as_ref()
            .and_then(Value::as_array)
            .ok_or("known folds must return a fold array")?;
        if ranges.is_empty() {
            return Err(format!(
                "completed handler over the original request must still return known folds: {ranges:?}"
            )
            .into());
        }
        Ok(())
    }

    /// Malformed Perl in a CRLF document must still fold the region on
    /// the exact marker lines through production dispatch (#18034): the
    /// extractor's byte offsets feed `offset_to_line`, whose direct slice
    /// panicked when a CRLF drift landed inside a multi-byte character.
    #[test]
    fn folding_range_dispatch_handles_malformed_crlf_region()
    -> Result<(), Box<dyn std::error::Error>> {
        if production_folding_range_tests_blocked_by_test_fallbacks() {
            return Ok(());
        }

        let server = initialized_folding_range_server();
        server.test_handle_did_open(Some(json!({
            "textDocument": {
                "uri": "file:///crlf-region.pl",
                "languageId": "perl",
                "version": 1,
                "text": "#region\r\nmy $x = (\r\n#endregion",
            }
        })))?;

        let response = request_folding_range(
            &server,
            2,
            Some(json!({ "textDocument": { "uri": "file:///crlf-region.pl" } })),
        )?;
        if let Some(error) = response.error.as_ref() {
            return Err(format!("malformed CRLF region must succeed: {error:?}").into());
        }
        let ranges = response
            .result
            .as_ref()
            .and_then(Value::as_array)
            .ok_or("folding range must return an array")?;
        let region = ranges
            .iter()
            .find(|range| range.get("kind").and_then(Value::as_str) == Some("region"))
            .ok_or("the CRLF region fold must be present")?;
        assert_eq!(region.get("startLine"), Some(&json!(0)), "region start line: {region}");
        assert_eq!(region.get("endLine"), Some(&json!(2)), "region end line: {region}");
        Ok(())
    }

    #[test]
    fn folding_range_dispatch_preserves_invalid_params_instead_of_empty_success()
    -> Result<(), Box<dyn std::error::Error>> {
        if production_folding_range_tests_blocked_by_test_fallbacks() {
            return Ok(());
        }

        let server = initialized_folding_range_server();
        let response = request_folding_range(&server, 2, Some(json!({})))?;
        assert_folding_range_error(&response, INVALID_PARAMS, "textDocument.uri")?;

        let missing_params = request_folding_range(&server, 3, None)?;
        assert_folding_range_error(&missing_params, INVALID_PARAMS, "textDocument.uri")?;
        Ok(())
    }

    #[test]
    fn folding_range_dispatch_preserves_content_modified_instead_of_empty_success()
    -> Result<(), Box<dyn std::error::Error>> {
        if production_folding_range_tests_blocked_by_test_fallbacks() {
            return Ok(());
        }

        let server = initialized_folding_range_server();
        open_foldable_document(&server, "file:///stale-folds.pl")?;

        let response = request_folding_range(
            &server,
            2,
            Some(json!({
                "textDocument": {
                    "uri": "file:///stale-folds.pl",
                    "version": 1
                }
            })),
        )?;
        assert_folding_range_error(&response, CONTENT_MODIFIED, "Document changed")?;
        Ok(())
    }

    #[test]
    fn folding_range_dispatch_preserves_request_cancelled_instead_of_empty_success()
    -> Result<(), Box<dyn std::error::Error>> {
        if production_folding_range_tests_blocked_by_test_fallbacks() {
            return Ok(());
        }

        let server = initialized_folding_range_server();
        open_foldable_document(&server, "file:///cancelled-folds.pl")?;
        let _fault = server.inject_folding_range_test_fault(FoldingRangeTestFault::Cancelled);

        let response = request_folding_range(
            &server,
            2,
            Some(json!({ "textDocument": { "uri": "file:///cancelled-folds.pl" } })),
        )?;
        assert_folding_range_error(&response, REQUEST_CANCELLED, "cancelled")?;
        Ok(())
    }

    #[test]
    fn folding_range_dispatch_preserves_provider_failure_instead_of_empty_success()
    -> Result<(), Box<dyn std::error::Error>> {
        if production_folding_range_tests_blocked_by_test_fallbacks() {
            return Ok(());
        }

        let server = initialized_folding_range_server();
        open_foldable_document(&server, "file:///provider-failure-folds.pl")?;
        let _fault = server.inject_folding_range_test_fault(FoldingRangeTestFault::ProviderFailure);

        let response = request_folding_range(
            &server,
            2,
            Some(json!({ "textDocument": { "uri": "file:///provider-failure-folds.pl" } })),
        )?;
        assert_folding_range_error(&response, INTERNAL_ERROR, "provider failure")?;
        Ok(())
    }

    #[test]
    fn folding_range_dispatch_does_not_record_unavailable_analysis_as_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        if production_folding_range_tests_blocked_by_test_fallbacks() {
            return Ok(());
        }

        let server = initialized_folding_range_server();
        {
            let mut features = server.advertised_features.lock();
            features.folding_range = false;
        }
        let gated = request_folding_range(
            &server,
            2,
            Some(json!({ "textDocument": { "uri": "file:///unadvertised-folds.pl" } })),
        )?;
        assert_folding_range_error(&gated, METHOD_NOT_FOUND, "not advertised")?;

        {
            let mut features = server.advertised_features.lock();
            features.folding_range = true;
        }
        open_foldable_document(&server, "file:///unavailable-folds.pl")?;
        let _fault = server.inject_folding_range_test_fault(FoldingRangeTestFault::Unavailable);
        let unavailable = request_folding_range(
            &server,
            3,
            Some(json!({ "textDocument": { "uri": "file:///unavailable-folds.pl" } })),
        )?;
        assert_folding_range_error(&unavailable, REQUEST_FAILED, "unavailable")?;
        Ok(())
    }

    #[test]
    fn folding_range_dispatch_preserves_instrument_failure_instead_of_empty_success()
    -> Result<(), Box<dyn std::error::Error>> {
        if production_folding_range_tests_blocked_by_test_fallbacks() {
            return Ok(());
        }

        let server = initialized_folding_range_server();
        open_foldable_document(&server, "file:///instrument-failure-folds.pl")?;
        let _fault =
            server.inject_folding_range_test_fault(FoldingRangeTestFault::InstrumentFailure);

        let response = request_folding_range(
            &server,
            2,
            Some(json!({ "textDocument": { "uri": "file:///instrument-failure-folds.pl" } })),
        )?;
        assert_folding_range_error(&response, INTERNAL_ERROR, "instrument failure")?;
        Ok(())
    }

    fn ranged_violation(uri: &str, version: i32) -> Value {
        json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 1 }
                },
                "text": "x"
            }]
        })
    }

    fn malformed_outer_did_change(uri: &str, version: i32) -> Value {
        json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": null
        })
    }

    fn document_symbol_names(value: &Value) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let mut names = Vec::new();
        fn walk(value: &Value, names: &mut Vec<String>) {
            match value {
                Value::Array(items) => {
                    for item in items {
                        walk(item, names);
                    }
                }
                Value::Object(map) => {
                    if let Some(name) = map.get("name").and_then(Value::as_str) {
                        names.push(name.to_string());
                    }
                    if let Some(children) = map.get("children") {
                        walk(children, names);
                    }
                }
                _ => {}
            }
        }
        walk(value, &mut names);
        Ok(names)
    }

    #[test]
    fn document_symbol_fails_closed_after_rejected_change_and_recovers()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let ranged_uri = "file:///desync-document-symbol.pl";
        let predecessor = "sub pred_symbol { 1 }\n";
        let recovered = "sub recovered_symbol { 1 }\n";

        server.test_apply_did_open(ranged_uri, predecessor, 1)?;
        let live = server
            .handle_document_symbol(Some(json!({ "textDocument": { "uri": ranged_uri } })))?
            .ok_or("live documentSymbol must return a result")?;
        let live_names = document_symbol_names(&live)?;
        assert!(
            live_names.iter().any(|name| name == "pred_symbol"),
            "live documentSymbol must publish the current subroutine: {live}"
        );

        server.handle_did_change(Some(ranged_violation(ranged_uri, 2)))?;
        {
            let documents = server.documents_guard();
            let doc =
                server.get_document(&documents, ranged_uri).ok_or("desynchronized document")?;
            assert!(doc.full_sync_required());
            assert!(
                doc.text.contains("pred_symbol"),
                "predecessor text remains stored and must not be reused for outline answers"
            );
        }
        let desync = server
            .handle_document_symbol(Some(json!({ "textDocument": { "uri": ranged_uri } })))?
            .ok_or("desync documentSymbol must return a result")?;
        let desync_names = document_symbol_names(&desync)?;
        assert!(
            desync_names.is_empty(),
            "Full-sync unavailability must not publish predecessor outline symbols: {desync}"
        );

        server.test_apply_did_change(ranged_uri, recovered, 3)?;
        let restored = server
            .handle_document_symbol(Some(json!({ "textDocument": { "uri": ranged_uri } })))?
            .ok_or("recovered documentSymbol must return a result")?;
        let restored_names = document_symbol_names(&restored)?;
        assert!(
            restored_names.iter().any(|name| name == "recovered_symbol"),
            "accepted full replacement must restore current documentSymbol: {restored}"
        );
        assert!(
            !restored_names.iter().any(|name| name == "pred_symbol"),
            "recovered documentSymbol must not keep the predecessor name: {restored}"
        );

        let malformed_uri = "file:///malformed-outer-document-symbol.pl";
        server.test_apply_did_open(malformed_uri, predecessor, 1)?;
        let malformed_live = server
            .handle_document_symbol(Some(json!({ "textDocument": { "uri": malformed_uri } })))?
            .ok_or("malformed-path live documentSymbol must return a result")?;
        assert!(
            document_symbol_names(&malformed_live)?.iter().any(|name| name == "pred_symbol"),
            "malformed-path live documentSymbol must publish the current subroutine: {malformed_live}"
        );
        server.handle_did_change(Some(malformed_outer_did_change(malformed_uri, 2)))?;
        let malformed_desync = server
            .handle_document_symbol(Some(json!({ "textDocument": { "uri": malformed_uri } })))?
            .ok_or("malformed desync documentSymbol must return a result")?;
        assert!(
            document_symbol_names(&malformed_desync)?.is_empty(),
            "malformed-outer didChange must fail-close documentSymbol: {malformed_desync}"
        );
        server.test_apply_did_change(malformed_uri, recovered, 3)?;
        let malformed_restored = server
            .handle_document_symbol(Some(json!({ "textDocument": { "uri": malformed_uri } })))?
            .ok_or("malformed-path recovered documentSymbol must return a result")?;
        let malformed_restored_names = document_symbol_names(&malformed_restored)?;
        assert!(
            malformed_restored_names.iter().any(|name| name == "recovered_symbol"),
            "full replacement must recover documentSymbol after malformed-outer didChange: {malformed_restored}"
        );
        Ok(())
    }

    #[test]
    fn folding_range_fails_closed_after_ranged_did_change_and_recovers()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///desync-folding-range.pl";
        let predecessor = "print \"ok\\n\";\n__DATA__\nalpha\nbeta\n";
        let recovered = "print \"ok\\n\";\n__DATA__\ngamma\ndelta\n";

        server.test_apply_did_open(uri, predecessor, 1)?;
        let live = server
            .handle_folding_range(Some(json!({ "textDocument": { "uri": uri } })))?
            .ok_or("live foldingRange must return a result")?;
        let live_ranges = live.as_array().ok_or("live foldingRange must return an array")?;
        assert!(!live_ranges.is_empty(), "live foldingRange must publish current folds: {live}");

        server.handle_did_change(Some(ranged_violation(uri, 2)))?;
        {
            let documents = server.documents_guard();
            let doc = server.get_document(&documents, uri).ok_or("desynchronized document")?;
            assert!(doc.full_sync_required());
            assert!(
                doc.text.contains("__DATA__"),
                "predecessor text remains stored and must not be reused for folds"
            );
        }
        let desync = server
            .handle_folding_range(Some(json!({ "textDocument": { "uri": uri } })))?
            .ok_or("desync foldingRange must return a result")?;
        let desync_ranges = desync.as_array().ok_or("desync foldingRange must return an array")?;
        assert!(
            desync_ranges.is_empty(),
            "Full-sync unavailability must not publish predecessor folds: {desync}"
        );

        server.test_apply_did_change(uri, recovered, 3)?;
        let restored = server
            .handle_folding_range(Some(json!({ "textDocument": { "uri": uri } })))?
            .ok_or("recovered foldingRange must return a result")?;
        let restored_ranges =
            restored.as_array().ok_or("recovered foldingRange must return an array")?;
        assert!(
            !restored_ranges.is_empty(),
            "accepted full replacement must restore current foldingRange: {restored}"
        );
        Ok(())
    }
}
