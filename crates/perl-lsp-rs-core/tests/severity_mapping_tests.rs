//! Unit tests for perlcritic Severity to LSP DiagnosticSeverity mapping.
//!
//! This test suite validates that Perl::Critic severity levels map correctly to LSP diagnostic severity:
//! - Perl::Critic uses 1 (Brutal/strictest) to 5 (Gentle/most lenient)
//! - LSP DiagnosticSeverity uses 1 (Error/most severe) to 4 (Hint/least severe)
//!
//! The mapping must be inverted: high Perl::Critic values (lenient) should map to low LSP values (least severe).

use lsp_types::DiagnosticSeverity;
use perl_lsp_rs_core::tooling::perl_critic::Severity;

#[test]
fn test_gentle_maps_to_hint() {
    // Gentle is severity 5 (most lenient) and should map to HINT (least severe)
    assert_eq!(
        Severity::Gentle.to_diagnostic_severity(),
        DiagnosticSeverity::HINT,
        "Gentle (severity 5, most lenient) should map to HINT (least severe)"
    );
}

#[test]
fn test_stern_maps_to_information() {
    // Stern is severity 4 and should map to INFORMATION
    assert_eq!(
        Severity::Stern.to_diagnostic_severity(),
        DiagnosticSeverity::INFORMATION,
        "Stern (severity 4) should map to INFORMATION"
    );
}

#[test]
fn test_harsh_maps_to_information() {
    // Harsh is severity 3 and should map to INFORMATION
    assert_eq!(
        Severity::Harsh.to_diagnostic_severity(),
        DiagnosticSeverity::INFORMATION,
        "Harsh (severity 3) should map to INFORMATION"
    );
}

#[test]
fn test_cruel_maps_to_warning() {
    // Cruel is severity 2 and should map to WARNING
    assert_eq!(
        Severity::Cruel.to_diagnostic_severity(),
        DiagnosticSeverity::WARNING,
        "Cruel (severity 2) should map to WARNING"
    );
}

#[test]
fn test_brutal_maps_to_error() {
    // Brutal is severity 1 (most strict/severe) and should map to ERROR (most severe)
    assert_eq!(
        Severity::Brutal.to_diagnostic_severity(),
        DiagnosticSeverity::ERROR,
        "Brutal (severity 1, most strict) should map to ERROR (most severe)"
    );
}

#[test]
fn test_severity_mapping_monotonic_order() {
    // Verify that the mapping preserves the severity order:
    // ERROR > WARNING > INFORMATION > HINT (LSP order)
    // should correspond to Brutal > Cruel/Stern/Harsh > Gentle (Perl::Critic order)

    let brutal = Severity::Brutal.to_diagnostic_severity();
    let cruel = Severity::Cruel.to_diagnostic_severity();
    let stern = Severity::Stern.to_diagnostic_severity();
    let harsh = Severity::Harsh.to_diagnostic_severity();
    let gentle = Severity::Gentle.to_diagnostic_severity();

    // Convert LSP severity to u32 for comparison (lower value = more severe)
    let brutal_val = brutal as u32;
    let cruel_val = cruel as u32;
    let stern_val = stern as u32;
    let harsh_val = harsh as u32;
    let gentle_val = gentle as u32;

    // Brutal should be most severe (ERROR = 1)
    assert_eq!(brutal_val, 1, "Brutal should map to ERROR (value 1)");

    // Cruel should be more severe than Stern/Harsh
    assert!(cruel_val < stern_val, "Cruel should be more severe than Stern");
    assert!(cruel_val < harsh_val, "Cruel should be more severe than Harsh");

    // Stern and Harsh should have the same severity
    assert_eq!(stern_val, harsh_val, "Stern and Harsh should map to same severity");

    // Gentle should be least severe (HINT = 4)
    assert_eq!(gentle_val, 4, "Gentle should map to HINT (value 4)");

    // Overall ordering
    assert!(brutal_val < cruel_val, "Brutal should be more severe than Cruel");
    assert!(cruel_val < stern_val, "Cruel should be more severe than Stern");
    assert!(stern_val < gentle_val, "Stern should be more severe than Gentle");
}

#[test]
fn test_severity_from_number_round_trip() {
    // Verify that from_number and to_diagnostic_severity work together correctly

    // Severity 1 (Brutal) -> ERROR
    let brutal = Severity::from_number(1);
    assert_eq!(
        brutal.to_diagnostic_severity(),
        DiagnosticSeverity::ERROR,
        "from_number(1) -> Brutal -> ERROR"
    );

    // Severity 2 (Cruel) -> WARNING
    let cruel = Severity::from_number(2);
    assert_eq!(
        cruel.to_diagnostic_severity(),
        DiagnosticSeverity::WARNING,
        "from_number(2) -> Cruel -> WARNING"
    );

    // Severity 3 (Harsh) -> INFORMATION
    let harsh = Severity::from_number(3);
    assert_eq!(
        harsh.to_diagnostic_severity(),
        DiagnosticSeverity::INFORMATION,
        "from_number(3) -> Harsh -> INFORMATION"
    );

    // Severity 4 (Stern) -> INFORMATION
    let stern = Severity::from_number(4);
    assert_eq!(
        stern.to_diagnostic_severity(),
        DiagnosticSeverity::INFORMATION,
        "from_number(4) -> Stern -> INFORMATION"
    );

    // Severity 5 (Gentle) -> HINT
    let gentle = Severity::from_number(5);
    assert_eq!(
        gentle.to_diagnostic_severity(),
        DiagnosticSeverity::HINT,
        "from_number(5) -> Gentle -> HINT"
    );
}
