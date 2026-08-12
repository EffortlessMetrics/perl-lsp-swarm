//! Target-contract shape validation.

use crate::model::{
    TARGET_SELECTION_SCHEMA_VERSION, TargetAuthorityKind, TargetKind, TargetSelectionContract,
    TargetSelector, TargetTerminalPolicy,
};
use std::collections::BTreeSet;

impl TargetSelectionContract {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != TARGET_SELECTION_SCHEMA_VERSION {
            return Err(format!(
                "target {} uses unsupported schema {}",
                self.target_id, self.schema_version
            ));
        }
        validate_stable_id(&self.target_id, "target ID")?;
        validate_nonempty(&self.upstream_name, "upstream name")?;
        validate_sorted_unique_strings(&self.aliases, "target alias")?;
        if self.aliases.iter().any(|alias| alias == &self.upstream_name) {
            return Err(format!(
                "target {} repeats its upstream name as an alias",
                self.target_id
            ));
        }
        validate_nonempty(&self.display_name, "display name")?;
        validate_nonempty(&self.perl_version_row, "Perl version row")?;
        validate_nonempty(&self.authority.entrypoint, "authority entrypoint")?;
        if let Some(authority) = &self.selection_authority {
            validate_nonempty(&authority.entrypoint, "selection authority entrypoint")?;
            if authority.kind == TargetAuthorityKind::Make {
                return Err(format!(
                    "target {} selection authority must name a test scheduler, not a Make target",
                    self.target_id
                ));
            }
        }
        validate_optional_stable_id(self.variant_of.as_deref(), "variant target ID")?;
        validate_optional_stable_id(self.replaces_target_id.as_deref(), "replaced target ID")?;
        if self.replaces_target_id.is_some() && self.change_reason.is_none() {
            return Err(format!(
                "target {} replaces another target without a change reason",
                self.target_id
            ));
        }
        if let Some(reason) = self.change_reason.as_deref() {
            validate_nonempty(reason, "change reason")?;
        }

        let mut selectors = BTreeSet::new();
        for selector in &self.selectors {
            validate_selector(selector)?;
            if !selectors.insert(selector) {
                return Err(format!("target {} contains a duplicate selector", self.target_id));
            }
        }
        if self.script_forms.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(format!(
                "target {} script forms must be strictly sorted and unique",
                self.target_id
            ));
        }
        validate_sorted_unique_strings(&self.composite_members, "composite member")?;
        validate_unique_strings_in_order(&self.runner_switches, "runner switch")?;
        validate_sorted_unique_strings(&self.capability_predicates, "capability predicate")?;
        validate_sorted_unique_strings(&self.preparation.required_products, "required product")?;

        validate_string_map(&self.variant_parameters, "variant parameter")?;
        validate_string_map(&self.environment, "environment")?;
        for exclusion in &self.exclusions {
            validate_nonempty(&exclusion.subject, "exclusion subject")?;
            validate_stable_id(&exclusion.reason_code, "exclusion reason")?;
            validate_nonempty(&exclusion.claim_impact, "exclusion claim impact")?;
        }

        match self.target_kind {
            TargetKind::PhysicalSeries => self.validate_physical(),
            TargetKind::SelectorVariant => self.validate_selector_variant(),
            TargetKind::EnvironmentVariant => self.validate_environment_variant(),
            TargetKind::PreparationOnly => self.validate_preparation(),
            TargetKind::GeneratedComposite => self.validate_composite(),
            TargetKind::InstrumentationOnly => self.validate_instrumentation(),
        }
    }

    fn validate_physical(&self) -> Result<(), String> {
        if self.selectors.is_empty()
            || self.script_forms.is_empty()
            || self.selection_authority.is_none()
        {
            return Err(format!(
                "physical target {} requires a selection authority, selectors, and script forms",
                self.target_id
            ));
        }
        if self.variant_of.is_some()
            || !self.composite_members.is_empty()
            || self.composite_overlap_policy.is_some()
        {
            return Err(format!(
                "physical target {} cannot be a variant or composite",
                self.target_id
            ));
        }
        if !self.variant_parameters.is_empty() {
            return Err(format!(
                "physical target {} cannot define variant parameters",
                self.target_id
            ));
        }
        Ok(())
    }

    fn validate_selector_variant(&self) -> Result<(), String> {
        if self.variant_of.is_none()
            || self.selectors.is_empty()
            || self.script_forms.is_empty()
            || self.selection_authority.is_none()
        {
            return Err(format!(
                "selector variant {} requires a base target, selection authority, selectors, and script forms",
                self.target_id
            ));
        }
        if !self.composite_members.is_empty() || self.composite_overlap_policy.is_some() {
            return Err(format!(
                "selector variant {} cannot contain composite state",
                self.target_id
            ));
        }
        Ok(())
    }

    fn validate_environment_variant(&self) -> Result<(), String> {
        if self.variant_of.is_none() || !self.selectors.is_empty() {
            return Err(format!(
                "environment variant {} must inherit one target without new selectors",
                self.target_id
            ));
        }
        if !self.composite_members.is_empty() || self.composite_overlap_policy.is_some() {
            return Err(format!(
                "environment variant {} cannot contain composite state",
                self.target_id
            ));
        }
        let changes_invocation = self.selection_authority.is_some()
            || !self.runner_switches.is_empty()
            || !self.variant_parameters.is_empty()
            || !self.environment.is_empty()
            || !self.script_forms.is_empty()
            || !self.capability_predicates.is_empty()
            || self.terminal_policy != TargetTerminalPolicy::Inherited;
        if !changes_invocation {
            return Err(format!(
                "environment variant {} does not change any declared invocation input",
                self.target_id
            ));
        }
        Ok(())
    }

    fn validate_preparation(&self) -> Result<(), String> {
        if !self.selectors.is_empty()
            || !self.script_forms.is_empty()
            || self.preparation.make_target.is_none()
            || self.selection_authority.is_some()
            || self.variant_of.is_some()
            || !self.composite_members.is_empty()
            || self.composite_overlap_policy.is_some()
            || !self.variant_parameters.is_empty()
        {
            return Err(format!(
                "preparation target {} cannot define a source denominator or variant",
                self.target_id
            ));
        }
        Ok(())
    }

    fn validate_composite(&self) -> Result<(), String> {
        if self.composite_members.is_empty()
            || !self.selectors.is_empty()
            || !self.script_forms.is_empty()
            || self.variant_of.is_some()
            || self.selection_authority.is_some()
            || self.composite_overlap_policy.is_none()
            || !self.variant_parameters.is_empty()
        {
            return Err(format!(
                "composite target {} requires only members and an overlap policy",
                self.target_id
            ));
        }
        Ok(())
    }

    fn validate_instrumentation(&self) -> Result<(), String> {
        if self.variant_of.is_none()
            || !self.selectors.is_empty()
            || !self.script_forms.is_empty()
            || !self.composite_members.is_empty()
            || self.composite_overlap_policy.is_some()
            || self.selection_authority.is_some()
        {
            return Err(format!(
                "instrumentation target {} must reference one existing target",
                self.target_id
            ));
        }
        Ok(())
    }
}

fn validate_selector(selector: &TargetSelector) -> Result<(), String> {
    match selector {
        TargetSelector::RecursiveRoot { path } => validate_literal_local_selector(
            path,
            "recursive-root selector",
        ),
        TargetSelector::ExactFile { path } => {
            validate_literal_local_selector(path, "exact-file selector")
        }
        TargetSelector::NonRecursiveGlob { pattern } => validate_nonrecursive_glob(pattern),
        TargetSelector::ExternalGlob { pattern } => validate_external_selector(pattern),
        TargetSelector::ManifestPopulation { .. } => Ok(()),
    }
}

fn validate_literal_local_selector(value: &str, label: &str) -> Result<(), String> {
    validate_local_selector(value)?;
    if contains_glob_metacharacter(value) {
        return Err(format!("{label} cannot contain glob metacharacters: {value}"));
    }
    Ok(())
}

fn validate_nonrecursive_glob(value: &str) -> Result<(), String> {
    validate_local_selector(value)?;
    if value.contains("**") {
        return Err(format!("non-recursive glob cannot contain **: {value}"));
    }
    if !contains_glob_metacharacter(value) {
        return Err(format!("non-recursive glob must contain a glob pattern: {value}"));
    }
    Ok(())
}

fn contains_glob_metacharacter(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']'))
}

fn validate_local_selector(value: &str) -> Result<(), String> {
    validate_nonempty(value, "local selector")?;
    if value.starts_with('/')
        || value.starts_with("../")
        || value.contains('\\')
        || value.split('/').any(|component| component == "." || component == "..")
    {
        return Err(format!("invalid t-relative selector {value}"));
    }
    Ok(())
}

fn validate_external_selector(value: &str) -> Result<(), String> {
    validate_nonempty(value, "external selector")?;
    let Some(rest) = value.strip_prefix("../") else {
        return Err(format!("external selector must begin with ../: {value}"));
    };
    if rest.is_empty()
        || rest.starts_with('/')
        || rest.contains('\\')
        || rest.split('/').any(|component| component == "." || component == "..")
    {
        return Err(format!("invalid external selector {value}"));
    }
    Ok(())
}

fn validate_string_map(
    values: &std::collections::BTreeMap<String, String>,
    label: &str,
) -> Result<(), String> {
    for (name, value) in values {
        validate_nonempty(name, &format!("{label} key"))?;
        validate_nonempty(value, &format!("{label} value"))?;
    }
    Ok(())
}

pub(crate) fn validate_unique_strings_in_order(
    values: &[String],
    label: &str,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for value in values {
        validate_nonempty(value, label)?;
        if !seen.insert(value) {
            return Err(format!("{label} values must be unique"));
        }
    }
    Ok(())
}

pub(crate) fn validate_sorted_unique_strings(
    values: &[String],
    label: &str,
) -> Result<(), String> {
    for value in values {
        validate_nonempty(value, label)?;
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(format!("{label} values must be strictly sorted and unique"));
    }
    Ok(())
}

pub(crate) fn validate_optional_stable_id(
    value: Option<&str>,
    label: &str,
) -> Result<(), String> {
    if let Some(value) = value {
        validate_stable_id(value, label)?;
    }
    Ok(())
}

pub(crate) fn validate_stable_id(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(format!("{label} must match [a-z0-9_]+: {value}"));
    }
    Ok(())
}

pub(crate) fn validate_nonempty(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} cannot be empty"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn validate_external_selector_for_test(value: &str) -> Result<(), String> {
    validate_external_selector(value)
}

#[cfg(test)]
pub(crate) fn validate_selector_for_test(selector: &TargetSelector) -> Result<(), String> {
    validate_selector(selector)
}
