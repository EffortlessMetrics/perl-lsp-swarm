//! Cross-target, ownership, fingerprint, and drift validation.

use crate::contract::{
    validate_nonempty, validate_sorted_unique_strings, validate_stable_id,
};
use crate::model::{
    TARGET_MATRIX_INDEX_SCHEMA_VERSION, TARGET_MATRIX_PART_SCHEMA_VERSION,
    TARGET_MATRIX_SCHEMA_VERSION, TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION, TargetDisposition,
    TargetKind, TargetMatrixEntry, TargetMatrixIndex, TargetMatrixPart, TargetTopologyDrift,
    UpstreamTargetMatrix,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const PINNED_PERL_5422_ROW: &str = "5.42.2";
const PINNED_PERL_5422_REF: &str = "v5.42.2";
const PINNED_PERL_5422_SHA: &str = "b62845c7186b0b6a8e4e83419e6b5ef64ceef3ed";
const PINNED_PERL_5422_TARGET_IDS: &[&str] = &[
    "component_base",
    "component_class",
    "component_cmd",
    "component_comp",
    "component_io",
    "component_mro",
    "component_op",
    "component_op_hook",
    "component_opbasic",
    "component_perf",
    "component_porting",
    "component_re",
    "component_run",
    "component_t_lib",
    "component_test_pl",
    "component_uni",
    "instrument_valgrind",
    "legacy_custom_core",
    "legacy_custom_full",
    "make_minitest_notty",
    "make_minitest_tty",
    "make_test_choose",
    "make_test_harness_choose",
    "make_test_harness_notty",
    "make_test_notty",
    "make_test_porting",
    "make_test_reonly",
    "make_test_tty",
    "manifest_cpan",
    "manifest_dist",
    "manifest_ext",
    "manifest_root_lib",
    "optional_benchmark",
    "optional_bigmem",
    "optional_japh",
    "platform_os2",
    "platform_win32",
    "prep_minitest",
    "prep_test",
    "prep_test_reonly",
    "selector_test_core",
    "variant_deparse",
    "variant_taintwarn",
    "variant_utf16_be_bom",
    "variant_utf16_be_no_bom",
    "variant_utf16_le_bom",
    "variant_utf16_le_no_bom",
    "variant_utf8",
];
const PINNED_PERL_5422_TOPOLOGY_SOURCES: &[(&str, &str)] = &[
    (
        "Makefile.SH",
        "8732bb922b4f3365ce48f3979f29b30df850f885",
    ),
    (
        "t/TEST",
        "60c3f01b66a2c82062dc288aa3d336d5531d3b12",
    ),
    (
        "t/harness",
        "c038ad3c96a5e3e9450f3d3fe91ba932356ebfa4",
    ),
];

impl TargetMatrixIndex {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != TARGET_MATRIX_INDEX_SCHEMA_VERSION {
            return Err(format!(
                "unsupported target matrix index schema {}",
                self.schema_version
            ));
        }
        validate_nonempty(&self.perl_version_row, "matrix Perl version row")?;
        validate_nonempty(&self.perl_requested_ref, "matrix requested Perl ref")?;
        validate_git_sha(&self.perl_resolved_ref, "matrix resolved Perl ref")?;
        validate_nonempty(&self.claim_boundary, "matrix claim boundary")?;
        if self.topology_sources.is_empty() {
            return Err("target matrix index requires topology source identities".to_string());
        }
        for (path, digest) in &self.topology_sources {
            validate_nonempty(path, "topology source path")?;
            validate_git_sha(digest, "topology source blob SHA")?;
        }
        validate_sorted_unique_strings(&self.target_files, "target matrix part path")?;
        if self.target_files.is_empty() {
            return Err("target matrix index contains no target parts".to_string());
        }
        for path in &self.target_files {
            validate_matrix_part_path(path)?;
        }
        Ok(())
    }

    pub fn assemble(&self, parts: Vec<TargetMatrixPart>) -> Result<UpstreamTargetMatrix, String> {
        self.validate()?;
        if parts.len() != self.target_files.len() {
            return Err(format!(
                "target matrix loaded {} parts but index declares {}",
                parts.len(),
                self.target_files.len()
            ));
        }
        let mut targets = Vec::new();
        for part in parts {
            part.validate()?;
            targets.extend(part.targets);
        }
        let matrix = UpstreamTargetMatrix {
            schema_version: TARGET_MATRIX_SCHEMA_VERSION.to_string(),
            perl_version_row: self.perl_version_row.clone(),
            perl_requested_ref: self.perl_requested_ref.clone(),
            perl_resolved_ref: self.perl_resolved_ref.clone(),
            topology_sources: self.topology_sources.clone(),
            targets,
            claim_boundary: self.claim_boundary.clone(),
        };
        matrix.validate()?;
        Ok(matrix)
    }
}

impl TargetMatrixPart {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != TARGET_MATRIX_PART_SCHEMA_VERSION {
            return Err(format!(
                "unsupported target matrix part schema {}",
                self.schema_version
            ));
        }
        if self.targets.is_empty() {
            return Err("target matrix part contains no rows".to_string());
        }
        if self.targets.windows(2).any(|pair| {
            pair[0].contract.target_id.as_str() >= pair[1].contract.target_id.as_str()
        }) {
            return Err("target matrix part rows must be strictly sorted by target ID".to_string());
        }
        Ok(())
    }
}

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
            if matches!(entry.disposition, TargetDisposition::PlatformUnavailable)
                && (entry.contract.capability_predicates.is_empty()
                    || entry.contract.exclusions.is_empty())
            {
                return Err(format!(
                    "platform-unavailable target {} requires capability and exclusion evidence",
                    entry.contract.target_id
                ));
            }
            if matches!(entry.disposition, TargetDisposition::PolicyExcluded)
                && entry.contract.exclusions.is_empty()
            {
                return Err(format!(
                    "policy-excluded target {} requires an exclusion reason",
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

        validate_reference_graph(&self.targets)?;
        self.validate_pinned_5422_inventory()?;
        Ok(())
    }

    fn validate_pinned_5422_inventory(&self) -> Result<(), String> {
        if self.perl_version_row != PINNED_PERL_5422_ROW {
            return Ok(());
        }
        if self.perl_requested_ref != PINNED_PERL_5422_REF
            || self.perl_resolved_ref != PINNED_PERL_5422_SHA
        {
            return Err(format!(
                "Perl 5.42.2 target matrix must bind {PINNED_PERL_5422_REF} at {PINNED_PERL_5422_SHA}"
            ));
        }
        let actual_ids = self
            .targets
            .iter()
            .map(|entry| entry.contract.target_id.as_str())
            .collect::<Vec<_>>();
        if actual_ids.as_slice() != PINNED_PERL_5422_TARGET_IDS {
            let actual = actual_ids.iter().copied().collect::<BTreeSet<_>>();
            let expected = PINNED_PERL_5422_TARGET_IDS
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            let missing = expected.difference(&actual).copied().collect::<Vec<_>>();
            let unexpected = actual.difference(&expected).copied().collect::<Vec<_>>();
            return Err(format!(
                "Perl 5.42.2 target inventory drifted; missing={missing:?}, unexpected={unexpected:?}"
            ));
        }
        let expected_sources = PINNED_PERL_5422_TOPOLOGY_SOURCES
            .iter()
            .map(|(path, sha)| ((*path).to_string(), (*sha).to_string()))
            .collect::<BTreeMap<_, _>>();
        if self.topology_sources != expected_sources {
            return Err("Perl 5.42.2 topology source identities drifted".to_string());
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
        if self.observed_topology_sources.is_empty() {
            return Err("topology drift requires observed source identities".to_string());
        }
        for (path, digest) in &self.observed_topology_sources {
            validate_nonempty(path, "observed topology source path")?;
            validate_git_sha(digest, "observed topology source blob SHA")?;
        }
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

fn validate_reference_graph(entries: &[TargetMatrixEntry]) -> Result<(), String> {
    let edges = entries
        .iter()
        .map(|entry| {
            let mut references = entry.contract.composite_members.clone();
            if let Some(base) = &entry.contract.variant_of {
                references.push(base.clone());
            }
            references.sort();
            references.dedup();
            (entry.contract.target_id.clone(), references)
        })
        .collect::<BTreeMap<_, _>>();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for target_id in edges.keys() {
        visit_target(target_id, &edges, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn visit_target(
    target_id: &str,
    edges: &BTreeMap<String, Vec<String>>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) -> Result<(), String> {
    if visited.contains(target_id) {
        return Ok(());
    }
    if !visiting.insert(target_id.to_string()) {
        return Err(format!("target reference graph contains a cycle at {target_id}"));
    }
    if let Some(references) = edges.get(target_id) {
        for reference in references {
            visit_target(reference, edges, visiting, visited)?;
        }
    }
    visiting.remove(target_id);
    visited.insert(target_id.to_string());
    Ok(())
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

fn validate_matrix_part_path(value: &str) -> Result<(), String> {
    if value.starts_with('/')
        || value.contains('/')
        || value.contains('\\')
        || !value.ends_with(".json")
    {
        return Err(format!("invalid target matrix part path {value}"));
    }
    Ok(())
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
