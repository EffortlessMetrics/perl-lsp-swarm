use perl_regex::validator::RegexValidator;

#[test]
fn interpolation_marks_batch_analysis_dynamic() {
    let analysis = RegexValidator::new().analyze("$runtime");

    assert!(analysis.completeness.has_dynamic_boundary());
    assert!(!analysis.completeness.is_complete());
}
