use super::{
    ConceptRegistry, FixtureExpectationSidecar, SidecarValidation, SidecarValidationContext,
    ValidatedSidecarPair,
};
use anyhow::{Context, Result};
use std::path::Path;

/// Parse canonical schema-v1 TOML from memory.
pub fn parse_sidecar_str(raw: &str) -> Result<FixtureExpectationSidecar> {
    toml::from_str(raw).context("deserializing fixture expectation schema fixture_expectation.v1")
}

/// Parse the exact retained bytes of a previously validated pair.
///
/// This is the deterministic interposition seam for tests that replace the
/// filesystem path after resolution: parsing remains bound to retained bytes.
pub fn parse_validated_sidecar(
    pair: &ValidatedSidecarPair,
) -> Result<FixtureExpectationSidecar> {
    let raw = std::str::from_utf8(pair.sidecar_bytes()).with_context(|| {
        format!(
            "sidecar {} is not valid UTF-8",
            pair.identity().sidecar_path.display()
        )
    })?;
    parse_sidecar_str(raw).with_context(|| {
        format!(
            "parsing sidecar {}",
            pair.identity().sidecar_path.display()
        )
    })
}

/// Open, validate, retain, and parse a sidecar through one bound root.
pub fn parse_sidecar(
    context: &SidecarValidationContext,
    sidecar_path: &Path,
) -> Result<FixtureExpectationSidecar> {
    let pair = context.resolve_pair(sidecar_path)?;
    parse_validated_sidecar(&pair)
}

/// Validate sidecar semantics against an already opened and content-bound pair.
pub fn validate_validated_sidecar(
    pair: &ValidatedSidecarPair,
    sidecar: &FixtureExpectationSidecar,
    concept_registry: Option<&ConceptRegistry>,
) -> SidecarValidation {
    let mut validation = SidecarValidation::default();
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
    if pair.fixture_bytes().is_empty() {
        validation
            .warnings
            .push("paired fixture is empty".to_string());
    }
    validation
}

/// Validate a parsed sidecar through one bound root and optional concept registry.
pub fn validate_sidecar(
    context: &SidecarValidationContext,
    sidecar_path: &Path,
    sidecar: &FixtureExpectationSidecar,
    concept_registry: Option<&ConceptRegistry>,
) -> SidecarValidation {
    match context.resolve_pair(sidecar_path) {
        Ok(pair) => validate_validated_sidecar(&pair, sidecar, concept_registry),
        Err(error) => SidecarValidation {
            errors: vec![error.to_string()],
            warnings: Vec::new(),
        },
    }
}

/// Open once, parse retained bytes, and validate the same pair.
pub fn load_and_validate_sidecar(
    context: &SidecarValidationContext,
    sidecar_path: &Path,
    concept_registry: Option<&ConceptRegistry>,
) -> Result<SidecarValidation> {
    let pair = context.resolve_pair(sidecar_path)?;
    let sidecar = parse_validated_sidecar(&pair)?;
    Ok(validate_validated_sidecar(
        &pair,
        &sidecar,
        concept_registry,
    ))
}
