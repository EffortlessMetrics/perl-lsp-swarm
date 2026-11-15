//! Framing facts and application category policy have distinct owners (#13996).

use perl_lsp_rs_core::protocol::framing_error_class::FRAMING_ERROR_ADAPTER;
use perl_lsp_rs_core::protocol::{ClassificationRoute, error_type_inventory};
use perl_lsp_rs_core::transport::{ContentLengthFramer, FramingError, frame};
use std::{error::Error, marker::PhantomData};

struct ErrorClassProbe<T>(PhantomData<T>);
trait ErrorClassProbeFallback {
    const IMPLEMENTS_ERROR_CLASS: bool = false;
}
impl<T> ErrorClassProbeFallback for ErrorClassProbe<T> {}
impl<T: perl_parser_core::ErrorClass> ErrorClassProbe<T> {
    const IMPLEMENTS_ERROR_CLASS: bool = true;
}
struct ClassifiedControl;
impl perl_parser_core::ErrorClass for ClassifiedControl {
    fn error_class(&self) -> perl_parser_core::ErrorCategory {
        perl_parser_core::ErrorCategory::Protocol
    }
}

const _: () = assert!(ErrorClassProbe::<ClassifiedControl>::IMPLEMENTS_ERROR_CLASS);
const _: () = assert!(
    !ErrorClassProbe::<FramingError>::IMPLEMENTS_ERROR_CLASS,
    "FramingError must not regain the parser taxonomy"
);

#[test]
fn legacy_paths_refer_to_one_framing_type() {
    let identity: fn(perl_lsp_rs_core::transport::framing::FramingError) -> FramingError =
        |error| error;
    assert_eq!(identity(FramingError::InvalidHeader), FramingError::InvalidHeader);
}

#[test]
fn every_variant_keeps_display_clone_equality_and_std_error() {
    for (error, message) in [
        (FramingError::InvalidHeader, "invalid Content-Length header"),
        (FramingError::InvalidHeaderUtf8, "header contains invalid UTF-8"),
        (FramingError::MissingContentLength, "missing Content-Length header"),
        (FramingError::InvalidContentLength, "invalid Content-Length value"),
        (FramingError::FrameTooLarge { len: 99 }, "frame too large: 99 bytes"),
    ] {
        assert_eq!(error.to_string(), message);
        assert_eq!(error.clone(), error);
        assert!(error.source().is_none());
    }
}

#[test]
fn inventory_names_the_application_projection_not_a_trait_impl() {
    let inventory = error_type_inventory();
    let rows: Vec<_> = inventory.iter().filter(|entry| entry.type_name == "FramingError").collect();
    assert_eq!(rows.len(), 1);
    for row in rows {
        assert!(!row.has_error_class);
        assert_eq!(
            row.classification_route,
            ClassificationRoute::AppAdapter(FRAMING_ERROR_ADAPTER)
        );
        assert_eq!(row.sample_category, Some(perl_parser_core::ErrorCategory::Protocol));
    }
}

#[test]
fn malformed_header_recovery_still_reaches_the_following_frame() {
    let mut wire = b"Content-Length: nope\r\n\r\n".to_vec();
    wire.extend(frame(b"next"));
    let mut framer = ContentLengthFramer::new();
    framer.push(&wire);
    assert!(matches!(framer.try_next(), Err(FramingError::InvalidContentLength)));
    assert_eq!(framer.try_next(), Ok(Some(b"next".to_vec())));
    assert_eq!(framer.try_next(), Ok(None));
}

#[test]
fn error_facts_have_one_definition_and_no_direct_application_import() {
    let facts = include_str!("../src/transport/framing_error.rs");
    let framer = include_str!("../src/transport/framing.rs");
    assert_eq!(facts.matches("pub enum FramingError").count(), 1);
    assert_eq!(framer.matches("pub enum FramingError").count(), 0);
    let code: String = facts
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in ["perl_parser_core", "ErrorClass", "ErrorCategory", "crate::protocol"] {
        assert!(!code.contains(forbidden), "framing facts acquired {forbidden}");
    }
}
