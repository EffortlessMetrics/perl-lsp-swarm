//! Cross-target, ownership, fingerprint, and drift validation.

use crate::contract::{
    validate_nonempty, validate_sorted_unique_strings, validate_stable_id,
};
use crate::model::{
    TARGET_MATRIX_SCHEMA_VERSION, TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION, TargetDisposition,
    TargetKind, TargetMatrixEntry, TargetTopologyDrift, UpstreamTargetMatrix,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

impl UpstreamTargetMatrix {
    pub fn fingerprint(&self) -> Result<String, String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("serializing normalized target matrix: {error}"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != TARGET_MATRIX_SCHEMA_VERSION {
            return Err(format!("unsupported target matrix schema {}", self.schema_version));
        }
        validate_nonempty(&self.perl_version_row, "matrix Perl version row")?;
        validate_nonempty(&self.perl_requested_ref, "matrix requested Perl ref")?;
        validate_git_sha(&self.perl_resolved_ref, "matrix resolved Perl ref")?;
        validate_nonempty(&self.claim_boundary, "matrix claim boundary")?;
        if self.topology_sources.is_empty() {
            return Err("target matrix requires topology source identities".to_string());
        }
        for (path, digest) in &self.topology_sources {
            validate_nonempty(path, "topology source path")?;
            validate_git_sha(digest, "topology source blob SHA")?;
        }
        if self.targets.is_empty() {
            return Err("target matrix contains no targets".to_string());
        }
        if self.targets.windows(2).any(|pair| {
            pair[0].contract.target_id.as_str() >= pair[1].contract.target_id.as_str()
        }) {
            return Err("target matrix rows must be strictly sorted by target ID".to_string());
        }

        let ids = self
            .targets
            .iter()
            .map(|entry| entry.contract.target_id.as_str())
            .collect::<BTreeSet<_>>();
        if ids.len() != self.targets.len() {
            return Err("target matrix contains duplicate target IDs".to_string());
        }

        for entry in &self.targets {
            entry.contract.validate()?;
            if entry.contract.perl_version_row != self.perl_version_row {
                return Err(format!(
                    "target {} belongs to Perl row {}, expected {}",
                    entry.contract.target_id,
                    entry.contract.perl_version_row,
                    self.perl_version_row
                ));
            }
            validate_nonempty(&entry.claim_boundary, "target claim boundary")?;
            if matches!(entry.disposition, TargetDisposition::Planned)
                && entry.owner_issue.is_none()
            {
                return Err(format!(
                    "planned target {} requires an owner issue",
                    entry.contract.target_id
                ));
            }
            validate_disposition_kind(entry)?;
            if let Some(base) = entry.contract.variant_of.as_deref()
                && (!ids.contains(base) || base == entry.contract.target_id)
            {
                return Err(format!(
                    "target {} references missing or self base target {}",
                    entry.contract.target_id, base
                ));
            }
            for member in &entry.contract.composite_members {
                if !ids.contains(member.as_str()) || member == &entry.contract.target_id {
                    return Err(format!(
                        "target {} references missing or self composite member {}",
                        entry.contract.target_id, member
                    ));
                }
            }
        }
        Ok(())
    }
}

impl TargetTopologyDrift {
    pub fn validate_against(
        &self,
        pinned: &UpstreamTargetMatrix,
        pinned_fingerprint: &str,
    ) -> Result<(), String> {
        if self.schema_version != TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION {
            return Err(format!("unsupported topology drift schema {}", self.schema_version));
        }
        validate_sha256(
            &self.pinned_matrix_fingerprint,
            "pinned matrix fingerprint",
        )?;
        if self.pinned_matrix_fingerprint != pinned_fingerprint {
            return Err("topology drift references a different pinned matrix".to_string());
        }
        validate_nonempty(&self.observed_perl_ref, "observed Perl ref")?;
        validate_git_sha(
            &self.observed_perl_resolved_ref,
            "observed resolved Perl ref",
        )?;
        validate_nonempty(&self.claim_boundary, "topology drift claim boundary")?;
        validate_sorted_unique_strings(&self.added_target_ids, "added target ID")?;
        validate_sorted_unique_strings(&self.removed_target_ids, "removed target ID")?;
        validate_sorted_unique_strings(&self.changed_target_ids, "changed target ID")?;

        let pinned_ids = pinned
            .targets
            .iter()
            .map(|entry| entry.contract.target_id.as_str())
            .collect::<BTreeSet<_>>();
        for id in &self.added_target_ids {
            validate_stable_id(id, "added target ID")?;
            if pinned_ids.contains(id.as_str()) {
                return Err(format!("added target {id} already exists in the pinned matrix"));
            }
        }
        for id in self
            .removed_target_ids
            .iter()
            .chain(self.changed_target_ids.iter())
        {
            validate_stable_id(id, "removed or changed target ID")?;
            if !pinned_ids.contains(id.as_str()) {
                return Err(format!(
                    "removed or changed target {id} is absent from the pinned matrix"
                ));
            }
        }
        let added = self.added_target_ids.iter().collect::<BTreeSet<_>>();
        let removed = self.removed_target_ids.iter().collect::<BTreeSet<_>>();
        let changed = self.changed_target_ids.iter().collect::<BTreeSet<_>>();
        if !added.is_disjoint(&removed)
            || !added.is_disjoint(&changed)
            || !removed.is_disjoint(&changed)
        {
            return Err("topology drift classifications must be disjoint".to_string());
        }
        Ok(())
    }
}

fn validate_disposition_kind(entry: &TargetMatrixEntry) -> Result<(), String> {
    let valid = match entry.disposition {
        TargetDisposition::GeneratedComposite => {
            entry.contract.target_kind == TargetKind::GeneratedComposite
        }
        TargetDisposition::PreparationOnly => {
            entry.contract.target_kind == TargetKind::PreparationOnly
        }
        TargetDisposition::InstrumentationOnly => {
            entry.contract.target_kind == TargetKind::InstrumentationOnly
        }
        TargetDisposition::Implemented
        | TargetDisposition::Planned
        | TargetDisposition::PlatformUnavailable
        | TargetDisposition::PolicyExcluded => !matches!(
            entry.contract.target_kind,
            TargetKind::PreparationOnly
                | TargetKind::GeneratedComposite
                | TargetKind::InstrumentationOnly
        ),
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "target {} has disposition {:?} incompatible with kind {:?}",
            entry.contract.target_id, entry.disposition, entry.contract.target_kind
        ))
    }
}

fn validate_sha256(value: &str, label: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Err(format!("{label} must be a 64-character hexadecimal digest: {value}"))
    } else {
        Ok(())
    }
}

fn validate_git_sha(value: &str, label: &str) -> Result<(), String> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Err(format!("{label} must be a 40-character hexadecimal SHA: {value}"))
    } else {
        Ok(())
    }
}
