use super::{
    ConceptRegistry, FixtureExpectationSidecar, SidecarValidation, SidecarValidationContext,
};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Parse canonical schema-v1 TOML from memory.
pub fn parse_sidecar_str(raw: &str) -> Result<FixtureExpectationSidecar> {
    toml::from_str(raw).context("deserializing fixture expectation schema fixture_expectation.v1")
}

/// Read and parse a sidecar through one bound root and validated pair.
pub fn parse_sidecar(
    context: &SidecarValidationContext,
    sidecar_path: &Path,
) -> Result<FixtureExpectationSidecar> {
    let pair = context.resolve_pair(sidecar_path)?;
    let raw = fs::read_to_string(pair.sidecar_path()).with_context(|| {
        format!(
            "reading sidecar {}",
            pair.identity().sidecar_path.display()
        )
    })?;
    parse_sidecar_str(&raw).with_context(|| {
        format!(
            "parsing sidecar {}",
            pair.identity().sidecar_path.display()
        )
    })
}

/// Validate a parsed sidecar through one bound root and optional concept registry.
pub fn validate_sidecar(
    context: &SidecarValidationContext,
    sidecar_path: &Path,
    sidecar: &FixtureExpectationSidecar,
    concept_registry: Option<&ConceptRegistry>,
) -> SidecarValidation {
    let mut validation = SidecarValidation::default();
    if let Err(error) = context.resolve_pair(sidecar_path) {
        validation.errors.push(error.to_string());
    }

    if sidecar.concept.id.trim().is_empty() {
        validation
            .errors
            .push("concept.id must not be empty".to_string());
    } else if let Some(registry) = concept_registry {
        if !registry.contains(&sidecar.concept.id) {
            validation.errors.push(format!(
                "concept.id '{}' is not present in the loaded concept registry",
                sidecar.concept.id
            ));
        }
    } else {
        validation.warnings.push(format!(
            "concept registry unavailable; concept resolution pending for '{}'",
            sidecar.concept.id
        ));
    }

    if sidecar.concept.tier.trim().is_empty() {
        validation
            .errors
            .push("concept.tier must not be empty".to_string());
    }
    validation
}

/// Read and validate a sidecar through one bound root.
pub fn load_and_validate_sidecar(
    context: &SidecarValidationContext,
    sidecar_path: &Path,
    concept_registry: Option<&ConceptRegistry>,
) -> Result<SidecarValidation> {
    let sidecar = parse_sidecar(context, sidecar_path)?;
    Ok(validate_sidecar(
        context,
        sidecar_path,
        &sidecar,
        concept_registry,
    ))
}
