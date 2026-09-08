#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use std::error::Error;
use std::io;

use perl_workspace_core::{
    CompileEffectFacts, Confidence, Digest, FactClasses, FileId, FileRecord, FileRole,
    ModelLimitation, PackageId, PackageRecord, ParseStatus, ProjectFactShard, ProjectModel,
    RelationFact, RelationKind, ShardError, SourceRange,
};

fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
    if condition { Ok(()) } else { Err(io::Error::other(message).into()) }
}

fn file(path: &str, source: &str) -> FileRecord {
    let digest = Digest::of(source);
    FileRecord {
        file_id: FileId::new(path, &digest),
        relative_path: path.to_string(),
        role: FileRole::Lib,
        digest,
        parse_status: ParseStatus::Clean,
    }
}

fn shard(path: &str, source: &str, generation: u64) -> ProjectFactShard {
    let mut shard =
        ProjectFactShard::empty(file(path, source), generation, "test-builder", FactClasses::all());
    shard.source_len_bytes = u32::try_from(source.len()).unwrap_or(u32::MAX);
    shard
}

fn package_record(file_id: &FileId, name: &str) -> PackageRecord {
    PackageRecord {
        package_id: PackageId::new(file_id, name, 0),
        name: name.to_string(),
        file_id: file_id.clone(),
        declaration_range: SourceRange {
            start_byte: 0,
            end_byte: 1,
            start_line: 0,
            start_column_utf8: 0,
            end_line: 0,
            end_column_utf8: 0,
        },
        version: None,
        parents: Vec::new(),
        roles: Vec::new(),
        confidence: Confidence::High,
    }
}

#[test]
fn insert_replay_replace_and_remove_are_generation_aware() -> Result<(), Box<dyn Error>> {
    let mut model = ProjectModel::empty(".", FactClasses::all());
    let first = shard("lib/App.pm", "package App;\n", 1);
    let first_id = first.file.file_id.clone();

    let added = model.insert_or_replace(first.clone())?;
    require(added.added_files == vec![first_id.clone()], "first insertion was not reported")?;
    require(model.file(&first_id).is_some(), "first file was not inserted")?;

    let replay = model.insert_or_replace(first)?;
    require(
        replay.added_files.is_empty() && replay.changed_files.is_empty(),
        "identical replay was not idempotent",
    )?;

    let second = shard("lib/App.pm", "package App;\nsub run {}\n", 2);
    let second_id = second.file.file_id.clone();
    let changed = model.insert_or_replace(second)?;
    require(changed.changed_files == vec![second_id.clone()], "replacement was not reported")?;
    require(model.file(&first_id).is_none(), "old content-derived file identity remained")?;
    require(model.file(&second_id).is_some(), "new file identity was not inserted")?;
    require(model.files.len() == 1, "replacement duplicated the file path")?;

    let stale = model.insert_or_replace(shard("lib/App.pm", "package App;\n", 1));
    require(
        matches!(stale, Err(ShardError::StaleGeneration { current: 2, incoming: 1 })),
        "stale generation was not rejected",
    )?;
    let conflict = model.insert_or_replace(shard("lib/App.pm", "package Changed;\n", 2));
    require(
        matches!(conflict, Err(ShardError::ConflictingGeneration { generation: 2 })),
        "conflicting payload at the current generation was not rejected",
    )?;

    let stale_remove = model.remove_file(&second_id, 1);
    require(
        matches!(stale_remove, Err(ShardError::StaleRemoval { current: 2, incoming: 1 })),
        "stale removal was not rejected",
    )?;
    let removed = model.remove_file(&second_id, 2)?;
    require(removed.removed_files == vec![second_id], "removal was not reported")?;
    require(model.files.is_empty(), "file contributions remained after removal")?;
    Ok(())
}

#[test]
fn replacement_removes_limitations_owned_by_the_previous_shard() -> Result<(), Box<dyn Error>> {
    let mut model = ProjectModel::empty(".", FactClasses::all());
    let mut first = shard("lib/App.pm", "package App;\n", 1);
    first.limitations.push(ModelLimitation {
        id: "parse-failed:lib/App.pm".to_string(),
        kind: "parse_failure".to_string(),
        message: "old limitation".to_string(),
    });
    model.insert_or_replace(first)?;

    let second = shard("lib/App.pm", "package App;\nsub run {}\n", 2);
    model.insert_or_replace(second)?;
    require(
        model.limitations.iter().all(|item| item.id != "parse-failed:lib/App.pm"),
        "replacement retained a limitation owned by the prior shard",
    )
}

#[test]
fn validation_rejects_cross_file_facts() -> Result<(), Box<dyn Error>> {
    let mut incoming = shard("lib/App.pm", "package App;\n", 1);
    let other = file("lib/Other.pm", "package Other;\n");
    incoming.compile_effects.push(CompileEffectFacts {
        file_id: other.file_id,
        strict: false,
        warnings: false,
        utf8: false,
        unicode_strings: false,
        features: Vec::new(),
        disabled_warnings: Vec::new(),
        perl_version: None,
    });
    incoming.populated |= FactClasses::COMPILE_EFFECTS;

    let result = incoming.validate();
    require(
        matches!(result, Err(ShardError::WrongFileOwner { fact_kind: "compile_effect" })),
        "cross-file fact ownership was not rejected",
    )
}

#[test]
fn fingerprint_and_snapshot_identity_are_deterministic() -> Result<(), Box<dyn Error>> {
    let incoming = shard("lib/App.pm", "package App;\n", 7);
    let fingerprint_a = incoming.fingerprint()?;
    let fingerprint_b = incoming.clone().fingerprint()?;
    require(fingerprint_a == fingerprint_b, "shard fingerprint changed across clones")?;

    let mut model_a = ProjectModel::empty(".", FactClasses::all());
    let mut model_b = ProjectModel::empty(".", FactClasses::all());
    model_a.insert_or_replace(incoming.clone())?;
    model_b.insert_or_replace(incoming)?;
    require(
        model_a.snapshot_identity()? == model_b.snapshot_identity()?,
        "equivalent models produced different snapshot identities",
    )
}

#[test]
fn remove_and_replace_invalidate_relation_dependents_by_package_name() -> Result<(), Box<dyn Error>>
{
    let mut model = ProjectModel::empty(".", FactClasses::all());

    let mut base = shard("lib/Base.pm", "package Base;\n", 1);
    let base_id = base.file.file_id.clone();
    base.packages.push(package_record(&base_id, "Base"));
    base.populated |= FactClasses::SYMBOLS;
    model.insert_or_replace(base)?;

    let mut consumer = shard("lib/Consumer.pm", "package Consumer;\nuse Base;\n", 1);
    let consumer_id = consumer.file.file_id.clone();
    consumer.packages.push(package_record(&consumer_id, "Consumer"));
    consumer.relations.push(RelationFact {
        kind: RelationKind::Uses,
        source: "lib/Consumer.pm".to_string(),
        target: "Base".to_string(),
        file_id: consumer_id.clone(),
        confidence: Confidence::High,
    });
    consumer.populated |= FactClasses::SYMBOLS | FactClasses::RELATIONS;
    model.insert_or_replace(consumer)?;

    let removed = model.remove_file(&base_id, 1)?;
    require(
        removed.invalidated_files == vec![consumer_id.clone()],
        "removal did not invalidate files that use a removed package",
    )?;

    let mut base_v2 = shard("lib/Base.pm", "package Base;\nsub run {}\n", 2);
    let base_v2_id = base_v2.file.file_id.clone();
    base_v2.packages.push(package_record(&base_v2_id, "Base"));
    base_v2.populated |= FactClasses::SYMBOLS;
    model.insert_or_replace(base_v2)?;

    let mut base_v3 = shard("lib/Base.pm", "package Base;\nsub run {}\n", 3);
    let base_v3_id = base_v3.file.file_id.clone();
    base_v3.packages.push(package_record(&base_v3_id, "Base"));
    base_v3.populated |= FactClasses::SYMBOLS;
    let replaced = model.insert_or_replace(base_v3)?;
    require(
        replaced.invalidated_files == vec![consumer_id],
        "replacement did not invalidate files that use a replaced package",
    )?;
    Ok(())
}

#[test]
fn schema_v1_model_without_shard_state_remains_readable() -> Result<(), Box<dyn Error>> {
    let model = ProjectModel::empty(".", FactClasses::all());
    let mut legacy = serde_json::to_value(model)?;
    let object = legacy
        .as_object_mut()
        .ok_or_else(|| io::Error::other("serialized ProjectModel was not an object"))?;
    object.remove("shard_states");

    let decoded: ProjectModel = serde_json::from_value(legacy)?;
    require(decoded.shard_states.is_empty(), "legacy model did not default missing shard state")
}
