//! CPAN Meta structural validation for bounded `META.yml` outcomes (#7176).
//!
//! Parse success and spec conformance are intentionally separate: a document
//! can parse under the YAML subset while still failing required-field or shape
//! rules for the recognized Meta spec version.

use serde::{Deserialize, Serialize};

use crate::dist::DistMetadataFacts;
use crate::meta_yml::MetaSpecVersion;

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
    /// The declared `meta-spec` version is one this validator understands, or
    /// legacy implicit v1.4 applies when `meta-spec` is omitted.
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

/// Validate CPAN Meta structure for a successfully parsed `META.yml`.
#[must_use]
pub fn validate_meta_yml_spec(
    spec_version: Option<MetaSpecVersion>,
    meta_spec_key_present: bool,
    top_level_keys: &[String],
    facts: &DistMetadataFacts,
) -> MetaYmlSpecValidation {
    let mut findings = Vec::new();
    let effective = effective_spec(spec_version, meta_spec_key_present, &mut findings);
    let recognized_spec = effective.is_some();
    if let Some(spec) = effective {
        check_required_fields(spec, facts, &mut findings);
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
) -> Option<MetaSpecVersion> {
    match spec_version {
        Some(v) => Some(v),
        None if meta_spec_key_present => {
            findings.push(MetaYmlSpecFinding {
                kind: MetaYmlSpecFindingKind::UnrecognizedSpec,
                field: Some("meta-spec".to_string()),
                detail: "meta-spec is present but its version is not recognized".to_string(),
            });
            None
        }
        None => Some(MetaSpecVersion::V1),
    }
}

fn check_required_fields(
    spec: MetaSpecVersion,
    facts: &DistMetadataFacts,
    findings: &mut Vec<MetaYmlSpecFinding>,
) {
    require_non_empty(facts.name.as_deref(), "name", findings);
    require_non_empty(facts.version.as_deref(), "version", findings);
    if matches!(spec, MetaSpecVersion::V2) {
        require_non_empty(facts.summary.as_deref(), "abstract", findings);
    }
    if facts.licenses.is_empty() {
        findings.push(MetaYmlSpecFinding {
            kind: MetaYmlSpecFindingKind::MissingRequiredField,
            field: Some("license".to_string()),
            detail: "license must be declared for CPAN Meta conformance".to_string(),
        });
    }
}

fn require_non_empty(value: Option<&str>, field: &str, findings: &mut Vec<MetaYmlSpecFinding>) {
    if value.map(str::trim).filter(|s| !s.is_empty()).is_some() {
        return;
    }
    findings.push(MetaYmlSpecFinding {
        kind: MetaYmlSpecFindingKind::MissingRequiredField,
        field: Some(field.to_string()),
        detail: format!("`{field}` is required for CPAN Meta conformance"),
    });
}

fn check_unsupported_top_level(
    spec: MetaSpecVersion,
    keys: &[String],
    findings: &mut Vec<MetaYmlSpecFinding>,
) {
    let allowed = match spec {
        MetaSpecVersion::V1 => V1_TOP_LEVEL,
        MetaSpecVersion::V2 => V2_TOP_LEVEL,
    };
    for key in keys {
        if allowed.contains(&key.as_str()) {
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
    fn implicit_v1_recognized_and_conforms_with_required_fields() {
        let validation = validate_meta_yml_spec(
            None,
            false,
            &["name".to_string(), "version".to_string(), "license".to_string()],
            &facts("Foo-Bar", "1.0", None, &["perl_5"]),
        );
        assert!(validation.recognized_spec);
        assert!(validation.conforms_to_supported_spec);
        assert!(validation.findings.is_empty());
    }

    #[test]
    fn v2_requires_abstract() {
        let validation = validate_meta_yml_spec(
            Some(MetaSpecVersion::V2),
            true,
            &[
                "meta-spec".to_string(),
                "name".to_string(),
                "version".to_string(),
                "license".to_string(),
            ],
            &facts("Foo-Bar", "2.0", None, &["perl_5"]),
        );
        assert!(validation.recognized_spec);
        assert!(!validation.conforms_to_supported_spec);
        assert!(validation.findings.iter().any(|f| f.field.as_deref() == Some("abstract")));
    }

    #[test]
    fn unrecognized_meta_spec_is_not_recognized() {
        let validation = validate_meta_yml_spec(
            None,
            true,
            &["meta-spec".to_string(), "name".to_string()],
            &facts("Foo-Bar", "1.0", None, &["perl_5"]),
        );
        assert!(!validation.recognized_spec);
        assert!(!validation.conforms_to_supported_spec);
        assert!(
            validation.findings.iter().any(|f| f.kind == MetaYmlSpecFindingKind::UnrecognizedSpec)
        );
    }

    #[test]
    fn unsupported_top_level_is_reported_without_blocking_recognition() {
        let validation = validate_meta_yml_spec(
            Some(MetaSpecVersion::V1),
            true,
            &[
                "meta-spec".to_string(),
                "name".to_string(),
                "version".to_string(),
                "license".to_string(),
                "custom_extension".to_string(),
            ],
            &facts("Foo-Bar", "1.0", None, &["perl_5"]),
        );
        assert!(validation.recognized_spec);
        assert!(validation.conforms_to_supported_spec);
        assert!(validation.findings.iter().any(|f| f.field.as_deref() == Some("custom_extension")));
    }
}
