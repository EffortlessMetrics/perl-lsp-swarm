//! Trusted-base containment for internally contradictory GitHub closing relations.
//!
//! This is CP00 from issue #10413. It does not decide whether an issue is
//! semantically complete. It rejects only a closed set of contradictions that
//! are visible in stable PR sections and exact issue classifications.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::{Parser, ValueEnum};
use color_eyre::eyre::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, exit},
};

const REPORT_SCHEMA: &str = "semantic_close_containment_report.v1";
const FIXTURE_SCHEMA: &str = "semantic_close_containment_fixture.v1";
const EXIT_CONTRADICTION: i32 = 2;
const EXIT_NOT_PROVEN: i32 = 3;
const MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_FIXTURE_BYTES: usize = 512 * 1024;
const MAX_PR_BODY_BYTES: usize = 128 * 1024;
const MAX_ISSUE_BODY_BYTES: usize = 256 * 1024;
const MAX_GITHUB_OUTPUT_BYTES: usize = 512 * 1024;
const MAX_RELATIONS: usize = 32;
const MAX_SECTION_BYTES: usize = 32 * 1024;
const MAX_SOURCE_LINE_BYTES: usize = 2 * 1024;

#[derive(Debug, Parser)]
#[command(name = "semantic-close-containment")]
#[command(about = "Reject high-confidence contradictory terminal issue relations")]
struct Args {
    /// GitHub event payload. Defaults to GITHUB_EVENT_PATH when neither input is supplied.
    #[arg(long, conflicts_with = "fixture")]
    event: Option<PathBuf>,

    /// Immutable offline regression fixture.
    #[arg(long, conflicts_with = "event")]
    fixture: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ResultCode {
    PassNotApplicable,
    PassNoHighConfidenceContradiction,
    FailPhaseTerminalRelation,
    FailExplicitUnprovenRequiredWork,
    FailRemainingWorkSameIssue,
    FailControllerPacketMissing,
    FailPredecessorSuccessorCollapse,
    FailProofLevelContradiction,
    NotProvenGithub,
    InstrumentFailure,
}

impl ResultCode {
    fn is_failure(self) -> bool {
        matches!(
            self,
            Self::FailPhaseTerminalRelation
                | Self::FailExplicitUnprovenRequiredWork
                | Self::FailRemainingWorkSameIssue
                | Self::FailControllerPacketMissing
                | Self::FailPredecessorSuccessorCollapse
                | Self::FailProofLevelContradiction
        )
    }

    fn is_not_proven(self) -> bool {
        matches!(self, Self::NotProvenGithub | Self::InstrumentFailure)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::PassNotApplicable => "PASS_NOT_APPLICABLE",
            Self::PassNoHighConfidenceContradiction => {
                "PASS_NO_HIGH_CONFIDENCE_CONTRADICTION"
            }
            Self::FailPhaseTerminalRelation => "FAIL_PHASE_TERMINAL_RELATION",
            Self::FailExplicitUnprovenRequiredWork => {
                "FAIL_EXPLICIT_UNPROVEN_REQUIRED_WORK"
            }
            Self::FailRemainingWorkSameIssue => "FAIL_REMAINING_WORK_SAME_ISSUE",
            Self::FailControllerPacketMissing => "FAIL_CONTROLLER_PACKET_MISSING",
            Self::FailPredecessorSuccessorCollapse => {
                "FAIL_PREDECESSOR_SUCCESSOR_COLLAPSE"
            }
            Self::FailProofLevelContradiction => "FAIL_PROOF_LEVEL_CONTRADICTION",
            Self::NotProvenGithub => "NOT_PROVEN_GITHUB",
            Self::InstrumentFailure => "INSTRUMENT_FAILURE",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum RuleId {
    PhaseTerminal,
    ExplicitlyNotProven,
    RemainingSameIssue,
    ControllerPacketMissing,
    PredecessorSuccessorCollapse,
    ProofLevelContradiction,
}

impl RuleId {
    fn as_str(self) -> &'static str {
        match self {
            Self::PhaseTerminal => "CP00-PHASE-TERMINAL",
            Self::ExplicitlyNotProven => "CP00-EXPLICITLY-NOT-PROVEN",
            Self::RemainingSameIssue => "CP00-REMAINING-SAME-ISSUE",
            Self::ControllerPacketMissing => "CP00-CONTROLLER-PACKET-MISSING",
            Self::PredecessorSuccessorCollapse => "CP00-PREDECESSOR-SUCCESSOR-COLLAPSE",
            Self::ProofLevelContradiction => "CP00-PROOF-LEVEL-CONTRADICTION",
        }
    }

    fn retirement_mapping(self) -> &'static str {
        match self {
            Self::PhaseTerminal => "CP03 denominator/close-mode evaluation",
            Self::ExplicitlyNotProven => "CP03 explicitly-not-established row evaluation",
            Self::RemainingSameIssue => "CP03 denominator row disposition evaluation",
            Self::ControllerPacketMissing => "CP03 controller fan-in and packet evaluation",
            Self::PredecessorSuccessorCollapse => {
                "CP03 proposition identity and successor-retirement evaluation"
            }
            Self::ProofLevelContradiction => "CP03 required proof-level evaluation",
        }
    }

    #[cfg(test)]
    fn all() -> [Self; 6] {
        [
            Self::PhaseTerminal,
            Self::ExplicitlyNotProven,
            Self::RemainingSameIssue,
            Self::ControllerPacketMissing,
            Self::PredecessorSuccessorCollapse,
            Self::ProofLevelContradiction,
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct IssueKey {
    repository: String,
    number: u64,
}

#[derive(Clone, Debug)]
struct PullRequestSubject {
    repository: String,
    number: u64,
    title: String,
    body: String,
}

#[derive(Clone, Debug)]
struct IssueSubject {
    number: u64,
    title: String,
    body: String,
}

#[derive(Clone, Debug)]
enum IssueEvidence {
    Available(IssueSubject),
    Unavailable(String),
}

#[derive(Clone, Debug)]
struct ClosingRelation {
    key: IssueKey,
    keyword: String,
    source_line: String,
    line_number: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SectionKind {
    Claim,
    ControllingIssue,
    GoverningContract,
    ClaimBoundary,
    NonGoals,
    RemainingWork,
    Summary,
    Changes,
    ReviewMap,
    Acceptance,
    Objective,
    Outcome,
}

#[derive(Clone, Debug, Default)]
struct Section {
    headings: Vec<String>,
    body: String,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: &'static str,
    repository: String,
    pull_request_number: u64,
    pull_request_title: String,
    aggregate_code: ResultCode,
    semantic_completion_proven: bool,
    rows: Vec<RelationResult>,
}

impl Report {
    fn exit_code(&self) -> i32 {
        if self.rows.iter().any(|row| row.code.is_failure()) {
            EXIT_CONTRADICTION
        } else if self.rows.iter().any(|row| row.code.is_not_proven()) {
            EXIT_NOT_PROVEN
        } else {
            0
        }
    }
}

#[derive(Debug, Serialize)]
struct RelationResult {
    repository: String,
    issue_number: u64,
    keyword: String,
    source_line: String,
    line_number: usize,
    code: ResultCode,
    rule_id: Option<String>,
    reason: String,
    suggested_relation: Option<String>,
    retirement_mapping: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubEvent {
    repository: GithubRepository,
    pull_request: GithubPullRequest,
}

#[derive(Debug, Deserialize)]
struct GithubRepository {
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct GithubPullRequest {
    number: u64,
    title: String,
    body: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubIssuePayload {
    number: u64,
    title: String,
    body: Option<String>,
    pull_request: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    schema_version: String,
    provenance: FixtureProvenance,
    repository: String,
    pull_request: FixturePullRequest,
    issues: Vec<FixtureIssue>,
    expected: FixtureExpected,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureProvenance {
    captured_at: String,
    sources: Vec<String>,
    subject_shas: Vec<String>,
    boundary: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixturePullRequest {
    number: u64,
    title: String,
    body: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureIssue {
    repository: String,
    number: u64,
    title: String,
    body: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureExpected {
    aggregate_code: ResultCode,
    rows: Vec<FixtureExpectedRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureExpectedRow {
    repository: String,
    issue_number: u64,
    code: ResultCode,
}

fn main() {
    if let Err(error) = run_cli() {
        eprintln!(
            "INSTRUMENT_FAILURE semantic-close-containment: {}",
            sanitize_for_output(&error.to_string(), 1_024)
        );
        exit(EXIT_NOT_PROVEN);
    }
}

fn run_cli() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    let (report, fixture_expected) = if let Some(path) = args.fixture {
        let fixture = load_fixture(&path)?;
        let report = evaluate_fixture(&fixture)?;
        let expected = fixture.expected;
        (report, Some(expected))
    } else {
        let event_path = match args.event {
            Some(path) => path,
            None => PathBuf::from(
                env::var("GITHUB_EVENT_PATH")
                    .context("supply --event/--fixture or set GITHUB_EVENT_PATH")?,
            ),
        };
        (evaluate_live_event(&event_path)?, None)
    };

    if let Some(expected) = fixture_expected {
        verify_expected(&report, &expected)?;
    }

    match args.format {
        OutputFormat::Human => print_human(&report),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }

    let code = report.exit_code();
    if code != 0 {
        exit(code);
    }
    Ok(())
}

fn evaluate_live_event(path: &Path) -> Result<Report> {
    let raw = read_bounded(path, MAX_EVENT_BYTES, "GitHub event payload")?;
    let event: GithubEvent =
        serde_json::from_slice(&raw).context("parsing pull_request_target event payload")?;
    let pull = PullRequestSubject {
        repository: canonical_repository(&event.repository.full_name)?,
        number: event.pull_request.number,
        title: event.pull_request.title,
        body: event.pull_request.body.unwrap_or_default(),
    };

    let mut cache: BTreeMap<IssueKey, IssueEvidence> = BTreeMap::new();
    evaluate(&pull, |key| {
        if let Some(cached) = cache.get(key) {
            return cached.clone();
        }
        let evidence = fetch_issue_live(key);
        cache.insert(key.clone(), evidence.clone());
        evidence
    })
}

fn load_fixture(path: &Path) -> Result<Fixture> {
    let raw = read_bounded(path, MAX_FIXTURE_BYTES, "semantic-close fixture")?;
    let fixture: Fixture = serde_json::from_slice(&raw).context("parsing strict fixture")?;
    validate_fixture(&fixture)?;
    Ok(fixture)
}

fn validate_fixture(fixture: &Fixture) -> Result<()> {
    if fixture.schema_version != FIXTURE_SCHEMA {
        bail!(
            "unsupported fixture schema {:?}; expected {FIXTURE_SCHEMA}",
            fixture.schema_version
        );
    }
    canonical_repository(&fixture.repository)?;
    if fixture.provenance.captured_at.trim().is_empty()
        || fixture.provenance.sources.is_empty()
        || fixture.provenance.boundary.trim().is_empty()
    {
        bail!("fixture provenance must retain capture date, sources, and boundary");
    }
    if fixture
        .provenance
        .sources
        .iter()
        .any(|source| !source.starts_with("https://github.com/"))
    {
        bail!("fixture provenance sources must be canonical GitHub URLs");
    }
    if fixture
        .provenance
        .subject_shas
        .iter()
        .any(|sha| !sha.is_empty() && !is_full_sha(sha))
    {
        bail!("fixture provenance subject SHAs must be empty or full lowercase hex");
    }
    if fixture.pull_request.title.trim().is_empty() {
        bail!("fixture pull request title must not be empty");
    }
    let mut seen = BTreeMap::new();
    for issue in &fixture.issues {
        let key = IssueKey {
            repository: canonical_repository(&issue.repository)?,
            number: issue.number,
        };
        if seen.insert(key, ()).is_some() {
            bail!("duplicate issue subject in fixture");
        }
    }
    Ok(())
}

fn evaluate_fixture(fixture: &Fixture) -> Result<Report> {
    let pull = PullRequestSubject {
        repository: canonical_repository(&fixture.repository)?,
        number: fixture.pull_request.number,
        title: fixture.pull_request.title.clone(),
        body: fixture.pull_request.body.clone(),
    };
    let mut issues = BTreeMap::new();
    for issue in &fixture.issues {
        let key = IssueKey {
            repository: canonical_repository(&issue.repository)?,
            number: issue.number,
        };
        issues.insert(
            key,
            IssueEvidence::Available(IssueSubject {
                number: issue.number,
                title: issue.title.clone(),
                body: issue.body.clone(),
            }),
        );
    }
    evaluate(&pull, |key| {
        issues.get(key).cloned().unwrap_or_else(|| {
            IssueEvidence::Unavailable("fixture omitted the referenced issue".to_string())
        })
    })
}

fn verify_expected(report: &Report, expected: &FixtureExpected) -> Result<()> {
    if report.aggregate_code != expected.aggregate_code {
        bail!(
            "fixture aggregate mismatch: expected {}, observed {}",
            expected.aggregate_code.as_str(),
            report.aggregate_code.as_str()
        );
    }
    if report.rows.len() != expected.rows.len() {
        bail!(
            "fixture row-count mismatch: expected {}, observed {}",
            expected.rows.len(),
            report.rows.len()
        );
    }
    for (observed, expected_row) in report.rows.iter().zip(&expected.rows) {
        let expected_repo = canonical_repository(&expected_row.repository)?;
        if observed.repository != expected_repo
            || observed.issue_number != expected_row.issue_number
            || observed.code != expected_row.code
        {
            bail!(
                "fixture row mismatch for {}#{}: expected {}, observed {}#{} {}",
                expected_repo,
                expected_row.issue_number,
                expected_row.code.as_str(),
                observed.repository,
                observed.issue_number,
                observed.code.as_str()
            );
        }
    }
    Ok(())
}

fn evaluate<F>(pull: &PullRequestSubject, mut issue_lookup: F) -> Result<Report>
where
    F: FnMut(&IssueKey) -> IssueEvidence,
{
    let relations = parse_closing_relations(&pull.body, &pull.repository)?;
    if relations.is_empty() {
        return Ok(Report {
            schema_version: REPORT_SCHEMA,
            repository: pull.repository.clone(),
            pull_request_number: pull.number,
            pull_request_title: pull.title.clone(),
            aggregate_code: ResultCode::PassNotApplicable,
            semantic_completion_proven: false,
            rows: Vec::new(),
        });
    }

    let sections = parse_sections(&pull.body, MAX_PR_BODY_BYTES)?;
    let relation_count = relations.len();
    let mut rows = Vec::with_capacity(relation_count);
    for relation in relations {
        let evidence = issue_lookup(&relation.key);
        rows.push(evaluate_relation(
            pull,
            &sections,
            relation_count,
            relation,
            evidence,
        ));
    }

    let aggregate_code = rows
        .iter()
        .find(|row| row.code.is_failure())
        .map(|row| row.code)
        .or_else(|| {
            rows.iter()
                .find(|row| row.code.is_not_proven())
                .map(|row| row.code)
        })
        .unwrap_or(ResultCode::PassNoHighConfidenceContradiction);

    Ok(Report {
        schema_version: REPORT_SCHEMA,
        repository: pull.repository.clone(),
        pull_request_number: pull.number,
        pull_request_title: pull.title.clone(),
        aggregate_code,
        semantic_completion_proven: false,
        rows,
    })
}

fn evaluate_relation(
    pull: &PullRequestSubject,
    sections: &BTreeMap<SectionKind, Section>,
    relation_count: usize,
    relation: ClosingRelation,
    evidence: IssueEvidence,
) -> RelationResult {
    let unavailable = |code: ResultCode, reason: String| RelationResult {
        repository: relation.key.repository.clone(),
        issue_number: relation.key.number,
        keyword: relation.keyword.clone(),
        source_line: relation.source_line.clone(),
        line_number: relation.line_number,
        code,
        rule_id: None,
        reason,
        suggested_relation: None,
        retirement_mapping: None,
    };

    let issue = match evidence {
        IssueEvidence::Available(issue) => issue,
        IssueEvidence::Unavailable(reason) => {
            return unavailable(
                ResultCode::NotProvenGithub,
                format!(
                    "terminal relation could not be checked against its issue subject: {}",
                    sanitize_for_output(&reason, 512)
                ),
            );
        }
    };

    if issue.number != relation.key.number {
        return unavailable(
            ResultCode::InstrumentFailure,
            "issue lookup returned a different issue number".to_string(),
        );
    }
    if issue.body.len() > MAX_ISSUE_BODY_BYTES {
        return unavailable(
            ResultCode::InstrumentFailure,
            "issue body exceeded the bounded semantic-containment input".to_string(),
        );
    }

    let issue_sections = match parse_sections(&issue.body, MAX_ISSUE_BODY_BYTES) {
        Ok(parsed) => parsed,
        Err(error) => {
            return unavailable(
                ResultCode::InstrumentFailure,
                format!(
                    "issue structure could not be parsed within bounds: {}",
                    sanitize_for_output(&error.to_string(), 512)
                ),
            );
        }
    };

    let controlling = section_text(sections, &[SectionKind::ControllingIssue]);
    let scoped_to_issue = relation_count == 1
        || references_issue(
            &controlling,
            &relation.key,
            &pull.repository,
        );

    if issue_is_controller(&issue)
        && !has_semantic_close_packet(sections, &relation.key, &pull.repository)
    {
        return failed_row(
            &relation,
            ResultCode::FailControllerPacketMissing,
            RuleId::ControllerPacketMissing,
            "terminal closure targets a controller/programme issue without an explicit semantic close packet reference",
            suggested_advances(&relation.key, &pull.repository),
        );
    }

    if issue_names_pull_as_historical_predecessor(&issue.body, pull.number) {
        return failed_row(
            &relation,
            ResultCode::FailPredecessorSuccessorCollapse,
            RuleId::PredecessorSuccessorCollapse,
            "the issue identifies this PR as historical predecessor/deletion evidence, not proof that the surviving successor proposition is retired",
            suggested_refs(&relation.key, &pull.repository),
        );
    }

    if proof_level_is_explicitly_excluded(sections, &issue_sections) {
        return failed_row(
            &relation,
            ResultCode::FailProofLevelContradiction,
            RuleId::ProofLevelContradiction,
            "the PR explicitly excludes an installed/public/packaged/presentation proof level required by the issue",
            suggested_advances(&relation.key, &pull.repository),
        );
    }

    let remaining = section_text(sections, &[SectionKind::RemainingWork]);
    if references_issue(&remaining, &relation.key, &pull.repository) {
        return failed_row(
            &relation,
            ResultCode::FailRemainingWorkSameIssue,
            RuleId::RemainingSameIssue,
            "the structured Remaining work section assigns required work to the same issue that the PR asks GitHub to close",
            suggested_advances(&relation.key, &pull.repository),
        );
    }

    if scoped_to_issue && !issue_is_phase_leaf(&issue) {
        let phase_text = format!(
            "{}\n{}",
            section_text(sections, &[SectionKind::ClaimBoundary]),
            relation.source_line
        );
        if contains_partial_boundary(&phase_text) {
            return failed_row(
                &relation,
                ResultCode::FailPhaseTerminalRelation,
                RuleId::PhaseTerminal,
                "the PR's structured boundary describes a phase/partial/slice while requesting terminal closure of the broader issue",
                suggested_advances(&relation.key, &pull.repository),
            );
        }
    }

    if scoped_to_issue && explicitly_not_proven_required_work(sections) {
        return failed_row(
            &relation,
            ResultCode::FailExplicitUnprovenRequiredWork,
            RuleId::ExplicitlyNotProven,
            "the structured claim boundary explicitly says required full/complete issue work is not proved or established",
            suggested_advances(&relation.key, &pull.repository),
        );
    }

    RelationResult {
        repository: relation.key.repository.clone(),
        issue_number: relation.key.number,
        keyword: relation.keyword.clone(),
        source_line: relation.source_line.clone(),
        line_number: relation.line_number,
        code: ResultCode::PassNoHighConfidenceContradiction,
        rule_id: None,
        reason: "no supported high-confidence contradiction was found; this is not semantic issue-close proof"
            .to_string(),
        suggested_relation: None,
        retirement_mapping: None,
    }
}

fn failed_row(
    relation: &ClosingRelation,
    code: ResultCode,
    rule: RuleId,
    reason: &str,
    suggested_relation: String,
) -> RelationResult {
    RelationResult {
        repository: relation.key.repository.clone(),
        issue_number: relation.key.number,
        keyword: relation.keyword.clone(),
        source_line: relation.source_line.clone(),
        line_number: relation.line_number,
        code,
        rule_id: Some(rule.as_str().to_string()),
        reason: reason.to_string(),
        suggested_relation: Some(suggested_relation),
        retirement_mapping: Some(rule.retirement_mapping().to_string()),
    }
}

fn fetch_issue_live(key: &IssueKey) -> IssueEvidence {
    if let Err(error) = canonical_repository(&key.repository) {
        return IssueEvidence::Unavailable(error.to_string());
    }
    let endpoint = format!("repos/{}/issues/{}", key.repository, key.number);
    let output = match Command::new("gh")
        .args(["api", "--method", "GET", &endpoint])
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            return IssueEvidence::Unavailable(format!("failed to start gh api: {error}"));
        }
    };
    if !output.status.success() {
        return IssueEvidence::Unavailable(format!(
            "gh api exited with status {}",
            output.status
        ));
    }
    if output.stdout.len() > MAX_GITHUB_OUTPUT_BYTES {
        return IssueEvidence::Unavailable("GitHub issue response exceeded the input bound".into());
    }
    let payload: GithubIssuePayload = match serde_json::from_slice(&output.stdout) {
        Ok(payload) => payload,
        Err(error) => {
            return IssueEvidence::Unavailable(format!("invalid GitHub issue response: {error}"));
        }
    };
    if payload.number != key.number {
        return IssueEvidence::Unavailable("GitHub returned a different issue number".into());
    }
    if payload.pull_request.is_some() {
        return IssueEvidence::Unavailable(
            "terminal relation resolved to a pull request rather than an issue".into(),
        );
    }
    IssueEvidence::Available(IssueSubject {
        number: payload.number,
        title: payload.title,
        body: payload.body.unwrap_or_default(),
    })
}

fn parse_closing_relations(body: &str, current_repository: &str) -> Result<Vec<ClosingRelation>> {
    if body.len() > MAX_PR_BODY_BYTES {
        bail!("PR body exceeds the bounded containment input");
    }
    let relation_re = Regex::new(
        r"(?i)\b(close(?:s|d)?|fix(?:es|ed)?|resolve(?:s|d)?)\s+(?:(?P<repo>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+))?#(?P<number>[0-9]+)\b",
    )
    .context("compiling closing-relation parser")?;
    let mut relations = Vec::new();
    let mut fence: Option<char> = None;

    for (index, line) in body.lines().enumerate() {
        if line.len() > MAX_SOURCE_LINE_BYTES {
            bail!("PR body contains a line larger than the relation-parser bound");
        }
        let trimmed = line.trim_start();
        if let Some(marker) = fence_marker(trimmed) {
            if fence == Some(marker) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(marker);
            }
            continue;
        }
        if fence.is_some() || trimmed.starts_with('>') {
            continue;
        }

        for capture in relation_re.captures_iter(line) {
            if relations.len() >= MAX_RELATIONS {
                bail!("PR body exceeds the supported terminal-relation count");
            }
            let keyword = capture
                .get(1)
                .map(|value| value.as_str().to_ascii_lowercase())
                .context("closing relation omitted keyword")?;
            let repository = match capture.name("repo") {
                Some(value) => canonical_repository(value.as_str())?,
                None => current_repository.to_string(),
            };
            let number = capture
                .name("number")
                .context("closing relation omitted issue number")?
                .as_str()
                .parse::<u64>()
                .context("parsing closing relation issue number")?;
            if number == 0 {
                continue;
            }
            relations.push(ClosingRelation {
                key: IssueKey { repository, number },
                keyword,
                source_line: sanitize_for_output(line.trim(), MAX_SOURCE_LINE_BYTES),
                line_number: index + 1,
            });
        }
    }
    Ok(relations)
}

fn parse_sections(body: &str, max_body_bytes: usize) -> Result<BTreeMap<SectionKind, Section>> {
    if body.len() > max_body_bytes {
        bail!("Markdown body exceeds the bounded section-parser input");
    }
    let mut sections: BTreeMap<SectionKind, Section> = BTreeMap::new();
    let mut current: Option<SectionKind> = None;
    let mut fence: Option<char> = None;

    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(marker) = fence_marker(trimmed) {
            if fence == Some(marker) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(marker);
            }
            continue;
        }
        if fence.is_some() || trimmed.starts_with('>') {
            continue;
        }
        if let Some((kind, heading)) = classify_heading(trimmed) {
            let section = sections.entry(kind).or_default();
            section.headings.push(heading);
            current = Some(kind);
            continue;
        }
        if let Some(kind) = current {
            let section = sections.entry(kind).or_default();
            if section.body.len() + line.len() + 1 > MAX_SECTION_BYTES {
                bail!("stable PR section exceeds the configured byte bound");
            }
            if !section.body.is_empty() {
                section.body.push('\n');
            }
            section.body.push_str(line);
        }
    }
    Ok(sections)
}

fn classify_heading(line: &str) -> Option<(SectionKind, String)> {
    let hash_count = line.chars().take_while(|character| *character == '#').count();
    if !(2..=6).contains(&hash_count) {
        return None;
    }
    let heading = line.get(hash_count..)?.trim().trim_matches('*').trim();
    if heading.is_empty() {
        return None;
    }
    let normalized = heading.to_ascii_lowercase().replace('_', " ");
    let kind = if normalized == "claim" {
        SectionKind::Claim
    } else if normalized.starts_with("controlling issue") {
        SectionKind::ControllingIssue
    } else if normalized.starts_with("governing contract") {
        SectionKind::GoverningContract
    } else if normalized.starts_with("claim boundary") {
        SectionKind::ClaimBoundary
    } else if normalized.starts_with("non-goals") || normalized.starts_with("non goals") {
        SectionKind::NonGoals
    } else if normalized.starts_with("remaining work")
        || normalized.starts_with("remaining ungated callers")
        || normalized.starts_with("remaining tasks")
    {
        SectionKind::RemainingWork
    } else if normalized == "summary" {
        SectionKind::Summary
    } else if normalized == "changes" || normalized == "what changed" {
        SectionKind::Changes
    } else if normalized.starts_with("review map") {
        SectionKind::ReviewMap
    } else if normalized.starts_with("acceptance") {
        SectionKind::Acceptance
    } else if normalized == "objective" {
        SectionKind::Objective
    } else if normalized == "outcome" {
        SectionKind::Outcome
    } else {
        return None;
    };
    Some((kind, sanitize_for_output(heading, 512)))
}

fn fence_marker(line: &str) -> Option<char> {
    if line.starts_with("```") {
        Some('`')
    } else if line.starts_with("~~~") {
        Some('~')
    } else {
        None
    }
}

fn section_text(sections: &BTreeMap<SectionKind, Section>, kinds: &[SectionKind]) -> String {
    let mut text = String::new();
    for kind in kinds {
        if let Some(section) = sections.get(kind) {
            for heading in &section.headings {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(heading);
            }
            if !section.body.is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&section.body);
            }
        }
    }
    text
}

fn issue_is_controller(issue: &IssueSubject) -> bool {
    let title = issue.title.to_ascii_lowercase();
    let body = issue.body.to_ascii_lowercase();
    title.starts_with("program(")
        || title.starts_with("programme(")
        || title.starts_with("controller(")
        || body.contains("this is the **accepted") && body.contains("controller")
        || body.contains("not a coding pr unit")
}

fn has_semantic_close_packet(
    sections: &BTreeMap<SectionKind, Section>,
    key: &IssueKey,
    current_repository: &str,
) -> bool {
    let contract = section_text(sections, &[SectionKind::GoverningContract]);
    let lower = contract.to_ascii_lowercase();
    (lower.contains("semantic close packet") || lower.contains("close packet"))
        && references_issue(&contract, key, current_repository)
}

fn issue_names_pull_as_historical_predecessor(issue_body: &str, pull_number: u64) -> bool {
    issue_body.lines().any(|line| {
        let lower = line.trim().to_ascii_lowercase();
        (lower.starts_with("historical deletion:")
            || lower.starts_with("historical predecessor:"))
            && references_number(line, pull_number)
    })
}

fn proof_level_is_explicitly_excluded(
    pr_sections: &BTreeMap<SectionKind, Section>,
    issue_sections: &BTreeMap<SectionKind, Section>,
) -> bool {
    let issue_requirements = section_text(
        issue_sections,
        &[
            SectionKind::Acceptance,
            SectionKind::Objective,
            SectionKind::Outcome,
        ],
    )
    .to_ascii_lowercase();
    let exclusions = section_text(
        pr_sections,
        &[SectionKind::ClaimBoundary, SectionKind::NonGoals],
    )
    .to_ascii_lowercase();
    if !contains_explicit_exclusion(&exclusions) {
        return false;
    }
    const TERMS: [&str; 6] = [
        "installed",
        "public",
        "packaged",
        "presentation",
        "release",
        "actual host",
    ];
    TERMS
        .iter()
        .any(|term| issue_requirements.contains(term) && exclusions.contains(term))
}

fn explicitly_not_proven_required_work(
    sections: &BTreeMap<SectionKind, Section>,
) -> bool {
    let text = section_text(
        sections,
        &[SectionKind::ClaimBoundary, SectionKind::NonGoals],
    )
    .to_ascii_lowercase();
    contains_explicit_exclusion(&text)
        && [
            "full",
            "complete",
            "remaining",
            "every",
            "all ",
            "installed",
            "public",
            "packaged",
            "acceptance",
            "presentation",
        ]
        .iter()
        .any(|marker| text.contains(marker))
}

fn contains_explicit_exclusion(text: &str) -> bool {
    [
        "does not prove",
        "does not establish",
        "does not claim",
        "not proved",
        "not proven",
        "not established",
        "not claimed",
        "explicitly out of scope",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn contains_partial_boundary(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    (contains_word(&lower, "phase") && contains_word(&lower, "only"))
        || contains_word(&lower, "partial")
        || lower.contains("slice only")
        || lower.contains("bounded slice")
}

fn issue_is_phase_leaf(issue: &IssueSubject) -> bool {
    let text = format!("{}\n{}", issue.title, issue.body).to_ascii_lowercase();
    text.contains("this is phase ")
        || text.contains("phase leaf")
        || text.contains("phase-leaf")
        || text.contains("phase_leaf")
}

fn references_issue(text: &str, key: &IssueKey, current_repository: &str) -> bool {
    if key.repository == current_repository && references_number(text, key.number) {
        return true;
    }
    let qualified = format!("{}#{}", key.repository, key.number);
    text.to_ascii_lowercase().contains(&qualified)
}

fn references_number(text: &str, number: u64) -> bool {
    let needle = format!("#{number}");
    text.match_indices(&needle).any(|(index, _)| {
        let after = index + needle.len();
        text.get(after..)
            .and_then(|tail| tail.chars().next())
            .is_none_or(|character| !character.is_ascii_digit())
    })
}

fn contains_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(index, _)| {
        let before_ok = text
            .get(..index)
            .and_then(|prefix| prefix.chars().next_back())
            .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_');
        let after_index = index + word.len();
        let after_ok = text
            .get(after_index..)
            .and_then(|suffix| suffix.chars().next())
            .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_');
        before_ok && after_ok
    })
}

fn suggested_advances(key: &IssueKey, current_repository: &str) -> String {
    if key.repository == current_repository {
        format!("Advances #{}", key.number)
    } else {
        format!("Advances {}#{}", key.repository, key.number)
    }
}

fn suggested_refs(key: &IssueKey, current_repository: &str) -> String {
    if key.repository == current_repository {
        format!("Refs #{}", key.number)
    } else {
        format!("Refs {}#{}", key.repository, key.number)
    }
}

fn canonical_repository(repository: &str) -> Result<String> {
    let trimmed = repository.trim();
    if trimmed.len() > 200 {
        bail!("repository identity exceeds 200 bytes");
    }
    let (owner, name) = trimmed
        .split_once('/')
        .context("repository identity must be owner/name")?;
    if owner.is_empty()
        || name.is_empty()
        || name.contains('/')
        || !owner.chars().all(valid_repo_character)
        || !name.chars().all(valid_repo_character)
    {
        bail!("invalid repository identity {trimmed:?}");
    }
    Ok(format!(
        "{}/{}",
        owner.to_ascii_lowercase(),
        name.to_ascii_lowercase()
    ))
}

fn valid_repo_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

fn is_full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character))
}

fn read_bounded(path: &Path, max_bytes: usize, label: &str) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path).with_context(|| format!("reading {label} metadata"))?;
    let length = usize::try_from(metadata.len()).context("input size does not fit usize")?;
    if length > max_bytes {
        bail!("{label} exceeds {max_bytes} bytes");
    }
    let bytes = fs::read(path).with_context(|| format!("reading {label}"))?;
    if bytes.len() > max_bytes {
        bail!("{label} exceeded the bound while being read");
    }
    Ok(bytes)
}

fn sanitize_for_output(value: &str, max_bytes: usize) -> String {
    let mut output = String::new();
    for character in value.chars() {
        let rendered = if character.is_control() && character != '\t' {
            '�'
        } else {
            character
        };
        if output.len() + rendered.len_utf8() > max_bytes {
            output.push('…');
            break;
        }
        output.push(rendered);
    }
    output
}

fn print_human(report: &Report) {
    println!(
        "{}   {}#{}   {}",
        report.aggregate_code.as_str(),
        report.repository,
        report.pull_request_number,
        sanitize_for_output(&report.pull_request_title, 512)
    );
    if report.rows.is_empty() {
        println!("  no automatic closing relation; issue/domain lookup skipped");
        return;
    }
    for row in &report.rows {
        println!(
            "  {}   {}#{}   line {}",
            row.code.as_str(),
            row.repository,
            row.issue_number,
            row.line_number
        );
        if let Some(rule) = &row.rule_id {
            println!("    rule: {rule}");
        }
        println!("    reason: {}", row.reason);
        println!("    relation: {}", row.source_line);
        if let Some(replacement) = &row.suggested_relation {
            println!("    replacement: {replacement}");
        }
        if let Some(mapping) = &row.retirement_mapping {
            println!("    retirement: {mapping}");
        }
    }
    println!(
        "  semantic_completion_proven: false (a containment pass is never semantic-close proof)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const FIXTURES: [(&str, &str); 12] = [
        (
            "invalid-phase-terminal-5023-5001",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../.ci/semantic-close-containment/fixtures/invalid-phase-terminal-5023-5001.json"
            )),
        ),
        (
            "invalid-partial-slice-6239-5016",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../.ci/semantic-close-containment/fixtures/invalid-partial-slice-6239-5016.json"
            )),
        ),
        (
            "invalid-proof-level-6282-5901",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../.ci/semantic-close-containment/fixtures/invalid-proof-level-6282-5901.json"
            )),
        ),
        (
            "invalid-predecessor-successor-5968-5231",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../.ci/semantic-close-containment/fixtures/invalid-predecessor-successor-5968-5231.json"
            )),
        ),
        (
            "invalid-controller-no-packet",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../.ci/semantic-close-containment/fixtures/invalid-controller-no-packet.json"
            )),
        ),
        (
            "invalid-explicit-unproven",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../.ci/semantic-close-containment/fixtures/invalid-explicit-unproven.json"
            )),
        ),
        (
            "invalid-remaining-same-issue",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../.ci/semantic-close-containment/fixtures/invalid-remaining-same-issue.json"
            )),
        ),
        (
            "valid-controller-packet",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../.ci/semantic-close-containment/fixtures/valid-controller-packet.json"
            )),
        ),
        (
            "valid-phase-leaf-2624",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../.ci/semantic-close-containment/fixtures/valid-phase-leaf-2624.json"
            )),
        ),
        (
            "valid-atomic",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../.ci/semantic-close-containment/fixtures/valid-atomic.json"
            )),
        ),
        (
            "no-terminal-relation",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../.ci/semantic-close-containment/fixtures/no-terminal-relation.json"
            )),
        ),
        (
            "multiple-relations-one-invalid",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../.ci/semantic-close-containment/fixtures/multiple-relations-one-invalid.json"
            )),
        ),
    ];

    #[test]
    fn immutable_fixture_matrix_matches_expected_dispositions() -> Result<()> {
        for (name, raw) in FIXTURES {
            let fixture: Fixture = serde_json::from_str(raw)
                .with_context(|| format!("parsing embedded fixture {name}"))?;
            validate_fixture(&fixture)
                .with_context(|| format!("validating embedded fixture {name}"))?;
            let report = evaluate_fixture(&fixture)
                .with_context(|| format!("evaluating embedded fixture {name}"))?;
            verify_expected(&report, &fixture.expected)
                .with_context(|| format!("checking embedded fixture {name}"))?;
        }
        Ok(())
    }

    #[test]
    fn code_fences_and_blockquotes_do_not_create_terminal_relations() -> Result<()> {
        let body = "```text\nCloses #123\n```\n> Fixes #456\nRefs #789\n";
        let relations = parse_closing_relations(body, "effortlessmetrics/perl-lsp-swarm")?;
        assert!(relations.is_empty());
        Ok(())
    }

    #[test]
    fn cross_repository_terminal_relation_is_normalized() -> Result<()> {
        let relations = parse_closing_relations(
            "Resolves OtherOrg/OtherRepo#42",
            "effortlessmetrics/perl-lsp-swarm",
        )?;
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].key.repository, "otherorg/otherrepo");
        assert_eq!(relations[0].key.number, 42);
        Ok(())
    }

    #[test]
    fn rule_registry_has_explicit_cp03_retirement_mapping() {
        for rule in RuleId::all() {
            assert!(rule.as_str().starts_with("CP00-"));
            assert!(rule.retirement_mapping().starts_with("CP03 "));
        }
    }

    #[test]
    fn hostile_pr_text_is_data_and_never_executed() -> Result<()> {
        let temp = tempdir()?;
        let marker = temp.path().join("must-not-exist");
        let body = format!(
            "## Claim Boundary\nCandidate text: $(touch {})\n\nCloses #9000001\n",
            marker.display()
        );
        let pull = PullRequestSubject {
            repository: "effortlessmetrics/perl-lsp-swarm".into(),
            number: 9000002,
            title: "test: hostile metadata stays inert (#9000001)".into(),
            body,
        };
        let issue = IssueEvidence::Available(IssueSubject {
            number: 9000001,
            title: "fix: atomic fixture".into(),
            body: "## Acceptance\nOne bounded defect is fixed.".into(),
        });
        let report = evaluate(&pull, |_| issue.clone())?;
        assert_eq!(
            report.aggregate_code,
            ResultCode::PassNoHighConfidenceContradiction
        );
        assert!(!marker.exists());
        Ok(())
    }

    #[test]
    fn oversized_pr_body_fails_closed() {
        let body = "x".repeat(MAX_PR_BODY_BYTES + 1);
        let result = parse_closing_relations(&body, "effortlessmetrics/perl-lsp-swarm");
        assert!(result.is_err());
    }

    #[test]
    fn fixture_decoder_rejects_unknown_fields() {
        let raw = r#"{
          "schema_version":"semantic_close_containment_fixture.v1",
          "provenance":{"captured_at":"2026-08-21","sources":["https://github.com/a/b/issues/1"],"subject_shas":[],"boundary":"bounded"},
          "repository":"a/b",
          "pull_request":{"number":2,"title":"x","body":"Refs #1"},
          "issues":[],
          "expected":{"aggregate_code":"PASS_NOT_APPLICABLE","rows":[]},
          "unexpected":true
        }"#;
        let parsed = serde_json::from_str::<Fixture>(raw);
        assert!(parsed.is_err());
    }
}
