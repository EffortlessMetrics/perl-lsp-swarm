//! Evidence-backed compatibility between the Claude `perl-lsp-rs` plugin and `perllsp`.
//!
//! Compatibility is deliberately independent from release-number equality. The initial
//! contract recognizes only exact reviewed rows; an unlisted pair remains `not_proven`.

use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Machine-readable compatibility schema implemented by this module.
pub const SCHEMA_VERSION: &str = "claude_plugin_server_compat.v1";
/// Durable Claude plugin slug.
pub const PLUGIN_SLUG: &str = "perl-lsp-rs";
/// Durable language-server executable identity.
pub const SERVER_EXECUTABLE: &str = "perllsp";

/// Compatibility disposition for one exact plugin/server subject pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityResult {
    /// Direct evidence establishes the pair as compatible for the row's stated scope.
    Compatible,
    /// Direct evidence establishes the pair as incompatible for the row's stated scope.
    Incompatible,
    /// The row deliberately records that compatibility has not been established.
    NotProven,
}

impl CompatibilityResult {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::Incompatible => "incompatible",
            Self::NotProven => "not_proven",
        }
    }
}

/// Stable reason describing how a compatibility decision was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityReason {
    /// An exact reviewed compatible row matched the observed subjects.
    ExactEvidence,
    /// An exact reviewed incompatible row matched the observed subjects.
    ExactKnownBad,
    /// An exact row explicitly records an unresolved/not-proven relationship.
    ExplicitNotProven,
    /// No exact reviewed row covers the observed subjects.
    ExactPairNotEstablished,
    /// Runtime/setup observation lacks the exact identity needed to query the authority.
    SubjectIdentityIncomplete,
}

impl CompatibilityReason {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExactEvidence => "exact_evidence",
            Self::ExactKnownBad => "exact_known_bad",
            Self::ExplicitNotProven => "explicit_not_proven",
            Self::ExactPairNotEstablished => "exact_pair_not_established",
            Self::SubjectIdentityIncomplete => "subject_identity_incomplete",
        }
    }
}

/// Exact installable Claude plugin identity used as a compatibility subject.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PluginSubject {
    /// Claude plugin slug. Must be `perl-lsp-rs` for this contract.
    pub slug: String,
    /// Explicit plugin package version.
    pub version: String,
    /// Digest of the complete installable plugin tree.
    pub tree_digest: String,
    /// Digest of the installable package projection.
    pub package_digest: String,
    /// Digest covering launch, root, and activation semantics.
    pub contract_digest: String,
}

/// Exact `perllsp` binary identity used as a compatibility subject.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ServerSubject {
    /// Executable identity. Must be `perllsp` for this contract.
    pub executable: String,
    /// Reported product version.
    pub version: String,
    /// Exact source/build revision.
    pub build_revision: String,
    /// Digest of the executable artifact.
    pub artifact_sha256: String,
    /// Runtime platform or execution-environment family.
    pub platform: String,
    /// Runtime architecture.
    pub arch: String,
}

/// Optional host-coupled identity for compatibility rows that depend on Claude behavior.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HostSubject {
    /// Exact Claude Code version or reviewed host identity token.
    pub claude_code_version: String,
    /// Optional integration-control schema version when compatibility classification is coupled to it.
    pub control_plane_schema: Option<String>,
}

/// One exact evidence-backed compatibility row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityRow {
    /// Plugin subject.
    pub plugin: PluginSubject,
    /// Server subject.
    pub server: ServerSubject,
    /// Optional host contract. `None` means the row is not host-version-coupled.
    pub host: Option<HostSubject>,
    /// Compatibility disposition.
    pub result: CompatibilityResult,
    /// Durable evidence references establishing this row.
    pub evidence_refs: Vec<String>,
    /// Explicit limitations or unresolved boundaries.
    pub limitations: Vec<String>,
}

/// Versioned collection of exact compatibility rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityCatalog {
    /// Schema identity. Must equal [`SCHEMA_VERSION`].
    pub schema_version: String,
    /// Exact compatibility rows.
    pub rows: Vec<CompatibilityRow>,
}

/// Result returned to runtime/setup consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityDecision {
    /// Compatibility disposition.
    pub result: CompatibilityResult,
    /// Stable reason for the disposition.
    pub reason: CompatibilityReason,
    /// Evidence references from an exact row, if one matched.
    pub evidence_refs: Vec<String>,
    /// Limitations from an exact row, if one matched.
    pub limitations: Vec<String>,
}

impl CompatibilityCatalog {
    /// Validate catalog identity, exact subjects, evidence requirements, and duplicate rows.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "unexpected schema_version {:?}; expected {SCHEMA_VERSION}",
                self.schema_version
            ));
        }

        let mut seen = BTreeSet::new();
        for row in &self.rows {
            validate_plugin(&row.plugin)?;
            validate_server(&row.server)?;
            if let Some(host) = row.host.as_ref() {
                validate_nonempty(&host.claude_code_version, "host.claude_code_version")?;
                if let Some(schema) = host.control_plane_schema.as_deref() {
                    validate_nonempty(schema, "host.control_plane_schema")?;
                }
            }

            for evidence in &row.evidence_refs {
                validate_nonempty(evidence, "evidence_refs")?;
            }
            for limitation in &row.limitations {
                validate_nonempty(limitation, "limitations")?;
            }

            match row.result {
                CompatibilityResult::Compatible | CompatibilityResult::Incompatible => {
                    if row.evidence_refs.is_empty() {
                        return Err(
                            "compatible/incompatible rows require direct evidence_refs".to_string()
                        );
                    }
                }
                CompatibilityResult::NotProven => {
                    if row.limitations.is_empty() {
                        return Err("not_proven rows require an explicit limitation".to_string());
                    }
                }
            }

            let key = (&row.plugin, &row.server, &row.host);
            if !seen.insert(key) {
                return Err("duplicate exact plugin/server/host compatibility row".to_string());
            }
        }
        Ok(())
    }

    /// Resolve an exact observed subject pair without inferring a version range.
    pub fn decision_for(
        &self,
        plugin: &PluginSubject,
        server: &ServerSubject,
        host: Option<&HostSubject>,
    ) -> CompatibilityDecision {
        let matching = self.rows.iter().find(|row| {
            row.plugin == *plugin
                && row.server == *server
                && match row.host.as_ref() {
                    Some(expected) => host.is_some_and(|observed| observed == expected),
                    None => true,
                }
        });

        let Some(row) = matching else {
            return not_proven_decision(
                CompatibilityReason::ExactPairNotEstablished,
                "exact plugin/server pair is not established by current compatibility evidence",
            );
        };

        let reason = match row.result {
            CompatibilityResult::Compatible => CompatibilityReason::ExactEvidence,
            CompatibilityResult::Incompatible => CompatibilityReason::ExactKnownBad,
            CompatibilityResult::NotProven => CompatibilityReason::ExplicitNotProven,
        };
        CompatibilityDecision {
            result: row.result,
            reason,
            evidence_refs: row.evidence_refs.clone(),
            limitations: row.limitations.clone(),
        }
    }

    /// Resolve a runtime observation that may not yet contain exact plugin/server identity.
    ///
    /// Missing load-bearing identity is deliberately `not_proven`; consumers must never invent
    /// hashes, compare release numbers, or treat structural setup as compatibility evidence.
    pub fn decision_for_observation(
        &self,
        plugin: Option<&PluginSubject>,
        server: Option<&ServerSubject>,
        host: Option<&HostSubject>,
    ) -> CompatibilityDecision {
        match (plugin, server) {
            (Some(plugin), Some(server)) => self.decision_for(plugin, server, host),
            _ => not_proven_decision(
                CompatibilityReason::SubjectIdentityIncomplete,
                "exact plugin/server compatibility subject is incomplete in this observation",
            ),
        }
    }

    /// Deterministic machine-readable projection for receipts, support joins, and diagnostics.
    pub fn to_json(&self) -> Value {
        let rows = self.rows.iter().map(row_to_json).collect::<Vec<_>>();
        json!({
            "schema_version": self.schema_version,
            "rows": rows,
        })
    }
}

impl CompatibilityDecision {
    /// Deterministic machine-readable decision consumed by setup/status surfaces.
    pub fn to_json(&self) -> Value {
        json!({
            "result": self.result.as_str(),
            "reason": self.reason.as_str(),
            "evidence_refs": self.evidence_refs,
            "limitations": self.limitations,
        })
    }
}

/// Load the conservative compatibility catalog compiled into the `perllsp` package.
///
/// The catalog intentionally starts empty. Actual compatible/incompatible rows are added only by
/// reviewed evidence-producing work; unknown pairs therefore remain `not_proven` by construction.
pub fn embedded_catalog() -> CompatibilityCatalog {
    CompatibilityCatalog { schema_version: SCHEMA_VERSION.to_string(), rows: Vec::new() }
}

fn not_proven_decision(reason: CompatibilityReason, limitation: &str) -> CompatibilityDecision {
    CompatibilityDecision {
        result: CompatibilityResult::NotProven,
        reason,
        evidence_refs: Vec::new(),
        limitations: vec![limitation.to_string()],
    }
}

fn row_to_json(row: &CompatibilityRow) -> Value {
    json!({
        "plugin": plugin_to_json(&row.plugin),
        "server": server_to_json(&row.server),
        "host": row.host.as_ref().map(host_to_json),
        "result": row.result.as_str(),
        "evidence_refs": row.evidence_refs,
        "limitations": row.limitations,
    })
}

fn plugin_to_json(plugin: &PluginSubject) -> Value {
    json!({
        "slug": plugin.slug,
        "version": plugin.version,
        "tree_digest": plugin.tree_digest,
        "package_digest": plugin.package_digest,
        "contract_digest": plugin.contract_digest,
    })
}

fn server_to_json(server: &ServerSubject) -> Value {
    json!({
        "executable": server.executable,
        "version": server.version,
        "build_revision": server.build_revision,
        "artifact_sha256": server.artifact_sha256,
        "platform": server.platform,
        "arch": server.arch,
    })
}

fn host_to_json(host: &HostSubject) -> Value {
    json!({
        "claude_code_version": host.claude_code_version,
        "control_plane_schema": host.control_plane_schema,
    })
}

fn validate_plugin(plugin: &PluginSubject) -> Result<(), String> {
    if plugin.slug != PLUGIN_SLUG {
        return Err(format!("plugin.slug must be {PLUGIN_SLUG}"));
    }
    validate_nonempty(&plugin.version, "plugin.version")?;
    validate_sha256(&plugin.tree_digest, "plugin.tree_digest")?;
    validate_sha256(&plugin.package_digest, "plugin.package_digest")?;
    validate_sha256(&plugin.contract_digest, "plugin.contract_digest")
}

fn validate_server(server: &ServerSubject) -> Result<(), String> {
    if server.executable != SERVER_EXECUTABLE {
        return Err(format!("server.executable must be {SERVER_EXECUTABLE}"));
    }
    validate_nonempty(&server.version, "server.version")?;
    if !is_lower_hex(&server.build_revision, 40) {
        return Err("server.build_revision must be 40 lowercase hex characters".to_string());
    }
    validate_sha256(&server.artifact_sha256, "server.artifact_sha256")?;
    validate_nonempty(&server.platform, "server.platform")?;
    validate_nonempty(&server.arch, "server.arch")
}

fn validate_nonempty(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} cannot be empty"));
    }
    Ok(())
}

fn validate_sha256(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field} must use sha256:<64 lowercase hex>"));
    };
    if !is_lower_hex(hex, 64) {
        return Err(format!("{field} must use sha256:<64 lowercase hex>"));
    }
    Ok(())
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
