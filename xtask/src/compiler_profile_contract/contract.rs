//! Constructors, in-memory validation, the import closure law, and
//! deterministic semantic fingerprints for maintained compiler profiles
//! (#12186).
//!
//! Validation is fail-closed and typed: incomplete or unresolvable identities,
//! modified imports, dropped rows or limitations, cross-axis evidence, empty
//! obligations, and authorization ceilings are refused with
//! [`CompilerProfileContractError`] variants that name the broken law.
//!
//! This layer owns no file syntax, no repository loading, no receipt
//! adaptation, and no evaluation: the successor initial-row inventory and
//! #12187's manifest serialize this model; they never reinterpret it.

use sha2::{Digest as ShaDigest, Sha256};

use super::CompilerProfileContractError;
use super::model::{
    AllowedLimitation, ClaimCeiling, ClaimFamily, CompilerProfileDefinition, CompilerProfileId,
    CompilerProfileImport, CompilerProfileRow, CompilerProfileVersion, CompletenessRule,
    LegacyExitRequirement, LimitationPolicy, ProfileDigest, RowDisposition,
};
use super::{canonical_json, hex_digest, is_stable_token};

impl CompilerProfileDefinition {
    /// Constructor-validated definition. Structural laws are enforced here and
    /// again by [`Self::validate`] after deserialization.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        profile_id: CompilerProfileId,
        version: CompilerProfileVersion,
        purpose: String,
        imports: std::collections::BTreeSet<CompilerProfileImport>,
        rows: std::collections::BTreeMap<super::model::CompilerProfileRowId, CompilerProfileRow>,
        limitations: std::collections::BTreeMap<String, AllowedLimitation>,
    ) -> Result<Self, CompilerProfileContractError> {
        let definition = Self { profile_id, version, purpose, imports, rows, limitations };
        definition.validate()?;
        Ok(definition)
    }

    /// Checked deserialization: parse, then validate every closure law.
    pub fn from_json_str(json: &str) -> Result<Self, CompilerProfileContractError> {
        let definition: Self =
            serde_json::from_str(json).map_err(|error| CompilerProfileContractError::Schema {
                field: "compiler_profile_definition".to_string(),
                message: error.to_string(),
            })?;
        definition.validate()?;
        Ok(definition)
    }

    /// Deterministic serialization. Row and limitation maps are `BTreeMap`s,
    /// so insertion order never leaks into the bytes.
    pub fn to_canonical_json(&self) -> Result<String, CompilerProfileContractError> {
        canonical_json(self)
    }

    /// Structural validation of the definition in isolation: identity tokens,
    /// row self-laws, referential integrity of limitation policies, and the
    /// authorization ceiling law. Import resolution is
    /// [`Self::validate_closure`].
    pub fn validate(&self) -> Result<(), CompilerProfileContractError> {
        if self.purpose.trim().is_empty() {
            return Err(CompilerProfileContractError::Schema {
                field: "purpose".to_string(),
                message: "must not be empty".to_string(),
            });
        }
        if self.rows.is_empty() {
            return Err(CompilerProfileContractError::Schema {
                field: "rows".to_string(),
                message: "a profile must declare at least one row".to_string(),
            });
        }
        self.validate_imports()?;
        for (key, row) in &self.rows {
            if key.as_str() != row.row_id.as_str() {
                return Err(CompilerProfileContractError::Schema {
                    field: "rows".to_string(),
                    message: format!(
                        "map key `{}` does not match row id `{}`",
                        key.as_str(),
                        row.row_id.as_str()
                    ),
                });
            }
            row.validate()?;
        }
        self.validate_limitations()?;
        Ok(())
    }

    /// The import closure law. Each import must resolve to a provided lower
    /// definition whose current fingerprint equals the recorded digest, and
    /// every imported row and limitation must be preserved verbatim. Import
    /// cycles and self-imports fail closed. Preservation is checked before
    /// this profile's own structural laws so that dropped or modified
    /// imported rows and limitations surface as closure failures, not as
    /// incidental schema noise.
    pub fn validate_closure(
        &self,
        resolved: &[&CompilerProfileDefinition],
    ) -> Result<(), CompilerProfileContractError> {
        for import in &self.imports {
            let lower = resolved
                .iter()
                .find(|definition| {
                    definition.profile_id == import.profile_id
                        && definition.version == import.version
                })
                .ok_or(CompilerProfileContractError::Identity {
                    message: format!(
                        "import of `{}` `{}` has no resolved lower definition",
                        import.profile_id.as_str(),
                        import.version.as_str()
                    ),
                })?;
            lower.validate()?;
            let fingerprint = lower.semantic_fingerprint()?;
            if fingerprint != import.digest {
                return Err(CompilerProfileContractError::Identity {
                    message: format!(
                        "import of `{}` `{}` binds digest `{}`, but the resolved definition \
                         fingerprints as `{}`",
                        import.profile_id.as_str(),
                        import.version.as_str(),
                        import.digest.as_str(),
                        fingerprint.as_str()
                    ),
                });
            }
            if lower.profile_id == self.profile_id {
                return Err(CompilerProfileContractError::Identity {
                    message: format!("profile `{}` imports itself", self.profile_id.as_str()),
                });
            }
            self.check_no_import_cycle(import, resolved)?;
            self.check_preserved(import, lower)?;
        }
        self.validate()
    }

    /// Deterministic semantic fingerprint over the canonical serialization.
    /// Identity changes when any semantic row, limitation, purpose, version,
    /// or import field changes, and never on insertion order.
    pub fn semantic_fingerprint(&self) -> Result<ProfileDigest, CompilerProfileContractError> {
        self.validate()?;
        let canonical = self.to_canonical_json()?;
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        ProfileDigest::from_hex(&hex_digest(&hasher.finalize()))
    }

    fn validate_imports(&self) -> Result<(), CompilerProfileContractError> {
        for import in &self.imports {
            if import.profile_id == self.profile_id {
                return Err(CompilerProfileContractError::Identity {
                    message: format!("profile `{}` imports itself", self.profile_id.as_str()),
                });
            }
        }
        Ok(())
    }

    fn validate_limitations(&self) -> Result<(), CompilerProfileContractError> {
        for (id, limitation) in &self.limitations {
            if !is_stable_token(id) {
                return Err(CompilerProfileContractError::Schema {
                    field: "limitations".to_string(),
                    message: format!("`{id}` is not a stable limitation token"),
                });
            }
            if limitation.boundary.trim().is_empty() {
                return Err(CompilerProfileContractError::Schema {
                    field: "limitations.boundary".to_string(),
                    message: format!("limitation `{id}` has an empty boundary"),
                });
            }
            limitation.owner.owner.as_str();
        }
        // Referential completeness: every limitation must bound at least one
        // row, so limitations cannot accumulate as dead weight.
        let mut referenced = std::collections::BTreeSet::new();
        for row in self.rows.values() {
            if let LimitationPolicy::BoundedBy { limitation_ids } = &row.limitation_policy {
                if limitation_ids.is_empty() {
                    return Err(CompilerProfileContractError::Schema {
                        field: "limitation_policy.limitation_ids".to_string(),
                        message: format!(
                            "row `{}` is bounded by an empty limitation set",
                            row.row_id.as_str()
                        ),
                    });
                }
                for id in limitation_ids {
                    if !self.limitations.contains_key(id) {
                        return Err(CompilerProfileContractError::Schema {
                            field: "limitation_policy.limitation_ids".to_string(),
                            message: format!(
                                "row `{}` references unknown limitation `{id}`",
                                row.row_id.as_str()
                            ),
                        });
                    }
                }
                referenced.extend(limitation_ids.iter().cloned());
            }
        }
        for id in self.limitations.keys() {
            if !referenced.contains(id) {
                return Err(CompilerProfileContractError::Schema {
                    field: "limitations".to_string(),
                    message: format!("limitation `{id}` bounds no row"),
                });
            }
        }
        Ok(())
    }

    fn check_no_import_cycle(
        &self,
        import: &CompilerProfileImport,
        resolved: &[&CompilerProfileDefinition],
    ) -> Result<(), CompilerProfileContractError> {
        let mut visited = std::collections::BTreeSet::new();
        let mut frontier = vec![import.clone()];
        while let Some(current) = frontier.pop() {
            let key = (current.profile_id.clone(), current.version.clone());
            if !visited.insert(key) {
                continue;
            }
            let lower = resolved.iter().find(|definition| {
                definition.profile_id == current.profile_id && definition.version == current.version
            });
            let Some(lower) = lower else { continue };
            if lower.profile_id == self.profile_id {
                return Err(CompilerProfileContractError::Identity {
                    message: format!(
                        "import cycle reaches `{}` through `{}`",
                        self.profile_id.as_str(),
                        current.profile_id.as_str()
                    ),
                });
            }
            frontier.extend(lower.imports.iter().cloned());
        }
        Ok(())
    }

    fn check_preserved(
        &self,
        import: &CompilerProfileImport,
        lower: &CompilerProfileDefinition,
    ) -> Result<(), CompilerProfileContractError> {
        for (row_id, lower_row) in &lower.rows {
            match self.rows.get(row_id) {
                None => {
                    return Err(CompilerProfileContractError::Closure {
                        message: format!(
                            "imported row `{}` of `{}` is missing: omission is not a disposition",
                            row_id.as_str(),
                            import.profile_id.as_str()
                        ),
                    });
                }
                Some(own_row) => {
                    if own_row != lower_row {
                        return Err(CompilerProfileContractError::Closure {
                            message: format!(
                                "imported row `{}` of `{}` was modified: imports preserve rows \
                                 verbatim",
                                row_id.as_str(),
                                import.profile_id.as_str()
                            ),
                        });
                    }
                }
            }
        }
        for (id, lower_limitation) in &lower.limitations {
            match self.limitations.get(id) {
                None => {
                    return Err(CompilerProfileContractError::Closure {
                        message: format!(
                            "imported limitation `{id}` of `{}` is missing",
                            import.profile_id.as_str()
                        ),
                    });
                }
                Some(own_limitation) => {
                    if own_limitation != lower_limitation {
                        return Err(CompilerProfileContractError::Closure {
                            message: format!(
                                "imported limitation `{id}` of `{}` was modified",
                                import.profile_id.as_str()
                            ),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

impl CompilerProfileRow {
    /// Row self-laws: exhaustive typed disposition, non-empty obligations,
    /// legacy exit proof restrictions, and the profile-evidence ceiling.
    pub fn validate(&self) -> Result<(), CompilerProfileContractError> {
        if self.statement.trim().is_empty() {
            return Err(CompilerProfileContractError::Schema {
                field: "row.statement".to_string(),
                message: format!("row `{}` has an empty statement", self.row_id.as_str()),
            });
        }
        match &self.disposition {
            RowDisposition::Conditional { condition } => {
                if condition.trim().is_empty() {
                    return Err(CompilerProfileContractError::Schema {
                        field: "row.disposition.condition".to_string(),
                        message: format!(
                            "conditional row `{}` must name its condition",
                            self.row_id.as_str()
                        ),
                    });
                }
            }
            RowDisposition::Unsupported { reason } => {
                if reason.trim().is_empty() {
                    return Err(CompilerProfileContractError::Schema {
                        field: "row.disposition.reason".to_string(),
                        message: format!(
                            "unsupported row `{}` must name its reason",
                            self.row_id.as_str()
                        ),
                    });
                }
            }
            RowDisposition::NotApplicable { ruling } => {
                if ruling.trim().is_empty() {
                    return Err(CompilerProfileContractError::Schema {
                        field: "row.disposition.ruling".to_string(),
                        message: format!(
                            "not-applicable row `{}` must name its ruling",
                            self.row_id.as_str()
                        ),
                    });
                }
            }
            RowDisposition::Required | RowDisposition::Optional => {}
        }
        if self.evidence.required_classes.is_empty() {
            return Err(CompilerProfileContractError::Schema {
                field: "row.evidence.required_classes".to_string(),
                message: format!("row `{}` names no proof class", self.row_id.as_str()),
            });
        }
        if self.evidence.required_tiers.is_empty() {
            return Err(CompilerProfileContractError::Schema {
                field: "row.evidence.required_tiers".to_string(),
                message: format!("row `{}` names no source tier", self.row_id.as_str()),
            });
        }
        match self.completeness.rule {
            CompletenessRule::RepresentativeSample { ref sample_id } => {
                if !is_stable_token(sample_id) {
                    return Err(CompilerProfileContractError::Schema {
                        field: "row.completeness.sample_id".to_string(),
                        message: format!(
                            "row `{}` has a malformed sample id",
                            self.row_id.as_str()
                        ),
                    });
                }
            }
            CompletenessRule::ExactDenominator { ref denominator_id } => {
                if !is_stable_token(denominator_id) {
                    return Err(CompilerProfileContractError::Schema {
                        field: "row.completeness.denominator_id".to_string(),
                        message: format!(
                            "row `{}` has a malformed denominator id",
                            self.row_id.as_str()
                        ),
                    });
                }
            }
            CompletenessRule::CurrentSubjectState | CompletenessRule::ExhaustiveCoverage => {}
        }
        if let Some(exit) = &self.legacy_exit {
            exit.validate(self.row_id.as_str())?;
        }
        if self.invalidation.is_empty() {
            return Err(CompilerProfileContractError::Schema {
                field: "row.invalidation".to_string(),
                message: format!("row `{}` names no invalidation input", self.row_id.as_str()),
            });
        }
        // Ceiling law: a profile result is profile evidence only. Support,
        // release, and publication authorization live on their own surfaces
        // and cannot be inferred from a profile result.
        if self.claim_ceiling != ClaimCeiling::profile_evidence() {
            return Err(CompilerProfileContractError::Authorization {
                message: format!(
                    "row `{}` carries a `{}` ceiling; profile rows are profile evidence only",
                    self.row_id.as_str(),
                    self.claim_ceiling.family().as_str()
                ),
            });
        }
        if !self.claim_ceiling.permits(ClaimFamily::ProfileEvidence) {
            return Err(CompilerProfileContractError::Authorization {
                message: format!(
                    "row `{}` cannot support its own profile evidence",
                    self.row_id.as_str()
                ),
            });
        }
        Ok(())
    }
}

impl LegacyExitRequirement {
    /// Legacy exits prove old-path absence and recurrence; no other proof
    /// class retires a legacy path.
    fn validate(&self, row_id: &str) -> Result<(), CompilerProfileContractError> {
        if self.legacy_path.trim().is_empty() {
            return Err(CompilerProfileContractError::Schema {
                field: "row.legacy_exit.legacy_path".to_string(),
                message: format!("row `{row_id}` has an empty legacy path"),
            });
        }
        if self.required_proof.is_empty() {
            return Err(CompilerProfileContractError::Schema {
                field: "row.legacy_exit.required_proof".to_string(),
                message: format!("row `{row_id}` must demand old-path absence or recurrence proof"),
            });
        }
        let allowed =
            [super::model::ProofClass::OldPathAbsence, super::model::ProofClass::RecurrenceProof];
        for class in &self.required_proof {
            if !allowed.contains(class) {
                return Err(CompilerProfileContractError::Schema {
                    field: "row.legacy_exit.required_proof".to_string(),
                    message: format!(
                        "row `{row_id}` demands `{}` for a legacy exit; only old-path absence \
                         and recurrence proof retire a legacy path",
                        class.as_str()
                    ),
                });
            }
        }
        Ok(())
    }
}
