use super::model::{
    EXTERNAL_WRITE_POLICY, PacketInput, RulingBinding, SOURCE_AUTHORITY_SCHEMA_VERSION,
    Sensitivity, SourceAuthorityClass, SourceAuthorityManifest, normalized_digest,
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
    let packet_root = contain_packet_root(manifest, repo_root, &mut violations);

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

    if let Some(packet_root) = packet_root.as_deref() {
        verify_input_table(manifest, packet_root, repo_root, &mut violations);
    }
    // A failed containment already emitted its violation; the packet tree is
    // never dereferenced.
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

/// Contain the manifest-controlled packet root before anything dereferences
/// it. Subjects are individually normalized elsewhere, but the root itself is
/// also manifest-controlled: an absolute value would replace the repository
/// root, `..` segments would escape it, and a symlinked root could redirect
/// the packet walk outside the checkout. The root therefore gets the same
/// lexical normalization as subjects plus a canonical containment check, and
/// the manifest file must be a single plain file name. Any failure is a
/// violation and the tree is never walked.
fn contain_packet_root(
    manifest: &SourceAuthorityManifest,
    repo_root: &Path,
    violations: &mut Vec<Violation>,
) -> Option<PathBuf> {
    let root = match normalized_subject_path(&manifest.packet_root) {
        Ok(root) => root,
        Err(detail) => {
            violations.push(Violation {
                code: "invalid_packet_root".into(),
                subject: manifest.packet_root.clone(),
                detail,
            });
            return None;
        }
    };
    if !is_single_file_name(&manifest.manifest_file) {
        violations.push(Violation {
            code: "invalid_manifest_file".into(),
            subject: manifest.manifest_file.clone(),
            detail: "manifest_file must be a single plain file name at the packet root".into(),
        });
    }
    let canonical_repo = match repo_root.canonicalize() {
        Ok(canonical) => canonical,
        Err(error) => {
            violations.push(Violation {
                code: "repo_root_unreadable".into(),
                subject: manifest.packet_root.clone(),
                detail: format!("repository root cannot be canonicalized: {error}"),
            });
            return None;
        }
    };
    let joined = repo_root.join(normalize_separators(&root));
    match joined.canonicalize() {
        Ok(canonical_root) if canonical_root.starts_with(&canonical_repo) => Some(joined),
        Ok(canonical_root) => {
            violations.push(Violation {
                code: "packet_root_escapes_repository".into(),
                subject: manifest.packet_root.clone(),
                detail: format!(
                    "packet root resolves to {} outside the repository (absolute path, \
                     traversal, or symlink escape); refusing to dereference it",
                    canonical_root.display()
                ),
            });
            None
        }
        Err(error) => {
            violations.push(Violation {
                code: "packet_root_unreadable".into(),
                subject: manifest.packet_root.clone(),
                detail: format!("packet root does not exist or cannot be canonicalized: {error}"),
            });
            None
        }
    }
}

/// Whether `name` is one plain path component: no separators, no dot
/// segments, and non-empty.
fn is_single_file_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\')
}

fn verify_input_table(
    manifest: &SourceAuthorityManifest,
    packet_root: &Path,
    repo_root: &Path,
    violations: &mut Vec<Violation>,
) {
    let mut by_id = BTreeMap::new();
    let mut by_subject = BTreeMap::new();
    let mut invalid_subjects = BTreeSet::new();
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
                let previous = by_subject.insert(subject.clone(), input.id.clone());
                if let Some(existing_id) = previous {
                    violations.push(Violation {
                        code: "duplicate_subject".into(),
                        subject,
                        detail: format!(
                            "input {} binds a subject that input {} already classifies",
                            input.id, existing_id
                        ),
                    });
                }
            }
            Err(detail) => {
                invalid_subjects.insert(input.subject.clone());
                violations.push(Violation {
                    code: "invalid_subject".into(),
                    subject: input.subject.clone(),
                    detail,
                });
            }
        }
    }

    // Every declared subject must exist and be current. A subject whose path
    // failed normalization is never read at all: traversal or absolute paths
    // stay rejected as declarations instead of being dereferenced.
    for input in &manifest.inputs {
        if invalid_subjects.contains(&input.subject) {
            continue;
        }
        let subject_path = packet_root.join(normalize_separators(&input.subject));
        if subject_traverses_symlink(packet_root, &input.subject) {
            violations.push(Violation {
                code: "symlinked_subject".into(),
                subject: input.subject.clone(),
                detail: "subject path traverses a symbolic link; packet content must be \
                         ordinary in-repository files"
                    .into(),
            });
            continue;
        }
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

        verify_semantics(input, violations, repo_root);
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

fn verify_semantics(input: &PacketInput, violations: &mut Vec<Violation>, repo_root: &Path) {
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
                if let Some(detail) = ruling_binding_failure(binding, repo_root) {
                    violations.push(Violation {
                        code: "directive_without_binding".into(),
                        subject: input.subject.clone(),
                        detail,
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

/// Validate that a directive input's provenance is checkable against the
/// repository: a shaped ruling identity plus an existing governed subject.
/// Returns the rejection detail, or `None` when provenance holds.
fn ruling_binding_failure(binding: &RulingBinding, repo_root: &Path) -> Option<String> {
    let ruling_id = binding.ruling_id.trim();
    if ruling_id.is_empty() {
        return Some("directive input records no ruling identity".into());
    }
    let reference_shape = "issue#<n>, pr#<n>, or <repo-relative path>#<anchor>";
    let identity_ok = if let Some(number) =
        ruling_id.strip_prefix("issue#").or_else(|| ruling_id.strip_prefix("pr#"))
    {
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
    } else {
        match ruling_id.split_once('#') {
            Some((path_part, anchor)) => {
                !anchor.trim().is_empty()
                    && normalized_subject_path(path_part).ok().is_some_and(|relative| {
                        repo_root.join(normalize_separators(&relative)).is_file()
                    })
            }
            None => false,
        }
    };
    if !identity_ok {
        return Some(format!(
            "ruling identity {ruling_id:?} is not checkable; use {reference_shape} with an \
             existing repository file for path references"
        ));
    }

    let governed = normalized_subject_path(&binding.subject_path)
        .map_err(|detail| format!("governed subject is not a usable relative path: {detail}"));
    match governed {
        Err(detail) => Some(detail),
        Ok(relative) => {
            if repo_root.join(normalize_separators(&relative)).exists() {
                None
            } else {
                Some(format!(
                    "governed subject {relative:?} does not exist in the repository; directive \
                     authority must bind a real subject"
                ))
            }
        }
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
        declared.insert(generator.path.replace('\\', "/"));
        // A generator declaration is itself a path the manifest author
        // controls, so it gets the same normalization rejection as subjects:
        // traversal or absolute paths are refused instead of dereferenced.
        if let Err(detail) = normalized_subject_path(&generator.path) {
            violations.push(Violation {
                code: "invalid_generator_path".into(),
                subject: generator.path.clone(),
                detail,
            });
            continue;
        }
        let path = repo_root.join(normalize_separators(&generator.path));
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

    // Both directions of the loop, within the scanned surface: every
    // `.sh`/`.py` file under `scripts/**` (excluding test-harness directories)
    // whose text references the packet tree by any joined or slash-split
    // spelling must itself be declared. The scan is textual over those two
    // languages; a consumer placed outside `scripts/`, written in another
    // language, or referencing the tree only through assembled path fragments
    // can evade it — that residual is the manifest reviewer's responsibility.
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
///
/// Symbolic links are never traversed, matching [`walk_packet_tree`]: entry
/// types are resolved without following links, so a symlinked directory can
/// neither redirect the scan outside the repository nor loop it forever. A
/// symlinked script is surfaced as a file and read through the link, so it
/// still has to be declared.
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
            let file_type = entry
                .file_type()
                .wrap_err_with(|| format!("resolving entry type for {}", path.display()))?;
            if file_type.is_dir() {
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

/// Deterministically list every regular entry under `root`, excluding
/// `excluded_name` at the root level. Symbolic links are never traversed:
/// entry types are resolved without following links, so a symlinked directory
/// cannot redirect the walk and a symlinked file is surfaced to the
/// unclassified-content and subject checks instead of being dereferenced.
fn walk_packet_tree(root: &Path, excluded_name: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            if entry.file_type()?.is_dir() {
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

/// Whether the subject path traverses a symbolic link in any component
/// between the packet root and the subject file, so a declared read cannot be
/// redirected outside the packet tree.
fn subject_traverses_symlink(packet_root: &Path, subject: &str) -> bool {
    let mut current = packet_root.to_path_buf();
    for segment in normalize_separators(subject).split('/') {
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return true,
            Ok(_) => {}
            // A missing component is reported by the subject read itself.
            Err(_) => return false,
        }
    }
    false
}

fn normalize_separators(path: &str) -> String {
    path.replace('\\', "/")
}

#[cfg(test)]
mod verify_tests {
    //! In-crate coverage of the fail-closed verifier seams. These complement
    //! the cross-crate integration suites (`zed_source_authority`,
    //! `zed_packet_prompt_injection`) by exercising `verify_manifest` from the
    //! same crate, so static tracers can connect each violation family to its
    //! covering test without a cross-crate hop.

    use super::*;
    use crate::source_authority::model::{GeneratorPath, RulingBinding};
    use std::fs;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn input(id: &str, subject: &str) -> PacketInput {
        PacketInput {
            id: id.into(),
            subject: subject.into(),
            authority: SourceAuthorityClass::ReceiptEvidence,
            digest: "0".repeat(64),
            instruction_allowed: false,
            sensitivity: Sensitivity::Public,
            digest_only: false,
            active: true,
            superseded_by: None,
            conflict_key: None,
            verified_against_current_code: false,
            converted_to_action: false,
            ruling_binding: None,
        }
    }

    fn tree(subjects: &[(&str, &[u8])]) -> std::io::Result<(tempfile::TempDir, PathBuf)> {
        let dir = tempfile::TempDir::new()?;
        let packets = dir.path().join("packets");
        fs::create_dir_all(&packets)?;
        for (name, content) in subjects {
            let path = packets.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, content)?;
        }
        let root = dir.path().to_path_buf();
        Ok((dir, root))
    }

    fn manifest(inputs: Vec<PacketInput>) -> SourceAuthorityManifest {
        SourceAuthorityManifest {
            schema_version: SOURCE_AUTHORITY_SCHEMA_VERSION.into(),
            packet_root: "packets".into(),
            external_write_policy: EXTERNAL_WRITE_POLICY.into(),
            manifest_file: "source-authority.v1.json".into(),
            generators: Vec::new(),
            inputs,
        }
    }

    fn has(receipt: &Receipt, code: &str) -> bool {
        receipt.violations.iter().any(|violation| violation.code == code)
    }

    #[test]
    fn schema_and_policy_drift_are_reported() -> TestResult {
        let (_dir, root) = tree(&[])?;
        let mut drifted = manifest(Vec::new());
        drifted.schema_version = "zed-source-authority.v0".into();
        drifted.external_write_policy = "agent_auto_submit".into();
        let receipt = verify_manifest(&drifted, &root).map_err(|error| error.to_string())?;
        assert!(has(&receipt, "schema_mismatch"));
        assert!(has(&receipt, "external_write_policy_drift"));
        Ok(())
    }

    #[test]
    fn duplicate_subjects_name_the_existing_owner() -> TestResult {
        let (_dir, root) = tree(&[("a.txt", b"one\n")])?;
        let inputs = vec![input("first", "a.txt"), input("second", "a.txt")];
        let receipt =
            verify_manifest(&manifest(inputs), &root).map_err(|error| error.to_string())?;
        assert!(has(&receipt, "duplicate_subject"));
        let detail = receipt
            .violations
            .iter()
            .find(|violation| violation.code == "duplicate_subject")
            .map(|violation| violation.detail.clone())
            .unwrap_or_default();
        assert!(detail.contains("first"), "detail names the existing owner: {detail}");
        Ok(())
    }

    #[test]
    fn traversal_subjects_are_rejected_without_being_read() -> TestResult {
        let (_dir, root) = tree(&[])?;
        let outside = vec![input("escape", "../outside.txt")];
        let receipt =
            verify_manifest(&manifest(outside), &root).map_err(|error| error.to_string())?;
        assert!(has(&receipt, "invalid_subject"));
        assert!(!has(&receipt, "stale_digest"), "invalid paths are never dereferenced");
        Ok(())
    }

    #[test]
    fn stale_digests_and_missing_subjects_fail_closed() -> TestResult {
        let (_dir, root) = tree(&[("present.txt", b"content\n")])?;
        let mut stale = input("stale", "present.txt");
        stale.digest = "f".repeat(64);
        let absent = input("absent", "missing.txt");
        let receipt = verify_manifest(&manifest(vec![stale, absent]), &root)
            .map_err(|error| error.to_string())?;
        assert!(has(&receipt, "stale_digest"));
        assert!(has(&receipt, "missing_subject"));
        Ok(())
    }

    #[test]
    fn unclassified_files_in_the_tree_are_named() -> TestResult {
        let (_dir, root) = tree(&[("known.txt", b"x\n")])?;
        fs::write(root.join("packets").join("smuggled.txt"), b"unclassified\n")?;
        let receipt = verify_manifest(&manifest(vec![input("known", "known.txt")]), &root)
            .map_err(|error| error.to_string())?;
        assert!(has(&receipt, "unclassified_content"));
        Ok(())
    }

    #[test]
    fn same_key_divergence_blocks_instead_of_resolving() -> TestResult {
        let (_dir, root) = tree(&[("left.txt", b"one\n"), ("right.txt", b"two\n")])?;
        let mut left = input("claim-a", "left.txt");
        left.conflict_key = Some("subject".into());
        let mut right = input("claim-b", "right.txt");
        right.conflict_key = Some("subject".into());
        right.digest = "f".repeat(64);
        let receipt = verify_manifest(&manifest(vec![left, right]), &root)
            .map_err(|error| error.to_string())?;
        assert!(has(&receipt, "blocked_authority_conflict"));
        Ok(())
    }

    #[test]
    fn directive_inputs_need_checkable_repository_provenance() -> TestResult {
        let (dir, root) = tree(&[("ruling.txt", b"ruling body\n")])?;
        fs::create_dir_all(dir.path().join("docs/policy"))?;
        fs::write(dir.path().join("docs/policy/stage-authority.md"), "# policy\n")?;

        let mut claimed = input("claimed", "ruling.txt");
        claimed.authority = SourceAuthorityClass::MaintainerRuling;
        claimed.instruction_allowed = true;

        let unbound =
            verify_manifest(&manifest(vec![claimed.clone()]), &root).map_err(|e| e.to_string())?;
        assert!(has(&unbound, "directive_without_binding"));

        // Fabricated provenance: the governed subject does not exist.
        claimed.ruling_binding = Some(RulingBinding {
            ruling_id: "issue#11726".into(),
            subject_path: "docs/policy/does-not-exist.md".into(),
        });
        let fabricated =
            verify_manifest(&manifest(vec![claimed.clone()]), &root).map_err(|e| e.to_string())?;
        assert!(has(&fabricated, "directive_without_binding"));

        // Malformed identity shape is equally rejected.
        claimed.ruling_binding = Some(RulingBinding {
            ruling_id: "trust me".into(),
            subject_path: "docs/policy/stage-authority.md".into(),
        });
        let malformed =
            verify_manifest(&manifest(vec![claimed.clone()]), &root).map_err(|e| e.to_string())?;
        assert!(has(&malformed, "directive_without_binding"));

        // A real issue reference bound to a real repository subject passes.
        claimed.ruling_binding = Some(RulingBinding {
            ruling_id: "issue#11726".into(),
            subject_path: "docs/policy/stage-authority.md".into(),
        });
        let bound = verify_manifest(&manifest(vec![claimed]), &root).map_err(|e| e.to_string())?;
        assert!(!has(&bound, "directive_without_binding"), "{:?}", bound.violations);
        Ok(())
    }

    #[test]
    fn generator_declarations_reject_traversal_paths() -> TestResult {
        let (_dir, root) = tree(&[])?;
        let mut escaped = manifest(Vec::new());
        escaped.generators.push(GeneratorPath { path: "../outside/tool.sh".into() });
        let receipt = verify_manifest(&escaped, &root).map_err(|error| error.to_string())?;
        assert!(has(&receipt, "invalid_generator_path"));
        Ok(())
    }

    #[test]
    fn manifest_controlled_packet_roots_are_lexically_rejected() -> TestResult {
        let (dir, root) = tree(&[("a.txt", b"one\n")])?;
        // An absolute root would replace the repository root entirely.
        let mut absolute = manifest(vec![input("first", "a.txt")]);
        let absolute_root = if cfg!(windows) {
            dir.path().ancestors().nth(1).unwrap_or(dir.path()).to_string_lossy().into_owned()
        } else {
            "/etc".to_string()
        };
        absolute.packet_root = absolute_root;
        let receipt = verify_manifest(&absolute, &root).map_err(|error| error.to_string())?;
        assert!(has(&receipt, "invalid_packet_root"), "{:?}", receipt.violations);
        assert!(!has(&receipt, "stale_digest"), "an uncontained root is never dereferenced");

        // Traversal segments would escape the checkout.
        let mut traversal = manifest(vec![input("first", "a.txt")]);
        traversal.packet_root = "packets/../..".into();
        let receipt = verify_manifest(&traversal, &root).map_err(|error| error.to_string())?;
        assert!(has(&receipt, "invalid_packet_root"));
        assert!(!has(&receipt, "unclassified_content"));

        // A root that simply does not exist fails closed without reads.
        let mut missing = manifest(vec![input("first", "a.txt")]);
        missing.packet_root = "packets-absent".into();
        let receipt = verify_manifest(&missing, &root).map_err(|error| error.to_string())?;
        assert!(has(&receipt, "packet_root_unreadable"));
        assert!(
            !has(&receipt, "missing_subject"),
            "subjects are not read when the root failed containment"
        );
        Ok(())
    }

    #[test]
    fn manifest_file_must_be_a_single_file_name() -> TestResult {
        let (_dir, root) = tree(&[])?;
        let mut shaped = manifest(Vec::new());
        shaped.manifest_file = "../evil.v1.json".into();
        let receipt = verify_manifest(&shaped, &root).map_err(|error| error.to_string())?;
        assert!(has(&receipt, "invalid_manifest_file"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_packet_root_is_contained() -> TestResult {
        let (dir, root) = tree(&[("a.txt", b"one\n")])?;
        let outside = dir.path().join("outside-sandbox");
        std::os::unix::fs::symlink("/etc", &outside)?;
        let mut escaped = manifest(vec![input("first", "a.txt")]);
        escaped.packet_root = "outside-sandbox".into();
        let receipt = verify_manifest(&escaped, &root).map_err(|error| error.to_string())?;
        assert!(has(&receipt, "packet_root_escapes_repository"), "{:?}", receipt.violations);
        assert!(!has(&receipt, "missing_subject"), "an escaped root is never dereferenced");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_subject_is_rejected_without_being_read() -> TestResult {
        let (dir, root) = tree(&[("a.txt", b"one\n")])?;
        std::os::unix::fs::symlink("/etc/hostname", dir.path().join("packets").join("link.txt"))?;
        let inputs = vec![input("linked", "link.txt")];
        let receipt =
            verify_manifest(&manifest(inputs), &root).map_err(|error| error.to_string())?;
        assert!(has(&receipt, "symlinked_subject"), "{:?}", receipt.violations);
        Ok(())
    }
}
