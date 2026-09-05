//! Integration proof for the parser-independent metamorphic byte substrate.

use std::error::Error;

use perl_lsp_rs_core::hashing::sha256_hex;

#[allow(dead_code)]
#[path = "../src/tasks/metrics/parser_accuracy_metamorphic_transform.rs"]
mod transform;

use transform::{
    ByteRange, ContentAddressedSource, ExactEdit, PositionRelation, RangeRelation, TransformError,
    apply_exact_edits,
};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn exact_bytes_drive_the_public_map_surface() -> TestResult {
    let source = ContentAddressedSource::from_bytes(b"abc\r\n".to_vec())?;
    let transformed = apply_exact_edits(
        &source,
        "trailing-horizontal-whitespace.v1",
        vec![ExactEdit::new("line-0-space".to_owned(), 3, 3, Vec::new(), b" ".to_vec())],
    )?;

    assert_eq!(transformed.final_bytes, b"abc \r\n");
    assert_eq!(transformed.coordinate_map.base_len(), 5);
    assert_eq!(transformed.coordinate_map.transformed_len(), 6);
    assert_eq!(
        transformed.coordinate_map.map_base_position(3),
        PositionRelation::Ambiguous { lower: 3, upper: 4 }
    );
    assert_eq!(
        transformed.coordinate_map.map_base_range(ByteRange::new(3, 5)),
        RangeRelation::Mapped { range: ByteRange::new(4, 6) }
    );

    Ok(())
}

#[test]
fn stale_subject_identity_is_typed_and_rejected_at_construction() {
    let result = ContentAddressedSource::from_claimed("sha256:stale".to_owned(), b"abc".to_vec());
    assert!(matches!(
        result,
        Err(TransformError::StaleSourceIdentity { claimed, observed })
            if claimed == "sha256:stale" && observed == sha256_hex(b"abc")
    ));
}
