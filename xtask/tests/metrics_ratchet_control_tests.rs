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

    /// Command text identifying this route in the ratchet guide.
    fn documented_command(&self) -> &'static str {
        match self {
            RatchetRoute::Subsystem(_) => "metrics ratchet-check",
            RatchetRoute::EditorUx => "ux-scorecard",
        }
    }

    /// Disposition the guide must state for this route, on the same line as the
    /// command, so the two stay bound together.
    ///
    /// Whole-file token checks are not enough: `metrics ratchet-check` and
    /// `ux-scorecard` both appear in the guide's command examples, so a check
    /// for their mere presence stays green even if every explanation of what
    /// the route does on missing metrics is deleted. Requiring command and
    /// disposition to co-occur is what makes this pin behavior rather than
    /// vocabulary.
    fn documented_dispositions(&self) -> &'static [&'static str] {
        match self {
            RatchetRoute::Subsystem(_) => &["bootstrap-safe", "bootstrap safe"],
            RatchetRoute::EditorUx => &["fail-closed", "fail closed"],
        }
    }
}

/// Extract the logical command lines of the `ci-metrics-ratchet` recipe body.
///
/// A `just` recipe body runs from its header to the first non-blank *unindented*
/// line. A blank line does **not** end the recipe — verified against `just`
/// 1.21.0, whose `--dump --dump-format json` keeps lines after an interior blank
/// in the same recipe body. Treating a blank line as the end would silently drop
/// every route after it, which is the same class of hole this contract exists to
/// close, so blanks are skipped rather than used as a terminator.
///
/// Scoping to the recipe body keeps sibling recipes — notably
/// `ci-metrics-ratchet-check <subsystem>`, which carries a `{{subsystem}}`
/// placeholder — from being read as coverage. Trailing `\` continuations are
/// joined so a command wrapped for readability stays one logical command.
///
/// Known limitation: command text is matched literally, so shell quoting is not
/// interpreted. A non-`@` `echo "... metrics ratchet-check foo ..."` would be
/// read as a route. That fails loudly rather than silently under-reporting
/// coverage, and no such line exists in this recipe.
fn ci_metrics_ratchet_body(justfile: &str) -> Result<Vec<String>, Box<dyn Error>> {
    // Accept a header that later gains dependencies (`ci-metrics-ratchet: dep`).
    // `ci-metrics-ratchet-check` does not match, since it lacks the `:` here.
    let mut lines =
        justfile.lines().skip_while(|line| !line.trim_end().starts_with("ci-metrics-ratchet:"));
    lines.next().ok_or(
        "justfile no longer defines a `ci-metrics-ratchet` recipe; the nightly \
                scorecard ratchet job runs it by name",
    )?;

    let mut commands: Vec<String> = Vec::new();
    let mut continued = false;
    for line in lines.take_while(|line| line.trim().is_empty() || line.starts_with([' ', '\t'])) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let (text, continues) = match trimmed.strip_suffix('\\') {
            Some(head) => (head.trim_end(), true),
            None => (trimmed, false),
        };

        match commands.last_mut() {
            Some(previous) if continued => {
                previous.push(' ');
                previous.push_str(text);
            }
            _ => commands.push(text.to_string()),
        }
        continued = continues;
    }

    Ok(commands)
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
        // `just` line prefixes: `@` suppresses echo, `-` ignores failure, `+`
        // is the shebang-recipe form. Strip them before reading the command —
        // `@cargo run …` is a real invocation, not a progress line.
        let command = command.trim_start_matches(['@', '-', '+']).trim();

        // Progress lines and comments check nothing.
        if command.starts_with("echo ") || command.starts_with('#') || command.is_empty() {
            continue;
        }

        // A route only counts if the recipe actually runs it and lets it fail.
        // `echo cargo run … ratchet-check parser` or a `|| true` suffix leaves
        // the substring intact while the floor stops being enforced, so a
        // neutered command must not read as coverage.
        if let Some(suppressor) =
            ["|| true", "|| :", "|| exit 0", "||true"].iter().find(|s| command.contains(**s))
        {
            return Err(format!(
                "`ci-metrics-ratchet` suppresses the failure of `{command}` with `{suppressor}`. \
                 A ratchet route that cannot fail does not enforce its floor; remove the \
                 suppressor or drop the route rather than leaving it reading as covered."
            )
            .into());
        }
        if !command.starts_with("cargo ") {
            return Err(format!(
                "`ci-metrics-ratchet` command `{command}` is not a direct `cargo` invocation. \
                 Routes must be executed, not printed or wrapped; a wrapper would keep the \
                 command text while the floor stops being enforced."
            )
            .into());
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
/// also have overstated the guarantee: bootstrap safety is a property of the
/// receipt-backed routes, while the recipe's `editor_ux` route reads no receipt
/// at all and fails closed on its own missing instrumentation instead.
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

    // Each route the recipe actually runs must have its missing-metric
    // disposition documented in that guide, bound to the route rather than
    // merely present somewhere in the file.
    let justfile = fs::read_to_string(root.join("justfile"))?;
    let guide = fs::read_to_string(root.join(RATCHET_GUIDE))?;
    for route in ci_metrics_ratchet_routes(&justfile)? {
        assert!(
            guide_binds_route_to_disposition(&guide, &route),
            "{RATCHET_GUIDE} must state the missing-metric disposition of the `{}` route on \
             the same line as `{}` — one of {:?}. The guide's command examples contain that \
             command on their own, so a whole-file check would stay green with every \
             explanation deleted.",
            route.subsystem(),
            route.documented_command(),
            route.documented_dispositions()
        );
    }

    Ok(())
}

/// Whether the guide states this route's disposition on the same line as the
/// route's command, so deleting the explanation cannot leave the contract green.
fn guide_binds_route_to_disposition(guide: &str, route: &RatchetRoute) -> bool {
    guide.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains(route.documented_command())
            && route.documented_dispositions().iter().any(|d| lower.contains(d))
    })
}

/// The guide must keep receipt absence and missing instrumentation separate.
///
/// `ux-scorecard --ratchet-check` never reads `target/receipts/metrics/`; it
/// computes `editor_ux` metrics from the committed measurement input
/// (`xtask/src/tasks/ux_scorecard.rs`) and fails closed on missing or
/// non-finite values *there*. So a receipt-less run does not fail on its
/// account, and an operator told the job is simply "not bootstrap-safe" will
/// hunt for a missing receipt this route never wanted. The guide has to say
/// that the route reads no receipt, not just that it is fail-closed.
#[test]
fn ratchet_guide_separates_receipt_absence_from_missing_instrumentation()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let justfile = fs::read_to_string(root.join("justfile"))?;
    let guide = fs::read_to_string(root.join(RATCHET_GUIDE))?;

    let runs_editor_ux = ci_metrics_ratchet_routes(&justfile)?.contains(&RatchetRoute::EditorUx);
    if !runs_editor_ux {
        return Ok(());
    }

    let lower = guide.to_ascii_lowercase();
    assert!(
        lower.contains("never reads receipts")
            || lower.contains("does not read a receipt")
            || lower.contains("never consults"),
        "{RATCHET_GUIDE} must record that the `{EDITOR_UX_SUBSYSTEM}` route reads no receipt, \
         so operators do not read its fail-closed behavior as a missing-receipt symptom"
    );

    Ok(())
}
