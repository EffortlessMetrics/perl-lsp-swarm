//! The project model: the assembled set of facts for a workspace.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::boundary::DynamicBoundary;
use crate::dist::DistMetadataFacts;
use crate::effects::CompileEffectFacts;
pub use crate::error::ModelLimitation;
use crate::export::ExportFact;
use crate::fact_classes::FactClasses;
use crate::file::FileRecord;
use crate::id::FileId;
use crate::import::ImportFact;
use crate::package::PackageRecord;
use crate::pod::PodFact;
use crate::relation::RelationFact;
use crate::symbol::SymbolRecord;
use crate::test::TestFact;
use crate::{ProjectDelta, ProjectFactShard, ProjectShardState, ShardError};

/// The deterministic set of facts derived from a workspace.
///
/// Facts are stored as flat, deterministically-ordered vectors. Consumers query
/// by [`FileId`] via the helper methods; the ordering is stable across builds
/// of identical input, so serialized models diff cleanly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectModel {
    /// The repo-relative root that was scanned (forward-slash).
    pub root: String,
    /// Which fact classes were requested.
    pub requested: FactClasses,
    /// File facts, ordered by relative path.
    pub files: Vec<FileRecord>,
    /// Package facts, ordered by (file, declaration byte).
    pub packages: Vec<PackageRecord>,
    /// Symbol facts, ordered by (file, declaration byte).
    pub symbols: Vec<SymbolRecord>,
    /// Import facts (`use`/`no`/`require`), ordered by (file, byte).
    pub imports: Vec<ImportFact>,
    /// Export facts (`@EXPORT`/`@EXPORT_OK`), ordered by (file, byte).
    pub exports: Vec<ExportFact>,
    /// Compile-time effect facts, one per parsed file, ordered by file.
    pub compile_effects: Vec<CompileEffectFacts>,
    /// Distribution-metadata facts (from `META.json` / `cpanfile`), ordered by file.
    pub dist_metadata: Vec<DistMetadataFacts>,
    /// Test-file facts (framework + assertion counts), ordered by file.
    pub tests: Vec<TestFact>,
    /// POD documentation facts, ordered by file.
    pub pod: Vec<PodFact>,
    /// Relation edges (inherits/uses/tests), ordered by (kind, source, target).
    pub relations: Vec<RelationFact>,
    /// Dynamic boundaries, ordered by (file, byte).
    pub dynamic_boundaries: Vec<DynamicBoundary>,
    /// Things the model could not fully determine.
    pub limitations: Vec<ModelLimitation>,
    /// Generation and fingerprint metadata for shards adopted through the ingestion API.
    #[serde(default)]
    pub shard_states: BTreeMap<String, ProjectShardState>,
    /// Discovered-but-unread relative paths (walk saw them; the read failed).
    /// They stay in the source denominator so a known-unread source can never
    /// answer as a legitimate empty; adoption of a readable shard for the
    /// path retires the entry.
    #[serde(default)]
    pub unread_discovered: BTreeSet<String>,
}

impl ProjectModel {
    /// An empty model for a root, with the given requested classes.
    #[must_use]
    pub fn empty(root: impl Into<String>, requested: FactClasses) -> Self {
        Self {
            root: root.into(),
            requested,
            files: Vec::new(),
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
            shard_states: BTreeMap::new(),
            unread_discovered: BTreeSet::new(),
        }
    }

    /// The file record for a given id, if present.
    #[must_use]
    pub fn file(&self, file_id: &FileId) -> Option<&FileRecord> {
        self.files.iter().find(|f| &f.file_id == file_id)
    }

    /// The file record for a repo-relative path, if present.
    #[must_use]
    pub fn file_by_path(&self, relative_path: &str) -> Option<&FileRecord> {
        self.files.iter().find(|f| f.relative_path == relative_path)
    }

    /// All packages declared in a file.
    #[must_use]
    pub fn packages_in_file(&self, file_id: &FileId) -> Vec<&PackageRecord> {
        self.packages.iter().filter(|p| &p.file_id == file_id).collect()
    }

    /// All symbols declared in a file.
    #[must_use]
    pub fn symbols_in_file(&self, file_id: &FileId) -> Vec<&SymbolRecord> {
        self.symbols.iter().filter(|s| &s.file_id == file_id).collect()
    }

    /// All dynamic boundaries recorded for a file.
    #[must_use]
    pub fn boundaries_in_file(&self, file_id: &FileId) -> Vec<&DynamicBoundary> {
        self.dynamic_boundaries.iter().filter(|b| &b.file_id == file_id).collect()
    }

    /// All import facts recorded for a file.
    #[must_use]
    pub fn imports_in_file(&self, file_id: &FileId) -> Vec<&ImportFact> {
        self.imports.iter().filter(|i| &i.file_id == file_id).collect()
    }

    /// All export facts recorded for a file.
    #[must_use]
    pub fn exports_in_file(&self, file_id: &FileId) -> Vec<&ExportFact> {
        self.exports.iter().filter(|e| &e.file_id == file_id).collect()
    }

    /// The compile-effect facts for a file, if computed.
    #[must_use]
    pub fn compile_effects_for_file(&self, file_id: &FileId) -> Option<&CompileEffectFacts> {
        self.compile_effects.iter().find(|e| &e.file_id == file_id)
    }

    /// All declared prerequisites across every metadata file in the model.
    #[must_use]
    pub fn all_prereqs(&self) -> Vec<&crate::dist::Prereq> {
        self.dist_metadata.iter().flat_map(|d| d.prereqs.iter()).collect()
    }

    /// Total number of facts across all classes — a quick health signal.
    #[must_use]
    pub fn fact_count(&self) -> usize {
        self.files.len()
            + self.packages.len()
            + self.symbols.len()
            + self.imports.len()
            + self.exports.len()
            + self.compile_effects.len()
            + self.dist_metadata.len()
            + self.tests.len()
            + self.pod.len()
            + self.relations.len()
            + self.dynamic_boundaries.len()
    }

    /// Sort every fact vector into its canonical, build-stable order.
    ///
    /// The builder calls this before returning, so identical source always
    /// yields an identical (and cleanly diffable) model.
    pub fn sort_for_determinism(&mut self) {
        self.files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        self.packages.sort_by(|a, b| {
            (a.file_id.as_str(), a.declaration_range.start_byte)
                .cmp(&(b.file_id.as_str(), b.declaration_range.start_byte))
        });
        self.symbols.sort_by(|a, b| {
            (a.file_id.as_str(), a.declaration_range.start_byte, a.kind.tag()).cmp(&(
                b.file_id.as_str(),
                b.declaration_range.start_byte,
                b.kind.tag(),
            ))
        });
        self.imports.sort_by(|a, b| {
            (a.file_id.as_str(), a.range.start_byte).cmp(&(b.file_id.as_str(), b.range.start_byte))
        });
        self.exports.sort_by(|a, b| {
            (a.file_id.as_str(), a.range.start_byte).cmp(&(b.file_id.as_str(), b.range.start_byte))
        });
        self.compile_effects.sort_by(|a, b| a.file_id.as_str().cmp(b.file_id.as_str()));
        self.dist_metadata.sort_by(|a, b| a.file_id.as_str().cmp(b.file_id.as_str()));
        self.tests.sort_by(|a, b| a.file_id.as_str().cmp(b.file_id.as_str()));
        self.pod.sort_by(|a, b| a.file_id.as_str().cmp(b.file_id.as_str()));
        self.relations.sort_by(|a, b| {
            (a.kind.tag(), &a.source, &a.target).cmp(&(b.kind.tag(), &b.source, &b.target))
        });
        self.dynamic_boundaries.sort_by(|a, b| {
            (a.file_id.as_str(), a.range.start_byte).cmp(&(b.file_id.as_str(), b.range.start_byte))
        });
        self.limitations.sort_by(|a, b| a.id.cmp(&b.id));
    }

    /// Atomically insert or replace all contributions owned by one file.
    pub fn insert_or_replace(
        &mut self,
        shard: ProjectFactShard,
    ) -> Result<ProjectDelta, ShardError> {
        shard.validate()?;
        let shard = shard.normalized();
        let file_id = shard.file.file_id.clone();
        let relative_path = shard.file.relative_path.clone();
        let fingerprint = shard.fingerprint()?;

        if let Some(current) = self.shard_states.get(&relative_path) {
            if shard.generation < current.generation {
                return Err(ShardError::StaleGeneration {
                    current: current.generation,
                    incoming: shard.generation,
                });
            }
            if shard.generation == current.generation && fingerprint == current.fingerprint {
                return Ok(ProjectDelta::empty());
            }
            if shard.generation == current.generation {
                return Err(ShardError::ConflictingGeneration { generation: shard.generation });
            }
        }

        let replaced_file_id = self.file_by_path(&relative_path).map(|file| file.file_id.clone());
        let existed = replaced_file_id.is_some();
        let replaced_packages = replaced_file_id
            .as_ref()
            .map(|previous_file_id| self.package_names_for_file(previous_file_id));
        if let Some(previous_file_id) = replaced_file_id {
            self.remove_owned_facts(&previous_file_id, &relative_path);
        }
        let limitation_ids = shard.limitations.iter().map(|item| item.id.clone()).collect();
        // Structural limitation-to-path association: a shard owns exactly one
        // file, so a limitation that declares no paths bounds that file. This
        // is ownership, not id-text reconstruction.
        let shard_path = relative_path.clone();
        let limitation_paths = shard
            .limitations
            .iter()
            .map(|item| {
                let paths = if item.paths.is_empty() {
                    vec![shard_path.clone()]
                } else {
                    item.paths.clone()
                };
                (item.id.clone(), paths)
            })
            .collect();
        // A readable shard adopted for this path supersedes the walk-time
        // discovered-but-unread marker. Remove only the adopted path from
        // structural read-failure limitations; other unread paths remain
        // bounded by the same limitation.
        if self.unread_discovered.remove(relative_path.as_str()) {
            self.limitations.retain_mut(|limitation| {
                if limitation.kind != "read_failure" || limitation.paths.is_empty() {
                    return true;
                }
                limitation.paths.retain(|path| path != &relative_path);
                !limitation.paths.is_empty()
            });
        }
        self.files.push(shard.file);
        self.packages.extend(shard.packages);
        self.symbols.extend(shard.symbols);
        self.imports.extend(shard.imports);
        self.exports.extend(shard.exports);
        self.compile_effects.extend(shard.compile_effects);
        self.dist_metadata.extend(shard.dist_metadata);
        self.tests.extend(shard.tests);
        self.pod.extend(shard.pod);
        self.relations.extend(shard.relations);
        self.dynamic_boundaries.extend(shard.dynamic_boundaries);
        self.limitations.extend(shard.limitations);
        self.shard_states.insert(
            relative_path,
            ProjectShardState {
                generation: shard.generation,
                producer: shard.producer,
                schema_version: shard.schema_version,
                fingerprint,
                limitation_ids,
                populated: Some(shard.populated),
                limitation_paths,
            },
        );
        self.sort_for_determinism();

        let mut delta = ProjectDelta::empty();
        if existed {
            delta.changed_files.push(file_id.clone());
            if let Some(package_names) = replaced_packages {
                delta.invalidated_files =
                    self.dependents_for_removed_packages(&package_names, &file_id);
            }
        } else {
            delta.added_files.push(file_id);
        }
        Ok(delta)
    }

    /// Remove all contributions owned by a file at a current or newer generation.
    pub fn remove_file(
        &mut self,
        file_id: &FileId,
        generation: u64,
    ) -> Result<ProjectDelta, ShardError> {
        let relative_path = self.file(file_id).map(|file| file.relative_path.clone());
        let current = relative_path.as_ref().and_then(|path| self.shard_states.get(path));
        if let Some(current) = current
            && generation < current.generation
        {
            return Err(ShardError::StaleRemoval {
                current: current.generation,
                incoming: generation,
            });
        }

        let mut delta = ProjectDelta::empty();
        if self.file(file_id).is_none() {
            return Ok(delta);
        }
        let removed_packages = self.package_names_for_file(file_id);
        let removed_path = relative_path;
        if let Some(path) = &removed_path {
            self.remove_owned_facts(file_id, path);
        }
        if let Some(path) = &removed_path {
            self.shard_states.remove(path);
        }
        delta.removed_files.push(file_id.clone());
        delta.invalidated_files = self.dependents_for_removed_packages(&removed_packages, file_id);
        self.sort_for_determinism();
        Ok(delta)
    }

    /// Deterministic identity of the current serialized model.
    pub fn snapshot_identity(&self) -> Result<String, ShardError> {
        let encoded = serde_json::to_vec(self)
            .map_err(|error| ShardError::Serialization { message: error.to_string() })?;
        Ok(format!("fnv64:{:016x}", crate::fnv1a(&encoded)))
    }

    fn remove_owned_facts(&mut self, file_id: &FileId, relative_path: &str) {
        self.files.retain(|fact| &fact.file_id != file_id);
        self.packages.retain(|fact| &fact.file_id != file_id);
        self.symbols.retain(|fact| &fact.file_id != file_id);
        self.imports.retain(|fact| &fact.file_id != file_id);
        self.exports.retain(|fact| &fact.file_id != file_id);
        self.compile_effects.retain(|fact| &fact.file_id != file_id);
        self.dist_metadata.retain(|fact| &fact.file_id != file_id);
        self.tests.retain(|fact| &fact.file_id != file_id);
        self.pod.retain(|fact| &fact.file_id != file_id);
        self.relations.retain(|fact| &fact.file_id != file_id);
        self.dynamic_boundaries.retain(|fact| &fact.file_id != file_id);
        if let Some(state) = self.shard_states.get(relative_path) {
            let ids: BTreeSet<&str> = state.limitation_ids.iter().map(String::as_str).collect();
            self.limitations.retain(|limitation| !ids.contains(limitation.id.as_str()));
        }
    }

    fn package_names_for_file(&self, file_id: &FileId) -> BTreeSet<String> {
        self.packages
            .iter()
            .filter(|package| &package.file_id == file_id)
            .map(|package| package.name.clone())
            .collect()
    }

    fn dependents_for_removed_packages(
        &self,
        removed_packages: &BTreeSet<String>,
        excluded: &FileId,
    ) -> Vec<FileId> {
        if removed_packages.is_empty() {
            return Vec::new();
        }
        let mut dependents = BTreeSet::new();
        for relation in &self.relations {
            if &relation.file_id != excluded && removed_packages.contains(&relation.target) {
                dependents.insert(relation.file_id.clone());
            }
        }
        dependents.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    use super::*;
    use crate::id::Digest;

    #[test]
    fn empty_model_has_no_facts() {
        let model = ProjectModel::empty("lib", FactClasses::all());
        assert_eq!(model.fact_count(), 0);
        assert!(model.file_by_path("lib/App.pm").is_none());
    }

    #[test]
    fn queries_filter_by_file() {
        let mut model = ProjectModel::empty(".", FactClasses::all());
        let fa = FileId::new("lib/A.pm", &Digest::of("a"));
        let fb = FileId::new("lib/B.pm", &Digest::of("b"));
        model.files.push(FileRecord {
            file_id: fa.clone(),
            relative_path: "lib/A.pm".to_string(),
            role: crate::file::FileRole::Lib,
            digest: Digest::of("a"),
            parse_status: crate::file::ParseStatus::Clean,
        });
        model.files.push(FileRecord {
            file_id: fb.clone(),
            relative_path: "lib/B.pm".to_string(),
            role: crate::file::FileRole::Lib,
            digest: Digest::of("b"),
            parse_status: crate::file::ParseStatus::Clean,
        });
        assert!(model.file(&fa).is_some());
        assert_eq!(model.file_by_path("lib/B.pm").unwrap().file_id, fb);
        assert_eq!(model.packages_in_file(&fa).len(), 0);
    }
}
