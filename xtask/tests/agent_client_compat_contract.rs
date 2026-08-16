#[path = "support/agent_client_compat.rs"]
mod agent_client_compat;

use agent_client_compat::{
    AgentClientCompatReceipt, CANONICAL_EXPECTATION_IDS, EvidenceArtifact, EvidenceStage,
    FailureClass, HostIdentity, HostProduct, IntegrationIdentity, IntegrationMode, JourneyCell,
    ObservationResult, PlatformIdentity, Protocol, SCHEMA_VERSION, ServerIdentity,
    WorkspaceFixtureIdentity, fixture_digest,
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use walkdir::WalkDir;

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live below the repository root")
}

fn fixture_root() -> Result<PathBuf> {
    Ok(repository_root()?.join("crates/perl-lsp-ux-tests/fixtures/agent-client-compat"))
}

fn sha256(fill: char) -> String {
    let mut value = String::with_capacity("sha256:".len() + 64);
    value.push_str("sha256:");
    value.extend(std::iter::repeat_n(fill, 64));
    value
}

fn valid_receipt() -> Result<AgentClientCompatReceipt> {
    Ok(AgentClientCompatReceipt {
        schema_version: SCHEMA_VERSION.to_string(),
        observed_at: "2026-08-12T08:00:00Z".to_string(),
        stage: EvidenceStage::ExactSourceLocal,
        repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
        candidate_sha: "a".repeat(40),
        platform: PlatformIdentity {
            os: "linux".to_string(),
            os_version: "ubuntu-24.04".to_string(),
            arch: "x86_64".to_string(),
        },
        host: HostIdentity {
            product: HostProduct::ClaudeCode,
            version: "2.1.205".to_string(),
            instrument_model: Some("fixture-model".to_string()),
        },
        integration: IntegrationIdentity {
            mode: IntegrationMode::NativeLspPlugin,
            plugin_name: "perl-lsp".to_string(),
            plugin_version: "0.1.0".to_string(),
            marketplace_source: "EffortlessMetrics/perl-lsp-swarm".to_string(),
            marketplace_ref: "refs/heads/test/7237-agent-client-compat".to_string(),
            package_sha256: sha256('1'),
        },
        server: ServerIdentity {
            executable: "perllsp".to_string(),
            version: "0.18.0-dev".to_string(),
            build_revision: "a".repeat(40),
            artifact_sha256: sha256('2'),
            protocol: Protocol::Lsp,
            protocol_or_schema_version: "3.17".to_string(),
        },
        workspace_fixture: WorkspaceFixtureIdentity {
            id: "perl-agent-client-v1".to_string(),
            digest: fixture_digest(&fixture_root()?)?,
        },
        journey: vec![
            JourneyCell {
                id: "definition.cross_file".to_string(),
                result: ObservationResult::Pass,
                evidence: vec!["host-event.definition".to_string()],
                limitation: None,
            },
            JourneyCell {
                id: "lifecycle.shutdown".to_string(),
                result: ObservationResult::Pass,
                evidence: vec!["process-cleanup".to_string()],
                limitation: None,
            },
        ],
        result: ObservationResult::Pass,
        failure_class: None,
        limitations: Vec::new(),
        artifacts: vec![EvidenceArtifact { id: "claude-debug".to_string(), sha256: sha256('4') }],
        claim_boundary: "Exact-source Claude native-LSP fixture cells only.".to_string(),
    })
}

#[test]
fn agent_client_receipt_round_trips_and_validates() -> Result<()> {
    let receipt = valid_receipt()?;
    receipt.validate()?;

    let encoded = serde_json::to_string_pretty(&receipt)?;
    let decoded: AgentClientCompatReceipt = serde_json::from_str(&encoded)?;
    ensure!(decoded == receipt, "serialized receipt did not round-trip exactly");
    ensure!(
        serde_json::to_string_pretty(&decoded)? == encoded,
        "receipt serialization was not deterministic"
    );
    let value = serde_json::to_value(&receipt)?;
    ensure!(
        value.get("limitations").is_some_and(Value::is_array),
        "limitations must serialize when empty"
    );
    ensure!(value.get("artifacts").is_some_and(Value::is_array), "artifacts must always serialize");
    Ok(())
}

#[test]
fn evidence_stages_and_integration_modes_remain_distinct() -> Result<()> {
    let stages = [
        EvidenceStage::StaticPackage,
        EvidenceStage::ExactSourceLocal,
        EvidenceStage::PublicArtifact,
        EvidenceStage::OfficialDirectory,
    ];
    let stage_json =
        stages.iter().map(serde_json::to_value).collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        stage_json
            == vec![
                Value::String("static_package".to_string()),
                Value::String("exact_source_local".to_string()),
                Value::String("public_artifact".to_string()),
                Value::String("official_directory".to_string()),
            ],
        "evidence stages changed serialization identity"
    );

    let modes = [
        IntegrationMode::NativeLspPlugin,
        IntegrationMode::CliSkill,
        IntegrationMode::NativeLocalMcp,
        IntegrationMode::ExternalBridge,
    ];
    let mode_json =
        modes.iter().map(serde_json::to_value).collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        mode_json
            == vec![
                Value::String("native_lsp_plugin".to_string()),
                Value::String("cli_skill".to_string()),
                Value::String("native_local_mcp".to_string()),
                Value::String("external_bridge".to_string()),
            ],
        "integration modes changed serialization identity"
    );
    Ok(())
}

#[test]
fn validation_rejects_cross_protocol_and_false_green_shapes() -> Result<()> {
    let mut wrong_protocol = valid_receipt()?;
    wrong_protocol.server.protocol = Protocol::Mcp;
    ensure!(wrong_protocol.validate().is_err(), "native LSP receipt accepted MCP protocol");

    let mut duplicate_cell = valid_receipt()?;
    let first =
        duplicate_cell.journey.first().context("valid receipt has no journey cell")?.clone();
    duplicate_cell.journey.push(first);
    ensure!(duplicate_cell.validate().is_err(), "duplicate journey id was accepted");

    let mut false_green = valid_receipt()?;
    let first = false_green.journey.first_mut().context("valid receipt has no journey cell")?;
    first.result = ObservationResult::NotProven;
    first.limitation = Some("host did not expose the requested action".to_string());
    ensure!(false_green.validate().is_err(), "passing receipt accepted a not-proven journey cell");

    let mut failed_without_class = valid_receipt()?;
    failed_without_class.result = ObservationResult::Fail;
    ensure!(failed_without_class.validate().is_err(), "failed receipt omitted failure_class");

    let mut invalid_sha = valid_receipt()?;
    invalid_sha.candidate_sha = "ABC".to_string();
    ensure!(invalid_sha.validate().is_err(), "uppercase or short candidate SHA was accepted");

    let mut partial_without_limitation = valid_receipt()?;
    partial_without_limitation.result = ObservationResult::Partial;
    ensure!(
        partial_without_limitation.validate().is_err(),
        "partial receipt omitted its claim limitation"
    );
    Ok(())
}

#[test]
fn validation_rejects_private_or_escaping_artifact_identity() -> Result<()> {
    let mut receipt = valid_receipt()?;
    receipt.artifacts[0].id = "/home/user/claude-debug.log".to_string();
    ensure!(receipt.validate().is_err(), "Unix absolute artifact identity was accepted");

    let mut receipt = valid_receipt()?;
    receipt.artifacts[0].id = "C:/Users/alice/claude-debug.log".to_string();
    ensure!(receipt.validate().is_err(), "Windows drive-qualified artifact identity was accepted");

    let mut receipt = valid_receipt()?;
    receipt.artifacts[0].id = "file:///home/alice/claude-debug.log".to_string();
    ensure!(receipt.validate().is_err(), "URI-qualified artifact identity was accepted");

    let mut receipt = valid_receipt()?;
    receipt.integration.marketplace_ref = "../../private/checkout".to_string();
    ensure!(receipt.validate().is_err(), "parent-traversing marketplace ref was accepted");

    let mut receipt = valid_receipt()?;
    receipt.result = ObservationResult::NotProven;
    receipt.failure_class = Some(FailureClass::Instrument);
    let first = receipt.journey.first_mut().context("valid receipt has no journey cell")?;
    first.result = ObservationResult::NotProven;
    first.limitation = None;
    ensure!(receipt.validate().is_err(), "not-proven journey cell omitted its limitation");
    Ok(())
}

#[test]
fn subject_invalidation_is_identity_based_not_age_based() -> Result<()> {
    let previous = valid_receipt()?;
    let mut current = previous.clone();
    current.observed_at = "2026-09-01T00:00:00Z".to_string();
    current.candidate_sha = "b".repeat(40);
    ensure!(
        previous.subject_invalidations_against(&current).is_empty(),
        "age or unrelated candidate movement invalidated an unchanged subject"
    );

    current.host.version = "2.2.0".to_string();
    current.integration.package_sha256 = sha256('5');
    current.server.artifact_sha256 = sha256('6');
    current.workspace_fixture.digest = sha256('7');
    current.stage = EvidenceStage::PublicArtifact;
    current.artifacts[0].sha256 = sha256('8');
    current.claim_boundary = "Public-artifact fixture cells only.".to_string();

    ensure!(
        previous.subject_invalidations_against(&current)
            == BTreeSet::from([
                "artifacts",
                "claim_boundary",
                "evidence_stage",
                "fixture",
                "host",
                "plugin",
                "server",
            ]),
        "subject invalidation did not report every changed identity dimension"
    );
    Ok(())
}

#[test]
fn canonical_fixture_digest_is_deterministic_and_source_bound() -> Result<()> {
    let root = fixture_root()?;
    let first = fixture_digest(&root)?;
    let second = fixture_digest(&root)?;
    ensure!(first == second, "canonical fixture digest was not deterministic");
    ensure!(first.starts_with("sha256:"), "fixture digest omitted sha256 identity prefix");
    ensure!(first.len() == "sha256:".len() + 64, "fixture digest had the wrong encoded length");

    let copy = TempDir::new()?;
    copy_fixture(&root, copy.path())?;
    let before = fixture_digest(copy.path())?;
    fs::write(copy.path().join("app.pl"), b"# mutated\n")?;
    ensure!(fixture_digest(copy.path())? != before, "fixture digest ignored changed file bytes");

    let receipt = valid_receipt()?;
    ensure!(
        receipt.workspace_fixture.digest == first,
        "canonical receipt did not carry the canonical fixture digest"
    );

    let files = WalkFixture::new(&root)?.files;
    ensure!(
        files
            == BTreeSet::from([
                "app.pl".to_string(),
                "broken.pl".to_string(),
                "lib/Widget.pm".to_string(),
                "unicode.pl".to_string(),
            ]),
        "canonical fixture membership drifted"
    );
    Ok(())
}

struct WalkFixture {
    files: BTreeSet<String>,
}

impl WalkFixture {
    fn new(root: &Path) -> Result<Self> {
        let mut files = BTreeSet::new();
        for entry in walkdir::WalkDir::new(root) {
            let entry = entry?;
            if entry.file_type().is_file() {
                files.insert(entry.path().strip_prefix(root)?.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(Self { files })
    }
}

#[test]
fn canonical_fixture_has_independent_expected_cells() -> Result<()> {
    let expectations = CANONICAL_EXPECTATION_IDS.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        expectations.len() == CANONICAL_EXPECTATION_IDS.len(),
        "canonical expectation ids contain duplicates"
    );
    ensure!(expectations.len() == 12, "canonical expectation count changed");
    ensure!(expectations.contains("definition.widget_new"), "definition expectation is missing");
    ensure!(
        expectations.contains("edit_requery.widget_greet"),
        "edit/re-query expectation is missing"
    );
    ensure!(
        expectations.contains("workspace.partial_not_ready"),
        "workspace readiness expectation is missing"
    );
    Ok(())
}

#[test]
fn checked_in_schema_names_the_same_contract_and_stage_boundaries() -> Result<()> {
    let schema_path = repository_root()?.join(".ci/schemas/agent-client-compat.v1.schema.json");
    let schema: Value = serde_json::from_str(&fs::read_to_string(schema_path)?)?;

    ensure!(schema["title"] == SCHEMA_VERSION, "schema title drifted from Rust contract");
    ensure!(
        schema["properties"]["schema_version"]["const"] == SCHEMA_VERSION,
        "schema version const drifted from Rust contract"
    );
    let stages = schema["properties"]["stage"]["enum"].as_array().context("stage enum missing")?;
    for expected in
        ["static_package", "exact_source_local", "public_artifact", "official_directory"]
    {
        ensure!(
            stages.iter().any(|value| value.as_str() == Some(expected)),
            "missing evidence stage {expected}"
        );
    }
    let serialized = serde_json::to_value(valid_receipt()?)?;
    for required in schema["required"].as_array().context("top-level required list missing")? {
        let name = required.as_str().context("required property name is not a string")?;
        ensure!(
            serialized.get(name).is_some(),
            "valid receipt omitted schema-required property {name}"
        );
    }
    Ok(())
}

fn copy_fixture(source: &Path, destination: &Path) -> Result<()> {
    for entry in WalkDir::new(source) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
