//! Discriminating ErrorClass matrix for the #8739 fixed-origin DAP slice.
//!
//! Oracle: #4979 / #8739 category assignments. Classification is by variant
//! (and delegated framed source), never by Display/Debug/message text.

use perl_dap::eval::ValidationError;
use perl_dap::peer_protocol::PeerFrameError;
use perl_dap::security::SecurityError;
use perl_dap::stack::{FixedOriginStackParseError, StackParseError};
use perl_dap::variables::{FixedOriginVariableParseError, VariableParseError};
use perl_lsp_rs_core::transport::framing::{FramingError, MAX_FRAME_SIZE};
use perl_parser_core::path_security::WorkspacePathError;
use perl_parser_core::{ErrorCategory, ErrorClass};
use perl_tdd_support::must_err;
use perl_test_must::must_some_with;
use std::error::Error as StdError;

#[expect(
    clippy::invalid_regex,
    reason = "fixture constructs a regex::Error so ErrorClass mapping can be asserted"
)]
fn sample_regex_error() -> regex::Error {
    must_err(regex::Regex::new("("))
}

fn expected_validation_class(error: &ValidationError) -> ErrorCategory {
    match error {
        ValidationError::DangerousOperation(_)
        | ValidationError::AssignmentOperator(_)
        | ValidationError::IncrementDecrement
        | ValidationError::Backticks
        | ValidationError::RegexMutation(_)
        | ValidationError::ContainsNewlines => ErrorCategory::UserError,
    }
}

fn expected_security_class(error: &SecurityError) -> ErrorCategory {
    match error {
        SecurityError::PathTraversalAttempt(_)
        | SecurityError::PathOutsideWorkspace(_)
        | SecurityError::SymlinkOutsideWorkspace(_)
        | SecurityError::InvalidPathCharacters
        | SecurityError::InvalidExpression => ErrorCategory::UserError,
        SecurityError::ExcessiveTimeout(_) => ErrorCategory::ResourceLimit,
    }
}

fn expected_peer_frame_class(error: &PeerFrameError) -> ErrorCategory {
    match error {
        PeerFrameError::Framing(inner) => inner.error_class(),
        PeerFrameError::Json(_) => ErrorCategory::Protocol,
    }
}

fn expected_fixed_origin_stack_class(error: &FixedOriginStackParseError<'_>) -> ErrorCategory {
    match error {
        FixedOriginStackParseError::RegexError(_) => ErrorCategory::Bug,
    }
}

fn expected_fixed_origin_variable_class(
    error: &FixedOriginVariableParseError<'_>,
) -> ErrorCategory {
    match error {
        FixedOriginVariableParseError::MaxDepthExceeded(_) => ErrorCategory::ResourceLimit,
        FixedOriginVariableParseError::RegexError(_) => ErrorCategory::Bug,
    }
}

fn validation_variants() -> [ValidationError; 6] {
    [
        ValidationError::DangerousOperation("eval".to_string()),
        ValidationError::AssignmentOperator("=".to_string()),
        ValidationError::IncrementDecrement,
        ValidationError::Backticks,
        ValidationError::RegexMutation("s///".to_string()),
        ValidationError::ContainsNewlines,
    ]
}

fn security_variants() -> [SecurityError; 6] {
    [
        SecurityError::PathTraversalAttempt("../etc/passwd".to_string()),
        SecurityError::PathOutsideWorkspace("/tmp/outside.pl".to_string()),
        SecurityError::SymlinkOutsideWorkspace("/tmp/link".to_string()),
        SecurityError::InvalidPathCharacters,
        SecurityError::InvalidExpression,
        SecurityError::ExcessiveTimeout(500_000),
    ]
}

fn peer_frame_variants() -> [PeerFrameError; 6] {
    [
        PeerFrameError::Framing(FramingError::InvalidHeader),
        PeerFrameError::Framing(FramingError::InvalidHeaderUtf8),
        PeerFrameError::Framing(FramingError::MissingContentLength),
        PeerFrameError::Framing(FramingError::InvalidContentLength),
        PeerFrameError::Framing(FramingError::FrameTooLarge { len: MAX_FRAME_SIZE + 1 }),
        PeerFrameError::Json("not a peer message".to_string()),
    ]
}

#[test]
fn validation_error_current_variant_matrix() {
    for error in validation_variants() {
        assert_eq!(error.error_class(), expected_validation_class(&error), "{error:?}");
    }
}

#[test]
fn security_error_current_variant_matrix() {
    for error in security_variants() {
        assert_eq!(error.error_class(), expected_security_class(&error), "{error:?}");
    }
}

#[test]
fn peer_frame_error_current_variant_matrix() {
    for error in peer_frame_variants() {
        assert_eq!(error.error_class(), expected_peer_frame_class(&error), "{error:?}");
    }
}

#[test]
fn wrapped_oversized_frame_retains_resource_limit() {
    let inner = FramingError::FrameTooLarge { len: MAX_FRAME_SIZE + 1 };
    let error = PeerFrameError::Framing(inner.clone());
    assert_eq!(inner.error_class(), ErrorCategory::ResourceLimit);
    assert_eq!(error.error_class(), ErrorCategory::ResourceLimit);
    assert_eq!(error.error_class(), inner.error_class());
}

#[test]
fn invalid_expression_and_path_remain_user_error() {
    assert_eq!(SecurityError::InvalidExpression.error_class(), ErrorCategory::UserError);
    assert_eq!(SecurityError::InvalidPathCharacters.error_class(), ErrorCategory::UserError);
    assert_eq!(
        SecurityError::PathTraversalAttempt("..".to_string()).error_class(),
        ErrorCategory::UserError
    );
    assert_eq!(ValidationError::ContainsNewlines.error_class(), ErrorCategory::UserError);
}

#[test]
fn excessive_timeout_is_resource_limit_not_user_error() {
    assert_eq!(
        SecurityError::ExcessiveTimeout(500_000).error_class(),
        ErrorCategory::ResourceLimit
    );
}

#[test]
fn internal_constant_regex_failure_is_adapter_bug() {
    let regex_error = sample_regex_error();
    let stack = StackParseError::RegexError(regex_error.clone());
    let variables = VariableParseError::RegexError(regex_error);

    let stack_origin = must_some_with(
        stack.as_fixed_origin(),
        "regex compile failure is a fixed-origin adapter bug",
    );
    let variable_origin = must_some_with(
        variables.as_fixed_origin(),
        "regex compile failure is a fixed-origin adapter bug",
    );

    assert_eq!(stack_origin.error_class(), ErrorCategory::Bug);
    assert_eq!(variable_origin.error_class(), ErrorCategory::Bug);
    assert_eq!(stack_origin.error_class(), expected_fixed_origin_stack_class(&stack_origin));
    assert_eq!(
        variable_origin.error_class(),
        expected_fixed_origin_variable_class(&variable_origin)
    );
}

#[test]
fn variable_max_depth_is_resource_limit() {
    let error = VariableParseError::MaxDepthExceeded(50);
    let origin =
        must_some_with(error.as_fixed_origin(), "max depth is a fixed-origin resource bound");
    assert_eq!(origin.error_class(), ErrorCategory::ResourceLimit);
    assert_eq!(origin.error_class(), expected_fixed_origin_variable_class(&origin));
}

#[test]
fn context_dependent_stack_format_is_not_classified() {
    let error = StackParseError::UnrecognizedFormat("$ = debuggee prose".to_string());
    assert!(
        error.as_fixed_origin().is_none(),
        "unrecognized stack text is not a fixed-origin variant; wrap with OriginatedParseInput"
    );
}

#[test]
fn context_dependent_variable_parse_variants_are_not_classified() {
    let variants = [
        VariableParseError::UnrecognizedFormat("$x = debuggee prose".to_string()),
        VariableParseError::UnterminatedString,
        VariableParseError::UnterminatedCollection,
    ];
    for error in variants {
        assert!(
            error.as_fixed_origin().is_none(),
            "context-dependent {error:?} is not a fixed-origin variant; wrap with OriginatedParseInput"
        );
    }
}

#[test]
fn classification_does_not_inspect_rendered_text() {
    let bait = "Bug Protocol ResourceLimit Infra Transient user_error advisory";
    assert_eq!(
        ValidationError::DangerousOperation(bait.to_string()).error_class(),
        ErrorCategory::UserError
    );
    assert_eq!(
        SecurityError::PathTraversalAttempt(bait.to_string()).error_class(),
        ErrorCategory::UserError
    );
    assert_eq!(PeerFrameError::Json(bait.to_string()).error_class(), ErrorCategory::Protocol);

    let stack = StackParseError::UnrecognizedFormat(bait.to_string());
    assert!(stack.as_fixed_origin().is_none());
    assert!(
        stack.to_string().contains(bait),
        "payload text is retained on Display without driving classification"
    );
}

#[test]
fn typed_sources_remain_available() {
    let framing = FramingError::FrameTooLarge { len: MAX_FRAME_SIZE + 1 };
    let peer = PeerFrameError::Framing(framing);
    let peer_source =
        must_some_with(StdError::source(&peer), "PeerFrameError::Framing keeps FramingError");
    assert!(peer_source.downcast_ref::<FramingError>().is_some());

    let mapped = SecurityError::from(WorkspacePathError::InvalidPathCharacters);
    assert!(matches!(mapped, SecurityError::InvalidPathCharacters));
    assert_eq!(mapped.error_class(), ErrorCategory::UserError);

    let regex_error = sample_regex_error();
    let stack = StackParseError::RegexError(regex_error.clone());
    let stack_source =
        must_some_with(StdError::source(&stack), "StackParseError::RegexError keeps regex::Error");
    assert!(stack_source.downcast_ref::<regex::Error>().is_some());

    let variables = VariableParseError::RegexError(regex_error);
    let variable_source = must_some_with(
        StdError::source(&variables),
        "VariableParseError::RegexError keeps regex::Error",
    );
    assert!(variable_source.downcast_ref::<regex::Error>().is_some());
}

#[test]
fn dap_wire_display_messages_are_unchanged() {
    assert_eq!(ValidationError::ContainsNewlines.to_string(), "Expression cannot contain newlines");
    assert_eq!(SecurityError::InvalidExpression.to_string(), "Expression cannot contain newlines");
    assert_eq!(
        SecurityError::ExcessiveTimeout(500_000).to_string(),
        "Timeout exceeds maximum allowed value: 500000ms"
    );
}
