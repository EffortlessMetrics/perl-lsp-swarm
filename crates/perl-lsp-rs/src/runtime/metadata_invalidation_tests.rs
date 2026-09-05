//! Project-metadata invalidation on watched-file events (#13640).
//!
//! Internal proof that a change to project metadata reaches the authority that
//! owns dependency and environment facts, that it does so once per coalesced
//! batch, that deletes downgrade facts, and that unreadable or buffer-owned
//! metadata retains the previous snapshot instead of erasing it.
#![expect(
    clippy::expect_used,
    reason = "test-only policy proof: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]

use super::LspServer;
use super::workspace_folder::WorkspaceFolderState;
use serde_json::json;
use tempfile::TempDir;

const CHANGED: i32 = 2;
const DELETED: i32 = 3;

fn dir_uri(dir: &TempDir) -> String {
    url::Url::from_file_path(dir.path()).expect("temp dir must convert to file URI").to_string()
}

fn file_uri(dir: &TempDir, relative: &str) -> String {
    let mut path = dir.path().to_path_buf();
    for component in relative.split('/') {
        path.push(component);
    }
    url::Url::from_file_path(path).expect("temp path must convert to file URI").to_string()
}

fn write_file(dir: &TempDir, relative: &str, content: &str) {
    let mut path = dir.path().to_path_buf();
    for component in relative.split('/') {
        path.push(component);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dirs");
    }
    std::fs::write(path, content).expect("write temp workspace file");
}

/// A server with one registered workspace folder whose metadata facts have
/// already been established once, mirroring the initialize-time refresh.
fn workspace_server(dir: &TempDir) -> LspServer {
    let server = LspServer::new();
    let mut folder = WorkspaceFolderState::new(dir_uri(dir)).with_path(dir.path().to_path_buf());
    folder.refresh_workspace_metadata();
    server.workspace_folders.lock().push(folder);
    server
}

fn watched(server: &LspServer, changes: &[(&str, i32)]) {
    let changes =
        changes.iter().map(|(uri, typ)| json!({ "uri": uri, "type": typ })).collect::<Vec<_>>();
    server
        .handle_did_change_watched_files(Some(json!({ "changes": changes })))
        .expect("watched-files notification must parse");
}

fn declared_modules(server: &LspServer) -> Vec<String> {
    server
        .all_workspace_folders()
        .first()
        .map(|folder| {
            folder
                .effective_workspace_config
                .declared_dependencies
                .iter()
                .map(|dependency| dependency.module.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn index_symbols(server: &LspServer, query: &str) -> usize {
    server
        .coordinator()
        .map(|coordinator| coordinator.index().find_symbols(query).len())
        .unwrap_or_default()
}

/// The reproduction: before #13640 a `cpanfile` edit was classified as
/// non-Perl source and never reached the dependency-fact authority, so the
/// cached declarations stayed at their initialize-time value forever.
#[test]
fn watched_cpanfile_change_refreshes_declared_dependencies() {
    let dir = TempDir::new().expect("tempdir");
    write_file(&dir, "cpanfile", "requires 'JSON::PP';\n");
    let server = workspace_server(&dir);
    assert_eq!(declared_modules(&server), vec!["JSON::PP".to_string()], "initialize baseline");

    write_file(&dir, "cpanfile", "requires 'YAML::XS';\n");
    watched(&server, &[(&file_uri(&dir, "cpanfile"), CHANGED)]);

    assert_eq!(
        declared_modules(&server),
        vec!["YAML::XS".to_string()],
        "a watched cpanfile change must refresh declared-dependency facts (#13640)"
    );
}

/// `cpanfile` is not Perl source, so the metadata route must not smuggle it
/// into the source index.
#[test]
fn watched_metadata_change_does_not_index_metadata_as_perl_source() {
    let dir = TempDir::new().expect("tempdir");
    write_file(&dir, "cpanfile", "requires 'JSON::PP';\n");
    let server = workspace_server(&dir);

    watched(&server, &[(&file_uri(&dir, "cpanfile"), CHANGED)]);

    assert_eq!(
        index_symbols(&server, "requires"),
        0,
        "metadata must not enter Perl source indexing"
    );
}

/// Premise correction (#13640): `Makefile.PL` carries the `.PL` extension,
/// which the shared admission authority matches case-insensitively against
/// `pl`, so it is genuinely both Perl source and dependency metadata.
/// Routing it to metadata invalidation must be additive — suppressing its
/// source indexing would regress #14186.
#[test]
fn watched_makefile_pl_refreshes_dependencies_and_stays_perl_source() {
    let dir = TempDir::new().expect("tempdir");
    write_file(
        &dir,
        "Makefile.PL",
        "use ExtUtils::MakeMaker;\nsub makefile_pl_marker { 1 }\nWriteMakefile(PREREQ_PM => { 'JSON::PP' => '4.0' });\n",
    );
    let server = workspace_server(&dir);

    write_file(
        &dir,
        "Makefile.PL",
        "use ExtUtils::MakeMaker;\nsub makefile_pl_marker { 1 }\nWriteMakefile(PREREQ_PM => { 'YAML::XS' => '0.8' });\n",
    );
    watched(&server, &[(&file_uri(&dir, "Makefile.PL"), CHANGED)]);

    assert_eq!(
        declared_modules(&server),
        vec!["YAML::XS".to_string()],
        "Makefile.PL must refresh declared-dependency facts"
    );
    assert_eq!(
        index_symbols(&server, "makefile_pl_marker"),
        1,
        "Makefile.PL must remain an indexed Perl source (#14186 admission)"
    );
}

/// One coalesced batch is one observable refresh, not one per event.
#[test]
fn metadata_burst_advances_the_fact_generation_once() {
    let dir = TempDir::new().expect("tempdir");
    write_file(&dir, "cpanfile", "requires 'JSON::PP';\n");
    write_file(&dir, "META.json", "{}\n");
    write_file(&dir, "dist.ini", "name = Demo\n");
    let server = workspace_server(&dir);
    let before = server.dependency_facts_generation();

    watched(
        &server,
        &[
            (&file_uri(&dir, "cpanfile"), CHANGED),
            (&file_uri(&dir, "META.json"), CHANGED),
            (&file_uri(&dir, "dist.ini"), CHANGED),
        ],
    );

    assert_eq!(
        server.dependency_facts_generation(),
        before + 1,
        "a coalesced metadata burst must advance the generation exactly once"
    );
}

/// A delete downgrades facts immediately rather than leaving stale
/// declarations behind.
#[test]
fn deleted_cpanfile_downgrades_declared_dependencies() {
    let dir = TempDir::new().expect("tempdir");
    write_file(&dir, "cpanfile", "requires 'JSON::PP';\n");
    let server = workspace_server(&dir);
    assert_eq!(declared_modules(&server), vec!["JSON::PP".to_string()]);

    std::fs::remove_file(dir.path().join("cpanfile")).expect("remove cpanfile");
    watched(&server, &[(&file_uri(&dir, "cpanfile"), DELETED)]);

    assert!(
        declared_modules(&server).is_empty(),
        "a deleted cpanfile must not leave stale declarations"
    );
}

/// Open-buffer authority does not depend on the backing file still existing
/// (#8041). An external delete of an open metadata document — a branch switch
/// or a delete-and-rewrite save — must not erase facts the staged buffer still
/// declares, and must not retire the include root that `cpanfile`'s presence
/// gates.
#[test]
fn deleting_an_open_metadata_file_does_not_erase_buffer_owned_facts() {
    let dir = TempDir::new().expect("tempdir");
    write_file(&dir, "cpanfile", "requires 'JSON::PP';\n");
    write_file(&dir, "carton.lock", "snapshot\n");
    let server = workspace_server(&dir);
    let uri = file_uri(&dir, "cpanfile");
    assert_eq!(declared_modules(&server), vec!["JSON::PP".to_string()]);

    server
        .handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "requires 'JSON::PP';\nrequires 'Staged::Only';\n"
            }
        })))
        .expect("didOpen params are valid");

    std::fs::remove_file(dir.path().join("cpanfile")).expect("remove cpanfile");
    watched(&server, &[(&uri, DELETED)]);

    assert_eq!(
        declared_modules(&server),
        vec!["JSON::PP".to_string()],
        "an external delete must not erase facts an open metadata buffer still declares"
    );
    assert!(
        server.dependency_facts_are_stale(&dir_uri(&dir)),
        "the retained snapshot must be reported as stale"
    );
    assert!(
        server.all_workspace_folders().first().is_some_and(|folder| folder
            .effective_workspace_config
            .include_paths
            .iter()
            .any(|path| path == "local/lib/perl5")),
        "the Carton include root must survive a delete race on an open cpanfile"
    );
}

/// Negative control: the detectors only ever read workspace-root metadata, so
/// a nested file that merely shares the name is ordinary project content.
#[test]
fn nested_metadata_name_does_not_refresh_facts() {
    let dir = TempDir::new().expect("tempdir");
    write_file(&dir, "cpanfile", "requires 'JSON::PP';\n");
    write_file(&dir, "t/cpanfile", "requires 'Never::Seen';\n");
    let server = workspace_server(&dir);
    let before = server.dependency_facts_generation();

    watched(&server, &[(&file_uri(&dir, "t/cpanfile"), CHANGED)]);

    assert_eq!(
        server.dependency_facts_generation(),
        before,
        "a nested cpanfile is not project metadata"
    );
    assert_eq!(
        declared_modules(&server),
        vec!["JSON::PP".to_string()],
        "root facts must be untouched by a nested file"
    );
}

/// Negative control: a metadata change in an unrelated tree must not refresh
/// this workspace.
#[test]
fn metadata_outside_the_workspace_does_not_refresh_facts() {
    let dir = TempDir::new().expect("tempdir");
    let outside = TempDir::new().expect("outside tempdir");
    write_file(&dir, "cpanfile", "requires 'JSON::PP';\n");
    write_file(&outside, "cpanfile", "requires 'Never::Seen';\n");
    let server = workspace_server(&dir);
    let before = server.dependency_facts_generation();

    watched(&server, &[(&file_uri(&outside, "cpanfile"), CHANGED)]);

    assert_eq!(
        server.dependency_facts_generation(),
        before,
        "a change outside every workspace folder must not refresh"
    );
    assert_eq!(declared_modules(&server), vec!["JSON::PP".to_string()]);
}

/// Write bytes that `read_to_string` rejects, reproducing the detector's own
/// read failure without depending on permission bits (which do not constrain
/// a `root` test runner) or on any platform-specific sharing behavior.
fn write_unreadable_cpanfile(dir: &TempDir) {
    std::fs::write(dir.path().join("cpanfile"), [0xff_u8, 0xfe, 0xfd])
        .expect("write invalid UTF-8 cpanfile");
}

/// A read failure must retain the last known snapshot and mark it stale,
/// never silently erase it into "this project declares nothing".
#[test]
fn unreadable_cpanfile_retains_the_snapshot_and_marks_it_stale() {
    let dir = TempDir::new().expect("tempdir");
    write_file(&dir, "cpanfile", "requires 'JSON::PP';\n");
    let server = workspace_server(&dir);
    assert_eq!(declared_modules(&server), vec!["JSON::PP".to_string()]);

    write_unreadable_cpanfile(&dir);
    watched(&server, &[(&file_uri(&dir, "cpanfile"), CHANGED)]);

    assert_eq!(
        declared_modules(&server),
        vec!["JSON::PP".to_string()],
        "an unreadable cpanfile must not erase the previous snapshot"
    );
    assert!(
        server.dependency_facts_are_stale(&dir_uri(&dir)),
        "a retained snapshot must be reported as stale, not current"
    );
}

/// A later readable event clears the stale marker.
#[test]
fn a_readable_refresh_clears_the_stale_marker() {
    let dir = TempDir::new().expect("tempdir");
    write_file(&dir, "cpanfile", "requires 'JSON::PP';\n");
    let server = workspace_server(&dir);

    write_unreadable_cpanfile(&dir);
    watched(&server, &[(&file_uri(&dir, "cpanfile"), CHANGED)]);
    assert!(
        server.dependency_facts_are_stale(&dir_uri(&dir)),
        "the unreadable event must mark the snapshot stale"
    );

    write_file(&dir, "cpanfile", "requires 'YAML::XS';\n");
    watched(&server, &[(&file_uri(&dir, "cpanfile"), CHANGED)]);

    assert!(
        !server.dependency_facts_are_stale(&dir_uri(&dir)),
        "a readable refresh must clear the stale marker"
    );
    assert_eq!(declared_modules(&server), vec!["YAML::XS".to_string()]);
}

/// The retained-snapshot path must not silently stop advancing facts for
/// unrelated folders, and must not advance the generation when nothing
/// refreshed.
#[test]
fn a_retained_snapshot_does_not_advance_the_fact_generation() {
    let dir = TempDir::new().expect("tempdir");
    write_file(&dir, "cpanfile", "requires 'JSON::PP';\n");
    let server = workspace_server(&dir);
    write_unreadable_cpanfile(&dir);
    let before = server.dependency_facts_generation();

    watched(&server, &[(&file_uri(&dir, "cpanfile"), CHANGED)]);

    assert_eq!(
        server.dependency_facts_generation(),
        before,
        "retaining a snapshot is not a refresh and must not advance the generation"
    );
}

/// Open-buffer authority (#8041) extends to metadata documents: while the
/// editor holds staged text, a disk-derived refresh would record provenance
/// that contradicts what the user sees.
#[test]
fn open_metadata_buffer_retains_the_disk_snapshot_and_marks_it_stale() {
    let dir = TempDir::new().expect("tempdir");
    write_file(&dir, "cpanfile", "requires 'JSON::PP';\n");
    let server = workspace_server(&dir);
    let uri = file_uri(&dir, "cpanfile");

    server
        .handle_did_open(Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": "perl",
                "version": 1,
                "text": "requires 'Staged::Only';\n"
            }
        })))
        .expect("didOpen params are valid");

    write_file(&dir, "cpanfile", "requires 'YAML::XS';\n");
    watched(&server, &[(&uri, CHANGED)]);

    assert_eq!(
        declared_modules(&server),
        vec!["JSON::PP".to_string()],
        "disk bytes behind an authoritative open metadata buffer must not be adopted"
    );
    assert!(
        server.dependency_facts_are_stale(&dir_uri(&dir)),
        "a buffer-owned metadata document leaves the disk snapshot stale"
    );
}
