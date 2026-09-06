//! Bounded, non-executing distribution-authoring facts.
//!
//! `Makefile.PL`, `Build.PL`, and `dist.ini` declare name, version, licence,
//! build-tool identity, resources, and prerequisites. This module recovers
//! those declarations from source text without running authoring helpers or
//! Dist::Zilla plugins. Dynamic, conditional, and plugin-generated values stay
//! typed limitations. Authoring facts are stored separately from final
//! `META.*` facts so consumers can compare them instead of silently merging.

mod build_pl;
mod dist_ini;
mod makefile;
mod scan;

use serde::{Deserialize, Serialize};

use crate::dist::{DistMetadataFacts, DistMetadataSource, Prereq};
use crate::error::ModelLimitation;
use crate::id::{Digest, FileId};
use crate::provenance::{Confidence, EvidenceSource, Producer, Provenance};
use crate::range::{SourceRange, Utf8LineIndex};

pub use build_pl::parse_build_pl;
pub use dist_ini::parse_dist_ini;
pub use makefile::parse_makefile_pl;

use scan::{ScanPair, ScanValue};

/// Native-build MakeMaker keys owned by #2494/#2434; never absorbed here.
const IGNORE_MAKEFILE_KEYS: &[&str] =
    &["INC", "LIBS", "DEFINE", "OBJECT", "MYEXTLIB", "CCFLAGS", "LDDLFLAGS", "XS", "C", "H"];

/// Which authoring file produced a [`DistAuthoringFacts`] record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistAuthoringSource {
    /// `Makefile.PL`.
    MakefilePl,
    /// `Build.PL`.
    BuildPl,
    /// Dist::Zilla `dist.ini`.
    DistIni,
}

impl DistAuthoringSource {
    /// User-facing file name for this source.
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::MakefilePl => "Makefile.PL",
            Self::BuildPl => "Build.PL",
            Self::DistIni => "dist.ini",
        }
    }
}

/// Build-tool identity recovered from an authoring file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistAuthoringBuildTool {
    /// ExtUtils::MakeMaker / `WriteMakefile`.
    ExtUtilsMakeMaker,
    /// Module::Build / `Build.PL`.
    ModuleBuild,
    /// Dist::Zilla / `dist.ini`.
    DistZilla,
}

impl DistAuthoringBuildTool {
    /// Stable identity token for fingerprints and declarations.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExtUtilsMakeMaker => "extutils_makemaker",
            Self::ModuleBuild => "module_build",
            Self::DistZilla => "dist_zilla",
        }
    }
}

/// Kind of observed authoring declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistDeclarationKind {
    /// Distribution or package name.
    Name,
    /// Version literal.
    Version,
    /// `VERSION_FROM` / `version_from` path.
    VersionFrom,
    /// Abstract / summary.
    Abstract,
    /// `ABSTRACT_FROM` path.
    AbstractFrom,
    /// Licence token.
    License,
    /// Prerequisite entry.
    Prereq,
    /// Metadata resource.
    Resource,
    /// `provides` entry.
    Provides,
    /// Dist::Zilla plugin identity or configuration.
    Plugin,
    /// Root authoring metadata (`author`, copyright, …).
    AuthoringMetadata,
    /// `META_ADD` / `META_MERGE` overlay.
    MetaOverlay,
}

/// One observed declaration with a source range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistDeclaration {
    /// What the declaration is.
    pub kind: DistDeclarationKind,
    /// Source key (`NAME`, `homepage`, plugin heading, …).
    pub key: String,
    /// Statically recovered value, when one exists.
    pub value: Option<String>,
    /// Byte/UTF-8 span in the authoring file.
    pub range: SourceRange,
    /// True when the value was not a recoverable literal.
    pub dynamic: bool,
}

/// A metadata resource recovered from authoring source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistResource {
    /// Resource kind (`homepage`, `repository`, `bugtracker`, …).
    pub kind: String,
    /// Primary URL, if statically declared.
    pub url: Option<String>,
    /// Web URL, if statically declared.
    pub web: Option<String>,
    /// Resource type (`git`, …), if statically declared.
    pub type_name: Option<String>,
    /// Span covering the declaration.
    pub range: SourceRange,
}

/// A `provides` entry recovered from authoring source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistProvidesEntry {
    /// Provided package.
    pub module: String,
    /// Declared file, if any.
    pub file: Option<String>,
    /// Declared version, if any.
    pub version: Option<String>,
    /// Span covering the declaration.
    pub range: SourceRange,
}

/// A prerequisite with authoring provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoringPrereq {
    /// Required module.
    pub module: String,
    /// Version requirement, if statically declared.
    pub version: Option<String>,
    /// Phase: `configure` / `build` / `test` / `runtime` / `develop`.
    pub phase: String,
    /// Relation: `requires` / `recommends` / `suggests` / `conflicts`.
    pub relation: String,
    /// Span covering the declaration.
    pub range: SourceRange,
    /// Confidence in the recovered fact.
    pub confidence: Confidence,
    /// True when the version (or the whole entry) was not a literal.
    pub dynamic: bool,
}

impl AuthoringPrereq {
    /// Comparison-ready [`Prereq`] without source range.
    #[must_use]
    pub fn to_prereq(&self) -> Prereq {
        Prereq {
            module: self.module.clone(),
            version: self.version.clone(),
            phase: self.phase.clone(),
            relation: self.relation.clone(),
        }
    }
}

/// Two recovered values for the same field that do not agree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistAuthoringConflict {
    /// Field name (`name`, `version`, `license`, or `prereq:<phase>:<relation>:<module>`).
    pub field: String,
    /// Distinct observed values.
    pub values: Vec<String>,
}

/// Agreement between an authoring fact and a final `META.*` fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistFactAgreement {
    /// Both sides declare the same normalized value.
    Agree,
    /// Both sides declare a value and they differ.
    Disagree,
    /// Only the authoring file declared the field.
    AuthoringOnly,
    /// Only the final metadata file declared the field.
    MetadataOnly,
    /// Authoring could not prove a static value.
    Limited,
}

/// One comparison-ready field between authoring and final metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistFactComparison {
    /// Compared field (`name`, `version`, `license`, `prereq:…`).
    pub field: String,
    /// Authoring source that supplied the left-hand value.
    pub authoring_source: DistAuthoringSource,
    /// Metadata source that supplied the right-hand value.
    pub metadata_source: DistMetadataSource,
    /// Normalized authoring value, if any.
    pub authoring_value: Option<String>,
    /// Normalized metadata value, if any.
    pub metadata_value: Option<String>,
    /// Agreement classification.
    pub agreement: DistFactAgreement,
}

/// Distribution-authoring facts extracted from one authoring file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistAuthoringFacts {
    /// The authoring file these facts came from.
    pub file_id: FileId,
    /// Which authoring format.
    pub source: DistAuthoringSource,
    /// Provenance for the recovered facts.
    pub provenance: Provenance,
    /// Distribution name (`Foo-Bar`), if statically proven.
    pub name: Option<String>,
    /// Distribution version, if statically proven.
    pub version: Option<String>,
    /// `VERSION_FROM` / `version_from` path, if declared.
    pub version_from: Option<String>,
    /// Abstract / summary, if statically proven.
    pub summary: Option<String>,
    /// `ABSTRACT_FROM` path, if declared.
    pub abstract_from: Option<String>,
    /// Declared authors.
    pub authors: Vec<String>,
    /// Declared licences.
    pub licenses: Vec<String>,
    /// Authoring build tool.
    pub build_tool: DistAuthoringBuildTool,
    /// Generated installer implied by Dist::Zilla plugins, if declared.
    pub generated_build_tool: Option<DistAuthoringBuildTool>,
    /// Dist::Zilla plugin identities, in source order.
    pub plugins: Vec<String>,
    /// Resources recovered from static configuration.
    pub resources: Vec<DistResource>,
    /// `provides` entries recovered from static configuration.
    pub provides: Vec<DistProvidesEntry>,
    /// Prerequisites recovered from static configuration.
    pub prereqs: Vec<AuthoringPrereq>,
    /// Every observed declaration, including dynamic holes.
    pub declarations: Vec<DistDeclaration>,
    /// Duplicate/conflicting static declarations.
    pub conflicts: Vec<DistAuthoringConflict>,
    /// Dynamic, conditional, malformed, or plugin-generated limitations.
    pub limitations: Vec<ModelLimitation>,
    /// Digest of the authoring file text.
    pub source_fingerprint: Digest,
    /// Digest of the normalized comparison-ready facts (no ranges).
    pub fact_fingerprint: Digest,
}

impl DistAuthoringFacts {
    /// Comparison-ready prerequisites using the shared [`Prereq`] shape.
    #[must_use]
    pub fn comparison_prereqs(&self) -> Vec<Prereq> {
        self.prereqs.iter().map(AuthoringPrereq::to_prereq).collect()
    }
}

/// Compare one authoring record with one final metadata record.
///
/// Does not merge values. Dynamic authoring fields are [`DistFactAgreement::Limited`].
#[must_use]
pub fn compare_authoring_with_meta(
    authoring: &DistAuthoringFacts,
    metadata: &DistMetadataFacts,
) -> Vec<DistFactComparison> {
    let mut out = Vec::new();
    push_field_comparison(
        &mut out,
        authoring,
        metadata,
        "name",
        authoring.name.clone(),
        metadata.name.clone(),
        field_is_limited(authoring, DistDeclarationKind::Name, authoring.name.is_none())
            || field_conflicted(authoring, "name"),
    );
    let version_limited = authoring.version.is_none()
        && (authoring.version_from.is_some()
            || field_is_limited(authoring, DistDeclarationKind::Version, true)
            || field_conflicted(authoring, "version"));
    push_field_comparison(
        &mut out,
        authoring,
        metadata,
        "version",
        authoring.version.clone(),
        metadata.version.clone(),
        version_limited || field_conflicted(authoring, "version"),
    );
    let authoring_license = normalized_licenses(&authoring.licenses);
    let metadata_license = normalized_licenses(&metadata.licenses);
    push_field_comparison(
        &mut out,
        authoring,
        metadata,
        "license",
        authoring_license,
        metadata_license,
        field_is_limited(authoring, DistDeclarationKind::License, authoring.licenses.is_empty())
            || field_conflicted(authoring, "license"),
    );

    let mut keys = std::collections::BTreeSet::new();
    for prereq in &authoring.prereqs {
        keys.insert((prereq.module.clone(), prereq.phase.clone(), prereq.relation.clone()));
    }
    for prereq in &metadata.prereqs {
        keys.insert((prereq.module.clone(), prereq.phase.clone(), prereq.relation.clone()));
    }
    for (module, phase, relation) in keys {
        let prereq_field = format!("prereq:{phase}:{relation}:{module}");
        let limited = authoring.prereqs.iter().any(|item| {
            item.module == module
                && item.phase == phase
                && item.relation == relation
                && item.dynamic
        }) || field_conflicted(authoring, &prereq_field);
        let authoring_value = if limited {
            None
        } else {
            authoring
                .prereqs
                .iter()
                .filter(|item| {
                    item.module == module && item.phase == phase && item.relation == relation
                })
                .map(|item| item.version.clone().unwrap_or_else(|| "0".to_string()))
                .next()
        };
        let metadata_value = metadata
            .prereqs
            .iter()
            .filter(|item| {
                item.module == module && item.phase == phase && item.relation == relation
            })
            .map(|item| item.version.clone().unwrap_or_else(|| "0".to_string()))
            .next();
        push_field_comparison(
            &mut out,
            authoring,
            metadata,
            &prereq_field,
            authoring_value,
            metadata_value,
            limited,
        );
    }
    out
}

fn field_conflicted(authoring: &DistAuthoringFacts, field: &str) -> bool {
    authoring.conflicts.iter().any(|item| item.field == field)
}

fn field_is_limited(
    authoring: &DistAuthoringFacts,
    kind: DistDeclarationKind,
    missing_static: bool,
) -> bool {
    missing_static && authoring.declarations.iter().any(|item| item.kind == kind && item.dynamic)
}

fn push_field_comparison(
    out: &mut Vec<DistFactComparison>,
    authoring: &DistAuthoringFacts,
    metadata: &DistMetadataFacts,
    field: &str,
    authoring_value: Option<String>,
    metadata_value: Option<String>,
    limited: bool,
) {
    let agreement = if limited && authoring_value.is_none() {
        DistFactAgreement::Limited
    } else {
        match (&authoring_value, &metadata_value) {
            (Some(left), Some(right)) if left == right => DistFactAgreement::Agree,
            (Some(_), Some(_)) => DistFactAgreement::Disagree,
            (Some(_), None) => DistFactAgreement::AuthoringOnly,
            (None, Some(_)) if limited => DistFactAgreement::Limited,
            (None, Some(_)) => DistFactAgreement::MetadataOnly,
            (None, None) => return,
        }
    };
    out.push(DistFactComparison {
        field: field.to_string(),
        authoring_source: authoring.source,
        metadata_source: metadata.source,
        authoring_value,
        metadata_value,
        agreement,
    });
}

fn normalized_licenses(licenses: &[String]) -> Option<String> {
    if licenses.is_empty() {
        return None;
    }
    let mut normalized: Vec<String> = licenses.iter().map(|item| normalize_license(item)).collect();
    normalized.sort();
    normalized.dedup();
    Some(normalized.join(","))
}

fn normalize_license(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "perl" | "perl_5" => "perl_5".to_string(),
        other => other.to_string(),
    }
}

fn normalize_dist_name(raw: &str) -> String {
    raw.trim().replace("::", "-")
}

struct DistCollector<'a> {
    file_id: FileId,
    source: DistAuthoringSource,
    build_tool: DistAuthoringBuildTool,
    generated_build_tool: Option<DistAuthoringBuildTool>,
    content: &'a str,
    index: &'a Utf8LineIndex,
    names: Vec<(String, SourceRange)>,
    versions: Vec<(String, SourceRange)>,
    version_from: Option<String>,
    summaries: Vec<(String, SourceRange)>,
    abstract_from: Option<String>,
    authors: Vec<String>,
    licenses: Vec<(String, SourceRange)>,
    plugins: Vec<String>,
    resources: Vec<DistResource>,
    provides: Vec<DistProvidesEntry>,
    prereqs: Vec<AuthoringPrereq>,
    declarations: Vec<DistDeclaration>,
    limitations: Vec<ModelLimitation>,
}

impl<'a> DistCollector<'a> {
    fn new(
        file_id: FileId,
        source: DistAuthoringSource,
        build_tool: DistAuthoringBuildTool,
        content: &'a str,
        index: &'a Utf8LineIndex,
    ) -> Self {
        Self {
            file_id,
            source,
            build_tool,
            generated_build_tool: None,
            content,
            index,
            names: Vec::new(),
            versions: Vec::new(),
            version_from: None,
            summaries: Vec::new(),
            abstract_from: None,
            authors: Vec::new(),
            licenses: Vec::new(),
            plugins: Vec::new(),
            resources: Vec::new(),
            provides: Vec::new(),
            prereqs: Vec::new(),
            declarations: Vec::new(),
            limitations: Vec::new(),
        }
    }

    fn range(&self, start: usize, end: usize) -> SourceRange {
        let start_byte = u32::try_from(start).unwrap_or(u32::MAX);
        let end_byte = u32::try_from(end.max(start)).unwrap_or(u32::MAX);
        self.index.source_range(start_byte, end_byte)
    }

    fn declaration(
        &mut self,
        kind: DistDeclarationKind,
        key: &str,
        value: Option<String>,
        start: usize,
        end: usize,
        dynamic: bool,
    ) {
        self.declarations.push(DistDeclaration {
            kind,
            key: key.to_string(),
            value,
            range: self.range(start, end),
            dynamic,
        });
    }

    fn limitation(
        &mut self,
        kind: impl Into<String>,
        message: impl Into<String>,
        start: Option<usize>,
        end: Option<usize>,
    ) {
        let kind = kind.into();
        let id = match (start, end) {
            (Some(start), _) => format!("{kind}:{}:{start}", self.source.file_name()),
            _ => format!("{kind}:{}", self.source.file_name()),
        };
        if self.limitations.iter().any(|item| item.id == id) {
            return;
        }
        self.limitations.push(ModelLimitation { id, kind, message: message.into() });
    }

    fn set_name_from_package(&mut self, pair: &ScanPair) {
        self.set_name_pair(pair, true);
    }

    fn set_dist_name(&mut self, pair: &ScanPair) {
        self.set_name_pair(pair, false);
    }

    fn set_name_pair(&mut self, pair: &ScanPair, from_package: bool) {
        match pair.value.as_str() {
            Some(raw) => {
                let name =
                    if from_package { normalize_dist_name(raw) } else { raw.trim().to_string() };
                let range = self.range(pair.key_start, pair.value_end);
                self.names.push((name.clone(), range));
                self.declaration(
                    DistDeclarationKind::Name,
                    &pair.key,
                    Some(name),
                    pair.key_start,
                    pair.value_end,
                    false,
                );
            }
            None => self.dynamic_pair(DistDeclarationKind::Name, pair),
        }
    }

    fn set_literal_name(&mut self, value: &str, start: usize, end: usize) {
        let name = normalize_dist_name(value);
        self.names.push((name.clone(), self.range(start, end)));
        self.declaration(DistDeclarationKind::Name, "name", Some(name), start, end, false);
    }

    fn set_version(&mut self, pair: &ScanPair) {
        match pair.value.as_str() {
            Some(raw) => {
                let version = raw.trim().to_string();
                self.versions.push((version.clone(), self.range(pair.key_start, pair.value_end)));
                self.declaration(
                    DistDeclarationKind::Version,
                    &pair.key,
                    Some(version),
                    pair.key_start,
                    pair.value_end,
                    false,
                );
            }
            None => self.dynamic_pair(DistDeclarationKind::Version, pair),
        }
    }

    fn set_literal_version(&mut self, value: &str, start: usize, end: usize) {
        self.versions.push((value.to_string(), self.range(start, end)));
        self.declaration(
            DistDeclarationKind::Version,
            "version",
            Some(value.to_string()),
            start,
            end,
            false,
        );
    }

    fn set_version_from(&mut self, pair: &ScanPair) {
        match pair.value.as_str() {
            Some(raw) => {
                self.version_from = Some(raw.to_string());
                self.declaration(
                    DistDeclarationKind::VersionFrom,
                    &pair.key,
                    Some(raw.to_string()),
                    pair.key_start,
                    pair.value_end,
                    false,
                );
                self.limitation(
                    "version_from",
                    format!("version is declared via `{raw}`; the referenced file is not executed"),
                    Some(pair.value_start),
                    Some(pair.value_end),
                );
            }
            None => self.dynamic_pair(DistDeclarationKind::VersionFrom, pair),
        }
    }

    fn set_abstract(&mut self, pair: &ScanPair) {
        match pair.value.as_str() {
            Some(raw) => {
                self.summaries.push((raw.to_string(), self.range(pair.key_start, pair.value_end)));
                self.declaration(
                    DistDeclarationKind::Abstract,
                    &pair.key,
                    Some(raw.to_string()),
                    pair.key_start,
                    pair.value_end,
                    false,
                );
            }
            None => self.dynamic_pair(DistDeclarationKind::Abstract, pair),
        }
    }

    fn set_literal_abstract(&mut self, value: &str, start: usize, end: usize) {
        self.summaries.push((value.to_string(), self.range(start, end)));
        self.declaration(
            DistDeclarationKind::Abstract,
            "abstract",
            Some(value.to_string()),
            start,
            end,
            false,
        );
    }

    fn set_abstract_from(&mut self, pair: &ScanPair) {
        match pair.value.as_str() {
            Some(raw) => {
                self.abstract_from = Some(raw.to_string());
                self.declaration(
                    DistDeclarationKind::AbstractFrom,
                    &pair.key,
                    Some(raw.to_string()),
                    pair.key_start,
                    pair.value_end,
                    false,
                );
                self.limitation(
                    "abstract_from",
                    format!(
                        "abstract is declared via `{raw}`; the referenced file is not executed"
                    ),
                    Some(pair.value_start),
                    Some(pair.value_end),
                );
            }
            None => self.dynamic_pair(DistDeclarationKind::AbstractFrom, pair),
        }
    }

    fn add_license(&mut self, pair: &ScanPair) {
        match &pair.value {
            ScanValue::String(raw) => {
                self.push_license(raw, pair.key_start, pair.value_end, &pair.key)
            }
            ScanValue::List(items) => {
                for item in items {
                    if let Some(raw) = item.as_str() {
                        self.push_license(raw, pair.key_start, pair.value_end, &pair.key);
                    } else {
                        self.dynamic_pair(DistDeclarationKind::License, pair);
                    }
                }
            }
            _ => self.dynamic_pair(DistDeclarationKind::License, pair),
        }
    }

    fn add_literal_license(&mut self, value: &str, start: usize, end: usize) {
        self.push_license(value, start, end, "license");
    }

    fn push_license(&mut self, raw: &str, start: usize, end: usize, key: &str) {
        let license = normalize_license(raw);
        self.licenses.push((license.clone(), self.range(start, end)));
        self.declaration(DistDeclarationKind::License, key, Some(license), start, end, false);
    }

    fn add_author(&mut self, value: &str, start: usize, end: usize) {
        self.authors.push(value.to_string());
        self.declaration(
            DistDeclarationKind::AuthoringMetadata,
            "author",
            Some(value.to_string()),
            start,
            end,
            false,
        );
    }

    fn add_prereq_hash(&mut self, pair: &ScanPair, phase: &str, relation: &str) {
        match pair.value.as_hash() {
            Some(entries) => {
                for entry in entries {
                    self.add_prereq_pair(entry, phase, relation);
                }
            }
            None => self.dynamic_pair(DistDeclarationKind::Prereq, pair),
        }
    }

    fn add_prereq_pair(&mut self, pair: &ScanPair, phase: &str, relation: &str) {
        let module = pair.key.trim();
        if module.is_empty() {
            return;
        }
        match pair.value.as_str() {
            Some(version) => {
                self.push_prereq(
                    module,
                    Some(version),
                    phase,
                    relation,
                    pair.key_start,
                    pair.value_end,
                    false,
                );
            }
            None => {
                self.push_prereq(
                    module,
                    None,
                    phase,
                    relation,
                    pair.key_start,
                    pair.value_end,
                    true,
                );
                self.limitation(
                    "dynamic_value",
                    format!("prerequisite `{module}` version is not a static literal"),
                    Some(pair.value_start),
                    Some(pair.value_end),
                );
            }
        }
    }

    fn add_literal_prereq(
        &mut self,
        module: &str,
        version: Option<&str>,
        phase: &str,
        relation: &str,
        start: usize,
        end: usize,
    ) {
        let dynamic = version.is_none();
        self.push_prereq(module, version, phase, relation, start, end, dynamic);
    }

    fn push_prereq(
        &mut self,
        module: &str,
        version: Option<&str>,
        phase: &str,
        relation: &str,
        start: usize,
        end: usize,
        dynamic: bool,
    ) {
        let version = version.map(str::trim).filter(|item| !item.is_empty()).map(ToOwned::to_owned);
        self.prereqs.push(AuthoringPrereq {
            module: module.to_string(),
            version: version.clone(),
            phase: phase.to_string(),
            relation: relation.to_string(),
            range: self.range(start, end),
            confidence: if dynamic { Confidence::Low } else { Confidence::High },
            dynamic,
        });
        self.declaration(DistDeclarationKind::Prereq, module, version, start, end, dynamic);
    }

    fn add_meta_overlay(&mut self, pair: &ScanPair, overlay: &str) {
        self.declaration(
            DistDeclarationKind::MetaOverlay,
            overlay,
            None,
            pair.key_start,
            pair.value_end,
            pair.value.is_dynamic(),
        );
        let Some(hash) = pair.value.as_hash() else {
            self.dynamic_pair(DistDeclarationKind::MetaOverlay, pair);
            return;
        };
        for entry in hash {
            match entry.key.as_str() {
                "resources" => {
                    self.add_resources_value(&entry.value, entry.value_start, entry.value_end);
                }
                "provides" => {
                    self.add_provides_value(&entry.value, entry.value_start, entry.value_end)
                }
                "prereqs" => self.add_v2_prereqs(&entry.value),
                other => {
                    self.declaration(
                        DistDeclarationKind::MetaOverlay,
                        other,
                        entry.value.as_str().map(ToOwned::to_owned),
                        entry.key_start,
                        entry.value_end,
                        entry.value.is_dynamic(),
                    );
                }
            }
        }
    }

    fn add_resources_value(&mut self, value: &ScanValue, start: usize, end: usize) {
        let Some(hash) = value.as_hash() else {
            self.limitation(
                "dynamic_value",
                "resources value is not a static hash",
                Some(start),
                Some(end),
            );
            return;
        };
        for entry in hash {
            self.add_resource_entry(entry);
        }
    }

    fn add_resource_entry(&mut self, pair: &ScanPair) {
        let kind = pair.key.to_ascii_lowercase();
        match &pair.value {
            ScanValue::String(url) => {
                self.resources.push(DistResource {
                    kind: kind.clone(),
                    url: Some(url.clone()),
                    web: None,
                    type_name: None,
                    range: self.range(pair.key_start, pair.value_end),
                });
                self.declaration(
                    DistDeclarationKind::Resource,
                    &kind,
                    Some(url.clone()),
                    pair.key_start,
                    pair.value_end,
                    false,
                );
            }
            ScanValue::Hash(fields) => {
                let mut url = None;
                let mut web = None;
                let mut type_name = None;
                let mut dynamic = false;
                for field in fields {
                    match field.key.as_str() {
                        "url" => {
                            url = self.nested_static_string(field, &mut dynamic);
                        }
                        "web" => {
                            web = self.nested_static_string(field, &mut dynamic);
                        }
                        "type" => {
                            type_name = self.nested_static_string(field, &mut dynamic);
                        }
                        _ => {}
                    }
                }
                self.resources.push(DistResource {
                    kind: kind.clone(),
                    url: url.clone(),
                    web,
                    type_name,
                    range: self.range(pair.key_start, pair.value_end),
                });
                self.declaration(
                    DistDeclarationKind::Resource,
                    &kind,
                    url,
                    pair.key_start,
                    pair.value_end,
                    dynamic,
                );
            }
            _ => self.dynamic_pair(DistDeclarationKind::Resource, pair),
        }
    }

    fn add_literal_resource(&mut self, key: &str, value: &str, start: usize, end: usize) {
        let (kind, field) = match key.split_once('.') {
            Some((kind, field)) => (kind.to_ascii_lowercase(), Some(field)),
            None => (key.to_ascii_lowercase(), None),
        };
        if let Some(existing) = self.resources.iter_mut().find(|item| item.kind == kind) {
            match field {
                Some("web") => existing.web = Some(value.to_string()),
                Some("type") => existing.type_name = Some(value.to_string()),
                Some("url") | None => existing.url = Some(value.to_string()),
                Some(_) => existing.url = Some(value.to_string()),
            }
        } else {
            let mut resource = DistResource {
                kind: kind.clone(),
                url: None,
                web: None,
                type_name: None,
                range: self.range(start, end),
            };
            match field {
                Some("web") => resource.web = Some(value.to_string()),
                Some("type") => resource.type_name = Some(value.to_string()),
                _ => resource.url = Some(value.to_string()),
            }
            self.resources.push(resource);
        }
        self.declaration(
            DistDeclarationKind::Resource,
            key,
            Some(value.to_string()),
            start,
            end,
            false,
        );
    }

    fn add_provides_value(&mut self, value: &ScanValue, start: usize, end: usize) {
        let Some(hash) = value.as_hash() else {
            self.limitation(
                "dynamic_value",
                "provides value is not a static hash",
                Some(start),
                Some(end),
            );
            return;
        };
        for entry in hash {
            let mut file = None;
            let mut version = None;
            let mut dynamic = false;
            if let Some(fields) = entry.value.as_hash() {
                for field in fields {
                    match field.key.as_str() {
                        "file" => {
                            file = self.nested_static_string(field, &mut dynamic);
                        }
                        "version" => {
                            version = self.nested_static_string(field, &mut dynamic);
                        }
                        _ => {}
                    }
                }
            } else if let Some(path) = entry.value.as_str() {
                file = Some(path.to_string());
            } else {
                self.dynamic_pair(DistDeclarationKind::Provides, entry);
                continue;
            }
            self.provides.push(DistProvidesEntry {
                module: entry.key.clone(),
                file,
                version,
                range: self.range(entry.key_start, entry.value_end),
            });
            self.declaration(
                DistDeclarationKind::Provides,
                &entry.key,
                entry.value.as_str().map(ToOwned::to_owned),
                entry.key_start,
                entry.value_end,
                dynamic,
            );
        }
    }

    fn add_v2_prereqs(&mut self, value: &ScanValue) {
        let Some(phases) = value.as_hash() else {
            return;
        };
        for phase in phases {
            let Some(relations) = phase.value.as_hash() else {
                continue;
            };
            for relation in relations {
                if let Some(modules) = relation.value.as_hash() {
                    for module in modules {
                        self.add_prereq_pair(module, &phase.key, &relation.key);
                    }
                }
            }
        }
    }

    fn note_plugin(&mut self, heading: &str, start: usize, end: usize) {
        self.plugins.push(heading.to_string());
        self.declaration(DistDeclarationKind::Plugin, heading, None, start, end, false);
    }

    fn set_generated_build_tool(&mut self, tool: DistAuthoringBuildTool, start: usize, end: usize) {
        self.generated_build_tool = Some(tool);
        self.declaration(
            DistDeclarationKind::Plugin,
            "generated_build_tool",
            Some(tool.as_str().to_string()),
            start,
            end,
            false,
        );
    }

    fn nested_static_string(&mut self, field: &ScanPair, dynamic: &mut bool) -> Option<String> {
        if let ScanValue::String(raw) = &field.value {
            return Some(raw.clone());
        }
        *dynamic = true;
        self.limitation(
            "dynamic_value",
            format!("`{}` is not a static literal", field.key),
            Some(field.value_start),
            Some(field.value_end),
        );
        None
    }

    fn dynamic_pair(&mut self, kind: DistDeclarationKind, pair: &ScanPair) {
        let snippet = match &pair.value {
            ScanValue::Dynamic { snippet } => snippet.clone(),
            _ => scan::snippet(self.content, pair.value_start, pair.value_end),
        };
        self.declaration(kind, &pair.key, None, pair.key_start, pair.value_end, true);
        self.limitation(
            "dynamic_value",
            format!("`{}` is not a static literal (`{snippet}`)", pair.key),
            Some(pair.value_start),
            Some(pair.value_end),
        );
    }

    fn finish(mut self) -> DistAuthoringFacts {
        self.prereqs.sort_by(|a, b| {
            (&a.phase, &a.relation, &a.module, a.range.start_byte).cmp(&(
                &b.phase,
                &b.relation,
                &b.module,
                b.range.start_byte,
            ))
        });
        self.resources.sort_by(|a, b| a.kind.cmp(&b.kind));
        self.provides.sort_by(|a, b| a.module.cmp(&b.module));
        self.declarations.sort_by(|a, b| {
            (a.range.start_byte, a.key.as_str()).cmp(&(b.range.start_byte, b.key.as_str()))
        });
        self.limitations.sort_by(|a, b| a.id.cmp(&b.id));

        let name = pick_unique(&self.names);
        let version = pick_unique(&self.versions);
        let summary = pick_unique(&self.summaries);
        let licenses = unique_values(&self.licenses);
        let mut conflicts = Vec::new();
        push_conflict(&mut conflicts, "name", &self.names);
        push_conflict(&mut conflicts, "version", &self.versions);
        push_conflict(&mut conflicts, "abstract", &self.summaries);
        push_conflict(&mut conflicts, "license", &self.licenses);
        push_prereq_conflicts(&mut conflicts, &self.prereqs);

        let confidence = if name.is_none() && version.is_none() && self.prereqs.is_empty() {
            Confidence::Low
        } else if conflicts.is_empty()
            && self.limitations.iter().all(|item| {
                !matches!(
                    item.kind.as_str(),
                    "dynamic_value"
                        | "plugin_generated"
                        | "executable_construct"
                        | "conditional_declaration"
                        | "dynamic_constructor_arg"
                )
            })
        {
            Confidence::High
        } else {
            Confidence::Medium
        };

        let mut facts = DistAuthoringFacts {
            file_id: self.file_id,
            source: self.source,
            provenance: Provenance {
                producer: Producer::workspace_core(),
                source: EvidenceSource::Heuristic,
                confidence,
            },
            name,
            version,
            version_from: self.version_from,
            summary,
            abstract_from: self.abstract_from,
            authors: self.authors,
            licenses,
            build_tool: self.build_tool,
            generated_build_tool: self.generated_build_tool,
            plugins: self.plugins,
            resources: self.resources,
            provides: self.provides,
            prereqs: self.prereqs,
            declarations: self.declarations,
            conflicts,
            limitations: self.limitations,
            source_fingerprint: Digest::of(self.content),
            fact_fingerprint: Digest::of(""),
        };
        facts.fact_fingerprint = fingerprint_facts(&facts);
        facts
    }
}

fn pick_unique(values: &[(String, SourceRange)]) -> Option<String> {
    let mut uniq: Vec<&str> = values.iter().map(|(value, _)| value.as_str()).collect();
    uniq.sort_unstable();
    uniq.dedup();
    if uniq.len() == 1 { Some(uniq[0].to_string()) } else { None }
}

fn unique_values(values: &[(String, SourceRange)]) -> Vec<String> {
    let mut uniq: Vec<String> = values.iter().map(|(value, _)| value.clone()).collect();
    uniq.sort();
    uniq.dedup();
    uniq
}

fn push_conflict(
    out: &mut Vec<DistAuthoringConflict>,
    field: &str,
    values: &[(String, SourceRange)],
) {
    let mut uniq: Vec<String> = values.iter().map(|(value, _)| value.clone()).collect();
    uniq.sort();
    uniq.dedup();
    if uniq.len() > 1 {
        out.push(DistAuthoringConflict { field: field.to_string(), values: uniq });
    }
}

fn push_prereq_conflicts(out: &mut Vec<DistAuthoringConflict>, prereqs: &[AuthoringPrereq]) {
    let mut grouped: std::collections::BTreeMap<(String, String, String), Vec<String>> =
        std::collections::BTreeMap::new();
    for prereq in prereqs {
        grouped
            .entry((prereq.phase.clone(), prereq.relation.clone(), prereq.module.clone()))
            .or_default()
            .push(prereq.version.clone().unwrap_or_else(|| "0".to_string()));
    }
    for ((phase, relation, module), mut versions) in grouped {
        versions.sort();
        versions.dedup();
        if versions.len() > 1 {
            out.push(DistAuthoringConflict {
                field: format!("prereq:{phase}:{relation}:{module}"),
                values: versions,
            });
        }
    }
}

fn fingerprint_facts(facts: &DistAuthoringFacts) -> Digest {
    #[derive(Serialize)]
    struct Canonical<'a> {
        source: DistAuthoringSource,
        name: &'a Option<String>,
        version: &'a Option<String>,
        version_from: &'a Option<String>,
        summary: &'a Option<String>,
        abstract_from: &'a Option<String>,
        authors: &'a [String],
        licenses: &'a [String],
        build_tool: DistAuthoringBuildTool,
        generated_build_tool: Option<DistAuthoringBuildTool>,
        plugins: &'a [String],
        resources: Vec<(&'a str, Option<&'a str>, Option<&'a str>, Option<&'a str>)>,
        provides: Vec<(&'a str, Option<&'a str>, Option<&'a str>)>,
        prereqs: Vec<(&'a str, Option<&'a str>, &'a str, &'a str, bool)>,
        conflicts: Vec<(&'a str, &'a [String])>,
        limitation_kinds: Vec<&'a str>,
    }
    let canonical = Canonical {
        source: facts.source,
        name: &facts.name,
        version: &facts.version,
        version_from: &facts.version_from,
        summary: &facts.summary,
        abstract_from: &facts.abstract_from,
        authors: &facts.authors,
        licenses: &facts.licenses,
        build_tool: facts.build_tool,
        generated_build_tool: facts.generated_build_tool,
        plugins: &facts.plugins,
        resources: facts
            .resources
            .iter()
            .map(|item| {
                (
                    item.kind.as_str(),
                    item.url.as_deref(),
                    item.web.as_deref(),
                    item.type_name.as_deref(),
                )
            })
            .collect(),
        provides: facts
            .provides
            .iter()
            .map(|item| (item.module.as_str(), item.file.as_deref(), item.version.as_deref()))
            .collect(),
        prereqs: facts
            .prereqs
            .iter()
            .map(|item| {
                (
                    item.module.as_str(),
                    item.version.as_deref(),
                    item.phase.as_str(),
                    item.relation.as_str(),
                    item.dynamic,
                )
            })
            .collect(),
        conflicts: facts
            .conflicts
            .iter()
            .map(|item| (item.field.as_str(), item.values.as_slice()))
            .collect(),
        limitation_kinds: facts.limitations.iter().map(|item| item.kind.as_str()).collect(),
    };
    match serde_json::to_string(&canonical) {
        Ok(encoded) => Digest::of(&encoded),
        Err(_) => Digest::of("authoring-fingerprint-unavailable"),
    }
}

/// Parse one authoring file by its basename.
#[must_use]
pub fn parse_dist_authoring(
    file_id: FileId,
    relative_path: &str,
    content: &str,
) -> Option<DistAuthoringFacts> {
    let name = relative_path.rsplit('/').next().unwrap_or(relative_path);
    match name {
        "Makefile.PL" => Some(parse_makefile_pl(file_id, content)),
        "Build.PL" => Some(parse_build_pl(file_id, content)),
        "dist.ini" => Some(parse_dist_ini(file_id, content)),
        _ => None,
    }
}
