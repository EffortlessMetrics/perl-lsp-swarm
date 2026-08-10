//! Versioned per-file fact shards and deterministic model deltas.

use serde::{Deserialize, Serialize};

use crate::SCHEMA_VERSION;
use crate::boundary::DynamicBoundary;
use crate::dist::DistMetadataFacts;
use crate::effects::CompileEffectFacts;
use crate::error::ModelLimitation;
use crate::export::ExportFact;
use crate::fact_classes::FactClasses;
use crate::file::FileRecord;
use crate::id::{FileId, fnv1a};
use crate::import::ImportFact;
use crate::package::PackageRecord;
use crate::pod::PodFact;
use crate::relation::RelationFact;
use crate::symbol::SymbolRecord;
use crate::test::TestFact;

/// Metadata retained for each shard adopted by a [`crate::ProjectModel`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectShardState {
    /// Monotonic source/document generation supplied by the producer.
    pub generation: u64,
    /// Identity of the producer that created the shard.
    pub producer: String,
    /// Schema version used to encode the shard.
    pub schema_version: u32,
    /// Deterministic fingerprint of the complete normalized shard.
    pub fingerprint: String,
    /// Limitation ids owned by the shard and removed with it.
    #[serde(default)]
    pub limitation_ids: Vec<String>,
}

/// All facts owned by one file at one producer generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFactShard {
    /// Fact schema version. Must match [`SCHEMA_VERSION`] for ingestion.
    pub schema_version: u32,
    /// Monotonic source/document generation.
    pub generation: u64,
    /// Stable producer identity, separate from confidence or proof class.
    pub producer: String,
    /// Fact classes requested from the producer.
    pub requested: FactClasses,
    /// Fact classes the shard actually populated.
    pub populated: FactClasses,
    /// The file identity and parse state owning every fact in this shard.
    pub file: FileRecord,
    /// Source length used to validate every byte range in the shard.
    pub source_len_bytes: u32,
    /// Package declarations owned by the file.
    pub packages: Vec<PackageRecord>,
    /// Symbol declarations owned by the file.
    pub symbols: Vec<SymbolRecord>,
    /// Import facts owned by the file.
    pub imports: Vec<ImportFact>,
    /// Export facts owned by the file.
    pub exports: Vec<ExportFact>,
    /// Compile effects owned by the file.
    pub compile_effects: Vec<CompileEffectFacts>,
    /// Distribution metadata owned by the file.
    pub dist_metadata: Vec<DistMetadataFacts>,
    /// Test facts owned by the file.
    pub tests: Vec<TestFact>,
    /// POD facts owned by the file.
    pub pod: Vec<PodFact>,
    /// Relationships declared by the file.
    pub relations: Vec<RelationFact>,
    /// Dynamic boundaries owned by the file.
    pub dynamic_boundaries: Vec<DynamicBoundary>,
    /// Typed limitations attributable to this shard.
    pub limitations: Vec<ModelLimitation>,
}

impl ProjectFactShard {
    /// Construct an empty shard envelope for one file.
    #[must_use]
    pub fn empty(
        file: FileRecord,
        generation: u64,
        producer: impl Into<String>,
        requested: FactClasses,
    ) -> Self {
        let mut populated = FactClasses::NONE;
        if requested.contains(FactClasses::FILES) {
            populated |= FactClasses::FILES;
        }
        if requested.contains(FactClasses::SYNTAX) {
            populated |= FactClasses::SYNTAX;
        }
        Self {
            schema_version: SCHEMA_VERSION,
            generation,
            producer: producer.into(),
            requested,
            populated,
            file,
            source_len_bytes: 0,
            packages: Vec::new(),
            symbols: Vec::new(),
            imports: Vec::new(),
            exports: Vec::new(),
            compile_effects: Vec::new(),
            dist_metadata: Vec::new(),
            tests: Vec::new(),
            pod: Vec::new(),
            relations: Vec::new(),
            dynamic_boundaries: Vec::new(),
            limitations: Vec::new(),
        }
    }

    /// Validate schema, identity, fact-class, and per-file ownership invariants.
    pub fn validate(&self) -> Result<(), ShardError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ShardError::SchemaVersion {
                expected: SCHEMA_VERSION,
                actual: self.schema_version,
            });
        }
        if self.producer.trim().is_empty() {
            return Err(ShardError::EmptyProducer);
        }
        if !self.requested.contains(self.populated) {
            return Err(ShardError::PopulatedClassesNotRequested);
        }
        let expected_file_id = FileId::new(&self.file.relative_path, &self.file.digest);
        if expected_file_id != self.file.file_id {
            return Err(ShardError::FileIdentityMismatch);
        }

        let owner = &self.file.file_id;
        macro_rules! require_owner {
            ($facts:expr, $kind:literal) => {
                if $facts.iter().any(|fact| &fact.file_id != owner) {
                    return Err(ShardError::WrongFileOwner { fact_kind: $kind });
                }
            };
        }
        require_owner!(self.packages, "package");
        require_owner!(self.symbols, "symbol");
        require_owner!(self.imports, "import");
        require_owner!(self.exports, "export");
        require_owner!(self.compile_effects, "compile_effect");
        require_owner!(self.dist_metadata, "dist_metadata");
        require_owner!(self.tests, "test");
        require_owner!(self.pod, "pod");
        require_owner!(self.relations, "relation");
        require_owner!(self.dynamic_boundaries, "dynamic_boundary");
        self.validate_fact_classes()?;
        self.validate_unique_ids()?;
        self.validate_ranges()?;
        Ok(())
    }

    fn validate_fact_classes(&self) -> Result<(), ShardError> {
        let checks = [
            (
                !self.packages.is_empty() || !self.symbols.is_empty(),
                FactClasses::SYMBOLS,
                "symbols",
            ),
            (!self.imports.is_empty(), FactClasses::IMPORTS, "imports"),
            (!self.exports.is_empty(), FactClasses::EXPORTS, "exports"),
            (!self.compile_effects.is_empty(), FactClasses::COMPILE_EFFECTS, "compile_effects"),
            (!self.dist_metadata.is_empty(), FactClasses::DIST, "dist"),
            (!self.tests.is_empty(), FactClasses::TESTS, "tests"),
            (!self.pod.is_empty(), FactClasses::POD, "pod"),
            (!self.relations.is_empty(), FactClasses::RELATIONS, "relations"),
            (
                !self.dynamic_boundaries.is_empty(),
                FactClasses::DYNAMIC_BOUNDARIES,
                "dynamic_boundaries",
            ),
        ];
        for (has_facts, class, fact_kind) in checks {
            if has_facts && !self.populated.contains(class) {
                return Err(ShardError::UndeclaredFactClass { fact_kind });
            }
        }
        Ok(())
    }

    fn validate_unique_ids(&self) -> Result<(), ShardError> {
        let mut package_ids = std::collections::BTreeSet::new();
        for package in &self.packages {
            if !package_ids.insert(package.package_id.as_str()) {
                return Err(ShardError::DuplicateFactId {
                    fact_kind: "package",
                    fact_id: package.package_id.as_str().to_string(),
                });
            }
        }
        let mut symbol_ids = std::collections::BTreeSet::new();
        for symbol in &self.symbols {
            if !symbol_ids.insert(symbol.symbol_id.as_str()) {
                return Err(ShardError::DuplicateFactId {
                    fact_kind: "symbol",
                    fact_id: symbol.symbol_id.as_str().to_string(),
                });
            }
        }
        let mut limitation_ids = std::collections::BTreeSet::new();
        for limitation in &self.limitations {
            if !limitation_ids.insert(&limitation.id) {
                return Err(ShardError::DuplicateFactId {
                    fact_kind: "limitation",
                    fact_id: limitation.id.clone(),
                });
            }
        }
        Ok(())
    }

    fn validate_ranges(&self) -> Result<(), ShardError> {
        macro_rules! require_range {
            ($facts:expr, $field:ident, $kind:literal) => {
                if $facts.iter().any(|fact| {
                    fact.$field.start_byte > fact.$field.end_byte
                        || fact.$field.end_byte > self.source_len_bytes
                }) {
                    return Err(ShardError::RangeOutsideSource { fact_kind: $kind });
                }
            };
        }
        require_range!(self.packages, declaration_range, "package");
        require_range!(self.symbols, declaration_range, "symbol");
        require_range!(self.imports, range, "import");
        require_range!(self.exports, range, "export");
        require_range!(self.tests, range, "test");
        require_range!(self.dynamic_boundaries, range, "dynamic_boundary");
        if self.pod.iter().flat_map(|fact| &fact.sections).any(|section| {
            section.range.start_byte > section.range.end_byte
                || section.range.end_byte > self.source_len_bytes
        }) {
            return Err(ShardError::RangeOutsideSource { fact_kind: "pod" });
        }
        Ok(())
    }

    /// Return a normalized copy whose vector order is deterministic.
    #[must_use]
    pub fn normalized(&self) -> Self {
        let mut shard = self.clone();
        shard.packages.sort_by(|a, b| {
            (a.declaration_range.start_byte, a.package_id.as_str())
                .cmp(&(b.declaration_range.start_byte, b.package_id.as_str()))
        });
        shard.symbols.sort_by(|a, b| {
            (a.declaration_range.start_byte, a.symbol_id.as_str())
                .cmp(&(b.declaration_range.start_byte, b.symbol_id.as_str()))
        });
        shard.imports.sort_by_key(|fact| fact.range.start_byte);
        shard.exports.sort_by_key(|fact| fact.range.start_byte);
        shard.compile_effects.sort_by(|a, b| a.file_id.cmp(&b.file_id));
        shard.dist_metadata.sort_by(|a, b| a.file_id.cmp(&b.file_id));
        shard.tests.sort_by(|a, b| a.file_id.cmp(&b.file_id));
        shard.pod.sort_by(|a, b| a.file_id.cmp(&b.file_id));
        shard.relations.sort_by(|a, b| {
            (a.kind.tag(), &a.source, &a.target).cmp(&(b.kind.tag(), &b.source, &b.target))
        });
        shard.dynamic_boundaries.sort_by_key(|fact| fact.range.start_byte);
        shard.limitations.sort_by(|a, b| a.id.cmp(&b.id));
        shard
    }

    /// Compute the deterministic fingerprint of the normalized shard.
    pub fn fingerprint(&self) -> Result<String, ShardError> {
        self.validate()?;
        let encoded = serde_json::to_vec(&self.normalized())
            .map_err(|error| ShardError::Serialization { message: error.to_string() })?;
        Ok(format!("fnv64:{:016x}", fnv1a(&encoded)))
    }
}

/// Deterministic summary of one model mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDelta {
    /// Files newly added to the model.
    pub added_files: Vec<FileId>,
    /// Files replaced by a newer generation.
    pub changed_files: Vec<FileId>,
    /// Files removed from the model.
    pub removed_files: Vec<FileId>,
    /// Files whose declared relations target a package removed or replaced in this delta.
    pub invalidated_files: Vec<FileId>,
}

impl ProjectDelta {
    pub(crate) fn empty() -> Self {
        Self {
            added_files: Vec::new(),
            changed_files: Vec::new(),
            removed_files: Vec::new(),
            invalidated_files: Vec::new(),
        }
    }
}

/// Typed rejection from shard validation or model ingestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShardError {
    /// The shard schema does not match the model schema.
    SchemaVersion {
        /// Schema version accepted by the current crate.
        expected: u32,
        /// Schema version declared by the incoming shard.
        actual: u32,
    },
    /// Producer identity must be explicit.
    EmptyProducer,
    /// A populated fact class was not requested.
    PopulatedClassesNotRequested,
    /// A fact vector was populated without declaring its fact class.
    UndeclaredFactClass {
        /// Fact vector that was populated.
        fact_kind: &'static str,
    },
    /// The path/digest pair does not derive the declared file identity.
    FileIdentityMismatch,
    /// A fact in the shard belongs to another file.
    WrongFileOwner {
        /// Class of fact whose `file_id` did not match the shard owner.
        fact_kind: &'static str,
    },
    /// A stable fact id appeared more than once in a shard.
    DuplicateFactId {
        /// Class of duplicated fact.
        fact_kind: &'static str,
        /// Stable id that appeared more than once.
        fact_id: String,
    },
    /// A source range was reversed or exceeded the declared source length.
    RangeOutsideSource {
        /// Class of fact containing the invalid range.
        fact_kind: &'static str,
    },
    /// An older generation attempted to replace a newer generation.
    StaleGeneration {
        /// Generation already adopted by the model.
        current: u64,
        /// Older generation supplied by the producer.
        incoming: u64,
    },
    /// The same generation was supplied with a different fingerprint.
    ConflictingGeneration {
        /// Generation already adopted by the model.
        generation: u64,
    },
    /// A removal attempted to target an older generation.
    StaleRemoval {
        /// Generation already adopted by the model.
        current: u64,
        /// Older removal generation supplied by the producer.
        incoming: u64,
    },
    /// Deterministic serialization failed.
    Serialization {
        /// Underlying deterministic-serialization failure.
        message: String,
    },
}

impl std::fmt::Display for ShardError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SchemaVersion { expected, actual } => {
                write!(formatter, "shard schema {actual} does not match model schema {expected}")
            }
            Self::EmptyProducer => formatter.write_str("shard producer identity is empty"),
            Self::PopulatedClassesNotRequested => {
                formatter.write_str("shard populated fact classes that were not requested")
            }
            Self::UndeclaredFactClass { fact_kind } => {
                write!(formatter, "{fact_kind} facts were populated without declaring their class")
            }
            Self::FileIdentityMismatch => {
                formatter.write_str("shard file identity does not match its path and digest")
            }
            Self::WrongFileOwner { fact_kind } => {
                write!(formatter, "{fact_kind} fact belongs to another file")
            }
            Self::DuplicateFactId { fact_kind, fact_id } => {
                write!(formatter, "duplicate {fact_kind} fact id `{fact_id}`")
            }
            Self::RangeOutsideSource { fact_kind } => {
                write!(formatter, "{fact_kind} fact range is outside the source")
            }
            Self::StaleGeneration { current, incoming } => {
                write!(formatter, "incoming generation {incoming} is older than current {current}")
            }
            Self::ConflictingGeneration { generation } => {
                write!(formatter, "generation {generation} has a conflicting shard fingerprint")
            }
            Self::StaleRemoval { current, incoming } => {
                write!(formatter, "removal generation {incoming} is older than current {current}")
            }
            Self::Serialization { message } => {
                write!(formatter, "could not serialize project fact shard: {message}")
            }
        }
    }
}

impl std::error::Error for ShardError {}
