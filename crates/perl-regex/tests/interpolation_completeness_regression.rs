#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use perl_regex::validator::RegexValidator;

#[test]
fn interpolation_marks_batch_analysis_dynamic() {
    let analysis = RegexValidator::new().analyze("$runtime");

    assert!(analysis.completeness.has_dynamic_boundary());
    assert!(!analysis.completeness.is_complete());
}
