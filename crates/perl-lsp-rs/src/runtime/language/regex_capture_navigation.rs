//! Wire canonical regex capture navigation into live LSP handlers (#7030).

use perl_lsp_rs_core::providers::regex::{
    RegexCaptureDefinitionLink, SourceByteSpan, definition_links_at, reference_spans_at,
};
use perl_parser_core::RegexAnalysisTable;
use serde_json::{Value, json};

use super::super::LspServer;
use crate::state::DocumentState;

impl LspServer {
    pub(super) fn regex_capture_definitions(
        &self,
        uri: &str,
        doc: &DocumentState,
        table: &RegexAnalysisTable,
        offset: usize,
    ) -> Option<Value> {
        let links = definition_links_at(&doc.text, table, offset)?;
        if self.client_capabilities.lock().definition_link_support {
            let result: Vec<Value> = links
                .iter()
                .map(|link| self.regex_capture_definition_link_json(uri, doc, link))
                .collect();
            return Some(Value::Array(result));
        }
        if links.len() == 1 {
            return Some(self.regex_capture_location_json(uri, doc, links[0].target_selection));
        }
        Some(Value::Array(
            links
                .iter()
                .map(|link| self.regex_capture_location_json(uri, doc, link.target_selection))
                .collect(),
        ))
    }

    pub(super) fn regex_capture_references(
        &self,
        uri: &str,
        doc: &DocumentState,
        table: &RegexAnalysisTable,
        offset: usize,
        include_declaration: bool,
    ) -> Option<Vec<Value>> {
        let spans = reference_spans_at(&doc.text, table, offset, include_declaration)?;
        if spans.is_empty() {
            return None;
        }
        Some(spans.iter().map(|span| self.regex_capture_location_json(uri, doc, *span)).collect())
    }

    fn regex_capture_definition_link_json(
        &self,
        uri: &str,
        doc: &DocumentState,
        link: &RegexCaptureDefinitionLink,
    ) -> Value {
        let (origin_start, origin_end) = self.regex_capture_range_json(doc, link.origin_selection);
        let (target_start, target_end) = self.regex_capture_range_json(doc, link.target_range);
        let (selection_start, selection_end) =
            self.regex_capture_range_json(doc, link.target_selection);
        json!({
            "originSelectionRange": {
                "start": origin_start,
                "end": origin_end,
            },
            "targetUri": uri,
            "targetRange": {
                "start": target_start,
                "end": target_end,
            },
            "targetSelectionRange": {
                "start": selection_start,
                "end": selection_end,
            },
        })
    }

    fn regex_capture_location_json(
        &self,
        uri: &str,
        doc: &DocumentState,
        span: SourceByteSpan,
    ) -> Value {
        let (start, end) = self.regex_capture_range_json(doc, span);
        json!({
            "uri": uri,
            "range": {
                "start": start,
                "end": end,
            }
        })
    }

    fn regex_capture_range_json(
        &self,
        doc: &DocumentState,
        span: SourceByteSpan,
    ) -> (Value, Value) {
        let text_len = doc.text.len();
        let clamp = |value: usize| value.min(text_len);
        let (start_line, start_char) = self.offset_to_pos16(doc, clamp(span.start));
        let (end_line, end_char) = self.offset_to_pos16(doc, clamp(span.end));
        (
            json!({ "line": start_line, "character": start_char }),
            json!({ "line": end_line, "character": end_char }),
        )
    }
}
