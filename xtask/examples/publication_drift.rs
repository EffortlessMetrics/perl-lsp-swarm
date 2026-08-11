//! Classify an exact-SHA swarm/public-tree comparison without mutating either repository.

use clap::Parser;
use color_eyre::eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const SUPPORTED_SCHEMA_VERSION: u32 = 1;
const EXPECTED_TRANSLATION: &str = "expected_publication_translation";
const APPROVED_EXCLUSION: &str = "approved_lineage_exclusion";
const RELEASE_METADATA: &str = "release_metadata_only";
const PRODUCT_DRIFT: &str = "product_drift";
const NOT_PROVEN_CLASS: &str = "unknown_or_not_proven";

#[derive(Debug, Parser)]
#[command(about = "Classify an exact-SHA publication drift observation")]
struct Args {
    /// Comparison observation JSON.
    #[arg(long)]
    input: PathBuf,

    /// Receipt JSON written even when the verdict blocks promotion.
    #[arg(long)]
    out: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SubjectIdentity {
    repository: String,
    sha: String,
    version: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    schema_version: u32,
    swarm: SubjectIdentity,
    public: SubjectIdentity,
    manifest_digest: Option<String>,
    #[serde(default)]
    differences: Vec<ObservedDifference>,
    #[serde(default)]
    invariants: Vec<ObservedInvariant>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservedDifference {
    path: String,
    classification: String,
    behavior_changed: bool,
    #[serde(default)]
    evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservedInvariant {
    id: String,
    status: String,
    #[serde(default)]
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
    manifest_digest: Option<String>,
    differences: Vec<ClassifiedDifference>,
    invariants: Vec<ClassifiedInvariant>,
    blockers: Vec<String>,
    verdict: Verdict,
}

#[derive(Debug, Serialize)]
struct ClassifiedDifference {
    path: String,
    declared_classification: String,
    effective_classification: String,
    behavior_changed: bool,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ClassifiedInvariant {
    id: String,
    status: String,
    evidence: Vec<String>,
}

fn main() -> Result<()> {
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
    let mut blockers = Vec::new();
    let mut drift = false;
    let mut not_proven = false;

    if observation.schema_version != SUPPORTED_SCHEMA_VERSION {
        not_proven = true;
        blockers.push(format!(
            "unsupported schema version {}; expected {}",
            observation.schema_version, SUPPORTED_SCHEMA_VERSION
        ));
    }

    validate_subject("swarm", &observation.swarm, &mut blockers, &mut not_proven);
    validate_subject("public", &observation.public, &mut blockers, &mut not_proven);

    match observation.manifest_digest.as_deref() {
        Some(digest) if is_lower_hex(digest, 64) => {}
        Some(digest) => {
            not_proven = true;
            blockers.push(format!(
                "comparison manifest digest is not a lowercase SHA-256: {digest:?}"
            ));
        }
        None => {
            not_proven = true;
            blockers.push("comparison manifest digest is missing".to_string());
        }
    }

    let mut seen_paths = BTreeSet::new();
    let mut classified_differences = Vec::new();
    for difference in observation.differences {
        if !valid_relative_path(&difference.path) {
            not_proven = true;
            blockers.push(format!(
                "difference path is empty, absolute, or escapes the repository: {:?}",
                difference.path
            ));
        }
        if !seen_paths.insert(difference.path.clone()) {
            not_proven = true;
            blockers.push(format!(
                "difference path appears more than once: {:?}",
                difference.path
            ));
        }
        if !has_evidence(&difference.evidence) {
            not_proven = true;
            blockers.push(format!("difference {:?} has no evidence", difference.path));
        }

        let mut effective = difference.classification.clone();
        if !allowed_classification(&difference.classification) {
            effective = NOT_PROVEN_CLASS.to_string();
            not_proven = true;
            blockers.push(format!(
                "difference {:?} has unknown classification {:?}",
                difference.path, difference.classification
            ));
        }
        if difference.behavior_changed && effective != PRODUCT_DRIFT {
            effective = PRODUCT_DRIFT.to_string();
            blockers.push(format!(
                "difference {:?} changes behavior and cannot be accepted as {:?}",
                difference.path, difference.classification
            ));
        }

        match effective.as_str() {
            PRODUCT_DRIFT => drift = true,
            NOT_PROVEN_CLASS => not_proven = true,
            EXPECTED_TRANSLATION | APPROVED_EXCLUSION | RELEASE_METADATA => {}
            _ => not_proven = true,
        }

        classified_differences.push(ClassifiedDifference {
            path: difference.path,
            declared_classification: difference.classification,
            effective_classification: effective,
            behavior_changed: difference.behavior_changed,
            evidence: difference.evidence,
        });
    }

    let mut seen_invariants = BTreeSet::new();
    let mut classified_invariants = Vec::new();
    for invariant in observation.invariants {
        if invariant.id.trim().is_empty() {
            not_proven = true;
            blockers.push("invariant id is empty".to_string());
        }
        if !seen_invariants.insert(invariant.id.clone()) {
            not_proven = true;
            blockers.push(format!("invariant appears more than once: {:?}", invariant.id));
        }
        if !has_evidence(&invariant.evidence) {
            not_proven = true;
            blockers.push(format!("invariant {:?} has no evidence", invariant.id));
        }

        match invariant.status.as_str() {
            "pass" => {}
            "fail" => {
                drift = true;
                blockers.push(format!("invariant {:?} failed", invariant.id));
            }
            "not_proven" => {
                not_proven = true;
                blockers.push(format!("invariant {:?} is not proven", invariant.id));
            }
            other => {
                not_proven = true;
                blockers.push(format!(
                    "invariant {:?} has unknown status {:?}",
                    invariant.id, other
                ));
            }
        }

        classified_invariants.push(ClassifiedInvariant {
            id: invariant.id,
            status: invariant.status,
            evidence: invariant.evidence,
        });
    }

    if drift && observation.swarm.version == observation.public.version {
        blockers.push(format!(
            "same_version_divergent_product: version {} has behavior or invariant drift",
            observation.swarm.version
        ));
    }

    let verdict = if drift {
        Verdict::Drift
    } else if not_proven {
        Verdict::NotProven
    } else {
        Verdict::Clean
    };

    Receipt {
        schema_version: SUPPORTED_SCHEMA_VERSION,
        swarm: observation.swarm,
        public: observation.public,
        manifest_digest: observation.manifest_digest,
        differences: classified_differences,
        invariants: classified_invariants,
        blockers,
        verdict,
    }
}

fn validate_subject(
    label: &str,
    subject: &SubjectIdentity,
    blockers: &mut Vec<String>,
    not_proven: &mut bool,
) {
    if subject.repository.trim().is_empty() {
        *not_proven = true;
        blockers.push(format!("{label} repository is empty"));
    }
    if !is_lower_hex(&subject.sha, 40) {
        *not_proven = true;
        blockers.push(format!(
            "{label} SHA is not a 40-character lowercase commit id: {:?}",
            subject.sha
        ));
    }
    if subject.version.trim().is_empty() {
        *not_proven = true;
        blockers.push(format!("{label} version is empty"));
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

fn valid_relative_path(raw: &str) -> bool {
    let path = Path::new(raw);
    !raw.trim().is_empty()
        && !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

fn has_evidence(evidence: &[String]) -> bool {
    evidence.iter().any(|entry| !entry.trim().is_empty())
}

fn is_lower_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    if let Some(parent) = path.parent() {
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
        let observation: Observation = serde_json::from_str(include_str!(
            "../../fixtures/publication_drift/clean.json"
        ))?;
        let receipt = classify(observation);
        if receipt.verdict != Verdict::Clean {
            bail!("clean fixture returned {:?}: {:?}", receipt.verdict, receipt.blockers);
        }
        Ok(())
    }

    #[test]
    fn windows_arm64_incident_is_product_drift() -> Result<()> {
        let observation: Observation = serde_json::from_str(include_str!(
            "../../fixtures/publication_drift/windows_arm64_target_drift.json"
        ))?;
        let receipt = classify(observation);
        if receipt.verdict != Verdict::Drift {
            bail!("incident fixture returned {:?}", receipt.verdict);
        }
        if !receipt
            .blockers
            .iter()
            .any(|blocker| blocker.contains("same_version_divergent_product"))
        {
            bail!("same-version drift blocker was not emitted");
        }
        Ok(())
    }

    #[test]
    fn behavioral_translation_is_promoted_to_product_drift() -> Result<()> {
        let observation: Observation = serde_json::from_str(include_str!(
            "../../fixtures/publication_drift/behavioral_translation.json"
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
        let observation: Observation = serde_json::from_str(include_str!(
            "../../fixtures/publication_drift/missing_manifest.json"
        ))?;
        let receipt = classify(observation);
        if receipt.verdict != Verdict::NotProven {
            bail!("missing manifest returned {:?}", receipt.verdict);
        }
        Ok(())
    }
}
