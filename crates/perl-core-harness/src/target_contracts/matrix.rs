//! Cross-target, ownership, fingerprint, and drift validation.

use crate::contract::{validate_nonempty, validate_sorted_unique_strings};
use crate::model::{
    TARGET_MATRIX_INDEX_SCHEMA_VERSION, TARGET_MATRIX_PART_SCHEMA_VERSION,
    TARGET_MATRIX_SCHEMA_VERSION, TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION, TargetDisposition,
    TargetKind, TargetMatrixEntry, TargetMatrixIndex, TargetMatrixPart, TargetSelectionContract,
    TargetTopologyDrift, TargetTopologyDriftStatus, UpstreamTargetMatrix,
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
    "legacy_custom_core_harness",
    "legacy_custom_core_test",
    "legacy_custom_full_harness",
    "legacy_custom_full_test",
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
    ("Makefile.SH", "8732bb922b4f3365ce48f3979f29b30df850f885"),
    ("t/TEST", "60c3f01b66a2c82062dc288aa3d336d5531d3b12"),
    ("t/harness", "c038ad3c96a5e3e9450f3d3fe91ba932356ebfa4"),
];

impl TargetMatrixIndex {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != TARGET_MATRIX_INDEX_SCHEMA_VERSION {
            return Err(format!("unsupported target matrix index schema {}", self.schema_version));
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
            return Err(format!("unsupported target matrix part schema {}", self.schema_version));
        }
        if self.targets.is_empty() {
            return Err("target matrix part contains no rows".to_string());
        }
        if self
            .targets
            .windows(2)
            .any(|pair| pair[0].contract.target_id.as_str() >= pair[1].contract.target_id.as_str())
        {
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
        Ok(sha256_hex(&bytes))
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
        if self
            .targets
            .windows(2)
            .any(|pair| pair[0].contract.target_id.as_str() >= pair[1].contract.target_id.as_str())
        {
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
            if let Some(predecessor) = entry.contract.replaces_target_id.as_deref()
                && (!ids.contains(predecessor) || predecessor == entry.contract.target_id)
            {
                return Err(format!(
                    "target {} references missing or self replacement predecessor {}",
                    entry.contract.target_id, predecessor
                ));
            }
        }

        validate_global_target_namespace(&self.targets)?;
        validate_variant_base_kinds(&self.targets)?;
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
        let actual_ids =
            self.targets.iter().map(|entry| entry.contract.target_id.as_str()).collect::<Vec<_>>();
        if actual_ids.as_slice() != PINNED_PERL_5422_TARGET_IDS {
            let actual = actual_ids.iter().copied().collect::<BTreeSet<_>>();
            let expected = PINNED_PERL_5422_TARGET_IDS.iter().copied().collect::<BTreeSet<_>>();
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
        observed: Option<&UpstreamTargetMatrix>,
    ) -> Result<(), String> {
        if self.schema_version != TARGET_TOPOLOGY_DRIFT_SCHEMA_VERSION {
            return Err(format!("unsupported topology drift schema {}", self.schema_version));
        }
        validate_sha256(&self.pinned_matrix_fingerprint, "pinned matrix fingerprint")?;
        if self.pinned_matrix_fingerprint != pinned_fingerprint {
            return Err("topology drift references a different pinned matrix".to_string());
        }
        validate_nonempty(&self.observed_perl_ref, "observed Perl ref")?;
        validate_git_sha(&self.observed_perl_resolved_ref, "observed resolved Perl ref")?;
        if self.observed_topology_sources.is_empty() {
            return Err("topology drift requires observed source identities".to_string());
        }
        for (path, digest) in &self.observed_topology_sources {
            validate_nonempty(path, "observed topology source path")?;
            validate_git_sha(digest, "observed topology source blob SHA")?;
        }
        let pinned_source_paths = pinned.topology_sources.keys().collect::<BTreeSet<_>>();
        let observed_source_paths = self.observed_topology_sources.keys().collect::<BTreeSet<_>>();
        if observed_source_paths != pinned_source_paths {
            return Err(format!(
                "topology drift source set differs from pinned authorities: expected {pinned_source_paths:?}, observed {observed_source_paths:?}"
            ));
        }
        validate_nonempty(&self.claim_boundary, "topology drift claim boundary")?;
        validate_sorted_unique_strings(&self.added_target_ids, "added target ID")?;
        validate_sorted_unique_strings(&self.removed_target_ids, "removed target ID")?;
        validate_sorted_unique_strings(&self.changed_target_ids, "changed target ID")?;
        validate_disjoint_drift_lists(self)?;

        match self.status {
            TargetTopologyDriftStatus::NotProven => {
                if observed.is_some()
                    || self.observed_matrix_fingerprint.is_some()
                    || !self.added_target_ids.is_empty()
                    || !self.removed_target_ids.is_empty()
                    || !self.changed_target_ids.is_empty()
                {
                    return Err(
                        "not-proven topology drift cannot carry observed-matrix or classification claims"
                            .to_string(),
                    );
                }
                let reason = self
                    .not_proven_reason
                    .as_deref()
                    .ok_or_else(|| "not-proven topology drift requires a reason".to_string())?;
                validate_nonempty(reason, "not-proven reason")?;
            }
            TargetTopologyDriftStatus::Compared => {
                let observed = observed.ok_or_else(|| {
                    "compared topology drift requires an observed target matrix".to_string()
                })?;
                observed.validate()?;
                let observed_fingerprint = observed.fingerprint()?;
                validate_sha256(
                    self.observed_matrix_fingerprint.as_deref().ok_or_else(|| {
                        "compared topology drift requires an observed matrix fingerprint"
                            .to_string()
                    })?,
                    "observed matrix fingerprint",
                )?;
                if self.observed_matrix_fingerprint.as_deref()
                    != Some(observed_fingerprint.as_str())
                {
                    return Err(
                        "topology drift references a different observed target matrix".to_string()
                    );
                }
                if self.observed_perl_ref != observed.perl_requested_ref
                    || self.observed_perl_resolved_ref != observed.perl_resolved_ref
                    || self.observed_topology_sources != observed.topology_sources
                {
                    return Err(
                        "topology drift observed identity differs from the observed target matrix"
                            .to_string(),
                    );
                }
                if self.not_proven_reason.is_some() {
                    return Err(
                        "compared topology drift cannot retain a not-proven reason".to_string()
                    );
                }
                let (added, removed, changed) = compute_topology_drift(pinned, observed)?;
                if self.added_target_ids != added
                    || self.removed_target_ids != removed
                    || self.changed_target_ids != changed
                {
                    return Err(format!(
                        "topology drift classifications disagree with observed matrix; expected added={added:?}, removed={removed:?}, changed={changed:?}"
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Target IDs and aliases are globally unique: they are our own spellings, so a
/// collision is always an authoring error.
///
/// `upstream_name` is the one exception, and only under an explicit
/// equivalence. It records the upstream invocation a row measures, and upstream
/// genuinely exposes a single invocation for several distinct denominators —
/// `t/TEST --utf16` covers both byte orders and both BOM states. Those rows are
/// discriminated by `variant_parameters`, not by name, so requiring a unique
/// upstream name would force us to invent spellings upstream does not have.
///
/// The equivalence is therefore narrow: rows may share an upstream name only
/// when they are sibling variants of the same parent and each carries a
/// distinct, nonempty parameter set. Anything else — a shared name across
/// unrelated rows, siblings with identical parameters, or a shared name that
/// also collides with some row's ID or alias — remains ambiguous and is
/// rejected.
fn validate_global_target_namespace(entries: &[TargetMatrixEntry]) -> Result<(), String> {
    let mut owners = BTreeMap::<&str, &str>::new();
    for entry in entries {
        let owner = entry.contract.target_id.as_str();
        let names = std::iter::once(owner).chain(entry.contract.aliases.iter().map(String::as_str));
        for name in names {
            if let Some(existing_owner) = owners.insert(name, owner)
                && existing_owner != owner
            {
                return Err(format!(
                    "target name {name} is ambiguous between {existing_owner} and {owner}"
                ));
            }
        }
    }

    let mut by_upstream_name = BTreeMap::<&str, Vec<&TargetMatrixEntry>>::new();
    for entry in entries {
        by_upstream_name.entry(entry.contract.upstream_name.as_str()).or_default().push(entry);
    }
    for (name, sharers) in by_upstream_name {
        // An upstream name may never collide with a different row's ID or alias.
        if let Some(existing_owner) = owners.get(name)
            && !sharers.iter().any(|entry| entry.contract.target_id == *existing_owner)
        {
            let owner = sharers[0].contract.target_id.as_str();
            return Err(format!(
                "target name {name} is ambiguous between {existing_owner} and {owner}"
            ));
        }
        if sharers.len() == 1 {
            continue;
        }
        let mut parents = BTreeSet::new();
        let mut parameter_sets = BTreeSet::new();
        for entry in &sharers {
            let Some(parent) = entry.contract.variant_of.as_deref() else {
                return Err(format!(
                    "upstream name {name} is shared by {}, which is not a variant",
                    entry.contract.target_id
                ));
            };
            if entry.contract.variant_parameters.is_empty() {
                return Err(format!(
                    "upstream name {name} is shared by {} without discriminating variant parameters",
                    entry.contract.target_id
                ));
            }
            parents.insert(parent);
            parameter_sets.insert(
                entry
                    .contract
                    .variant_parameters
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        if parents.len() != 1 {
            return Err(format!(
                "upstream name {name} is shared across variants of different parents: {parents:?}"
            ));
        }
        if parameter_sets.len() != sharers.len() {
            return Err(format!(
                "upstream name {name} is shared by variants with identical parameters"
            ));
        }
    }
    Ok(())
}

fn validate_variant_base_kinds(entries: &[TargetMatrixEntry]) -> Result<(), String> {
    let by_id = entries
        .iter()
        .map(|entry| (entry.contract.target_id.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    for entry in entries {
        let Some(base_id) = entry.contract.variant_of.as_deref() else {
            continue;
        };
        let base = by_id.get(base_id).ok_or_else(|| {
            format!("target {} references missing base target {base_id}", entry.contract.target_id)
        })?;
        let direct_allowed = match entry.contract.target_kind {
            TargetKind::SelectorVariant => base.contract.target_kind == TargetKind::PhysicalSeries,
            TargetKind::EnvironmentVariant | TargetKind::InstrumentationOnly => matches!(
                base.contract.target_kind,
                TargetKind::PhysicalSeries
                    | TargetKind::SelectorVariant
                    | TargetKind::EnvironmentVariant
            ),
            _ => true,
        };
        if !direct_allowed {
            return Err(format!(
                "target {} cannot inherit from {:?} target {base_id}",
                entry.contract.target_id, base.contract.target_kind
            ));
        }
        if matches!(
            entry.contract.target_kind,
            TargetKind::EnvironmentVariant | TargetKind::InstrumentationOnly
        ) {
            let root = resolve_executable_root(&by_id, base_id)?;
            if !matches!(
                root.contract.target_kind,
                TargetKind::PhysicalSeries | TargetKind::SelectorVariant
            ) {
                return Err(format!(
                    "target {} does not resolve to an executable physical target",
                    entry.contract.target_id
                ));
            }
        }
    }
    Ok(())
}

fn resolve_executable_root<'a>(
    by_id: &BTreeMap<&str, &'a TargetMatrixEntry>,
    start: &str,
) -> Result<&'a TargetMatrixEntry, String> {
    let mut current = start;
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(current) {
            return Err(format!("variant chain contains a cycle at {current}"));
        }
        let entry = by_id
            .get(current)
            .copied()
            .ok_or_else(|| format!("variant chain references missing target {current}"))?;
        match entry.contract.target_kind {
            TargetKind::PhysicalSeries | TargetKind::SelectorVariant => return Ok(entry),
            TargetKind::EnvironmentVariant => {
                current =
                    entry.contract.variant_of.as_deref().ok_or_else(|| {
                        format!("environment variant {current} has no base target")
                    })?;
            }
            other => {
                return Err(format!(
                    "variant chain reaches incompatible {:?} target {current}",
                    other
                ));
            }
        }
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
            if let Some(predecessor) = &entry.contract.replaces_target_id {
                references.push(predecessor.clone());
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

fn compute_topology_drift(
    pinned: &UpstreamTargetMatrix,
    observed: &UpstreamTargetMatrix,
) -> Result<(Vec<String>, Vec<String>, Vec<String>), String> {
    let pinned_contracts = pinned
        .targets
        .iter()
        .map(|entry| {
            Ok((entry.contract.target_id.clone(), target_topology_digest(&entry.contract)?))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let observed_contracts = observed
        .targets
        .iter()
        .map(|entry| {
            Ok((entry.contract.target_id.clone(), target_topology_digest(&entry.contract)?))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let pinned_ids = pinned_contracts.keys().cloned().collect::<BTreeSet<_>>();
    let observed_ids = observed_contracts.keys().cloned().collect::<BTreeSet<_>>();
    let added = observed_ids.difference(&pinned_ids).cloned().collect::<Vec<_>>();
    let removed = pinned_ids.difference(&observed_ids).cloned().collect::<Vec<_>>();
    let changed = pinned_ids
        .intersection(&observed_ids)
        .filter(|target_id| pinned_contracts.get(*target_id) != observed_contracts.get(*target_id))
        .cloned()
        .collect::<Vec<_>>();
    Ok((added, removed, changed))
}

fn target_topology_digest(contract: &TargetSelectionContract) -> Result<String, String> {
    let mut normalized = contract.clone();
    normalized.display_name = "<display-name>".to_string();
    normalized.perl_version_row = "<version-row>".to_string();
    normalized.change_reason = None;
    let bytes = serde_json::to_vec(&normalized)
        .map_err(|error| format!("serializing target topology: {error}"))?;
    Ok(sha256_hex(&bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_disjoint_drift_lists(drift: &TargetTopologyDrift) -> Result<(), String> {
    let added = drift.added_target_ids.iter().collect::<BTreeSet<_>>();
    let removed = drift.removed_target_ids.iter().collect::<BTreeSet<_>>();
    let changed = drift.changed_target_ids.iter().collect::<BTreeSet<_>>();
    if !added.is_disjoint(&removed)
        || !added.is_disjoint(&changed)
        || !removed.is_disjoint(&changed)
    {
        return Err("topology drift classifications must be disjoint".to_string());
    }
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
