//! Target-contract shape validation.

use crate::model::{
    TARGET_SELECTION_SCHEMA_VERSION, TargetKind, TargetSelectionContract, TargetSelector,
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
        validate_nonempty(&self.display_name, "display name")?;
        validate_nonempty(&self.perl_version_row, "Perl version row")?;
        validate_nonempty(&self.authority.entrypoint, "authority entrypoint")?;
        validate_optional_stable_id(self.variant_of.as_deref(), "variant target ID")?;
        validate_optional_stable_id(self.replaces_target_id.as_deref(), "replaced target ID")?;
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
        let script_forms = self.script_forms.iter().collect::<BTreeSet<_>>();
        if script_forms.len() != self.script_forms.len() {
            return Err(format!("target {} contains duplicate script forms", self.target_id));
        }
        validate_sorted_unique_strings(&self.composite_members, "composite member")?;
        validate_sorted_unique_strings(&self.runner_switches, "runner switch")?;
        validate_sorted_unique_strings(&self.capability_predicates, "capability predicate")?;
        validate_sorted_unique_strings(&self.preparation.required_products, "required product")?;

        for (name, value) in &self.environment {
            validate_nonempty(name, "environment key")?;
            validate_nonempty(value, "environment value")?;
        }
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
        if self.selectors.is_empty() || self.script_forms.is_empty() {
            return Err(format!(
                "physical target {} requires selectors and script forms",
                self.target_id
            ));
        }
        if self.variant_of.is_some() || !self.composite_members.is_empty() {
            return Err(format!(
                "physical target {} cannot be a variant or composite",
                self.target_id
            ));
        }
        Ok(())
    }

    fn validate_selector_variant(&self) -> Result<(), String> {
        if self.variant_of.is_none() || self.selectors.is_empty() {
            return Err(format!(
                "selector variant {} requires a base target and selectors",
                self.target_id
            ));
        }
        if !self.composite_members.is_empty() {
            return Err(format!(
                "selector variant {} cannot contain composite members",
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
        if !self.composite_members.is_empty() {
            return Err(format!(
                "environment variant {} cannot contain composite members",
                self.target_id
            ));
        }
        Ok(())
    }

    fn validate_preparation(&self) -> Result<(), String> {
        if !self.selectors.is_empty()
            || !self.script_forms.is_empty()
            || self.preparation.make_target.is_none()
        {
            return Err(format!(
                "preparation target {} cannot define a source denominator",
                self.target_id
            ));
        }
        Ok(())
    }

    fn validate_composite(&self) -> Result<(), String> {
        if self.composite_members.is_empty()
            || !self.selectors.is_empty()
            || !self.script_forms.is_empty()
        {
            return Err(format!(
                "composite target {} requires only component target IDs",
                self.target_id
            ));
        }
        Ok(())
    }

    fn validate_instrumentation(&self) -> Result<(), String> {
        if self.variant_of.is_none()
            || !self.selectors.is_empty()
            || !self.script_forms.is_empty()
        {
            return Err(format!(
                "instrumentation target {} must reference an existing target",
                self.target_id
            ));
        }
        Ok(())
    }
}

fn validate_selector(selector: &TargetSelector) -> Result<(), String> {
    match selector {
        TargetSelector::RecursiveRoot { path } | TargetSelector::ExactFile { path } => {
            validate_local_selector(path)
        }
        TargetSelector::NonRecursiveGlob { pattern } => validate_local_selector(pattern),
        TargetSelector::ExternalGlob { pattern } => validate_external_selector(pattern),
        TargetSelector::ManifestPopulation { .. } => Ok(()),
    }
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
