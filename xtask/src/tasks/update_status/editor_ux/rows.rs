//! Receipt row rendering for the editor UX scorecard's metrics and workflows.

use crate::tasks::metrics::lsp_stats::{
    LatencyMetric, MeasuredEditorUxScorecard, RateMetric, WorkflowResult,
};

pub(super) fn top_line_metric_rows(
    scorecard: Option<&MeasuredEditorUxScorecard>,
) -> Vec<serde_json::Value> {
    let Some(scorecard) = scorecard else {
        return vec![
            planned_metric_row("workflow_pass_rate"),
            planned_metric_row("workflow_stability_rate"),
            planned_metric_row("p95_time_to_first_useful_result_ms"),
        ];
    };

    vec![
        rate_metric_row("workflow_pass_rate", &scorecard.top_line.workflow_pass_rate),
        rate_metric_row("workflow_stability_rate", &scorecard.top_line.workflow_stability_rate),
        latency_metric_row(
            "p95_time_to_first_useful_result_ms",
            &scorecard.top_line.p95_time_to_first_useful_result_ms,
        ),
    ]
}

pub(super) fn workflow_rows(
    scorecard: Option<&MeasuredEditorUxScorecard>,
) -> Vec<serde_json::Value> {
    scorecard
        .map(|scorecard| scorecard.workflows.iter().map(workflow_row).collect())
        .unwrap_or_default()
}

fn planned_metric_row(name: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "state": "planned",
        "owner": "perl-lsp-ux-tests",
    })
}

fn rate_metric_row(name: &str, metric: &RateMetric) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "state": metric.state,
        "value": metric.value,
        "basis": metric.basis,
        "coverage": metric.coverage,
        "confidence": metric.confidence,
        "assumptions": metric.assumptions,
    })
}

fn latency_metric_row(name: &str, metric: &LatencyMetric) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "state": metric.state,
        "value_ms": metric.value,
        "basis": metric.basis,
        "coverage": metric.coverage,
        "confidence": metric.confidence,
        "method": metric.method,
        "assumptions": metric.assumptions,
    })
}

fn workflow_row(workflow: &WorkflowResult) -> serde_json::Value {
    serde_json::json!({
        "id": workflow.id,
        "scenario": workflow.scenario,
        "subsystem_owner": workflow.subsystem_owner,
        "pass_rate_state": workflow.pass_rate.state,
        "stability_rate_state": workflow.stability_rate.state,
        "p95_time_to_first_useful_result_state": workflow.p95_time_to_first_useful_result_ms.state,
        "quarantine_age_days": workflow.quarantine_age_days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_scorecard_renders_planned_top_line_rows_and_no_workflows() {
        let rows = top_line_metric_rows(None);
        let names: Vec<&str> = rows.iter().filter_map(|row| row["name"].as_str()).collect();
        assert_eq!(
            names,
            vec![
                "workflow_pass_rate",
                "workflow_stability_rate",
                "p95_time_to_first_useful_result_ms",
            ]
        );
        for row in &rows {
            assert_eq!(row["state"], "planned");
        }
        assert!(workflow_rows(None).is_empty());
    }
}
