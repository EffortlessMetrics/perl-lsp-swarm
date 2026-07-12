use crate::DeadCodeAnalysis;

pub fn generate_report(analysis: &DeadCodeAnalysis) -> String {
    let mut report = String::new();

    report.push_str("=== Dead Code Analysis Report ===\n\n");
    report.push_str(&format!("Files analyzed: {}\n", analysis.files_analyzed));
    report.push_str(&format!("Total lines: {}\n", analysis.total_lines));
    report.push_str(&format!("Dead code items: {}\n\n", analysis.dead_code.len()));

    report.push_str("Statistics:\n");
    report.push_str(&format!("  Unused subroutines: {}\n", analysis.stats.unused_subroutines));
    report.push_str(&format!("  Unused variables: {}\n", analysis.stats.unused_variables));
    report.push_str(&format!("  Unused constants: {}\n", analysis.stats.unused_constants));
    report.push_str(&format!("  Unused packages: {}\n", analysis.stats.unused_packages));
    report.push_str(&format!(
        "  Unreachable statements: {}\n",
        analysis.stats.unreachable_statements
    ));
    report.push_str(&format!("  Dead branches: {}\n", analysis.stats.dead_branches));
    report.push_str(&format!("  Total dead lines: {}\n", analysis.stats.total_dead_lines));

    report
}

#[cfg(test)]
mod tests {
    use super::generate_report;
    use crate::{DeadCodeAnalysis, DeadCodeStats};

    #[test]
    fn generate_report_includes_all_stat_counters() {
        // Regression guard (M7, #3849 receipt-instrument proof commit):
        // every DeadCodeStats counter must actually appear in the rendered
        // report, with its real value -- not a hardcoded placeholder.
        let analysis = DeadCodeAnalysis {
            dead_code: Vec::new(),
            stats: DeadCodeStats {
                unused_subroutines: 3,
                unused_variables: 5,
                unused_constants: 1,
                unused_packages: 2,
                unreachable_statements: 4,
                dead_branches: 6,
                total_dead_lines: 42,
            },
            files_analyzed: 7,
            total_lines: 900,
        };

        let report = generate_report(&analysis);

        assert!(report.contains("Files analyzed: 7"));
        assert!(report.contains("Total lines: 900"));
        assert!(report.contains("Unused subroutines: 3"));
        assert!(report.contains("Unused variables: 5"));
        assert!(report.contains("Unused constants: 1"));
        assert!(report.contains("Unused packages: 2"));
        assert!(report.contains("Unreachable statements: 4"));
        assert!(report.contains("Dead branches: 6"));
        assert!(report.contains("Total dead lines: 42"));
    }
}
