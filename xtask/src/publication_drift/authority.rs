use super::model::{
    AuthoritySource, ClassificationState, LoadedManifest, ManifestIdentity, ManifestVerification,
    ManifestVerificationStatus, PublicationManifest, REQUIRED_INVARIANTS,
    SUPPORTED_MANIFEST_SCHEMA_VERSION, SubjectIdentity, ValidatedAuthority,
};
use super::path::{
    is_lower_hex, normalize_release_version, valid_repository_path, valid_repository_slug,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub(crate) fn load_authority(
    repo_root: &Path,
    identity: Option<&ManifestIdentity>,
) -> AuthoritySource {
    let Some(identity) = identity else {
        return AuthoritySource::Missing;
    };
    if !valid_repository_path(&identity.path) {
        return AuthoritySource::Invalid {
            message: format!(
                "comparison manifest path must use canonical repository-relative syntax: {:?}",
                identity.path
            ),
            actual_sha256: None,
        };
    }

    let canonical_root = match fs::canonicalize(repo_root) {
        Ok(root) => root,
        Err(error) => {
            return AuthoritySource::Invalid {
                message: format!(
                    "canonicalizing comparison repository root {}: {error}",
                    repo_root.display()
                ),
                actual_sha256: None,
            };
        }
    };
    let declared_path = repo_root.join(&identity.path);
    let path = match fs::canonicalize(&declared_path) {
        Ok(path) => path,
        Err(error) => {
            return AuthoritySource::Invalid {
                message: format!(
                    "resolving comparison manifest {}: {error}",
                    declared_path.display()
                ),
                actual_sha256: None,
            };
        }
    };
    if !path.starts_with(&canonical_root) {
        return AuthoritySource::Invalid {
            message: format!(
                "comparison manifest {} resolves outside repository root {}",
                declared_path.display(),
                canonical_root.display()
            ),
            actual_sha256: None,
        };
    }
    let raw = match fs::read(&path) {
        Ok(raw) => raw,
        Err(error) => {
            return AuthoritySource::Invalid {
                message: format!("reading comparison manifest {}: {error}", path.display()),
                actual_sha256: None,
            };
        }
    };
    let actual_sha256 = sha256_hex(&raw);
    let document = match serde_json::from_slice::<PublicationManifest>(&raw) {
        Ok(document) => document,
        Err(error) => {
            return AuthoritySource::Invalid {
                message: format!("parsing comparison manifest {}: {error}", path.display()),
                actual_sha256: Some(actual_sha256),
            };
        }
    };

    AuthoritySource::Loaded(LoadedManifest { document, actual_sha256 })
}

pub(crate) fn sha256_hex(raw: &[u8]) -> String {
    Sha256::digest(raw).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn validate_subject(
    label: &str,
    subject: &SubjectIdentity,
    state: &mut ClassificationState,
) {
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
            format!("{label} SHA is not a 40-character lowercase commit id: {:?}", subject.sha),
            "release-engineering",
        );
    }
    if !is_lower_hex(&subject.tree_digest, 64) {
        state.mark_not_proven(
            "invalid_subject_tree_digest",
            format!("{label} tree digest is not a lowercase SHA-256: {:?}", subject.tree_digest),
            "release-engineering",
        );
    }
    if normalize_release_version(&subject.version).is_none() {
        state.mark_not_proven(
            "invalid_subject_version",
            format!(
                "{label} version is not a supported normalized release identity: {:?}",
                subject.version
            ),
            "release-engineering",
        );
    }
}

pub(crate) fn validate_comparison_version(
    swarm: &SubjectIdentity,
    public: &SubjectIdentity,
    state: &mut ClassificationState,
) -> Option<String> {
    let swarm_version = normalize_release_version(&swarm.version);
    let public_version = normalize_release_version(&public.version);

    let (Some(swarm_version), Some(public_version)) = (swarm_version, public_version) else {
        return None;
    };
    if swarm_version != public_version {
        state.mark_not_proven(
            "cross_version_comparison",
            format!(
                "publication drift requires the same normalized release version; swarm {:?}, public {:?}",
                swarm.version, public.version
            ),
            "release-engineering",
        );
        return None;
    }
    Some(swarm_version)
}

pub(crate) fn validate_authority(
    identity: Option<&ManifestIdentity>,
    source: &AuthoritySource,
    swarm: &SubjectIdentity,
    public: &SubjectIdentity,
    comparison_version: Option<&str>,
    state: &mut ClassificationState,
) -> (Option<ValidatedAuthority>, ManifestVerification) {
    let Some(identity) = identity else {
        state.mark_not_proven(
            "comparison_manifest_missing",
            "comparison manifest authority is missing",
            "release-engineering",
        );
        return (
            None,
            ManifestVerification {
                status: ManifestVerificationStatus::Missing,
                actual_sha256: None,
                schema_version: None,
            },
        );
    };

    let mut valid = true;
    if !valid_repository_path(&identity.path) {
        valid = false;
        state.mark_not_proven(
            "invalid_manifest_path",
            format!(
                "comparison manifest path must use canonical repository-relative syntax: {:?}",
                identity.path
            ),
            "release-engineering",
        );
    }
    if !is_lower_hex(&identity.sha256, 64) {
        valid = false;
        state.mark_not_proven(
            "invalid_manifest_digest",
            format!("comparison manifest digest is not a lowercase SHA-256: {:?}", identity.sha256),
            "release-engineering",
        );
    }
    if !is_lower_hex(&identity.swarm_sha, 40) || identity.swarm_sha != swarm.sha {
        valid = false;
        state.mark_not_proven(
            "manifest_swarm_basis_mismatch",
            format!(
                "comparison manifest swarm basis {:?} does not match subject {:?}",
                identity.swarm_sha, swarm.sha
            ),
            "release-engineering",
        );
    }
    if !is_lower_hex(&identity.public_sha, 40) || identity.public_sha != public.sha {
        valid = false;
        state.mark_not_proven(
            "manifest_public_basis_mismatch",
            format!(
                "comparison manifest public basis {:?} does not match subject {:?}",
                identity.public_sha, public.sha
            ),
            "release-engineering",
        );
    }

    let loaded = match source {
        AuthoritySource::Missing => {
            state.mark_not_proven(
                "comparison_manifest_not_loaded",
                format!(
                    "comparison manifest {:?} was declared but no authority bytes were loaded",
                    identity.path
                ),
                "release-engineering",
            );
            return (
                None,
                ManifestVerification {
                    status: ManifestVerificationStatus::Missing,
                    actual_sha256: None,
                    schema_version: None,
                },
            );
        }
        AuthoritySource::Invalid { message, actual_sha256 } => {
            state.mark_not_proven(
                "comparison_manifest_invalid",
                message.clone(),
                "release-engineering",
            );
            return (
                None,
                ManifestVerification {
                    status: ManifestVerificationStatus::Invalid,
                    actual_sha256: actual_sha256.clone(),
                    schema_version: None,
                },
            );
        }
        AuthoritySource::Loaded(loaded) => loaded,
    };

    if loaded.actual_sha256 != identity.sha256 {
        valid = false;
        state.mark_not_proven(
            "manifest_digest_mismatch",
            format!(
                "comparison manifest digest mismatch: declared {:?}, actual {:?}",
                identity.sha256, loaded.actual_sha256
            ),
            "release-engineering",
        );
    }

    let document = &loaded.document;
    if document.schema_version != SUPPORTED_MANIFEST_SCHEMA_VERSION {
        valid = false;
        state.mark_not_proven(
            "unsupported_manifest_schema_version",
            format!(
                "unsupported comparison manifest schema version {}; expected {}",
                document.schema_version, SUPPORTED_MANIFEST_SCHEMA_VERSION
            ),
            "release-engineering",
        );
    }
    if !valid_repository_slug(&document.swarm_repository)
        || document.swarm_repository != swarm.repository
    {
        valid = false;
        state.mark_not_proven(
            "manifest_swarm_repository_mismatch",
            format!(
                "comparison manifest swarm repository {:?} does not match {:?}",
                document.swarm_repository, swarm.repository
            ),
            "release-engineering",
        );
    }
    if !valid_repository_slug(&document.public_repository)
        || document.public_repository != public.repository
    {
        valid = false;
        state.mark_not_proven(
            "manifest_public_repository_mismatch",
            format!(
                "comparison manifest public repository {:?} does not match {:?}",
                document.public_repository, public.repository
            ),
            "release-engineering",
        );
    }
    if !is_lower_hex(&document.swarm_tree_digest, 64)
        || document.swarm_tree_digest != swarm.tree_digest
    {
        valid = false;
        state.mark_not_proven(
            "manifest_swarm_tree_digest_mismatch",
            format!(
                "comparison manifest swarm tree digest {:?} does not match {:?}",
                document.swarm_tree_digest, swarm.tree_digest
            ),
            "release-engineering",
        );
    }
    if !is_lower_hex(&document.public_tree_digest, 64)
        || document.public_tree_digest != public.tree_digest
    {
        valid = false;
        state.mark_not_proven(
            "manifest_public_tree_digest_mismatch",
            format!(
                "comparison manifest public tree digest {:?} does not match {:?}",
                document.public_tree_digest, public.tree_digest
            ),
            "release-engineering",
        );
    }
    if document.swarm_sha != identity.swarm_sha || document.swarm_sha != swarm.sha {
        valid = false;
        state.mark_not_proven(
            "manifest_document_swarm_basis_mismatch",
            format!(
                "comparison manifest document swarm basis {:?} does not match {:?}",
                document.swarm_sha, swarm.sha
            ),
            "release-engineering",
        );
    }
    if document.public_sha != identity.public_sha || document.public_sha != public.sha {
        valid = false;
        state.mark_not_proven(
            "manifest_document_public_basis_mismatch",
            format!(
                "comparison manifest document public basis {:?} does not match {:?}",
                document.public_sha, public.sha
            ),
            "release-engineering",
        );
    }

    let manifest_version = normalize_release_version(&document.version);
    match (comparison_version, manifest_version.as_deref()) {
        (Some(expected), Some(found)) if expected == found => {}
        (Some(expected), Some(found)) => {
            valid = false;
            state.mark_not_proven(
                "manifest_version_mismatch",
                format!(
                    "comparison manifest version {:?} normalizes to {:?}, expected {:?}",
                    document.version, found, expected
                ),
                "release-engineering",
            );
        }
        (_, None) => {
            valid = false;
            state.mark_not_proven(
                "invalid_manifest_version",
                format!(
                    "comparison manifest version is not a supported release identity: {:?}",
                    document.version
                ),
                "release-engineering",
            );
        }
        (None, Some(_)) => {
            valid = false;
            state.mark_not_proven(
                "manifest_version_not_comparable",
                "comparison subjects do not establish one normalized release version",
                "release-engineering",
            );
        }
    }

    let mut rules = BTreeMap::new();
    for rule in &document.rules {
        let mut rule_valid = true;
        if rule.id.trim().is_empty() {
            rule_valid = false;
            state.mark_not_proven(
                "empty_manifest_rule_id",
                "comparison manifest contains an empty rule id",
                "release-engineering",
            );
        }
        if !valid_repository_path(&rule.path) {
            rule_valid = false;
            state.mark_not_proven(
                "invalid_manifest_rule_path",
                format!("comparison manifest rule {:?} has invalid path {:?}", rule.id, rule.path),
                owner_or_release(&rule.owner),
            );
        }
        if !is_clean_classification(&rule.classification) {
            rule_valid = false;
            state.mark_not_proven(
                "invalid_manifest_rule_classification",
                format!(
                    "comparison manifest rule {:?} cannot authorize classification {:?}",
                    rule.id, rule.classification
                ),
                owner_or_release(&rule.owner),
            );
        }
        if rule.owner.trim().is_empty() {
            rule_valid = false;
            state.mark_not_proven(
                "manifest_rule_owner_missing",
                format!("comparison manifest rule {:?} has no owner", rule.id),
                "release-engineering",
            );
        }
        if rules.contains_key(&rule.id) {
            rule_valid = false;
            state.mark_not_proven(
                "duplicate_manifest_rule",
                format!("comparison manifest rule id appears more than once: {:?}", rule.id),
                owner_or_release(&rule.owner),
            );
        }
        if rule_valid {
            rules.insert(rule.id.clone(), rule.clone());
        } else {
            valid = false;
        }
    }

    let mut required_invariants = BTreeMap::new();
    for invariant in &document.required_invariants {
        let mut invariant_valid = true;
        if invariant.id.trim().is_empty() {
            invariant_valid = false;
            state.mark_not_proven(
                "empty_manifest_invariant_id",
                "comparison manifest contains an empty invariant id",
                "release-engineering",
            );
        }
        if invariant.owner.trim().is_empty() {
            invariant_valid = false;
            state.mark_not_proven(
                "manifest_invariant_owner_missing",
                format!("comparison manifest invariant {:?} has no owner", invariant.id),
                "release-engineering",
            );
        }
        if required_invariants.contains_key(&invariant.id) {
            invariant_valid = false;
            state.mark_not_proven(
                "duplicate_manifest_invariant",
                format!("comparison manifest invariant appears more than once: {:?}", invariant.id),
                owner_or_release(&invariant.owner),
            );
        }
        if invariant_valid {
            required_invariants.insert(invariant.id.clone(), invariant.owner.clone());
        } else {
            valid = false;
        }
    }

    for required in REQUIRED_INVARIANTS {
        if !required_invariants.contains_key(*required) {
            valid = false;
            state.mark_not_proven(
                "manifest_required_invariant_missing",
                format!("comparison manifest omits minimum required invariant {required:?}"),
                "release-engineering",
            );
        }
    }

    let verification = ManifestVerification {
        status: if valid {
            ManifestVerificationStatus::Verified
        } else {
            ManifestVerificationStatus::Invalid
        },
        actual_sha256: Some(loaded.actual_sha256.clone()),
        schema_version: Some(document.schema_version),
    };

    if valid {
        (Some(ValidatedAuthority { rules, required_invariants }), verification)
    } else {
        (None, verification)
    }
}

pub(crate) fn default_required_invariants() -> BTreeMap<String, String> {
    REQUIRED_INVARIANTS
        .iter()
        .map(|id| ((*id).to_string(), "release-engineering".to_string()))
        .collect()
}

fn is_clean_classification(classification: &str) -> bool {
    matches!(
        classification,
        super::model::EXPECTED_TRANSLATION
            | super::model::APPROVED_EXCLUSION
            | super::model::RELEASE_METADATA
    )
}

fn owner_or_release(owner: &str) -> &str {
    if owner.trim().is_empty() { "release-engineering" } else { owner }
}

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn sha256_is_lowercase_and_stable() {
        assert_eq!(
            sha256_hex(b"publication-drift"),
            "d8c752363980f1e806ea6e58d7044bd594ea4c5bfbb1b1c2df4d878e85dee825"
        );
    }
}
