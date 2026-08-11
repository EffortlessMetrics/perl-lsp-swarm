//! Fail-closed publication-drift classifier for exact swarm/public repository observations.

use clap::Parser;
use color_eyre::eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const SUPPORTED_SCHEMA_VERSION: u32 = 1;
const EXPECTED_TRANSLATION: &str = "expected_publication_translation";
const APPROVED_EXCLUSION: &str = "approved_lineage_exclusion";
const RELEASE_METADATA: &str = "release_metadata_only";
const PRODUCT_DRIFT: &str = "product_drift";
const NOT_PROVEN_CLASS: &str = "unknown_or_not_proven";
const REQUIRED_INVARIANTS: &[&str] = &[
    "targets_requested_are_built",
    "archive_members_match_consumers",
    "server_dap_pairing",
    "extension_claims_match_vsix",
    "public_install_docs_are_executable",
    "support_posture_matches_claims",
    "artifact_traceable_to_public_sha",
    "product_path_coverage_complete",
    "release_repo_unique_dispositions_complete",
];

#[derive(Debug, Parser)]
#[command(about = "Classify an exact-SHA publication drift observation")]
struct Args {
    /// Comparison observation JSON.
    #[arg(long)]
    input: PathBuf,

    /// Receipt JSON written even when the verdict blocks promotion.
    #[arg(long, default_value = "target/receipts/publication-drift.json")]
    out: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SubjectIdentity {
    repository: String,
    sha: String,
    tree_digest: String,
    version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestIdentity {
    path: String,
    sha256: String,
    swarm_sha: String,
    public_sha: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    schema_version: u32,
    swarm: SubjectIdentity,
    public: SubjectIdentity,
    manifest: Option<ManifestIdentity>,
    differences: Option<Vec<ObservedDifference>>,
    invariants: Option<Vec<ObservedInvariant>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservedDifference {
    path: String,
    classification: String,
    behavior_changed: bool,
    manifest_rule: Option<String>,
    owner: String,
    evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservedInvariant {
    id: String,
    status: String,
    owner: String,
    evidence: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Verdict {
    Clean,
    Drift,
    NotProven,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: u32,
    swarm: SubjectIdentity,
    public: SubjectIdentity,
    manifest: Option<ManifestIdentity>,
    differences: Vec<ClassifiedDifference>,
    invariants: Vec<ClassifiedInvariant>,
    authority_valid: bool,
    blockers: Vec<Blocker>,
    verdict: Verdict,
}

#[derive(Debug, Serialize)]
struct ClassifiedDifference {
    path: String,
    declared_classification: String,
    effective_classification: String,
    behavior_changed: bool,
    manifest_rule: Option<String>,
    owner: String,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ClassifiedInvariant {
    id: String,
    status: String,
    owner: String,
    evidence: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Blocker {
    code: String,
    message: String,
    owner: String,
}

#[derive(Debug, Default)]
struct ClassificationState {
    drift: bool,
    not_proven: bool,
    blockers: Vec<Blocker>,
}

impl ClassificationState {
    fn mark_drift(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        owner: impl Into<String>,
    ) {
        self.drift = true;
        self.push_blocker(code, message, owner);
    }

    fn mark_not_proven(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        owner: impl Into<String>,
    ) {
        self.not_proven = true;
        self.push_blocker(code, message, owner);
    }

    fn push_blocker(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        owner: impl Into<String>,
    ) {
        self.blockers.push(Blocker {
            code: code.into(),
            message: message.into(),
            owner: owner.into(),
        });
    }
}

#[allow(dead_code)]
fn main() -> Result<()> {
    run()
}

pub(crate) fn run() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let observation = load_observation(&args.input)?;
    let receipt = classify(observation);
    write_receipt(&args.out, &receipt)?;

    match receipt.verdict {
        Verdict::Clean => {
            println!(
                "publication-drift: clean comparison {} -> {}",
                receipt.swarm.sha, receipt.public.sha
            );
            Ok(())
        }
        Verdict::Drift => bail!(
            "publication-drift: product drift detected; see {}",
            args.out.display()
        ),
        Verdict::NotProven => bail!(
            "publication-drift: comparison not proven; see {}",
            args.out.display()
        ),
    }
}

fn load_observation(path: &Path) -> Result<Observation> {
    let raw = fs::read_to_string(path)
        .wrap_err_with(|| format!("reading publication drift observation {}", path.display()))?;
    serde_json::from_str(&raw)
        .wrap_err_with(|| format!("parsing publication drift observation {}", path.display()))
}

fn classify(observation: Observation) -> Receipt {
    let Observation {
        schema_version,
        swarm,
        public,
        manifest,
        differences,
        invariants,
    } = observation;
    let mut state = ClassificationState::default();

    if schema_version != SUPPORTED_SCHEMA_VERSION {
        state.mark_not_proven(
            "unsupported_schema_version",
            format!(
                "unsupported schema version {schema_version}; expected {SUPPORTED_SCHEMA_VERSION}"
            ),
            "release-engineering",
        );
    }

    validate_subject("swarm", &swarm, &mut state);
    validate_subject("public", &public, &mut state);
    if swarm.repository == public.repository {
        state.mark_not_proven(
            "repository_identity_collision",
            format!(
                "swarm and public subjects identify the same repository {:?}",
                swarm.repository
            ),
            "release-engineering",
        );
    }

    validate_manifest(manifest.as_ref(), &swarm, &public, &mut state);

    let observed_differences = match differences {
        Some(differences) => differences,
        None => {
            state.mark_not_proven(
                "differences_collection_missing",
                "observation omitted required differences collection",
                "release-engineering",
            );
            Vec::new()
        }
    };
    let observed_invariants = match invariants {
        Some(invariants) => invariants,
        None => {
            state.mark_not_proven(
                "invariants_collection_missing",
                "observation omitted required invariants collection",
                "release-engineering",
            );
            Vec::new()
        }
    };

    let mut seen_paths = BTreeSet::new();
    let mut classified_differences = Vec::new();
    for difference in observed_differences {
        validate_owner(
            "difference",
            &difference.path,
            &difference.owner,
            &mut state,
        );
        if !valid_repository_path(&difference.path) {
            state.mark_not_proven(
                "invalid_difference_path",
                format!(
                    "difference path must use canonical repository-relative syntax: {:?}",
                    difference.path
                ),
                owner_or_default(&difference.owner),
            );
        }
        if !seen_paths.insert(difference.path.clone()) {
            state.mark_not_proven(
                "duplicate_difference_path",
                format!("difference path appears more than once: {:?}", difference.path),
                owner_or_default(&difference.owner),
            );
        }
        validate_evidence(
            "difference",
            &difference.path,
            &difference.evidence,
            &difference.owner,
            &mut state,
        );

        let mut effective = difference.classification.clone();
        if !allowed_classification(&difference.classification) {
            effective = NOT_PROVEN_CLASS.to_string();
            state.mark_not_proven(
                "unknown_difference_classification",
                format!(
                    "difference {:?} has unknown classification {:?}",
                    difference.path, difference.classification
                ),
                owner_or_default(&difference.owner),
            );
        }

        if requires_manifest_rule(&difference.classification)
            && difference
                .manifest_rule
                .as_deref()
                .is_none_or(|rule| rule.trim().is_empty())
        {
            state.mark_not_proven(
                "manifest_rule_missing",
                format!(
                    "difference {:?} is declared {:?} without a manifest rule",
                    difference.path, difference.classification
                ),
                owner_or_default(&difference.owner),
            );
        }

        if difference.behavior_changed && effective != PRODUCT_DRIFT {
            effective = PRODUCT_DRIFT.to_string();
            state.mark_drift(
                "behavioral_translation_is_product_drift",
                format!(
                    "difference {:?} changes behavior and cannot be accepted as {:?}",
                    difference.path, difference.classification
                ),
                owner_or_default(&difference.owner),
            );
        }

        match effective.as_str() {
            PRODUCT_DRIFT => state.drift = true,
            NOT_PROVEN_CLASS => state.not_proven = true,
            EXPECTED_TRANSLATION | APPROVED_EXCLUSION | RELEASE_METADATA => {}
            _ => state.not_proven = true,
        }

        classified_differences.push(ClassifiedDifference {
            path: difference.path,
            declared_classification: difference.classification,
            effective_classification: effective,
            behavior_changed: difference.behavior_changed,
            manifest_rule: difference.manifest_rule,
            owner: difference.owner,
            evidence: difference.evidence,
        });
    }
    classified_differences.sort_by(|left, right| left.path.cmp(&right.path));

    let mut seen_invariants = BTreeSet::new();
    let mut classified_invariants = Vec::new();
    for invariant in observed_invariants {
        validate_owner("invariant", &invariant.id, &invariant.owner, &mut state);
        if invariant.id.trim().is_empty() {
            state.mark_not_proven(
                "empty_invariant_id",
                "invariant id is empty",
                owner_or_default(&invariant.owner),
            );
        }
        if !seen_invariants.insert(invariant.id.clone()) {
            state.mark_not_proven(
                "duplicate_invariant",
                format!("invariant appears more than once: {:?}", invariant.id),
                owner_or_default(&invariant.owner),
            );
        }
        validate_evidence(
            "invariant",
            &invariant.id,
            &invariant.evidence,
            &invariant.owner,
            &mut state,
        );

        match invariant.status.as_str() {
            "pass" => {}
            "fail" => state.mark_drift(
                "invariant_failed",
                format!("invariant {:?} failed", invariant.id),
                owner_or_default(&invariant.owner),
            ),
            "not_proven" => state.mark_not_proven(
                "invariant_not_proven",
                format!("invariant {:?} is not proven", invariant.id),
                owner_or_default(&invariant.owner),
            ),
            other => state.mark_not_proven(
                "unknown_invariant_status",
                format!(
                    "invariant {:?} has unknown status {:?}",
                    invariant.id, other
                ),
                owner_or_default(&invariant.owner),
            ),
        }

        classified_invariants.push(ClassifiedInvariant {
            id: invariant.id,
            status: invariant.status,
            owner: invariant.owner,
            evidence: invariant.evidence,
        });
    }

    for required in REQUIRED_INVARIANTS {
        if !seen_invariants.contains(*required) {
            state.mark_not_proven(
                "required_invariant_missing",
                format!("required publication invariant {required:?} is absent"),
                "release-engineering",
            );
        }
    }
    classified_invariants.sort_by(|left, right| left.id.cmp(&right.id));

    if state.drift && swarm.version == public.version {
        state.push_blocker(
            "same_version_divergent_product",
            format!("version {} has behavior or invariant drift", swarm.version),
            "release-engineering",
        );
    }

    state.blockers.sort();
    state.blockers.dedup();
    let authority_valid = !state.not_proven;
    let verdict = if state.not_proven {
        Verdict::NotProven
    } else if state.drift {
        Verdict::Drift
    } else {
        Verdict::Clean
    };

    Receipt {
        schema_version: SUPPORTED_SCHEMA_VERSION,
        swarm,
        public,
        manifest,
        differences: classified_differences,
        invariants: classified_invariants,
        authority_valid,
        blockers: state.blockers,
        verdict,
    }
}

fn validate_subject(label: &str, subject: &SubjectIdentity, state: &mut ClassificationState) {
    if !valid_repository_slug(&subject.repository) {
        state.mark_not_proven(
            "invalid_subject_repository",
            format!(
                "{label} repository must use owner/repository syntax: {:?}",
                subject.repository
            ),
            "release-engineering",
        );
    }
    if !is_lower_hex(&subject.sha, 40) {
        state.mark_not_proven(
            "invalid_subject_sha",
            format!(
                "{label} SHA is not a 40-character lowercase commit id: {:?}",
                subject.sha
            ),
            "release-engineering",
        );
    }
    if !is_lower_hex(&subject.tree_digest, 64) {
        state.mark_not_proven(
            "invalid_subject_tree_digest",
            format!(
                "{label} tree digest is not a lowercase SHA-256: {:?}",
                subject.tree_digest
            ),
            "release-engineering",
        );
    }
    if subject.version.trim().is_empty() {
        state.mark_not_proven(
            "empty_subject_version",
            format!("{label} version is empty"),
            "release-engineering",
        );
    }
}

fn validate_manifest(
    manifest: Option<&ManifestIdentity>,
    swarm: &SubjectIdentity,
    public: &SubjectIdentity,
    state: &mut ClassificationState,
) {
    let Some(manifest) = manifest else {
        state.mark_not_proven(
            "comparison_manifest_missing",
            "comparison manifest authority is missing",
            "release-engineering",
        );
        return;
    };

    if !valid_repository_path(&manifest.path) {
        state.mark_not_proven(
            "invalid_manifest_path",
            format!(
                "comparison manifest path must use canonical repository-relative syntax: {:?}",
                manifest.path
            ),
            "release-engineering",
        );
    }
    if !is_lower_hex(&manifest.sha256, 64) {
        state.mark_not_proven(
            "invalid_manifest_digest",
            format!(
                "comparison manifest digest is not a lowercase SHA-256: {:?}",
                manifest.sha256
            ),
            "release-engineering",
        );
    }
    if !is_lower_hex(&manifest.swarm_sha, 40) || manifest.swarm_sha != swarm.sha {
        state.mark_not_proven(
            "manifest_swarm_basis_mismatch",
            format!(
                "comparison manifest swarm basis {:?} does not match subject {:?}",
                manifest.swarm_sha, swarm.sha
            ),
            "release-engineering",
        );
    }
    if !is_lower_hex(&manifest.public_sha, 40) || manifest.public_sha != public.sha {
        state.mark_not_proven(
            "manifest_public_basis_mismatch",
            format!(
                "comparison manifest public basis {:?} does not match subject {:?}",
                manifest.public_sha, public.sha
            ),
            "release-engineering",
        );
    }
}

fn validate_owner(
    kind: &str,
    identity: &str,
    owner: &str,
    state: &mut ClassificationState,
) {
    if owner.trim().is_empty() {
        state.mark_not_proven(
            "owner_missing",
            format!("{kind} {identity:?} has no owner"),
            "release-engineering",
        );
    }
}

fn validate_evidence(
    kind: &str,
    identity: &str,
    evidence: &[String],
    owner: &str,
    state: &mut ClassificationState,
) {
    if evidence.is_empty() || evidence.iter().any(|entry| entry.trim().is_empty()) {
        state.mark_not_proven(
            "evidence_missing",
            format!("{kind} {identity:?} has missing or empty evidence"),
            owner_or_default(owner),
        );
        return;
    }

    let unique = evidence
        .iter()
        .map(|entry| entry.trim())
        .collect::<BTreeSet<_>>();
    if unique.len() != evidence.len() {
        state.mark_not_proven(
            "duplicate_evidence",
            format!("{kind} {identity:?} repeats evidence entries"),
            owner_or_default(owner),
        );
    }
}

fn owner_or_default(owner: &str) -> &str {
    if owner.trim().is_empty() {
        "release-engineering"
    } else {
        owner
    }
}

fn allowed_classification(classification: &str) -> bool {
    matches!(
        classification,
        EXPECTED_TRANSLATION
            | APPROVED_EXCLUSION
            | RELEASE_METADATA
            | PRODUCT_DRIFT
            | NOT_PROVEN_CLASS
    )
}

fn requires_manifest_rule(classification: &str) -> bool {
    matches!(
        classification,
        EXPECTED_TRANSLATION | APPROVED_EXCLUSION | RELEASE_METADATA
    )
}

fn valid_repository_slug(raw: &str) -> bool {
    let Some((owner, repository)) = raw.split_once('/') else {
        return false;
    };
    !repository.contains('/')
        && valid_repository_segment(owner)
        && valid_repository_segment(repository)
}

fn valid_repository_segment(raw: &str) -> bool {
    !raw.is_empty()
        && raw != "."
        && raw != ".."
        && raw
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
}

fn valid_repository_path(raw: &str) -> bool {
    !raw.is_empty()
        && !raw.starts_with('/')
        && !raw.starts_with('\\')
        && !raw.contains('\\')
        && !raw.contains(':')
        && !raw.contains('\0')
        && raw
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn is_lower_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("creating publication drift output {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(receipt).wrap_err("serializing drift receipt")?;
    fs::write(path, format!("{raw}\n"))
        .wrap_err_with(|| format!("writing publication drift receipt {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{Observation, Verdict, classify};
    use color_eyre::eyre::{Result, bail};

    #[test]
    fn clean_translation_fixture_passes() -> Result<()> {
        assert_fixture_verdict(
            include_str!("../../../fixtures/publication_drift/clean.json"),
            Verdict::Clean,
        )
    }

    #[test]
    fn windows_arm64_incident_is_product_drift() -> Result<()> {
        let observation: Observation = serde_json::from_str(include_str!(
            "../../../fixtures/publication_drift/windows_arm64_target_drift.json"
        ))?;
        let receipt = classify(observation);
        if receipt.verdict != Verdict::Drift {
            bail!(
                "incident fixture returned {:?}: {:?}",
                receipt.verdict,
                receipt.blockers
            );
        }
        if !receipt
            .blockers
            .iter()
            .any(|blocker| blocker.code == "same_version_divergent_product")
        {
            bail!("same-version drift blocker was not emitted");
        }
        Ok(())
    }

    #[test]
    fn behavioral_translation_is_promoted_to_product_drift() -> Result<()> {
        let observation: Observation = serde_json::from_str(include_str!(
            "../../../fixtures/publication_drift/behavioral_translation.json"
        ))?;
        let receipt = classify(observation);
        if receipt.verdict != Verdict::Drift {
            bail!("behavioral translation returned {:?}", receipt.verdict);
        }
        if receipt.differences[0].effective_classification != "product_drift" {
            bail!("behavioral translation was not promoted to product drift");
        }
        Ok(())
    }

    #[test]
    fn missing_manifest_is_not_proven() -> Result<()> {
        assert_fixture_verdict(
            include_str!("../../../fixtures/publication_drift/missing_manifest.json"),
            Verdict::NotProven,
        )
    }

    #[test]
    fn invalid_authority_dominates_observed_drift() -> Result<()> {
        assert_fixture_verdict(
            include_str!(
                "../../../fixtures/publication_drift/invalid_authority_with_drift.json"
            ),
            Verdict::NotProven,
        )
    }

    #[test]
    fn windows_paths_are_rejected_on_every_host() -> Result<()> {
        assert_fixture_verdict(
            include_str!("../../../fixtures/publication_drift/windows_path.json"),
            Verdict::NotProven,
        )
    }

    #[test]
    fn omitted_required_collections_are_not_proven() -> Result<()> {
        let observation: Observation = serde_json::from_str(
            r#"{
  "schema_version": 1,
  "swarm": {
    "repository": "EffortlessMetrics/perl-lsp-swarm",
    "sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "tree_digest": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
    "version": "0.17.0"
  },
  "public": {
    "repository": "EffortlessMetrics/perl-lsp",
    "sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    "tree_digest": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
    "version": "0.17.0"
  },
  "manifest": {
    "path": "release_topology.v1.json",
    "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
    "swarm_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "public_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  }
}"#,
        )?;
        let receipt = classify(observation);
        if receipt.verdict != Verdict::NotProven {
            bail!("omitted collections returned {:?}", receipt.verdict);
        }
        if !receipt
            .blockers
            .iter()
            .any(|blocker| blocker.code == "differences_collection_missing")
        {
            bail!("missing differences collection was not retained as a blocker");
        }
        Ok(())
    }

    #[test]
    fn missing_required_invariant_is_not_proven() -> Result<()> {
        let mut observation: Observation = serde_json::from_str(include_str!(
            "../../../fixtures/publication_drift/clean.json"
        ))?;
        let invariants = observation
            .invariants
            .as_mut()
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture invariants missing"))?;
        invariants.retain(|invariant| invariant.id != "artifact_traceable_to_public_sha");
        let receipt = classify(observation);
        if receipt.verdict != Verdict::NotProven {
            bail!("missing required invariant returned {:?}", receipt.verdict);
        }
        Ok(())
    }

    #[test]
    fn receipt_collections_are_deterministically_ordered() -> Result<()> {
        let mut observation: Observation = serde_json::from_str(include_str!(
            "../../../fixtures/publication_drift/clean.json"
        ))?;
        observation
            .invariants
            .as_mut()
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture invariants missing"))?
            .reverse();
        let receipt = classify(observation);
        if !receipt
            .invariants
            .windows(2)
            .all(|window| window[0].id <= window[1].id)
        {
            bail!("invariants were not sorted in the receipt");
        }
        Ok(())
    }

    fn assert_fixture_verdict(raw: &str, expected: Verdict) -> Result<()> {
        let observation: Observation = serde_json::from_str(raw)?;
        let receipt = classify(observation);
        if receipt.verdict != expected {
            bail!(
                "fixture returned {:?}, expected {:?}: {:?}",
                receipt.verdict,
                expected,
                receipt.blockers
            );
        }
        Ok(())
    }
}
