//! Public workspace-rename writer controls. These do not exercise the LSP provider.
//!
//! Each invalid-range test is one failing case so a red run on the base observes
//! that mechanism independently (#18209).

use std::error::Error;
use std::path::Path;

use perl_parser::refactor::workspace_refactor::{FileEdit, TextEdit};
use perl_parser::refactor::workspace_rename::{
    InvalidEditReason, RenameStatistics, WorkspaceRename, WorkspaceRenameConfig,
    WorkspaceRenameError, WorkspaceRenameResult,
};
use perl_parser::workspace_index::WorkspaceIndex;

type TestResult = Result<(), Box<dyn Error>>;

fn engine() -> WorkspaceRename {
    WorkspaceRename::new(WorkspaceIndex::new(), WorkspaceRenameConfig::default())
}

fn edit(start: usize, end: usize) -> TextEdit {
    TextEdit { start, end, new_text: "renamed".to_string() }
}

fn file(path: &Path, edits: Vec<TextEdit>) -> FileEdit {
    FileEdit { file_path: path.to_path_buf(), edits }
}

fn plan(file_edits: Vec<FileEdit>) -> WorkspaceRenameResult {
    let files_modified = file_edits.len();
    let total_changes = file_edits.iter().map(|entry| entry.edits.len()).sum();
    WorkspaceRenameResult {
        file_edits,
        backup_info: None,
        description: "rename validity fixture".to_string(),
        warnings: vec![],
        statistics: RenameStatistics { files_modified, total_changes, elapsed_ms: 0 },
    }
}

fn two_file_dir()
-> Result<(tempfile::TempDir, std::path::PathBuf, std::path::PathBuf), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let first = dir.path().join("a.pl");
    let second = dir.path().join("b.pl");
    std::fs::write(&first, "old\n")?;
    std::fs::write(&second, "é old old\n")?;
    Ok((dir, first, second))
}

fn assert_unchanged(first: &Path, second: &Path) -> TestResult {
    assert_eq!(std::fs::read_to_string(first)?, "old\n", "first changed");
    assert_eq!(std::fs::read_to_string(second)?, "é old old\n", "second changed");
    Ok(())
}

fn refuses_later_range(start: usize, end: usize, reason: InvalidEditReason) -> TestResult {
    let (_dir, first, second) = two_file_dir()?;
    let result = engine().apply_edits(&plan(vec![
        file(&first, vec![edit(0, 3)]),
        file(&second, vec![edit(start, end)]),
    ]));
    assert!(
        matches!(
            result,
            Err(WorkspaceRenameError::InvalidEdit { ref file, start: got_start, end: got_end, reason: got_reason })
                if file == &second && got_start == start && got_end == end && got_reason == reason
        ),
        "expected InvalidEdit({reason:?}) for {start}..{end}, got {result:?}"
    );
    assert_unchanged(&first, &second)
}

#[test]
fn past_end_range_refuses_the_whole_plan_before_writing() -> TestResult {
    refuses_later_range(100, 103, InvalidEditReason::PastEnd)
}

#[test]
fn reversed_range_refuses_the_whole_plan_before_writing() -> TestResult {
    refuses_later_range(6, 3, InvalidEditReason::Reversed)
}

#[test]
fn start_inside_scalar_refuses_the_whole_plan_before_writing() -> TestResult {
    refuses_later_range(1, 2, InvalidEditReason::NonCharBoundary)
}

#[test]
fn end_inside_scalar_refuses_the_whole_plan_before_writing() -> TestResult {
    refuses_later_range(0, 1, InvalidEditReason::NonCharBoundary)
}

#[test]
fn overlapping_ranges_refuse_the_whole_plan_before_writing() -> TestResult {
    let (_dir, first, second) = two_file_dir()?;
    let result = engine().apply_edits(&plan(vec![
        file(&first, vec![edit(0, 3)]),
        file(&second, vec![edit(7, 10), edit(3, 8)]),
    ]));
    assert!(
        matches!(
            result,
            Err(WorkspaceRenameError::InvalidEdit { ref file, reason: InvalidEditReason::Overlapping, .. })
                if file == &second
        ),
        "expected overlapping refusal, got {result:?}"
    );
    assert_unchanged(&first, &second)
}

#[test]
fn ascending_ranges_refuse_the_whole_plan_before_writing() -> TestResult {
    let (_dir, first, second) = two_file_dir()?;
    let result = engine().apply_edits(&plan(vec![
        file(&first, vec![edit(0, 3)]),
        file(&second, vec![edit(3, 6), edit(7, 10)]),
    ]));
    assert!(
        matches!(
            result,
            Err(WorkspaceRenameError::InvalidEdit { ref file, reason: InvalidEditReason::OutOfOrder, .. })
                if file == &second
        ),
        "expected out-of-order refusal, got {result:?}"
    );
    assert_unchanged(&first, &second)
}

#[test]
fn missing_later_file_leaves_earlier_files_unchanged_without_backups() -> TestResult {
    let dir = tempfile::tempdir()?;
    let first = dir.path().join("a.pl");
    let second = dir.path().join("b.pl");
    std::fs::write(&first, "old\n")?;
    let result = engine()
        .apply_edits(&plan(vec![file(&first, vec![edit(0, 3)]), file(&second, vec![edit(0, 3)])]));
    assert!(
        matches!(
            result,
            Err(WorkspaceRenameError::FileSystemError { ref operation, .. }) if operation == "read"
        ),
        "expected read refusal, got {result:?}"
    );
    assert_eq!(std::fs::read_to_string(&first)?, "old\n");
    assert!(!second.exists());
    Ok(())
}

#[test]
fn invalid_utf8_later_file_leaves_earlier_files_unchanged_without_backups() -> TestResult {
    let dir = tempfile::tempdir()?;
    let first = dir.path().join("a.pl");
    let second = dir.path().join("b.pl");
    std::fs::write(&first, "old\n")?;
    std::fs::write(&second, [0xff])?;
    let result = engine()
        .apply_edits(&plan(vec![file(&first, vec![edit(0, 3)]), file(&second, vec![edit(0, 3)])]));
    assert!(
        matches!(
            result,
            Err(WorkspaceRenameError::FileSystemError { ref operation, .. }) if operation == "read"
        ),
        "expected read refusal, got {result:?}"
    );
    assert_eq!(std::fs::read_to_string(&first)?, "old\n");
    assert_eq!(std::fs::read(&second)?, vec![0xff]);
    Ok(())
}

#[test]
fn valid_multifile_unicode_edits_apply_every_occurrence_without_backups() -> TestResult {
    let dir = tempfile::tempdir()?;
    let first = dir.path().join("a.pl");
    let second = dir.path().join("b.pl");
    std::fs::write(&first, "old old\n")?;
    std::fs::write(&second, "é old old\n")?;
    engine().apply_edits(&plan(vec![
        file(&first, vec![edit(4, 7), edit(0, 3)]),
        file(&second, vec![edit(7, 10), edit(3, 6)]),
    ]))?;
    assert_eq!(std::fs::read_to_string(&first)?, "renamed renamed\n");
    assert_eq!(std::fs::read_to_string(&second)?, "é renamed renamed\n");
    Ok(())
}

#[test]
fn duplicate_document_plans_refuse_before_writing() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("a.pl");
    std::fs::write(&path, "old\n")?;
    let result = engine()
        .apply_edits(&plan(vec![file(&path, vec![edit(0, 3)]), file(&path, vec![edit(0, 3)])]));
    assert!(
        matches!(result, Err(WorkspaceRenameError::DuplicateDocument { ref file }) if file == &path),
        "expected duplicate-document refusal, got {result:?}"
    );
    assert_eq!(std::fs::read_to_string(&path)?, "old\n");
    Ok(())
}

#[test]
fn a_document_shortened_after_planning_refuses_without_changing_other_files() -> TestResult {
    let dir = tempfile::tempdir()?;
    let first = dir.path().join("a.pm");
    let second = dir.path().join("b.pm");
    let first_source = "package Sample;\nsub target { 1 }\ntarget();\n";
    let second_source = "package Sample;\ntarget();\n";
    let index = WorkspaceIndex::new();
    for (path, source) in [(&first, first_source), (&second, second_source)] {
        std::fs::write(path, source)?;
        let uri = url::Url::from_file_path(path).map_err(|_| "invalid fixture path")?;
        index.index_file(uri, source.to_string())?;
    }
    let engine = WorkspaceRename::new(
        index,
        WorkspaceRenameConfig { create_backups: false, ..WorkspaceRenameConfig::default() },
    );
    let result = engine.rename_symbol("Sample::target", "Sample::renamed", &first, (1, 4))?;
    assert_eq!(result.file_edits.len(), 2, "fixture must plan both documents");
    assert!(result.file_edits.iter().all(|entry| !entry.edits.is_empty()));
    std::fs::write(&second, "")?;
    assert!(engine.apply_edits(&result).is_err());
    assert_eq!(std::fs::read_to_string(&first)?, first_source);
    assert_eq!(std::fs::read_to_string(&second)?, "");
    Ok(())
}

#[cfg(unix)]
#[test]
fn an_unwritable_later_file_keeps_the_existing_backup_rollback_behavior() -> TestResult {
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;

    use perl_parser::refactor::workspace_rename::BackupInfo;

    let dir = tempfile::tempdir()?;
    let first = dir.path().join("a.pl");
    let second = dir.path().join("b.pl");
    let backup = dir.path().join("a.backup");
    std::fs::write(&first, "old\n")?;
    std::fs::write(&second, "old\n")?;
    std::fs::copy(&first, &backup)?;
    let mut result = plan(vec![file(&first, vec![edit(0, 3)]), file(&second, vec![edit(0, 3)])]);
    result.backup_info = Some(BackupInfo {
        backup_dir: dir.path().to_path_buf(),
        file_mappings: HashMap::from([(first.clone(), backup)]),
    });
    let permissions = std::fs::metadata(&second)?.permissions();
    std::fs::set_permissions(&second, std::fs::Permissions::from_mode(0o444))?;
    if std::fs::OpenOptions::new().write(true).open(&second).is_ok() {
        std::fs::set_permissions(&second, permissions)?;
        // Privileged environments can still write mode 0444; this control needs an
        // unprivileged user and is not a product defect when it cannot be armed.
        return Ok(());
    }
    let outcome = engine().apply_edits(&result);
    std::fs::set_permissions(&second, permissions)?;
    assert!(
        matches!(
            outcome,
            Err(WorkspaceRenameError::FileSystemError { ref operation, .. }) if operation == "write"
        ),
        "fixture requires an unprivileged user: {outcome:?}"
    );
    assert_eq!(std::fs::read_to_string(&first)?, "old\n");
    assert_eq!(std::fs::read_to_string(&second)?, "old\n");
    Ok(())
}

#[test]
fn adjacent_ranges_accept_deletion_growth_and_multibyte_replacement() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("a.pl");
    std::fs::write(&path, "🦀oldold\n")?;
    engine().apply_edits(&plan(vec![file(
        &path,
        vec![
            TextEdit { start: 7, end: 10, new_text: "ββ".to_string() },
            TextEdit { start: 4, end: 7, new_text: String::new() },
            TextEdit { start: 0, end: 4, new_text: "🙂".to_string() },
        ],
    )]))?;
    assert_eq!(std::fs::read_to_string(&path)?, "🙂ββ\n");
    Ok(())
}

#[test]
fn zero_length_insertions_at_eof_and_in_an_empty_document_are_valid() -> TestResult {
    let dir = tempfile::tempdir()?;
    let first = dir.path().join("a.pl");
    let second = dir.path().join("b.pl");
    std::fs::write(&first, "old")?;
    std::fs::write(&second, "")?;
    engine().apply_edits(&plan(vec![
        file(
            &first,
            vec![
                TextEdit { start: 3, end: 3, new_text: "!".to_string() },
                TextEdit { start: 0, end: 0, new_text: "«".to_string() },
            ],
        ),
        file(&second, vec![TextEdit { start: 0, end: 0, new_text: "é".to_string() }]),
    ]))?;
    assert_eq!(std::fs::read_to_string(&first)?, "«old!");
    assert_eq!(std::fs::read_to_string(&second)?, "é");
    Ok(())
}

fn refuses_four_byte_scalar_range_without_writes(
    start: usize,
    end: usize,
    reason: InvalidEditReason,
) -> TestResult {
    let dir = tempfile::tempdir()?;
    let first = dir.path().join("a.pl");
    let second = dir.path().join("b.pl");
    std::fs::write(&first, "old\n")?;
    std::fs::write(&second, "🦀old\n")?;
    let outcome = engine().apply_edits(&plan(vec![
        file(&first, vec![edit(0, 3)]),
        file(&second, vec![edit(start, end)]),
    ]));
    assert!(
        matches!(
            outcome,
            Err(WorkspaceRenameError::InvalidEdit {
                ref file,
                start: got_start,
                end: got_end,
                reason: got_reason,
            }) if file == &second && got_start == start && got_end == end && got_reason == reason
        ),
        "expected InvalidEdit({reason:?}) for {start}..{end}, got {outcome:?}"
    );
    assert_eq!(std::fs::read(&first)?, b"old\n");
    assert_eq!(std::fs::read_to_string(&second)?, "🦀old\n");
    Ok(())
}

#[test]
fn start_inside_four_byte_scalar_refuses_before_writes() -> TestResult {
    refuses_four_byte_scalar_range_without_writes(1, 4, InvalidEditReason::NonCharBoundary)
}

#[test]
fn end_inside_four_byte_scalar_refuses_before_writes() -> TestResult {
    refuses_four_byte_scalar_range_without_writes(0, 3, InvalidEditReason::NonCharBoundary)
}

#[test]
fn maximum_offsets_refuse_without_arithmetic_overflow_or_writes() -> TestResult {
    refuses_four_byte_scalar_range_without_writes(
        usize::MAX,
        usize::MAX,
        InvalidEditReason::PastEnd,
    )
}
