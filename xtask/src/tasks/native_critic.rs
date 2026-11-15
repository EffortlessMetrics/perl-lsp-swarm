//! Repository native critic checks through the shared service.

use chrono::{DateTime, Utc};
use color_eyre::eyre::{Context, Result, eyre};
use perl_lsp_rs_core::tooling::perl_critic::{
    CriticRelatedInformation, CriticSourceIdentity, EffectiveCriticState,
    EffectiveNativeCriticConfig, NativeCriticProfile, NativeCriticRun, NativeCriticRunCompleteness,
    NativeCriticService, NativeCriticSubject, NormalizedCriticFinding, RunGate,
    critic_source_identity_for_uri,
};
use perl_parser_core::{ParseCompletion, Parser};
use perl_source_identity::ContentDigest;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

const SCHEMA_VERSION: u32 = 2;

/// Options for `cargo xtask native-critic check`.
pub struct NativeCriticCheckConfig {
    /// Files or directories containing Perl sources.
    pub roots: Vec<PathBuf>,
    /// Native critic profile to run.
    pub profile: String,
    /// Minimum native critic severity to report.
    pub severity: u8,
    /// Native rule IDs to include. Empty means all selected-profile rules.
    pub include: Vec<String>,
    /// Native rule IDs to exclude.
    pub exclude: Vec<String>,
    /// Output JSON receipt path.
    pub receipt: PathBuf,
    /// Output markdown summary path.
    pub summary: PathBuf,
}

#[derive(Debug, Serialize)]
struct NativeCriticCheckReceipt {
    kind: &'static str,
    schema_version: u32,
    generated_at: DateTime<Utc>,
    commit: String,
    roots: Vec<String>,
    profile: String,
    engine: &'static str,
    severity: u8,
    include: Vec<String>,
    exclude: Vec<String>,
    // Registrations are summed across the individual file runs.
    rules_registered: usize,
    producer_evaluation_entered: usize,
    files_checked: usize,
    files_complete: usize,
    files_with_parse_errors: usize,
    findings_count: usize,
    suppressed_findings_count: Option<usize>,
    fixable_findings_count: usize,
    findings_by_code: BTreeMap<String, usize>,
    // The service observes the exact total, not a per-rule suppression count.
    suppressed_findings_by_rule: Option<BTreeMap<String, usize>>,
    files: Vec<NativeCriticFileResult>,
}

#[derive(Debug, Serialize)]
struct NativeCriticFileResult {
    path: String,
    source_identity: CriticSourceIdentity,
    source_digest: String,
    parse_ok: bool,
    parse_error: Option<String>,
    parse_diagnostics: Vec<String>,
    parse_completion: String,
    recovered_count: usize,
    parse_stop_cause: Option<String>,
    service_completeness: Option<CompletenessReceipt>,
    state_fingerprint: String,
    work: Option<WorkReceipt>,
    findings_count: usize,
    suppressed_findings_count: Option<usize>,
    fixable_findings_count: usize,
    findings: Vec<FindingReceipt>,
}

impl NativeCriticFileResult {
    fn is_complete(&self) -> bool {
        self.parse_ok && matches!(self.service_completeness, Some(CompletenessReceipt::Complete))
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CompletenessReceipt {
    Disabled,
    Complete,
    Partial { unresolved_producer_identities: usize },
    NotReady,
    Stale,
    Cancelled,
    InstrumentFailure,
}

impl From<&NativeCriticRunCompleteness> for CompletenessReceipt {
    fn from(value: &NativeCriticRunCompleteness) -> Self {
        match value {
            NativeCriticRunCompleteness::Disabled => Self::Disabled,
            NativeCriticRunCompleteness::Complete => Self::Complete,
            NativeCriticRunCompleteness::Partial { unresolved_producer_identities } => {
                Self::Partial { unresolved_producer_identities: *unresolved_producer_identities }
            }
            NativeCriticRunCompleteness::NotReady => Self::NotReady,
            NativeCriticRunCompleteness::Stale => Self::Stale,
            NativeCriticRunCompleteness::Cancelled => Self::Cancelled,
            NativeCriticRunCompleteness::InstrumentFailure => Self::InstrumentFailure,
        }
    }
}

#[derive(Debug, Serialize)]
struct WorkReceipt {
    rules_registered: usize,
    producer_evaluation_entered: usize,
    native_findings_collected: usize,
    observation_candidates_collected: usize,
    unresolved_producer_identities: usize,
    findings_suppressed: usize,
    findings_after_policy: usize,
}

impl From<&NativeCriticRun> for WorkReceipt {
    fn from(run: &NativeCriticRun) -> Self {
        let work = run.work();
        Self {
            rules_registered: work.rules_registered,
            producer_evaluation_entered: work.producer_evaluation_entered,
            native_findings_collected: work.native_findings_collected,
            observation_candidates_collected: work.observation_candidates_collected,
            unresolved_producer_identities: work.unresolved_producer_identities,
            findings_suppressed: run.findings_suppressed(),
            findings_after_policy: work.findings_after_policy,
        }
    }
}

#[derive(Debug, Serialize)]
struct FindingReceipt {
    #[serde(flatten)]
    finding: NormalizedCriticFinding,
    // The normalized row's general serializer omits these row projections;
    // repository evidence preserves them as well as contributor provenance.
    remediation_suggestion: Option<String>,
    remediation_related_information: Vec<CriticRelatedInformation>,
}

impl From<NormalizedCriticFinding> for FindingReceipt {
    fn from(finding: NormalizedCriticFinding) -> Self {
        Self {
            remediation_suggestion: finding.remediation_suggestion().map(ToOwned::to_owned),
            remediation_related_information: finding.remediation_related_information().to_vec(),
            finding,
        }
    }
}

/// Analyze source files once through the native service and write evidence.
pub fn check(config: NativeCriticCheckConfig) -> Result<()> {
    let roots = if config.roots.is_empty() { default_roots() } else { config.roots };
    let files = collect_paths(&roots)?;
    if files.is_empty() {
        return Err(eyre!(
            "no native critic source files found under {}",
            roots.iter().map(|root| root.display().to_string()).collect::<Vec<_>>().join(", ")
        ));
    }

    let profile = NativeCriticProfile::parse(&config.profile).ok_or_else(|| {
        eyre!("unknown native critic profile '{}'; expected recommended or strict", config.profile)
    })?;
    let policy = EffectiveNativeCriticConfig::new(
        profile,
        config.severity,
        &config.include,
        &config.exclude,
        None,
    );
    let state = EffectiveCriticState::Native(policy.clone());
    let results = files.iter().map(|file| check_file(&state, file)).collect::<Result<Vec<_>>>()?;
    let mut findings_by_code = BTreeMap::new();
    for result in &results {
        for finding in &result.findings {
            *findings_by_code.entry(finding.finding.public_code().to_string()).or_insert(0) += 1;
        }
    }

    let receipt = NativeCriticCheckReceipt {
        kind: "native_critic_check",
        schema_version: SCHEMA_VERSION,
        generated_at: Utc::now(),
        commit: current_commit(),
        roots: roots.iter().map(|root| root.display().to_string()).collect(),
        profile: profile.as_str().to_string(),
        engine: "native",
        severity: policy.severity_threshold,
        include: policy.include,
        exclude: policy.exclude,
        rules_registered: results
            .iter()
            .filter_map(|file| file.work.as_ref())
            .map(|work| work.rules_registered)
            .sum(),
        producer_evaluation_entered: results
            .iter()
            .filter_map(|file| file.work.as_ref())
            .map(|work| work.producer_evaluation_entered)
            .sum(),
        files_checked: results.len(),
        files_complete: results.iter().filter(|file| file.is_complete()).count(),
        files_with_parse_errors: results.iter().filter(|result| !result.parse_ok).count(),
        findings_count: results.iter().map(|result| result.findings_count).sum(),
        suppressed_findings_count: results
            .iter()
            .map(|result| result.suppressed_findings_count)
            .sum(),
        fixable_findings_count: results.iter().map(|result| result.fixable_findings_count).sum(),
        findings_by_code,
        suppressed_findings_by_rule: None,
        files: results,
    };

    if let Some(parent) = config.receipt.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = config.summary.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create {}", parent.display()))?;
    }
    write_json(&config.receipt, &receipt)?;
    write_summary(&config.summary, &receipt)?;
    println!(
        "native critic check: {} finding(s), {} suppressed, {} fixable; {} of {} file(s) complete; receipt: {}",
        receipt.findings_count,
        observed_count(receipt.suppressed_findings_count),
        receipt.fixable_findings_count,
        receipt.files_complete,
        receipt.files_checked,
        config.receipt.display()
    );
    Ok(())
}

fn check_file(state: &EffectiveCriticState, file: &Path) -> Result<NativeCriticFileResult> {
    let source =
        fs::read_to_string(file).wrap_err_with(|| format!("failed to read {}", file.display()))?;
    let uri = perl_uri::fs_path_to_uri(file)
        .map_err(|error| eyre!("failed to identify {}: {error}", file.display()))?;
    let source_identity = critic_source_identity_for_uri(&uri, 0);
    let output = Parser::new(&source).parse_with_recovery();
    let parse_diagnostics = output.diagnostics.iter().map(ToString::to_string).collect::<Vec<_>>();
    let parse_stop_cause = output.stop_cause().map(|cause| format!("{cause:?}"));
    let completion = output.completion();
    let parse_ok = matches!(completion, ParseCompletion::CompletedClean);
    let mut result = NativeCriticFileResult {
        path: file.display().to_string(),
        source_identity,
        source_digest: String::new(),
        parse_ok,
        parse_error: (!parse_ok).then(|| {
            parse_diagnostics
                .first()
                .cloned()
                .or_else(|| parse_stop_cause.clone())
                .unwrap_or_else(|| format!("{completion:?}"))
        }),
        parse_diagnostics,
        parse_completion: format!("{completion:?}"),
        recovered_count: output.recovered_count,
        parse_stop_cause,
        service_completeness: None,
        state_fingerprint: state.fingerprint(),
        work: None,
        findings_count: 0,
        suppressed_findings_count: None,
        fixable_findings_count: 0,
        findings: Vec::new(),
    };
    if !matches!(
        completion,
        ParseCompletion::CompletedClean | ParseCompletion::CompletedWithRecovery { .. }
    ) {
        result.source_digest = ContentDigest::of_bytes(source.as_bytes()).as_wire().to_string();
        return Ok(result);
    }

    let run = NativeCriticService::analyze(NativeCriticSubject::accepted(
        &uri,
        source_identity,
        &output.ast,
        &source,
        state.clone(),
        Vec::new(),
        RunGate::open(),
        RunGate::open(),
    ));
    result.source_digest = run.source_digest().as_wire().to_string();
    result.service_completeness = Some(run.completeness().into());
    result.findings = run.findings().iter().cloned().map(FindingReceipt::from).collect();
    result.findings_count = result.findings.len();
    result.fixable_findings_count =
        result.findings.iter().filter(|row| row.finding.has_available_fix()).count();
    result.suppressed_findings_count =
        (run.work().producer_evaluation_entered > 0).then_some(run.findings_suppressed());
    result.work = Some((&run).into());
    Ok(result)
}

fn observed_count(count: Option<usize>) -> String {
    count.map_or_else(|| "unavailable".to_string(), |value| value.to_string())
}

fn write_summary(path: &Path, receipt: &NativeCriticCheckReceipt) -> Result<()> {
    let mut markdown = String::new();
    markdown.push_str("# Native Critic Check\n\n");
    markdown.push_str(&format!("- Engine: `{}`\n", receipt.engine));
    markdown.push_str(&format!("- Profile: `{}`\n", receipt.profile));
    markdown.push_str(&format!("- Severity: `{}`\n", receipt.severity));
    markdown.push_str(&format!("- Files checked: `{}`\n", receipt.files_checked));
    markdown.push_str(&format!("- Files complete: `{}`\n", receipt.files_complete));
    markdown
        .push_str(&format!("- Files with parse errors: `{}`\n", receipt.files_with_parse_errors));
    markdown.push_str(&format!(
        "- Rule registrations across file runs: `{}`\n",
        receipt.rules_registered
    ));
    markdown.push_str(&format!(
        "- Producer evaluations entered: `{}`\n",
        receipt.producer_evaluation_entered
    ));
    markdown.push_str(&format!("- Findings: `{}`\n", receipt.findings_count));
    markdown.push_str(&format!(
        "- Suppressed findings: `{}`\n",
        observed_count(receipt.suppressed_findings_count)
    ));
    markdown.push_str("- Per-rule suppression counts: unavailable\n");
    markdown.push_str(&format!("- Fixable findings: `{}`\n\n", receipt.fixable_findings_count));
    markdown.push_str("## Findings By Code\n\n| Code | Findings |\n| --- | ---: |\n");
    for (code, count) in &receipt.findings_by_code {
        markdown.push_str(&format!("| `{code}` | {count} |\n"));
    }
    if receipt.findings_by_code.is_empty() {
        markdown.push_str("| _none_ | 0 |\n");
    }
    markdown.push_str("\n## Files\n\n| File | Parse clean | Service | Findings | Suppressed | Fixable |\n| --- | --- | --- | ---: | ---: | ---: |\n");
    for file in &receipt.files {
        markdown.push_str(&format!(
            "| `{}` | {} | {:?} | {} | {} | {} |\n",
            file.path,
            file.parse_ok,
            file.service_completeness,
            file.findings_count,
            observed_count(file.suppressed_findings_count),
            file.fixable_findings_count
        ));
    }
    fs::write(path, markdown).wrap_err_with(|| format!("failed to write {}", path.display()))
}

fn default_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from("examples/perl"),
        PathBuf::from("tests/perl-corpus"),
        PathBuf::from("crates/perl-corpus/fixtures/parser_accuracy"),
    ]
}

fn collect_paths(roots: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for root in roots {
        if root.is_file() {
            if is_perl_source(root) {
                paths.push(root.to_path_buf());
            }
            continue;
        }
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            if entry.file_type().is_file() && is_perl_source(entry.path()) {
                paths.push(entry.path().to_path_buf());
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn is_perl_source(path: &Path) -> bool {
    let filename = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    if filename.ends_with(".expected.pl") || filename.ends_with(".expected-diagnostics.txt") {
        return false;
    }
    matches!(path.extension().and_then(|ext| ext.to_str()), Some("pl" | "pm" | "t"))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    fs::write(path, format!("{json}\n"))
        .wrap_err_with(|| format!("failed to write {}", path.display()))
}

fn current_commit() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| stdout.trim().to_string())
        .filter(|commit| !commit.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn repository_findings_equal_the_direct_service_for_an_explicit_include() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let file = temp.path().join("Included.pm");
        let source = "use strict;\nuse warnings;\nsub run { my $unused = 1; return 1; }\n";
        fs::write(&file, source)?;
        let state = EffectiveCriticState::Native(EffectiveNativeCriticConfig::new(
            NativeCriticProfile::Recommended,
            1,
            &[
                " native.variables.unused_lexical ".to_string(),
                "native.variables.unused_lexical".to_string(),
            ],
            &[],
            None,
        ));
        let result = check_file(&state, &file)?;
        let uri = perl_uri::fs_path_to_uri(&file).map_err(|error| eyre!(error))?;
        let parsed = Parser::new(source).parse_with_recovery();
        let direct = NativeCriticService::analyze(NativeCriticSubject::accepted(
            &uri,
            critic_source_identity_for_uri(&uri, 0),
            &parsed.ast,
            source,
            state,
            Vec::new(),
            RunGate::open(),
            RunGate::open(),
        ));
        assert!(result.is_complete());
        assert_eq!(result.findings_count, 1, "explicit include admits a strict-only rule");
        let rows = result.findings.iter().map(|row| row.finding.clone()).collect::<Vec<_>>();
        assert_eq!(rows.as_slice(), direct.findings());
        assert_eq!(result.source_digest, direct.source_digest().as_wire());
        assert_eq!(result.source_identity, *direct.source_identity());
        assert_eq!(result.state_fingerprint, direct.state_fingerprint());
        assert_eq!(result.suppressed_findings_count, Some(direct.findings_suppressed()));
        let serialized = serde_json::to_value(&result)?;
        assert_eq!(serialized["work"]["rules_registered"], 1);
        assert_eq!(serialized["work"]["producer_evaluation_entered"], 1);
        assert_eq!(
            serialized["findings"][0]["remediation_suggestion"],
            serde_json::to_value(direct.findings()[0].remediation_suggestion())?
        );
        assert_eq!(
            serialized["findings"][0]["remediation_related_information"],
            serde_json::to_value(direct.findings()[0].remediation_related_information())?
        );
        assert!(serialized["findings"][0].get("contributors").is_some());
        Ok(())
    }

    #[test]
    fn repository_check_uses_the_canonical_policy_constructor() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("Policy.pm");
        fs::write(&source, "use strict; use warnings; sub run { my $unused = 1; return 1; }")?;
        let receipt = temp.path().join("critic.json");
        for (input, expected) in [(0, 1), (u8::MAX, 5)] {
            check(NativeCriticCheckConfig {
                roots: vec![source.clone()],
                profile: "recommended".to_string(),
                severity: input,
                include: vec![
                    " native.variables.unused_lexical ".to_string(),
                    "native.variables.unused_lexical".to_string(),
                    " ".to_string(),
                ],
                exclude: Vec::new(),
                receipt: receipt.clone(),
                summary: temp.path().join("critic.md"),
            })?;
            let value: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
            assert_eq!(value["severity"], expected);
            assert_eq!(value["include"], serde_json::json!(["native.variables.unused_lexical"]));
            assert_eq!(value["files_complete"], 1);
        }
        Ok(())
    }

    #[test]
    fn repository_check_preserves_recovery_and_terminal_parse_evidence() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let file = temp.path().join("Parse.pm");
        let state = EffectiveCriticState::Native(EffectiveNativeCriticConfig::new(
            NativeCriticProfile::Recommended,
            1,
            &[],
            &[],
            None,
        ));
        fs::write(&file, "my $x = ;")?;
        let recovered = check_file(&state, &file)?;
        assert!(!recovered.parse_ok);
        assert!(recovered.recovered_count > 0);
        assert!(!recovered.parse_diagnostics.is_empty());
        assert!(recovered.parse_stop_cause.is_none());
        assert!(matches!(recovered.service_completeness, Some(CompletenessReceipt::Complete)));
        assert!(!recovered.is_complete(), "service completion cannot erase parser recovery");

        fs::write(&file, format!("{}1{};", "(".repeat(1024), ")".repeat(1024)))?;
        let stopped = check_file(&state, &file)?;
        assert!(!stopped.is_complete());
        assert!(stopped.parse_stop_cause.is_some());
        assert!(stopped.service_completeness.is_none());
        assert!(stopped.work.is_none());
        assert_eq!(stopped.suppressed_findings_count, None);
        assert!(stopped.source_digest.starts_with("sha256:"));
        Ok(())
    }

    #[test]
    fn repository_finding_projection_retains_row_remediation() -> Result<()> {
        use perl_lsp_rs_core::tooling::perl_critic::{
            CriticFindingCandidate, CriticObservedIdentity, Severity, normalize_critic_findings,
        };
        use perl_parser_core::position::{Position, Range};
        let range = Range {
            start: Position { byte: 0, line: 0, column: 0 },
            end: Position { byte: 1, line: 0, column: 1 },
        };
        let candidate = CriticFindingCandidate::new(
            CriticObservedIdentity::built_in_system_call(),
            critic_source_identity_for_uri("file:///test.pm", 0),
            Severity::Harsh,
            range,
            "system call",
            None,
        )
        .with_remediation(
            Some("inspect the arguments".to_string()),
            vec![CriticRelatedInformation { range, message: "argument".to_string() }],
        );
        let row = normalize_critic_findings([candidate])
            .into_iter()
            .next()
            .ok_or_else(|| eyre!("expected one normalized row"))?;
        let json = serde_json::to_value(FindingReceipt::from(row))?;
        assert_eq!(json["remediation_suggestion"], "inspect the arguments");
        assert_eq!(json["remediation_related_information"][0]["message"], "argument");
        assert_eq!(json["contributors"].as_array().map(Vec::len), Some(1));
        Ok(())
    }

    #[test]
    fn native_critic_check_writes_receipt_and_summary() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("App.pm");
        fs::write(&source, "package App;\nsub run { my $unused = 1; return 1; }\n1;\n")?;
        let receipt = temp.path().join("native-critic-check.json");
        let summary = temp.path().join("native-critic-check.md");

        check(NativeCriticCheckConfig {
            roots: vec![source],
            profile: "strict".to_string(),
            severity: 1,
            include: vec!["native.variables.unused_lexical".to_string()],
            exclude: Vec::new(),
            receipt: receipt.clone(),
            summary: summary.clone(),
        })?;

        let value: Value = serde_json::from_str(&fs::read_to_string(receipt)?)?;
        assert_eq!(value["kind"], "native_critic_check");
        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["engine"], "native");
        assert_eq!(value["profile"], "strict");
        assert_eq!(value["files_checked"], 1);
        assert_eq!(value["files_complete"], 1);
        assert_eq!(value["rules_registered"], 1);
        assert_eq!(value["findings_count"], 1);
        assert_eq!(value["fixable_findings_count"], 1);
        assert_eq!(value["findings_by_code"]["native.variables.unused_lexical"], 1);

        let summary = fs::read_to_string(summary)?;
        assert!(summary.contains("# Native Critic Check"));
        assert!(summary.contains("| `native.variables.unused_lexical` | 1 |"));
        Ok(())
    }

    #[test]
    fn native_critic_check_counts_suppressed_findings() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("App.pm");
        fs::write(
            &source,
            "## no perl-lsp-critic native.variables.unused_lexical -- generated\npackage App;\nsub run { my $unused = 1; return 1; }\n1;\n",
        )?;
        let receipt = temp.path().join("native-critic-check.json");
        let summary = temp.path().join("native-critic-check.md");

        check(NativeCriticCheckConfig {
            roots: vec![source],
            profile: "strict".to_string(),
            severity: 1,
            include: vec!["native.variables.unused_lexical".to_string()],
            exclude: Vec::new(),
            receipt: receipt.clone(),
            summary,
        })?;

        let value: Value = serde_json::from_str(&fs::read_to_string(receipt)?)?;
        assert_eq!(value["findings_count"], 0);
        assert_eq!(value["suppressed_findings_count"], 1);
        assert_eq!(value["files"][0]["suppressed_findings_count"], 1);
        assert_eq!(value["suppressed_findings_by_rule"], Value::Null);
        assert_eq!(value["files"][0]["work"]["producer_evaluation_entered"], 1);
        Ok(())
    }

    #[test]
    fn native_critic_check_profiles_lower_noise_recommended_rules() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("App.pm");
        fs::write(&source, "package App;\nsub run { my $unused = 1; return 1; }\n1;\n")?;
        let receipt = temp.path().join("native-critic-check.json");
        let summary = temp.path().join("native-critic-check.md");

        check(NativeCriticCheckConfig {
            roots: vec![source],
            profile: "recommended".to_string(),
            severity: 1,
            include: Vec::new(),
            exclude: Vec::new(),
            receipt: receipt.clone(),
            summary,
        })?;

        let value: Value = serde_json::from_str(&fs::read_to_string(receipt)?)?;
        assert_eq!(value["profile"], "recommended");
        assert_eq!(value["rules_registered"], 16);
        assert_eq!(value["findings_by_code"]["native.variables.unused_lexical"], Value::Null);
        Ok(())
    }

    #[test]
    fn native_critic_check_keeps_false_positive_fixtures_clean() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let receipt = temp.path().join("native-critic-check.json");
        let summary = temp.path().join("native-critic-check.md");
        let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("xtask/tests/fixtures/native-critic/false-positive");

        check(NativeCriticCheckConfig {
            roots: vec![fixture_root],
            profile: "strict".to_string(),
            severity: 1,
            include: Vec::new(),
            exclude: Vec::new(),
            receipt: receipt.clone(),
            summary: summary.clone(),
        })?;

        let value: Value = serde_json::from_str(&fs::read_to_string(receipt)?)?;
        assert_eq!(value["files_checked"], 12);
        assert_eq!(value["files_with_parse_errors"], 0);
        assert_eq!(value["rules_registered"], 32 * 12);
        assert_eq!(value["findings_count"], 0);
        assert_eq!(value["suppressed_findings_count"], 0);
        assert_eq!(value["fixable_findings_count"], 0);
        assert_eq!(value["findings_by_code"].as_object().map(|rules| rules.len()), Some(0));

        let summary = fs::read_to_string(summary)?;
        assert!(summary.contains("- Findings: `0`"));
        assert!(summary.contains("| _none_ | 0 |"));
        Ok(())
    }
}
