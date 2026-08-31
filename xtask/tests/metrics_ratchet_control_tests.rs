use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

/// Subsystem evaluated by the `ux-scorecard --ratchet-check` route.
///
/// See `xtask/src/tasks/ux_scorecard.rs`, which loads
/// `.ci/metrics/baselines/editor_ux.json` and evaluates it fail-closed.
const EDITOR_UX_SUBSYSTEM: &str = "editor_ux";

/// Operator-facing guide the nightly job points at for missing-receipt behavior.
const RATCHET_GUIDE: &str = "docs/project/metrics/RATCHET.md";

struct MetricBaseline {
    subsystem: String,
    path: String,
    scorecard_ratchet: bool,
}

/// One checking command inside the `ci-metrics-ratchet` recipe, resolved to the
/// committed baseline subsystem it actually ratchets.
///
/// Recipe commands are resolved structurally rather than matched as literal
/// substrings of the whole `justfile`. A substring match cannot tell a real
/// check from the same words appearing in a comment or an unrelated recipe, and
/// it reports a subsystem as uncovered whenever its command shape differs —
/// which is exactly how `editor_ux` came to read as unguarded while being
/// checked on every run (#14175).
#[derive(Debug, PartialEq, Eq)]
enum RatchetRoute {
    /// `cargo run -p xtask -- metrics ratchet-check <subsystem>`
    Subsystem(String),
    /// `cargo run -p xtask -- ux-scorecard --format json --ratchet-check`
    EditorUx,
}

impl RatchetRoute {
    /// Committed baseline subsystem this route ratchets.
    fn subsystem(&self) -> &str {
        match self {
            RatchetRoute::Subsystem(name) => name,
            RatchetRoute::EditorUx => EDITOR_UX_SUBSYSTEM,
        }
    }

    /// Markers the ratchet guide must carry so operators can find this route's
    /// missing-receipt behavior documented.
    fn documentation_markers(&self) -> &'static [&'static str] {
        match self {
            RatchetRoute::Subsystem(_) => &["metrics ratchet-check"],
            RatchetRoute::EditorUx => &["ux-scorecard", "--ratchet-check"],
        }
    }
}

/// Extract the body of the `ci-metrics-ratchet` recipe from the `justfile`.
///
/// A `just` recipe body runs from its header to the first blank or unindented
/// line. Scoping to the recipe body keeps sibling recipes — notably
/// `ci-metrics-ratchet-check <subsystem>`, which carries a `{{subsystem}}`
/// placeholder — from being read as coverage.
fn ci_metrics_ratchet_body(justfile: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut lines = justfile.lines().skip_while(|line| line.trim_end() != "ci-metrics-ratchet:");
    lines.next().ok_or(
        "justfile no longer defines a `ci-metrics-ratchet` recipe; the nightly \
                scorecard ratchet job runs it by name",
    )?;

    Ok(lines
        .take_while(|line| !line.trim().is_empty() && line.starts_with([' ', '\t']))
        .map(|line| line.trim().to_string())
        .collect())
}

/// Resolve every checking command in the `ci-metrics-ratchet` recipe to the
/// baseline subsystem it covers.
///
/// Fail-closed: a command shape with no mapping is an error rather than a
/// silently uncovered subsystem, so adding a route to the recipe forces this
/// contract to be updated alongside it.
fn ci_metrics_ratchet_routes(justfile: &str) -> Result<Vec<RatchetRoute>, Box<dyn Error>> {
    let mut routes = Vec::new();

    for command in ci_metrics_ratchet_body(justfile)? {
        // `@echo` progress lines and comments check nothing.
        if command.starts_with('@') || command.starts_with('#') {
            continue;
        }

        if let Some((_, tail)) = command.split_once("metrics ratchet-check ") {
            let subsystem = tail.split_whitespace().next().ok_or_else(|| {
                format!("`metrics ratchet-check` names no subsystem: `{command}`")
            })?;
            routes.push(RatchetRoute::Subsystem(subsystem.to_string()));
        } else if command.contains("ux-scorecard") && command.contains("--ratchet-check") {
            routes.push(RatchetRoute::EditorUx);
        } else {
            return Err(format!(
                "unrecognised command in the `ci-metrics-ratchet` recipe: `{command}`. \
                 Every command in this recipe must resolve to the baseline subsystem it \
                 ratchets; add a `RatchetRoute` mapping for this shape rather than letting \
                 a subsystem read as covered without proof."
            )
            .into());
        }
    }

    Ok(routes)
}

fn project_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop();
    dir
}

fn committed_metric_baselines() -> Result<Vec<MetricBaseline>, Box<dyn std::error::Error>> {
    let root = project_root();
    let baseline_dir = root.join(".ci/metrics/baselines");
    let mut baselines = Vec::new();

    for entry in fs::read_dir(baseline_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("baseline path has non-utf8 file stem: {}", path.display()))?;
        let content = fs::read_to_string(&path)?;
        let value: Value = serde_json::from_str(&content)?;
        let scorecard_ratchet = value.get("subsystem").and_then(Value::as_str) == Some(stem)
            && value.get("measured_at").is_some()
            && value.get("floor_metrics").is_some();
        baselines.push(MetricBaseline {
            subsystem: stem.to_string(),
            path: format!(".ci/metrics/baselines/{stem}.json"),
            scorecard_ratchet,
        });
    }

    baselines.sort_by(|left, right| left.subsystem.cmp(&right.subsystem));
    Ok(baselines)
}

#[test]
fn ci_metrics_ratchet_recipe_checks_every_committed_baseline()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let justfile = fs::read_to_string(root.join("justfile"))?;

    let covered: BTreeSet<String> = ci_metrics_ratchet_routes(&justfile)?
        .iter()
        .map(|route| route.subsystem().to_string())
        .collect();
    let committed: BTreeSet<String> = committed_metric_baselines()?
        .into_iter()
        .filter(|baseline| baseline.scorecard_ratchet)
        .map(|baseline| baseline.subsystem)
        .collect();

    // Every committed scorecard baseline is actually ratcheted.
    for subsystem in &committed {
        assert!(
            covered.contains(subsystem),
            "just ci-metrics-ratchet must check committed baseline `{subsystem}`"
        );
    }

    // ...and nothing in the recipe ratchets a subsystem that is not a committed
    // scorecard baseline, which would fail at runtime on a missing baseline file.
    for subsystem in &covered {
        assert!(
            committed.contains(subsystem),
            "`just ci-metrics-ratchet` checks `{subsystem}`, which is not a committed \
             scorecard baseline in .ci/metrics/baselines/"
        );
    }

    Ok(())
}

#[test]
fn ratchet_guide_lists_every_committed_baseline() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let guide = fs::read_to_string(root.join(RATCHET_GUIDE))?;

    for baseline in committed_metric_baselines()? {
        assert!(guide.contains(&baseline.path), "RATCHET.md must list `{}`", baseline.path);
    }

    Ok(())
}

/// The nightly job must stay label-gated, use the shared recipe, and leave the
/// missing-receipt behavior of every route it runs documented where operators
/// read it.
///
/// This replaces an assertion that required the literal `bootstrap-safe` inside
/// `ci-nightly.yml`. That string has never appeared in that workflow: the
/// assertion, the workflow job, and the guide carrying the phrase all landed
/// together in `aafdaa0`, with the phrase written only into the guide, so the
/// assertion was red from its first run (#14175). Reinstating the literal would
/// also have documented a falsehood — the recipe's `editor_ux` route is
/// fail-closed on missing metrics, so the job as a whole is not bootstrap-safe.
#[test]
fn nightly_ratchet_job_is_label_gated_and_documents_missing_receipt_behavior()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workflow = fs::read_to_string(root.join(".github/workflows/ci-nightly.yml"))?;

    assert!(
        workflow.contains("scorecard-ratchet-check:"),
        "ci-nightly.yml must define the scorecard ratchet job"
    );
    assert!(
        workflow.contains("ci:metrics-ratchet"),
        "scorecard ratchet job must stay label-gated for PRs"
    );
    assert!(
        workflow.contains("just ci-metrics-ratchet"),
        "scorecard ratchet job must use the shared just recipe"
    );
    assert!(
        workflow.contains(RATCHET_GUIDE),
        "scorecard ratchet job must point operators at {RATCHET_GUIDE} for the \
         missing-receipt behavior of the routes it runs"
    );

    // Each route the recipe actually runs must be documented in that guide, so
    // adding a route to the recipe cannot leave its behavior undocumented.
    let justfile = fs::read_to_string(root.join("justfile"))?;
    let guide = fs::read_to_string(root.join(RATCHET_GUIDE))?;
    for route in ci_metrics_ratchet_routes(&justfile)? {
        for marker in route.documentation_markers() {
            assert!(
                guide.contains(marker),
                "{RATCHET_GUIDE} must document the `{}` route run by just \
                 ci-metrics-ratchet (missing marker `{marker}`)",
                route.subsystem()
            );
        }
    }

    Ok(())
}

/// The guide must not tell operators the nightly job passes with no receipts
/// while the recipe still runs the fail-closed `editor_ux` route.
///
/// `ux-scorecard --ratchet-check` computes its metrics directly and fails on
/// missing or non-finite instrumented floor metrics
/// (`xtask/src/tasks/ux_scorecard.rs`), unlike the receipt-backed
/// `metrics ratchet-check` route, which falls back to baseline values. An
/// operator who reads a blanket bootstrap-safety claim will mis-diagnose that
/// failure as infrastructure noise.
#[test]
fn ratchet_guide_scopes_bootstrap_safety_to_the_receipt_backed_route()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let justfile = fs::read_to_string(root.join("justfile"))?;
    let guide = fs::read_to_string(root.join(RATCHET_GUIDE))?;

    let runs_editor_ux =
        ci_metrics_ratchet_routes(&justfile)?.iter().any(|route| *route == RatchetRoute::EditorUx);
    if !runs_editor_ux {
        return Ok(());
    }

    assert!(
        guide.contains("fail-closed"),
        "{RATCHET_GUIDE} must record that the `{EDITOR_UX_SUBSYSTEM}` route is fail-closed \
         on missing metrics, because just ci-metrics-ratchet runs it"
    );

    Ok(())
}
