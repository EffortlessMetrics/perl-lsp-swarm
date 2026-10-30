//! Semantic validation and false-green rejection for `readiness_rehearsal.v1`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

use super::model::{
    Availability, Cleanup, CleanupDisposition, PathRole, ReasonCode, RehearsalError,
    RehearsalReceipt, SCHEMA_PATH, Status, TreeStatus, compute_receipt_digest,
};

const MAX_DURABLE_STRING: usize = 512;

pub(crate) fn validate_receipt(
    receipt: RehearsalReceipt,
) -> Result<RehearsalReceipt, RehearsalError> {
    reject_privacy(&receipt)?;
    reject_invariants(&receipt)?;
    reject_identities(&receipt)?;
    reject_instruments(&receipt)?;
    reject_artifacts(&receipt)?;
    reject_installed(&receipt)?;
    reject_cleanup_and_mutations(&receipt)?;
    reject_status_aggregation(&receipt)?;
    validate_schema_value(&serde_json::to_value(&receipt).map_err(|error| {
        RehearsalError::new(ReasonCode::MalformedDocument, redact_parse_error(&error.to_string()))
    })?)?;
    let expected = compute_receipt_digest(&receipt)?;
    if receipt.receipt_digest != expected {
        return Err(RehearsalError::new(
            ReasonCode::ReceiptDigestMismatch,
            "receipt fields were edited without regenerating receipt_digest",
        ));
    }
    Ok(receipt)
}

pub(crate) fn validate_bytes(bytes: &[u8]) -> Result<RehearsalReceipt, RehearsalError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| {
        RehearsalError::new(ReasonCode::MalformedDocument, redact_parse_error(&error.to_string()))
    })?;
    scan_privacy(&value, "$")?;
    let receipt: RehearsalReceipt = serde_json::from_value(value).map_err(|error| {
        RehearsalError::new(ReasonCode::MalformedDocument, redact_parse_error(&error.to_string()))
    })?;
    validate_receipt(receipt)
}

pub(crate) fn validate_schema_file(root: &Path) -> Result<(), RehearsalError> {
    let schema = load_schema(root)?;
    jsonschema::validator_for(&schema).map_err(|error| {
        RehearsalError::new(ReasonCode::SchemaViolation, format!("{SCHEMA_PATH}: {error}"))
    })?;
    Ok(())
}

fn load_schema(root: &Path) -> Result<Value, RehearsalError> {
    let path = root.join(SCHEMA_PATH);
    let text = std::fs::read_to_string(&path).map_err(|error| {
        RehearsalError::new(ReasonCode::MalformedDocument, format!("{}: {error}", path.display()))
    })?;
    serde_json::from_str(&text).map_err(|error| {
        RehearsalError::new(ReasonCode::MalformedDocument, format!("{}: {error}", path.display()))
    })
}

fn validate_schema_value(value: &Value) -> Result<(), RehearsalError> {
    let root = crate::utils::project_root()
        .map_err(|error| RehearsalError::new(ReasonCode::MalformedDocument, error.to_string()))?;
    let schema = load_schema(&root)?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| RehearsalError::new(ReasonCode::SchemaViolation, error.to_string()))?;
    let violations: Vec<String> =
        validator.iter_errors(value).map(|error| error.to_string()).collect();
    if violations.is_empty() {
        Ok(())
    } else {
        Err(RehearsalError::new(ReasonCode::SchemaViolation, violations.join("; ")))
    }
}

fn reject_invariants(receipt: &RehearsalReceipt) -> Result<(), RehearsalError> {
    if receipt.schema_version != super::model::SCHEMA_VERSION {
        return Err(RehearsalError::new(
            ReasonCode::SchemaViolation,
            format!("schema_version must be {}", super::model::SCHEMA_VERSION),
        ));
    }
    if receipt.release_cut {
        return Err(RehearsalError::new(
            ReasonCode::ReleaseCutTrue,
            "readiness_rehearsal.v1 never records a release cut",
        ));
    }
    if !receipt.published_channels.is_empty() {
        return Err(RehearsalError::new(
            ReasonCode::PublishedChannelsNonEmpty,
            "published_channels must remain empty on a no-publish rehearsal",
        ));
    }
    Ok(())
}

fn reject_identities(receipt: &RehearsalReceipt) -> Result<(), RehearsalError> {
    if receipt.package_version != receipt.extension_version {
        return Err(RehearsalError::new(
            ReasonCode::MixedVersion,
            format!(
                "package_version {} is not interchangeable with extension_version {}",
                receipt.package_version, receipt.extension_version
            ),
        ));
    }
    if matches!(receipt.status, Status::Pass)
        && !matches!(receipt.repository.tree_status, TreeStatus::Clean)
    {
        return Err(RehearsalError::new(
            ReasonCode::UncleanTreePass,
            "overall pass requires a clean tree before artifacts may claim exact-HEAD provenance",
        ));
    }
    for stage in &receipt.stages {
        for artifact in &stage.artifacts {
            if artifact.candidate_id != receipt.candidate_id {
                return Err(RehearsalError::new(
                    ReasonCode::MixedSourceSha,
                    format!(
                        "artifact {} candidate_id {} is not rehearsal candidate {}",
                        artifact.name, artifact.candidate_id, receipt.candidate_id
                    ),
                ));
            }
            if artifact.source_sha != receipt.repository.head_sha {
                return Err(RehearsalError::new(
                    ReasonCode::MixedSourceSha,
                    format!(
                        "artifact {} source_sha {} is not repository HEAD {}",
                        artifact.name, artifact.source_sha, receipt.repository.head_sha
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn reject_instruments(receipt: &RehearsalReceipt) -> Result<(), RehearsalError> {
    for stage in &receipt.stages {
        let instrument = &stage.instrument;
        if looks_like_git_sha(&instrument.identity) || looks_like_sha256(&instrument.identity) {
            return Err(RehearsalError::new(
                ReasonCode::IdentityKindMismatch,
                format!(
                    "stage {} instrument identity is not interchangeable with source or artifact digests",
                    stage.id
                ),
            ));
        }
        let unavailable = !matches!(instrument.availability, Availability::Available);
        if unavailable && matches!(stage.status, Status::Pass) {
            return Err(RehearsalError::new(
                ReasonCode::UnavailableInstrumentPass,
                format!(
                    "stage {} records unavailable/not_proven instrument {} as pass",
                    stage.id, instrument.identity
                ),
            ));
        }
        if !instrument.outcome.is_terminal_success_shape() && matches!(stage.status, Status::Pass) {
            return Err(RehearsalError::new(
                ReasonCode::NonTerminalStagePass,
                format!("stage {} records {} as pass", stage.id, instrument.outcome.as_str()),
            ));
        }
        if matches!(stage.status, Status::Pass) && instrument.exit_code != Some(0) {
            return Err(RehearsalError::new(
                ReasonCode::NonzeroExitPass,
                format!(
                    "stage {} records pass without a successful exit (exit_code={})",
                    stage.id,
                    match instrument.exit_code {
                        Some(code) => code.to_string(),
                        None => "none".to_string(),
                    }
                ),
            ));
        }
    }
    Ok(())
}

fn reject_artifacts(receipt: &RehearsalReceipt) -> Result<(), RehearsalError> {
    for stage in &receipt.stages {
        for artifact in &stage.artifacts {
            // Archive-ness is `members` presence, not the producer-chosen role string.
            let Some(members) = artifact.members.as_deref() else {
                continue;
            };
            if members.is_empty() {
                return Err(RehearsalError::new(
                    ReasonCode::MissingArchiveMember,
                    format!("archive {} declares no members", artifact.name),
                ));
            }
            let mut seen_names = BTreeSet::new();
            let mut seen_digests: BTreeMap<&str, &str> = BTreeMap::new();
            for member in members {
                if let Some(checksum) = member.checksum.as_deref()
                    && checksum != member.digest
                {
                    return Err(RehearsalError::new(
                        ReasonCode::ChecksumInvalidMember,
                        format!(
                            "archive {} member {} checksum {} does not match digest {}",
                            artifact.name, member.name, checksum, member.digest
                        ),
                    ));
                }
                if !seen_names.insert(member.name.as_str()) {
                    return Err(RehearsalError::new(
                        ReasonCode::DuplicateArchiveMember,
                        format!("archive {} duplicates member {}", artifact.name, member.name),
                    ));
                }
                if let Some(previous) =
                    seen_digests.insert(member.digest.as_str(), member.name.as_str())
                    && previous != member.name.as_str()
                {
                    return Err(RehearsalError::new(
                        ReasonCode::CollidingArchiveMember,
                        format!(
                            "archive {} members {previous} and {} share digest {}",
                            artifact.name, member.name, member.digest
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn reject_installed(receipt: &RehearsalReceipt) -> Result<(), RehearsalError> {
    for stage in &receipt.stages {
        let member_digests: BTreeSet<&str> = stage
            .artifacts
            .iter()
            .filter_map(|artifact| artifact.members.as_deref())
            .flatten()
            .map(|member| member.digest.as_str())
            .collect();
        for installed in &stage.installed {
            if matches!(stage.status, Status::Pass) && !installed.path_role.is_rehearsal_install() {
                return Err(RehearsalError::new(
                    ReasonCode::WrongBinaryPathRole,
                    format!(
                        "stage {} installed {} path_role {} is not a staged rehearsal subject",
                        stage.id,
                        installed.role,
                        installed.path_role.as_str()
                    ),
                ));
            }
            if matches!(stage.status, Status::Pass)
                && matches!(installed.path_role, PathRole::ArchiveExtracted)
                && !member_digests.contains(installed.digest.as_str())
            {
                return Err(RehearsalError::new(
                    ReasonCode::InstalledMemberMismatch,
                    format!(
                        "stage {} installed {} digest {} is not a member of the inspected archive",
                        stage.id, installed.role, installed.digest
                    ),
                ));
            }
        }
        for artifact in &stage.artifacts {
            let Some(declared) = artifact.declared_server_identity.as_deref() else {
                continue;
            };
            let bundled = stage
                .installed
                .iter()
                .find(|installed| matches!(installed.path_role, PathRole::VsixBundledServer));
            let Some(bundled) = bundled else {
                if matches!(stage.status, Status::Pass) {
                    return Err(RehearsalError::new(
                        ReasonCode::VsixServerMismatch,
                        format!(
                            "stage {} VSIX {} declares server {declared} with no bundled installed subject",
                            stage.id, artifact.name
                        ),
                    ));
                }
                continue;
            };
            if bundled.identity != declared {
                return Err(RehearsalError::new(
                    ReasonCode::VsixServerMismatch,
                    format!(
                        "stage {} VSIX declared server {declared} differs from bundled {}",
                        stage.id, bundled.identity
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn cleanup_is_complete(cleanup: &Cleanup) -> bool {
    matches!(cleanup.disposition, CleanupDisposition::Cleaned)
        && matches!(cleanup.status, Status::Pass)
}

fn cleanup_aggregate_status(cleanup: &Cleanup) -> Status {
    match cleanup.disposition {
        CleanupDisposition::Cleaned => cleanup.status,
        CleanupDisposition::Failed => Status::Failed,
        CleanupDisposition::Skipped | CleanupDisposition::NotProven => {
            cleanup.status.worse(Status::NotProven)
        }
    }
}

fn reject_cleanup_and_mutations(receipt: &RehearsalReceipt) -> Result<(), RehearsalError> {
    if matches!(receipt.status, Status::Pass) && !cleanup_is_complete(&receipt.cleanup) {
        return Err(RehearsalError::new(
            ReasonCode::FailedCleanupHidden,
            "incomplete or unproven cleanup cannot be hidden by an otherwise successful journey",
        ));
    }
    if !receipt.mutations.is_empty() {
        let kinds: Vec<&str> = receipt.mutations.iter().map(|item| item.kind.as_str()).collect();
        return Err(RehearsalError::new(
            ReasonCode::MutationAttempted,
            format!("no-publish rehearsal recorded mutation attempt(s): {}", kinds.join(", ")),
        ));
    }
    Ok(())
}

fn reject_status_aggregation(receipt: &RehearsalReceipt) -> Result<(), RehearsalError> {
    let mut aggregate = Status::Pass;
    let mut first_blocking: Option<&str> = None;
    for stage in &receipt.stages {
        if stage.mandatory {
            aggregate = aggregate.worse(stage.status);
            if first_blocking.is_none()
                && matches!(stage.status, Status::Failed | Status::NotProven)
            {
                first_blocking = Some(stage.id.as_str());
            }
        }
        if first_blocking.is_some()
            && matches!(stage.status, Status::Pass)
            && matches!(receipt.status, Status::Pass)
        {
            return Err(RehearsalError::new(
                ReasonCode::CompensatingStage,
                format!(
                    "stage {} pass does not compensate for an earlier failed or unproven mandatory stage",
                    stage.id
                ),
            ));
        }
    }
    aggregate = aggregate.worse(cleanup_aggregate_status(&receipt.cleanup));
    if receipt.status.rank() < aggregate.rank() {
        if first_blocking.is_some() && matches!(receipt.status, Status::Pass) {
            return Err(RehearsalError::new(
                ReasonCode::CompensatingStage,
                "overall pass cannot compensate for a failed or unproven mandatory stage",
            ));
        }
        return Err(RehearsalError::new(
            ReasonCode::StatusMismatch,
            format!(
                "declared status {} is greener than aggregated {}",
                receipt.status.as_str(),
                aggregate.as_str()
            ),
        ));
    }
    Ok(())
}

fn reject_privacy(receipt: &RehearsalReceipt) -> Result<(), RehearsalError> {
    let value = serde_json::to_value(receipt).map_err(|error| {
        RehearsalError::new(ReasonCode::MalformedDocument, redact_parse_error(&error.to_string()))
    })?;
    scan_privacy(&value, "$")
}

fn scan_privacy(value: &Value, path: &str) -> Result<(), RehearsalError> {
    match value {
        Value::String(text) => {
            if text.len() > MAX_DURABLE_STRING {
                return Err(RehearsalError::new(
                    ReasonCode::PrivacyLeak,
                    format!("{path} is unbounded durable text ({} bytes)", text.len()),
                ));
            }
            if let Some(reason) = privacy_hit(text) {
                return Err(RehearsalError::new(
                    ReasonCode::PrivacyLeak,
                    format!("{path} contains {reason}"),
                ));
            }
            Ok(())
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                scan_privacy(item, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for (key, item) in map {
                if key.len() > MAX_DURABLE_STRING {
                    return Err(RehearsalError::new(
                        ReasonCode::PrivacyLeak,
                        format!(
                            "{path} object key is unbounded durable text ({} bytes)",
                            key.len()
                        ),
                    ));
                }
                if let Some(reason) = privacy_hit(key) {
                    return Err(RehearsalError::new(
                        ReasonCode::PrivacyLeak,
                        format!("{path} object key contains {reason}"),
                    ));
                }
                scan_privacy(item, &format!("{path}.{key}"))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn privacy_hit(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("/home/")
        || lower.contains("/users/")
        || lower.contains("\\users\\")
        || lower.contains("/root/")
        || lower.contains("/tmp")
        || lower.contains("\\temp\\")
        || lower.contains("/var/folders/")
    {
        return Some("a home, temporary, or private filesystem path");
    }
    if lower.contains("ghp_")
        || lower.contains("gho_")
        || lower.contains("github_pat_")
        || lower.contains("ghu_")
        || lower.contains("ghs_")
        || lower.contains("begin private key")
        || lower.contains("x-access-token")
        || lower.contains("aws_secret")
        || lower.contains("akia")
    {
        return Some("a credential or secret marker");
    }
    if lower.contains("password=") || lower.contains("token=") || lower.contains("authorization:") {
        return Some("a private environment or credential assignment");
    }
    None
}

fn redact_parse_error(message: &str) -> String {
    if privacy_hit(message).is_some() || message.len() > MAX_DURABLE_STRING {
        "malformed readiness_rehearsal.v1 document (diagnostic redacted)".to_string()
    } else {
        message.to_string()
    }
}

fn looks_like_git_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn looks_like_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
