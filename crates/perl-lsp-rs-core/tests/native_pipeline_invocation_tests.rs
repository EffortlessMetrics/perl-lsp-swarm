//! NPC-003 provider-facing native pipeline canaries (#10302).
//!
//! Keep these adapter tests with the provider owner so the formatter's own
//! test graph does not depend on the LSP core. The remaining counter, benchmark,
//! and receipt canaries stay in perl-lsp-perltidy.

#![deny(clippy::map_err_ignore)]

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use perl_lsp_perltidy::native::{FormatContext, FormatReasonCode, NativePipelineCounters};

// Exact statement/LF/tabs/four-unit fixture previously supplied by SubjectSpec.
// This provider proof needs the source, not the benchmark registry or receipts.
const STATEMENT_SOURCE: &str = concat!(
    "\tif ( $flag_00 ) { return 1; }\n",
    "\tif ( $other_00 ) { return 2; }\n",
    "\tif ( $flag_01 ) { return 1; }\n",
    "\tif ( $other_01 ) { return 2; }\n",
    "\tif ( $flag_02 ) { return 1; }\n",
    "\tif ( $other_02 ) { return 2; }\n",
    "\tif ( $flag_03 ) { return 1; }\n",
    "\tif ( $other_03 ) { return 2; }\n",
);

// ---------------------------------------------------------------------------
// NPC-003 — one pipeline invocation per LSP request at both provider seams
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct RecordingRuntime {
    invoked: Arc<AtomicBool>,
}

impl perl_lsp_rs_core::tooling::SubprocessRuntime for RecordingRuntime {
    fn run_command(
        &self,
        _program: &str,
        _args: &[&str],
        _stdin: Option<&[u8]>,
    ) -> Result<
        perl_lsp_rs_core::tooling::SubprocessOutput,
        perl_lsp_rs_core::tooling::SubprocessError,
    > {
        self.invoked.store(true, Ordering::SeqCst);
        Ok(perl_lsp_rs_core::tooling::SubprocessOutput {
            stdout: b"my $external = 1;\n".to_vec(),
            stderr: Vec::new(),
            status_code: 0,
        })
    }
}

fn formatting_options() -> perl_lsp_rs_core::providers::formatting::FormattingOptions {
    perl_lsp_rs_core::providers::formatting::FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    }
}

fn native_provider() -> perl_lsp_rs_core::providers::formatting::FormattingProvider<RecordingRuntime>
{
    perl_lsp_rs_core::providers::formatting::FormattingProvider::new(RecordingRuntime {
        invoked: Arc::new(AtomicBool::new(false)),
    })
}

#[test]
fn document_request_parses_exactly_once() -> Result<(), Box<dyn std::error::Error>> {
    let provider = native_provider();
    let options = formatting_options();
    let context = FormatContext::new(Some("npc-003/document".to_string()), Some(1));

    let statement = STATEMENT_SOURCE.to_string();
    for content in [statement, "my $x = ;\n".to_string()] {
        let mut counters = NativePipelineCounters::default();
        let counted = provider.format_document_decision_with_counters(
            &content,
            &options,
            &context,
            &mut counters,
        )?;
        assert_eq!(counters.pipeline_invocations, 1, "one pipeline per document request");
        let expected_gates =
            u64::from(counted.outcome.reason != FormatReasonCode::SourceParseError) + 1;
        assert_eq!(counters.parse_gate_invocations, expected_gates);
        assert_eq!(counters.source_parse_gate_invocations, 1);
        assert_eq!(
            counters.formatted_output_parse_gate_invocations,
            u64::from(counted.outcome.reason != FormatReasonCode::SourceParseError)
        );
        assert!(
            !counted.outcome.identity.config_fingerprint.is_empty(),
            "the seam must keep exact config identity"
        );

        // The counted seam shares the exact decision path with the plain seam.
        let plain = provider.format_document_decision(&content, &options, &context)?;
        assert_eq!(
            plain.outcome.disposition, counted.outcome.disposition,
            "the counters-aware seam must not fork the decision path"
        );
        assert_eq!(plain.outcome.reason, counted.outcome.reason);
    }
    Ok(())
}

#[test]
fn range_request_parses_exactly_once() -> Result<(), Box<dyn std::error::Error>> {
    let provider = native_provider();
    let options = formatting_options();
    let context = FormatContext::new(Some("npc-003/range".to_string()), Some(1));

    let statement = STATEMENT_SOURCE.to_string();
    for content in [statement, "my $x = ;\n".to_string()] {
        let range = perl_lsp_rs_core::providers::formatting::FormatRange::whole_document(&content);
        let mut counters = NativePipelineCounters::default();
        let counted = provider.format_range_decision_with_counters(
            &content,
            &range,
            &options,
            &context,
            &mut counters,
        )?;
        assert_eq!(counters.pipeline_invocations, 1, "one pipeline per range request");
        assert_eq!(counters.source_parse_gate_invocations, 1);
        assert_eq!(
            counters.formatted_output_parse_gate_invocations,
            u64::from(counted.outcome.reason != FormatReasonCode::SourceParseError)
        );

        let plain = provider.format_range_decision(&content, &range, &options, &context)?;
        assert_eq!(plain.outcome.disposition, counted.outcome.disposition);
        assert_eq!(plain.outcome.reason, counted.outcome.reason);
    }
    Ok(())
}
