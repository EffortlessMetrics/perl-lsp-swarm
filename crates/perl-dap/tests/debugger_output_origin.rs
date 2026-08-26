//! Discriminating origin wrapper for stack/variable parser inputs (#8746).
//!
//! Oracle: #4979 PR 2 / #8746. Callers supply origin. Classification is by
//! origin plus variant, never by Display/Debug/message text. Fixed-origin
//! resource and regex categories remain stable across origins.

use perl_dap::parse_origin::{
    DebuggerOutputOrigin, OriginatedParseError, OriginatedParseInput, ParseIdentity,
};
use perl_dap::stack::{PerlStackParser, StackParseError};
use perl_dap::variables::{VariableParseError, VariableParser};
use perl_parser_core::{ErrorCategory, ErrorClass};
use perl_tdd_support::{must, must_err};
use std::error::Error as StdError;

const BAIT: &str = "DB<1> Bug Protocol ResourceLimit Infra Transient user_error advisory";
const ORDINARY_PROSE: &str = "Hello from the debuggee";
const UNRECOGNIZED_STACK: &str = "this is not a stack frame";
const UNRECOGNIZED_VARIABLE: &str = "not a $variable assignment";

fn all_origins() -> [DebuggerOutputOrigin; 4] {
    [
        DebuggerOutputOrigin::DebuggerControlPayload,
        DebuggerOutputOrigin::PeerProtocolPayload,
        DebuggerOutputOrigin::BestEffortDebuggeeOutput,
        DebuggerOutputOrigin::FixtureOrInstrumentInput,
    ]
}

fn sample_regex_error() -> regex::Error {
    must_err(regex::Regex::new("("))
}

fn wrap_stack(
    origin: DebuggerOutputOrigin,
    identity: ParseIdentity,
    error: StackParseError,
) -> OriginatedParseError<StackParseError> {
    OriginatedParseInput::new(origin, identity, "").attach(error)
}

fn wrap_variable(
    origin: DebuggerOutputOrigin,
    identity: ParseIdentity,
    error: VariableParseError,
) -> OriginatedParseError<VariableParseError> {
    OriginatedParseInput::new(origin, identity, "").attach(error)
}

fn parse_unrecognized_stack(
    origin: DebuggerOutputOrigin,
    text: &str,
) -> OriginatedParseError<StackParseError> {
    let mut parser = PerlStackParser::new();
    let input = OriginatedParseInput::new(origin, ParseIdentity::new(), text);
    must_err(parser.parse_frame_originated(input, 0))
}

fn parse_unrecognized_variable(
    origin: DebuggerOutputOrigin,
    text: &str,
) -> OriginatedParseError<VariableParseError> {
    let parser = VariableParser::new();
    let input = OriginatedParseInput::new(origin, ParseIdentity::new(), text);
    must_err(parser.parse_assignment_originated(input))
}

#[test]
fn identical_bytes_receive_different_dispositions_by_origin() {
    let debugger =
        parse_unrecognized_stack(DebuggerOutputOrigin::DebuggerControlPayload, ORDINARY_PROSE);
    let best_effort =
        parse_unrecognized_stack(DebuggerOutputOrigin::BestEffortDebuggeeOutput, ORDINARY_PROSE);

    assert_eq!(debugger.error_class(), ErrorCategory::Protocol);
    assert_eq!(best_effort.error_class(), ErrorCategory::Advisory);
    assert_ne!(
        debugger.error_class(),
        best_effort.error_class(),
        "identical bytes must not share a disposition across debugger-control and best-effort origins"
    );
}

#[test]
fn malformed_negotiated_stack_frame_is_protocol_failure() {
    for origin in
        [DebuggerOutputOrigin::DebuggerControlPayload, DebuggerOutputOrigin::PeerProtocolPayload]
    {
        let error = parse_unrecognized_stack(origin, UNRECOGNIZED_STACK);
        assert_eq!(error.error_class(), ErrorCategory::Protocol, "{origin:?}");
        assert!(matches!(error.parse_error(), StackParseError::UnrecognizedFormat(_)));
    }
}

#[test]
fn constructed_unterminated_variants_follow_origin_category() {
    // These variants are classified by origin when wrapped. parse_value does not
    // currently emit them for `$x = "abc` / `$x = [1,2` (see the parse-path
    // discriminator below). Changing that grammar is new parsing capability and
    // is out of scope for #8746.
    for origin in all_origins() {
        let expected = match origin {
            DebuggerOutputOrigin::DebuggerControlPayload
            | DebuggerOutputOrigin::PeerProtocolPayload => ErrorCategory::Protocol,
            DebuggerOutputOrigin::BestEffortDebuggeeOutput
            | DebuggerOutputOrigin::FixtureOrInstrumentInput => ErrorCategory::Advisory,
        };
        for variant in
            [VariableParseError::UnterminatedString, VariableParseError::UnterminatedCollection]
        {
            let error = wrap_variable(origin, ParseIdentity::new(), variant);
            assert_eq!(error.error_class(), expected, "{origin:?} {error:?}");
            assert!(error.parse_error().as_fixed_origin().is_none(), "{origin:?}");
        }
    }
}

#[test]
fn unterminated_looking_assignment_parses_as_scalar_not_unterminated_error() {
    let parser = VariableParser::new();
    for text in [r#"$x = "abc"#, "$x = [1,2"] {
        let input = OriginatedParseInput::new(
            DebuggerOutputOrigin::DebuggerControlPayload,
            ParseIdentity::new(),
            text,
        );
        let (name, value) = must(parser.parse_assignment_originated(input));
        assert_eq!(name, "$x", "{text}");
        assert!(
            matches!(value, perl_dap::value::PerlValue::Scalar(_)),
            "{text} must keep current parse_value fall-through; emitting Unterminated* here would be new parsing capability"
        );
    }
}

#[test]
fn best_effort_ordinary_program_output_is_advisory_not_bug() {
    let stack =
        parse_unrecognized_stack(DebuggerOutputOrigin::BestEffortDebuggeeOutput, ORDINARY_PROSE);
    let variable =
        parse_unrecognized_variable(DebuggerOutputOrigin::BestEffortDebuggeeOutput, ORDINARY_PROSE);

    assert_eq!(stack.error_class(), ErrorCategory::Advisory);
    assert_eq!(variable.error_class(), ErrorCategory::Advisory);
    assert_ne!(stack.error_class(), ErrorCategory::Bug);
    assert_ne!(variable.error_class(), ErrorCategory::Bug);
    assert_ne!(stack.error_class(), ErrorCategory::Protocol);
    assert_ne!(variable.error_class(), ErrorCategory::Protocol);
}

#[test]
fn fixture_unrecognized_input_is_advisory_not_protocol_or_bug() {
    let stack = parse_unrecognized_stack(
        DebuggerOutputOrigin::FixtureOrInstrumentInput,
        UNRECOGNIZED_STACK,
    );
    let variable = parse_unrecognized_variable(
        DebuggerOutputOrigin::FixtureOrInstrumentInput,
        UNRECOGNIZED_VARIABLE,
    );

    assert_eq!(stack.error_class(), ErrorCategory::Advisory);
    assert_eq!(variable.error_class(), ErrorCategory::Advisory);
    assert_ne!(stack.error_class(), ErrorCategory::Protocol);
    assert_ne!(stack.error_class(), ErrorCategory::Bug);
    assert_ne!(variable.error_class(), ErrorCategory::Protocol);
    assert_ne!(variable.error_class(), ErrorCategory::Bug);
}

#[test]
fn resource_bounds_stay_resource_limit_across_origins() {
    let parser = VariableParser::new().with_max_depth(1);
    for origin in all_origins() {
        let input = OriginatedParseInput::new(origin, ParseIdentity::new(), "(((1)))");
        let error = must_err(parser.parse_value_originated(input, 0));
        assert!(matches!(error.parse_error(), VariableParseError::MaxDepthExceeded(1)));
        assert_eq!(error.error_class(), ErrorCategory::ResourceLimit, "{origin:?}");
    }
}

#[test]
fn internal_regex_failure_stays_adapter_bug_across_origins() {
    let regex_error = sample_regex_error();
    for origin in all_origins() {
        let stack = wrap_stack(
            origin,
            ParseIdentity::new(),
            StackParseError::RegexError(regex_error.clone()),
        );
        let variable = wrap_variable(
            origin,
            ParseIdentity::new(),
            VariableParseError::RegexError(regex_error.clone()),
        );
        assert_eq!(stack.error_class(), ErrorCategory::Bug, "{origin:?}");
        assert_eq!(variable.error_class(), ErrorCategory::Bug, "{origin:?}");
    }
}

#[test]
fn typed_source_chain_survives_wrapping() {
    let identity =
        ParseIdentity::new().with_operation_id(7).with_session_id(3).with_suspension_generation(11);
    let mut parser = PerlStackParser::new();
    let input = OriginatedParseInput::new(
        DebuggerOutputOrigin::DebuggerControlPayload,
        identity,
        UNRECOGNIZED_STACK,
    );
    let error = must_err(parser.parse_frame_originated(input, 0));

    assert_eq!(error.origin(), DebuggerOutputOrigin::DebuggerControlPayload);
    assert_eq!(error.identity(), identity);
    assert_eq!(error.identity().operation_id(), Some(7));
    assert_eq!(error.identity().session_id(), Some(3));
    assert_eq!(error.identity().suspension_generation(), Some(11));

    let source = StdError::source(&error).expect("originated error keeps the parse source");
    assert!(source.downcast_ref::<StackParseError>().is_some());
    assert!(error.to_string().contains(UNRECOGNIZED_STACK));
}

#[test]
fn parsers_do_not_infer_origin_from_bait_text() {
    let stack = parse_unrecognized_stack(DebuggerOutputOrigin::BestEffortDebuggeeOutput, BAIT);
    let variable =
        parse_unrecognized_variable(DebuggerOutputOrigin::BestEffortDebuggeeOutput, BAIT);

    assert_eq!(stack.origin(), DebuggerOutputOrigin::BestEffortDebuggeeOutput);
    assert_eq!(variable.origin(), DebuggerOutputOrigin::BestEffortDebuggeeOutput);
    assert_eq!(stack.error_class(), ErrorCategory::Advisory);
    assert_eq!(variable.error_class(), ErrorCategory::Advisory);
    assert!(stack.to_string().contains(BAIT));
    assert!(variable.to_string().contains(BAIT));
}

#[test]
fn successful_parse_does_not_depend_on_origin() {
    let line = "  #0  main::foo at script.pl line 10";
    let mut parser = PerlStackParser::new();
    for origin in all_origins() {
        let input = OriginatedParseInput::new(origin, ParseIdentity::new(), line);
        let frame = must(parser.parse_frame_originated(input, 0));
        assert_eq!(frame.name, "main::foo");
        assert_eq!(frame.line, 10);
    }

    let assignment = "$x = 42";
    let variables = VariableParser::new();
    for origin in all_origins() {
        let input = OriginatedParseInput::new(origin, ParseIdentity::new(), assignment);
        let (name, value) = must(variables.parse_assignment_originated(input));
        assert_eq!(name, "$x");
        assert!(matches!(value, perl_dap::value::PerlValue::Integer(42)));
    }
}

#[test]
fn originated_stack_trace_skips_unrecognized_lines_without_changing_success_set() {
    let output = format!("{ORDINARY_PROSE}\n  #0  main::foo at script.pl line 10\n{BAIT}");
    let mut raw = PerlStackParser::new();
    let mut originated = PerlStackParser::new();
    let expected = raw.parse_stack_trace(&output);
    let input = OriginatedParseInput::new(
        DebuggerOutputOrigin::DebuggerControlPayload,
        ParseIdentity::new(),
        &output,
    );
    let frames = originated.parse_stack_trace_originated(input);
    assert_eq!(frames.len(), expected.len());
    assert_eq!(frames.first().map(|frame| frame.name.as_str()), Some("main::foo"));
}

#[test]
fn negative_request_seq_does_not_invent_an_operation_id() {
    let identity = ParseIdentity::new().with_operation_id_from_i64(-1);
    assert_eq!(identity.operation_id(), None);
    let identity = ParseIdentity::new().with_operation_id_from_i64(9);
    assert_eq!(identity.operation_id(), Some(9));
}

#[test]
fn originated_variable_dump_skips_unrecognized_lines_without_changing_success_set() {
    let output = format!("{ORDINARY_PROSE}\n$x = 42\n{BAIT}");
    let parser = VariableParser::new();
    let expected = parser.parse_variables(&output);
    let input = OriginatedParseInput::new(
        DebuggerOutputOrigin::DebuggerControlPayload,
        ParseIdentity::new(),
        &output,
    );
    let parsed = parser.parse_variables_originated(input);
    assert_eq!(parsed.len(), expected.len());
    assert_eq!(parsed.first().map(|(name, _)| name.as_str()), Some("$x"));
}

#[test]
fn origin_tokens_are_stable_and_exhaustive() {
    assert_eq!(DebuggerOutputOrigin::DebuggerControlPayload.as_str(), "debugger_control_payload");
    assert_eq!(DebuggerOutputOrigin::PeerProtocolPayload.as_str(), "peer_protocol_payload");
    assert_eq!(
        DebuggerOutputOrigin::BestEffortDebuggeeOutput.as_str(),
        "best_effort_debuggee_output"
    );
    assert_eq!(
        DebuggerOutputOrigin::FixtureOrInstrumentInput.as_str(),
        "fixture_or_instrument_input"
    );
}

#[test]
fn peer_protocol_malformed_payload_matches_debugger_control_category() {
    let debugger = parse_unrecognized_variable(
        DebuggerOutputOrigin::DebuggerControlPayload,
        UNRECOGNIZED_VARIABLE,
    );
    let peer = parse_unrecognized_variable(
        DebuggerOutputOrigin::PeerProtocolPayload,
        UNRECOGNIZED_VARIABLE,
    );
    assert_eq!(debugger.error_class(), ErrorCategory::Protocol);
    assert_eq!(peer.error_class(), debugger.error_class());
    assert_ne!(debugger.origin(), peer.origin());
}
