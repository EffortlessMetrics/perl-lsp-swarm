//! CPAN Meta structural validation for bounded `META.yml` outcomes (#7176).
//!
//! Parse success and spec conformance are intentionally separate: a document
//! can parse under the YAML subset while still failing required-field or shape
//! rules for the recognized Meta spec version.

use serde::{Deserialize, Serialize};

use crate::dist::DistMetadataFacts;
use crate::meta_yml::MetaSpecVersion;

/// Observed YAML shape of the top-level `license` field before normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaYmlLicenseShape {
    /// No `license` key.
    Absent,
    /// v1-style scalar license.
    Scalar,
    /// v2-style license list.
    Sequence,
}

/// One structural spec-validation finding (distinct from YAML parse findings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaYmlSpecFinding {
    /// Finding class.
    pub kind: MetaYmlSpecFindingKind,
    /// Top-level field or logical path, when applicable.
    pub field: Option<String>,
    /// Human-readable detail (bounded, path-safe).
    pub detail: String,
}

/// Classes of Meta spec validation findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaYmlSpecFindingKind {
    /// A required field for the recognized spec is absent or empty.
    MissingRequiredField,
    /// A field is present but not in a supported shape for this spec version.
    InvalidFieldShape,
    /// A top-level key is not part of the supported spec surface.
    UnsupportedTopLevelField,
    /// `meta-spec.version` was present but not recognized.
    UnrecognizedSpec,
}

/// Spec recognition and conformance, independent of YAML parse state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaYmlSpecValidation {
    /// The effective CPAN Meta spec version is one this validator understands.
    pub recognized_spec: bool,
    /// Structural CPAN Meta rules for the effective spec version are satisfied.
    pub conforms_to_supported_spec: bool,
    /// Bounded structural findings (may be non-empty while still parsed).
    pub findings: Vec<MetaYmlSpecFinding>,
}

const V1_TOP_LEVEL: &[&str] = &[
    "name",
    "version",
    "abstract",
    "license",
    "author",
    "generated_by",
    "release_status",
    "resources",
    "provides",
    "no_index",
    "keywords",
    "dynamic_config",
    "meta-spec",
    "requires",
    "recommends",
    "suggests",
    "conflicts",
    "build_requires",
    "configure_requires",
    "test_requires",
    "author_requires",
    "prereqs",
    "distribution_type",
    "private",
];

const V2_TOP_LEVEL: &[&str] = &[
    "name",
    "version",
    "abstract",
    "license",
    "author",
    "generated_by",
    "release_status",
    "resources",
    "provides",
    "no_index",
    "keywords",
    "dynamic_config",
    "meta-spec",
    "prereqs",
    "optional_features",
    "x_deprecated",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectiveSpec {
    /// Legacy implicit Meta 1.0 when `meta-spec` is omitted.
    ImplicitV1_0,
    /// Declared Meta 1.x family.
    V1,
    /// Declared Meta 2.x.
    V2,
}

/// Validate CPAN Meta structure for a successfully parsed `META.yml`.
#[must_use]
pub fn validate_meta_yml_spec(
    spec_version: Option<MetaSpecVersion>,
    meta_spec_key_present: bool,
    license_shape: MetaYmlLicenseShape,
    top_level_keys: &[String],
    facts: &DistMetadataFacts,
) -> MetaYmlSpecValidation {
    let mut findings = Vec::new();
    let effective = effective_spec(spec_version, meta_spec_key_present, &mut findings);
    let recognized_spec = effective.is_some();
    if let Some(spec) = effective {
        check_required_fields(spec, license_shape, top_level_keys, facts, &mut findings);
        check_unsupported_top_level(spec, top_level_keys, &mut findings);
    }
    let conforms_to_supported_spec = findings.iter().all(|f| {
        !matches!(
            f.kind,
            MetaYmlSpecFindingKind::MissingRequiredField
                | MetaYmlSpecFindingKind::InvalidFieldShape
                | MetaYmlSpecFindingKind::UnrecognizedSpec
        )
    });
    MetaYmlSpecValidation { recognized_spec, conforms_to_supported_spec, findings }
}

fn effective_spec(
    spec_version: Option<MetaSpecVersion>,
    meta_spec_key_present: bool,
    findings: &mut Vec<MetaYmlSpecFinding>,
) -> Option<EffectiveSpec> {
    match spec_version {
        Some(MetaSpecVersion::V2) => Some(EffectiveSpec::V2),
        Some(MetaSpecVersion::V1) => Some(EffectiveSpec::V1),
        None if meta_spec_key_present => {
            findings.push(MetaYmlSpecFinding {
                kind: MetaYmlSpecFindingKind::UnrecognizedSpec,
                field: Some("meta-spec".to_string()),
                detail: "meta-spec is present but its version is not recognized".to_string(),
            });
            None
        }
        None => Some(EffectiveSpec::ImplicitV1_0),
    }
}

fn check_required_fields(
    spec: EffectiveSpec,
    license_shape: MetaYmlLicenseShape,
    top_level_keys: &[String],
    facts: &DistMetadataFacts,
    findings: &mut Vec<MetaYmlSpecFinding>,
) {
    match spec {
        EffectiveSpec::ImplicitV1_0 => {
            require_non_empty(facts.version.as_deref(), "version", findings);
        }
        EffectiveSpec::V1 => {
            require_non_empty(facts.name.as_deref(), "name", findings);
            require_non_empty(facts.version.as_deref(), "version", findings);
            require_key_present(top_level_keys, "author", findings);
            require_key_present(top_level_keys, "generated_by", findings);
            match license_shape {
                MetaYmlLicenseShape::Absent => {
                    missing_field("license", findings);
                }
                MetaYmlLicenseShape::Scalar if facts.licenses.is_empty() => {
                    missing_field("license", findings);
                }
                MetaYmlLicenseShape::Sequence => findings.push(MetaYmlSpecFinding {
                    kind: MetaYmlSpecFindingKind::InvalidFieldShape,
                    field: Some("license".to_string()),
                    detail: "Meta v1.4 expects `license` as a scalar string".to_string(),
                }),
                MetaYmlLicenseShape::Scalar => {}
            }
        }
        EffectiveSpec::V2 => {
            require_non_empty(facts.name.as_deref(), "name", findings);
            require_non_empty(facts.version.as_deref(), "version", findings);
            require_non_empty(facts.summary.as_deref(), "abstract", findings);
            require_key_present(top_level_keys, "author", findings);
            require_key_present(top_level_keys, "generated_by", findings);
            match license_shape {
                MetaYmlLicenseShape::Absent => missing_field("license", findings),
                MetaYmlLicenseShape::Scalar => findings.push(MetaYmlSpecFinding {
                    kind: MetaYmlSpecFindingKind::InvalidFieldShape,
                    field: Some("license".to_string()),
                    detail: "Meta v2 expects `license` as a non-empty sequence".to_string(),
                }),
                MetaYmlLicenseShape::Sequence if facts.licenses.is_empty() => {
                    missing_field("license", findings);
                }
                MetaYmlLicenseShape::Sequence => {}
            }
        }
    }
}

fn require_key_present(keys: &[String], field: &str, findings: &mut Vec<MetaYmlSpecFinding>) {
    if keys.iter().any(|key| key == field) {
        return;
    }
    missing_field(field, findings);
}

fn missing_field(field: &str, findings: &mut Vec<MetaYmlSpecFinding>) {
    findings.push(MetaYmlSpecFinding {
        kind: MetaYmlSpecFindingKind::MissingRequiredField,
        field: Some(field.to_string()),
        detail: format!("`{field}` is required for CPAN Meta conformance"),
    });
}

fn require_non_empty(value: Option<&str>, field: &str, findings: &mut Vec<MetaYmlSpecFinding>) {
    if value.map(str::trim).filter(|s| !s.is_empty()).is_some() {
        return;
    }
    missing_field(field, findings);
}

fn is_custom_extension_key(key: &str) -> bool {
    key.starts_with("x_") || key.starts_with("X_")
}

fn check_unsupported_top_level(
    spec: EffectiveSpec,
    keys: &[String],
    findings: &mut Vec<MetaYmlSpecFinding>,
) {
    let allowed = match spec {
        EffectiveSpec::ImplicitV1_0 | EffectiveSpec::V1 => V1_TOP_LEVEL,
        EffectiveSpec::V2 => V2_TOP_LEVEL,
    };
    for key in keys {
        if allowed.contains(&key.as_str()) || is_custom_extension_key(key) {
            continue;
        }
        findings.push(MetaYmlSpecFinding {
            kind: MetaYmlSpecFindingKind::UnsupportedTopLevelField,
            field: Some(key.clone()),
            detail: format!("top-level field `{key}` is outside the supported Meta spec surface"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dist::{DistMetadataFacts, DistMetadataSource};
    use crate::id::{Digest, FileId};

    fn facts(
        name: &str,
        version: &str,
        summary: Option<&str>,
        licenses: &[&str],
    ) -> DistMetadataFacts {
        DistMetadataFacts {
            file_id: FileId::new("META.yml", &Digest::of("x")),
            source: DistMetadataSource::MetaYml,
            name: Some(name.to_string()),
            version: Some(version.to_string()),
            summary: summary.map(str::to_string),
            licenses: licenses.iter().map(|s| (*s).to_string()).collect(),
            prereqs: Vec::new(),
        }
    }

    #[test]
    fn implicit_v1_0_requires_only_version() {
        let validation = validate_meta_yml_spec(
            None,
            false,
            MetaYmlLicenseShape::Absent,
            &["version".to_string()],
            &facts("", "1.0", None, &[]),
        );
        assert!(validation.recognized_spec);
        assert!(validation.conforms_to_supported_spec);
    }

    #[test]
    fn declared_v1_requires_scalar_license_and_authorship() {
        let validation = validate_meta_yml_spec(
            Some(MetaSpecVersion::V1),
            true,
            MetaYmlLicenseShape::Sequence,
            &[
                "meta-spec".to_string(),
                "name".to_string(),
                "version".to_string(),
                "license".to_string(),
                "author".to_string(),
                "generated_by".to_string(),
            ],
            &facts("Foo-Bar", "1.0", None, &["perl_5"]),
        );
        assert!(!validation.conforms_to_supported_spec);
        assert!(
            validation.findings.iter().any(|f| f.kind == MetaYmlSpecFindingKind::InvalidFieldShape)
        );
    }

    #[test]
    fn v2_requires_abstract_and_sequence_license() {
        let validation = validate_meta_yml_spec(
            Some(MetaSpecVersion::V2),
            true,
            MetaYmlLicenseShape::Scalar,
            &[
                "meta-spec".to_string(),
                "name".to_string(),
                "version".to_string(),
                "license".to_string(),
                "author".to_string(),
                "generated_by".to_string(),
            ],
            &facts("Foo-Bar", "2.0", Some("summary"), &["perl_5"]),
        );
        assert!(!validation.conforms_to_supported_spec);
        assert!(
            validation.findings.iter().any(|f| f.kind == MetaYmlSpecFindingKind::InvalidFieldShape)
        );
    }

    #[test]
    fn custom_extension_keys_are_not_unsupported() {
        let validation = validate_meta_yml_spec(
            Some(MetaSpecVersion::V2),
            true,
            MetaYmlLicenseShape::Sequence,
            &[
                "meta-spec".to_string(),
                "name".to_string(),
                "version".to_string(),
                "abstract".to_string(),
                "license".to_string(),
                "author".to_string(),
                "generated_by".to_string(),
                "x_serialization_backend".to_string(),
            ],
            &facts("Foo-Bar", "2.0", Some("summary"), &["perl_5"]),
        );
        assert!(validation.conforms_to_supported_spec);
        assert!(
            !validation
                .findings
                .iter()
                .any(|f| f.kind == MetaYmlSpecFindingKind::UnsupportedTopLevelField)
        );
    }

    #[test]
    fn v1_distribution_type_is_supported_top_level() {
        let validation = validate_meta_yml_spec(
            Some(MetaSpecVersion::V1),
            true,
            MetaYmlLicenseShape::Scalar,
            &[
                "meta-spec".to_string(),
                "name".to_string(),
                "version".to_string(),
                "license".to_string(),
                "author".to_string(),
                "generated_by".to_string(),
                "distribution_type".to_string(),
            ],
            &facts("Foo-Bar", "1.0", None, &["perl_5"]),
        );
        assert!(
            !validation
                .findings
                .iter()
                .any(|f| f.kind == MetaYmlSpecFindingKind::UnsupportedTopLevelField)
        );
    }

    #[test]
    fn unrecognized_meta_spec_is_not_recognized() {
        let validation = validate_meta_yml_spec(
            None,
            true,
            MetaYmlLicenseShape::Absent,
            &["meta-spec".to_string(), "name".to_string()],
            &facts("Foo-Bar", "1.0", None, &["perl_5"]),
        );
        assert!(!validation.recognized_spec);
        assert!(
            validation.findings.iter().any(|f| f.kind == MetaYmlSpecFindingKind::UnrecognizedSpec)
        );
    }
}
