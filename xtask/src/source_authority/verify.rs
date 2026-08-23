use super::model::{
    EXTERNAL_WRITE_POLICY, PacketInput, SOURCE_AUTHORITY_SCHEMA_VERSION, Sensitivity,
    SourceAuthorityClass, SourceAuthorityManifest, normalized_digest,
};
use color_eyre::eyre::{WrapErr, eyre};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// One fail-closed finding from the boundary verifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Violation {
    pub code: String,
    pub subject: String,
    pub detail: String,
}

/// Verdict over one source-authority manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Receipt {
    pub schema_version: String,
    pub verdict: String,
    pub checked_inputs: usize,
    pub checked_generators: usize,
    pub violations: Vec<Violation>,
}

/// Repository directories scanned for undeclared packet consumers.
const GENERATOR_SCAN_ROOT: &str = "scripts";
/// Directory names skipped during the generator scan: test harnesses read
/// fixture subjects as proof input; they do not assemble stage packets.
const SKIPPED_SCAN_DIRECTORIES: [&str; 1] = ["tests"];

/// Matches every spelling the packet tree is addressed with in scripts:
/// joined (`".ci/fixtures/zed-perl-upstream"`), slash-split
/// (`".ci" / "fixtures" / "zed-perl-upstream"`), and shell-quoted variants.
const PACKET_REFERENCE_PATTERN: &str = r#"\.ci[\s"'/\\]+fixtures[\s"'/\\]+zed-perl-upstream"#;

pub fn verify_manifest(
    manifest: &SourceAuthorityManifest,
    repo_root: &Path,
) -> Result<Receipt, color_eyre::eyre::Report> {
    let mut violations = Vec::new();
    let packet_root = repo_root.join(&manifest.packet_root);

    if manifest.schema_version != SOURCE_AUTHORITY_SCHEMA_VERSION {
        violations.push(Violation {
            code: "schema_mismatch".into(),
            subject: manifest.manifest_file.clone(),
            detail: format!(
                "schema_version {:?} is not supported (expected {SOURCE_AUTHORITY_SCHEMA_VERSION:?})",
                manifest.schema_version
            ),
        });
    }
    if manifest.external_write_policy != EXTERNAL_WRITE_POLICY {
        violations.push(Violation {
            code: "external_write_policy_drift".into(),
            subject: manifest.manifest_file.clone(),
            detail: format!(
                "external_write_policy {:?} must stay {EXTERNAL_WRITE_POLICY:?}",
                manifest.external_write_policy
            ),
        });
    }

    verify_input_table(manifest, &packet_root, &mut violations);
    verify_generator_surface(manifest, repo_root, &mut violations)?;

    let verdict = if violations.is_empty() { "clean" } else { "blocked" };
    Ok(Receipt {
        schema_version: manifest.schema_version.clone(),
        verdict: verdict.into(),
        checked_inputs: manifest.inputs.len(),
        checked_generators: manifest.generators.len(),
        violations,
    })
}

fn verify_input_table(
    manifest: &SourceAuthorityManifest,
    packet_root: &Path,
    violations: &mut Vec<Violation>,
) {
    let mut by_id = BTreeMap::new();
    let mut by_subject = BTreeMap::new();
    for input in &manifest.inputs {
        if by_id.insert(input.id.clone(), input).is_some() {
            violations.push(Violation {
                code: "duplicate_input".into(),
                subject: input.id.clone(),
                detail: "input id is declared more than once".into(),
            });
        }
        match normalized_subject_path(&input.subject) {
            Ok(subject) => {
                if by_subject.insert(subject.clone(), input.id.clone()).is_some() {
                    violations.push(Violation {
                        code: "duplicate_subject".into(),
                        subject,
                        detail: format!(
                            "input {} binds a subject another input already classifies",
                            input.id
                        ),
                    });
                }
            }
            Err(detail) => violations.push(Violation {
                code: "invalid_subject".into(),
                subject: input.subject.clone(),
                detail,
            }),
        }
    }

    // Every declared subject must exist and be current.
    for input in &manifest.inputs {
        let subject_path = packet_root.join(normalize_separators(&input.subject));
        match fs::read(&subject_path) {
            Ok(raw) => match normalized_digest(&raw) {
                Ok(actual) if actual == input.digest => {}
                Ok(actual) => violations.push(Violation {
                    code: "stale_digest".into(),
                    subject: input.subject.clone(),
                    detail: format!(
                        "declared digest does not bind current content (declared {}, actual {actual}); \
                         reclassify before the packet may be consumed",
                        input.digest
                    ),
                }),
                Err(error) => violations.push(Violation {
                    code: "undigestable_content".into(),
                    subject: input.subject.clone(),
                    detail: format!("subject is not valid UTF-8 so no deterministic digest exists: {error}"),
                }),
            },
            Err(error) => violations.push(Violation {
                code: "missing_subject".into(),
                subject: input.subject.clone(),
                detail: format!("classified subject cannot be read: {error}"),
            }),
        }

        verify_semantics(input, violations);
        if let Some(superseded_by) = &input.superseded_by {
            if !manifest.inputs.iter().any(|other| &other.id == superseded_by) {
                violations.push(Violation {
                    code: "unknown_supersession".into(),
                    subject: input.subject.clone(),
                    detail: format!("superseded_by names unknown input {superseded_by:?}"),
                });
            }
            if input.active {
                violations.push(Violation {
                    code: "superseded_active".into(),
                    subject: input.subject.clone(),
                    detail: format!(
                        "input is superseded by {superseded_by:?} but still marked active; a superseded \
                         ruling cannot govern the packet"
                    ),
                });
            }
        }
    }

    // No file may ride inside the packet tree unclassified.
    match walk_packet_tree(packet_root, &manifest.manifest_file) {
        Ok(files) => {
            for path in files {
                let relative = relative_to(packet_root, &path);
                if !by_subject.contains_key(&relative) {
                    violations.push(Violation {
                        code: "unclassified_content".into(),
                        subject: relative,
                        detail:
                            "file sits inside the stage-packet tree without a source-authority \
                                 classification; classify it or remove it"
                                .into(),
                    });
                }
            }
        }
        Err(error) => violations.push(Violation {
            code: "packet_tree_unreadable".into(),
            subject: manifest.packet_root.clone(),
            detail: format!("walking the packet tree failed closed: {error}"),
        }),
    }

    // Same-key active inputs must agree byte-for-byte.
    let mut conflicts: BTreeMap<&str, Vec<&PacketInput>> = BTreeMap::new();
    for input in &manifest.inputs {
        if let (true, Some(key)) = (input.active, &input.conflict_key) {
            conflicts.entry(key.as_str()).or_default().push(input);
        }
    }
    for (key, group) in conflicts {
        let mut digests: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for input in &group {
            digests.entry(input.digest.as_str()).or_default().insert(&input.id);
        }
        if digests.len() > 1 {
            let parties: Vec<String> = group.iter().map(|input| input.id.clone()).collect();
            violations.push(Violation {
                code: "blocked_authority_conflict".into(),
                subject: key.to_string(),
                detail: format!(
                    "active inputs {parties:?} conflict on key {key:?} with divergent content; \
                     resolve by evidence or ruling repair instead of text order"
                ),
            });
        }
    }
}

fn verify_semantics(input: &PacketInput, violations: &mut Vec<Violation>) {
    if input.instruction_allowed != input.authority.may_direct_work() {
        violations.push(Violation {
            code: "instruction_flag_mismatch".into(),
            subject: input.subject.clone(),
            detail: format!(
                "instruction_allowed={} contradicts authority {} (capability is derived from the \
                 class, never from the declaring text)",
                input.instruction_allowed,
                input.authority.as_schema_name()
            ),
        });
    }

    if matches!(input.sensitivity, Sensitivity::MachineLocalForbidden) {
        violations.push(Violation {
            code: "machine_local_content".into(),
            subject: input.subject.clone(),
            detail: "machine-local or secret-bearing material cannot enter a stage packet at all"
                .into(),
        });
    }
    if matches!(input.sensitivity, Sensitivity::RedactRequired) && !input.digest_only {
        violations.push(Violation {
            code: "redaction_requires_digest_only".into(),
            subject: input.subject.clone(),
            detail:
                "redact_required input must be referenced by digest only, never rendered inline"
                    .into(),
        });
    }

    if input.authority.may_direct_work() {
        match &input.ruling_binding {
            Some(binding) => {
                if binding.ruling_id.trim().is_empty() || binding.subject_path.trim().is_empty() {
                    violations.push(Violation {
                        code: "directive_without_binding".into(),
                        subject: input.subject.clone(),
                        detail: "directive-classified input needs a non-empty ruling identity and \
                                 governed repository subject"
                            .into(),
                    });
                }
            }
            None => violations.push(Violation {
                code: "directive_without_binding".into(),
                subject: input.subject.clone(),
                detail: format!(
                    "authority {} may direct work but records no durable ruling binding",
                    input.authority.as_schema_name()
                ),
            }),
        }
    }

    if input.authority.is_review_finding()
        && input.converted_to_action
        && !(input.verified_against_current_code
            && input.authority == SourceAuthorityClass::VerifiedReviewFinding)
    {
        violations.push(Violation {
            code: "unverified_finding_actionable".into(),
            subject: input.subject.clone(),
            detail: "finding was converted to an action without current-code confirmation by a \
                     verified_review_finding"
                .into(),
        });
    }

    if input.authority.is_rendered_external_body() && input.instruction_allowed {
        violations.push(Violation {
            code: "rendered_body_executable".into(),
            subject: input.subject.clone(),
            detail: "an outbound rendered body can never carry instruction capability".into(),
        });
    }
}

fn verify_generator_surface(
    manifest: &SourceAuthorityManifest,
    repo_root: &Path,
    violations: &mut Vec<Violation>,
) -> Result<(), color_eyre::eyre::Report> {
    let pattern = regex::Regex::new(PACKET_REFERENCE_PATTERN)
        .wrap_err("compiling the packet-reference pattern")?;

    let mut declared = BTreeSet::new();
    for generator in &manifest.generators {
        let path = repo_root.join(normalize_separators(&generator.path));
        declared.insert(generator.path.replace('\\', "/"));
        match fs::read(&path) {
            Ok(raw) => {
                let text = String::from_utf8_lossy(&raw);
                if !pattern.is_match(&text) {
                    violations.push(Violation {
                        code: "generator_does_not_address_packet".into(),
                        subject: generator.path.clone(),
                        detail: "declared generator contains no reference to the packet tree; the \
                                 declaration drifted from reality"
                            .into(),
                    });
                }
            }
            Err(error) => violations.push(Violation {
                code: "missing_generator".into(),
                subject: generator.path.clone(),
                detail: format!("declared generator cannot be read: {error}"),
            }),
        }
    }

    // Both directions of the loop: any script addressing the packet tree must
    // itself be declared, so new consumers cannot bypass classification.
    let scan_root = repo_root.join(GENERATOR_SCAN_ROOT);
    for path in generator_scan_files(&scan_root)? {
        let is_script =
            path.extension().is_some_and(|extension| extension == "sh" || extension == "py");
        if !is_script {
            continue;
        }
        let raw = fs::read(&path)
            .wrap_err_with(|| format!("reading candidate generator {}", path.display()))?;
        if !pattern.is_match(&String::from_utf8_lossy(&raw)) {
            continue;
        }
        let relative = relative_to(repo_root, &path).replace('\\', "/");
        if !declared.contains(&relative) {
            violations.push(Violation {
                code: "undeclared_generator".into(),
                subject: relative,
                detail: "script addresses the stage-packet tree but is not declared as a \
                         generator; declare it so its inputs stay classified"
                    .into(),
            });
        }
    }
    Ok(())
}

/// Deterministically list `.sh`/`.py` candidates under the scan root,
/// descending through every subdirectory except skipped test harnesses.
fn generator_scan_files(root: &Path) -> Result<Vec<PathBuf>, color_eyre::eyre::Report> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(eyre!("reading generator scan root {}: {error}", dir.display()));
            }
        };
        for entry in entries {
            let entry = entry
                .wrap_err_with(|| format!("iterating generator scan root {}", dir.display()))?;
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if !SKIPPED_SCAN_DIRECTORIES.contains(&name.to_string_lossy().as_ref()) {
                    stack.push(path);
                }
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Deterministically list every regular file under `root`, excluding
/// `excluded_name` at the root level.
fn walk_packet_tree(root: &Path, excluded_name: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                stack.push(path);
            } else if dir.as_path() != root || name.to_string_lossy() != excluded_name {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn relative_to(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Normalize a manifest-declared relative subject path.
fn normalized_subject_path(subject: &str) -> Result<String, String> {
    let subject = subject.trim();
    if subject.is_empty() {
        return Err("subject path is empty".into());
    }
    let forward = subject.replace('\\', "/");
    if subject.starts_with('/') || Path::new(&forward).is_absolute() {
        return Err("subject path must be packet-relative".into());
    }
    let mut segments = Vec::new();
    for segment in forward.split('/') {
        match segment {
            "" | "." => {}
            ".." => return Err("subject path must not traverse upward".into()),
            other => segments.push(other.to_string()),
        }
    }
    if segments.is_empty() {
        return Err("subject path has no segments".into());
    }
    Ok(segments.join("/"))
}

fn normalize_separators(path: &str) -> String {
    path.replace('\\', "/")
}
