use std::error::Error;
use std::io;

use perl_workspace_core::{
    CompileEffectFacts, Digest, FactClasses, FileId, FileRecord, FileRole, ModelLimitation,
    ParseStatus, ProjectFactShard, ProjectModel, ShardError,
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
