//! `ci-route-plan` — canonical route-plan publication CLI (#10179).
//!
//! ```text
//! ci-route-plan compile <input.json> <output.json>   compile + publish
//! ci-route-plan validate <plan.json>                 parse + validate
//! ci-route-plan explain <plan.json> [gate_id]        presentation only
//! ```
//!
//! Every surface fails closed with a typed refusal: a non-zero exit and a
//! `kind: detail` message on stderr; success prints the semantic
//! fingerprint after verification.
//!
//! ## Publication contract (single-writer durable handoff)
//!
//! `compile` treats the output artifact as a single-writer durable
//! handoff, not a shared store:
//!
//! 1. one accepted input -> one complete canonical byte sequence
//!    (validated before any filesystem effect);
//! 2. a unique temporary file in the target directory
//!    (`.ci-route-plan.<pid>.<nanos>.<seq>.tmp`, created with
//!    create-new semantics so two writers can never share it);
//! 3. complete write + flush + `sync_all` durability step;
//! 4. read-back verification of the durable temp, so only fully verified
//!    bytes are ever promoted;
//! 5. atomic publication by rename, which replaces an existing
//!    destination on both POSIX and Windows (std uses `rename(2)` /
//!    `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` — an explicit
//!    cross-platform overwrite contract, not a Unix-only accident);
//! 6. POSIX directory sync of the rename, so the published name survives
//!    a host crash (an explicit typed no-op boundary on Windows);
//! 7. final read-back verification: success is returned only after the
//!    destination contains exactly the canonical bytes.
//!
//! Any failure (serialization, directory creation, temp creation, write,
//! sync, rename, directory sync, read-back) is a typed non-success. A
//! failure before this writer's rename promoted (temp write, sync, temp
//! read-back, rename) removes only the temporary this writer provably
//! owns: any file already at the destination was never produced by this
//! invocation and may be a concurrent publication's completed artifact,
//! which a losing writer must never delete, so the destination is left
//! untouched. A post-promotion failure (directory sync or final
//! read-back) likewise leaves the destination untouched — it may hold
//! this writer's promoted bytes or a concurrent writer's completed
//! artifact — and the refusal states that this invocation could not
//! verify the artifact. Consumers must trust the typed refusal, never
//! the presence or absence of an artifact. Multi-writer/shared-store
//! semantics are explicitly not implied; downstream consumers needing
//! them require a separate contract.

// CLI instrument: the typed refusal/success lines are this tool's interface.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use xtask::ci_route_plan::{CiRoutePlanV1, CompileRoutePlanInput};

/// Monotonic sequence within this process so two publications in one
/// process can never propose the same temporary name.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let result = match args.next().as_deref() {
        Some("compile") => compile_command(&mut args),
        Some("validate") => validate_command(&mut args),
        Some("explain") => explain_command(&mut args),
        _ => Err(CliError::Usage(
            "ci-route-plan compile <input.json> <output.json> | validate <plan.json> | explain \
             <plan.json> [gate_id]"
                .to_string(),
        )),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("ci-route-plan: {error}");
            ExitCode::FAILURE
        }
    }
}

fn compile_command(mut args: impl Iterator<Item = String>) -> Result<String, CliError> {
    let input = required_path(&mut args, "compile input")?;
    let output = required_path(&mut args, "compile output")?;
    reject_extra(args)?;
    let payload: CompileRoutePlanInput = read_json(&input)?;
    let plan = CiRoutePlanV1::compile(payload).map_err(CliError::Compile)?;
    let bytes = plan.canonical_json().map_err(CliError::Compile)?;
    publish_atomically(Path::new(&output), &bytes)?;
    Ok(format!("ci-route-plan: published {} ({})", output, plan.semantic_fingerprint))
}

fn validate_command(mut args: impl Iterator<Item = String>) -> Result<String, CliError> {
    let input = required_path(&mut args, "plan")?;
    reject_extra(args)?;
    let plan: CiRoutePlanV1 = read_json(&input)?;
    plan.validate().map_err(CliError::Validate)?;
    Ok(format!("ci-route-plan: valid {}", plan.semantic_fingerprint))
}

fn explain_command(mut args: impl Iterator<Item = String>) -> Result<String, CliError> {
    let input = required_path(&mut args, "plan")?;
    let gate = args.next();
    reject_extra(args)?;
    let plan: CiRoutePlanV1 = read_json(&input)?;
    plan.explain(gate.as_deref()).map_err(CliError::Validate)
}

/// Typed CLI refusal vocabulary. Every failure mode names its kind; none
/// can be converted into a success verdict by a later artifact upload.
#[derive(Debug)]
enum CliError {
    Usage(String),
    MissingArgument(String),
    UnexpectedArgument(String),
    Read { path: String, source: io::Error },
    Parse { path: String, source: serde_json::Error },
    Compile(String),
    Validate(String),
    Publish(PublicationError),
}

impl From<PublicationError> for CliError {
    fn from(error: PublicationError) -> Self {
        CliError::Publish(error)
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Usage(usage) => write!(formatter, "usage: {usage}"),
            CliError::MissingArgument(subject) => {
                write!(formatter, "missing {subject} path")
            }
            CliError::UnexpectedArgument(extra) => {
                write!(formatter, "unexpected argument {extra:?}")
            }
            CliError::Read { path, source } => {
                write!(formatter, "read {path:?}: {source}")
            }
            CliError::Parse { path, source } => {
                write!(formatter, "parse {path:?}: {source}")
            }
            CliError::Compile(detail) => write!(formatter, "compile refused: {detail}"),
            CliError::Validate(detail) => write!(formatter, "validate refused: {detail}"),
            CliError::Publish(error) => write!(formatter, "publication refused: {error}"),
        }
    }
}

fn required_path(
    args: &mut impl Iterator<Item = String>,
    subject: &str,
) -> Result<String, CliError> {
    args.next().ok_or_else(|| CliError::MissingArgument(subject.to_string()))
}

fn reject_extra(mut args: impl Iterator<Item = String>) -> Result<(), CliError> {
    if let Some(extra) = args.next() {
        return Err(CliError::UnexpectedArgument(extra));
    }
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, CliError> {
    let bytes =
        fs::read(path).map_err(|source| CliError::Read { path: path.to_string(), source })?;
    serde_json::from_slice(&bytes)
        .map_err(|source| CliError::Parse { path: path.to_string(), source })
}

/// Typed publication failure vocabulary: one variant per contract step, in
/// pipeline order. No variant is convertible into success.
#[derive(Debug)]
enum PublicationError {
    CreateDirectory {
        dir: String,
        source: io::Error,
    },
    TempCreate {
        dir: String,
        source: io::Error,
    },
    Write {
        temp: PathBuf,
        source: io::Error,
    },
    Sync {
        temp: PathBuf,
        source: io::Error,
    },
    Rename {
        from: PathBuf,
        to: PathBuf,
        source: io::Error,
    },
    /// POSIX-only: the post-rename directory sync refused.
    #[cfg(unix)]
    DirSync {
        dir: String,
        source: io::Error,
    },
    ReadBack {
        path: PathBuf,
        source: io::Error,
    },
    ReadBackMismatch {
        path: PathBuf,
        expected_len: usize,
        found_len: usize,
    },
}

impl std::fmt::Display for PublicationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PublicationError::CreateDirectory { dir, source } => {
                write!(formatter, "create directory {dir:?}: {source}")
            }
            PublicationError::TempCreate { dir, source } => {
                write!(formatter, "create unique temporary file in {dir:?}: {source}")
            }
            PublicationError::Write { temp, source } => {
                write!(formatter, "write {}: {source}", temp.display())
            }
            PublicationError::Sync { temp, source } => {
                write!(formatter, "sync {}: {source}", temp.display())
            }
            PublicationError::Rename { from, to, source } => {
                write!(formatter, "atomic rename {} -> {}: {source}", from.display(), to.display())
            }
            #[cfg(unix)]
            PublicationError::DirSync { dir, source } => {
                write!(formatter, "sync directory {dir:?}: {source}")
            }
            PublicationError::ReadBack { path, source } => {
                write!(formatter, "read back {}: {source}", path.display())
            }
            PublicationError::ReadBackMismatch { path, expected_len, found_len } => {
                write!(
                    formatter,
                    "read-back mismatch at {}: expected {expected_len} canonical bytes, found \
                     {found_len}",
                    path.display()
                )
            }
        }
    }
}

/// Publish `bytes` to `path` under the single-writer durable contract
/// documented on this binary. Returns only after the destination contains
/// exactly `bytes`.
fn publish_atomically(path: &Path, bytes: &[u8]) -> Result<(), PublicationError> {
    let parent =
        path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|source| PublicationError::CreateDirectory {
        dir: parent.display().to_string(),
        source,
    })?;
    let (temp, file) = create_unique_temp(parent).map_err(|source| {
        PublicationError::TempCreate { dir: parent.display().to_string(), source }
    })?;
    if let Err(refusal) = write_and_verify_temp(&temp, bytes, file) {
        // Pre-promotion failure: nothing of this writer reached the
        // destination. Only the temporary is provably this writer's; any
        // file at the destination may be a concurrent publication's
        // completed artifact and must never be removed by a losing writer.
        return Err(cleanup_failed_publication(&temp, refusal));
    }
    match promote(&temp, path, bytes) {
        Ok(()) => Ok(()),
        // The rename never promoted this writer's bytes, so exactly like a
        // pre-promotion failure nothing at the destination is attributable
        // to this invocation: only the temporary is removed.
        Err(refusal @ PublicationError::Rename { .. }) => {
            Err(cleanup_failed_publication(&temp, refusal))
        }
        // Post-promotion failure (directory sync, final read-back): the
        // destination now holds either this writer's promoted bytes or a
        // concurrent writer's completed artifact. Removing another
        // publication would break the two-writer contract, so the
        // destination is left and the refusal says the artifact could not
        // be verified by this invocation.
        Err(refusal) => Err(refusal),
    }
}

/// Best-effort cleanup after a publication that never promoted this
/// writer's bytes: it removes only the temporary this writer provably
/// owns. The destination is never touched — before promotion this writer
/// cannot attribute a destination file to itself, and the file there may
/// be a concurrent publication's completed artifact, which a losing
/// writer must never delete.
fn cleanup_failed_publication(temp: &Path, refusal: PublicationError) -> PublicationError {
    drop(fs::remove_file(temp));
    refusal
}

/// Write, flush, sync, and verify the durable temporary: only fully
/// verified bytes are ever promoted.
fn write_and_verify_temp(
    temp: &Path,
    bytes: &[u8],
    mut file: File,
) -> Result<(), PublicationError> {
    file.write_all(bytes)
        .map_err(|source| PublicationError::Write { temp: temp.to_path_buf(), source })?;
    file.flush().map_err(|source| PublicationError::Write { temp: temp.to_path_buf(), source })?;
    file.sync_all()
        .map_err(|source| PublicationError::Sync { temp: temp.to_path_buf(), source })?;
    // Close the handle before the rename so the atomic publication cannot
    // race an open writer on any platform.
    drop(file);
    verify_published(temp, bytes)
}

/// Atomically promote the verified temporary to the final path, sync the
/// rename, and read the destination back: success is returned only after
/// the destination contains exactly the canonical bytes.
fn promote(temp: &Path, final_path: &Path, bytes: &[u8]) -> Result<(), PublicationError> {
    fs::rename(temp, final_path).map_err(|source| PublicationError::Rename {
        from: temp.to_path_buf(),
        to: final_path.to_path_buf(),
        source,
    })?;
    // POSIX durability: the rename's directory entry must itself be synced
    // before publication is reported, or a host crash can revert the name.
    // Windows is an explicit typed boundary: std cannot open directory
    // handles there and NTFS metadata journaling applies.
    sync_parent_directory(final_path)?;
    verify_published(final_path, bytes)
}

/// Sync the containing directory so the published rename survives a host
/// crash. Explicit platform boundary: a real (not accidental) no-op on
/// Windows.
#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), PublicationError> {
    let parent =
        path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let dir = File::open(parent).map_err(|source| PublicationError::DirSync {
        dir: parent.display().to_string(),
        source,
    })?;
    dir.sync_all()
        .map_err(|source| PublicationError::DirSync { dir: parent.display().to_string(), source })
}

#[cfg(windows)]
fn sync_parent_directory(_path: &Path) -> Result<(), PublicationError> {
    // Documented boundary: directory-handle fsync is unavailable through
    // std on Windows; the file-level sync plus atomic rename remain.
    Ok(())
}

/// Unique temporary file in `dir`, created with create-new semantics:
/// two concurrent writers can never hold the same temporary path. The
/// name embeds pid, nanosecond timestamp, and an in-process sequence.
fn create_unique_temp(dir: &Path) -> io::Result<(PathBuf, File)> {
    let pid = std::process::id();
    let mut sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    loop {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let candidate = dir.join(format!(".ci-route-plan.{pid}.{nanos}.{sequence}.tmp"));
        match File::create_new(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::SeqCst);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Read-back verification: the publication is successful only when the
/// final path contains exactly the canonical bytes.
fn verify_published(path: &Path, expected: &[u8]) -> Result<(), PublicationError> {
    let found = fs::read(path)
        .map_err(|source| PublicationError::ReadBack { path: path.to_path_buf(), source })?;
    if found != expected {
        return Err(PublicationError::ReadBackMismatch {
            path: path.to_path_buf(),
            expected_len: expected.len(),
            found_len: found.len(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Publication contract falsifiers
//
// One negative control per required publication control. These run with
// `cargo test -p xtask --bin ci-route-plan --locked` and use only
// cross-platform std filesystem behavior.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod publication_spec {
    use super::*;
    use std::sync::Barrier;
    use xtask::ci_route_plan::{
        Applicability, CompileRoutePlanInput, ExpansionStatus, GateSelectorInput,
        LifecycleDisposition, LifecycleState, PlannedOutcome, PolicyRole, Resolution,
        RouteDispositionInput, RouteExecutionIdentity, RouteProfileExpansionInput,
        RouteSelectionEvidence, RouteSubjectRef, SelectorPlacement, SelectorProof, SelectorRole,
    };

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const DIGEST_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn input(profile: &str) -> CompileRoutePlanInput {
        CompileRoutePlanInput {
            subject: RouteSubjectRef {
                kind: "pull_request".to_string(),
                head_sha: SHA_A.to_string(),
                base_sha: Some(SHA_B.to_string()),
                subject_digest: DIGEST_A.to_string(),
            },
            expansion: RouteProfileExpansionInput {
                requested_profile: profile.to_string(),
                included_native_tiers: vec!["pr_fast".to_string()],
                semantic_fingerprint: DIGEST_B.to_string(),
                policy_digest: DIGEST_C.to_string(),
                denominator: vec!["fmt_gate".to_string()],
                resolution: ExpansionStatus::Complete,
                detail: None,
            },
            dispositions: vec![RouteDispositionInput {
                gate_id: "fmt_gate".to_string(),
                policy_role: PolicyRole::Required,
                lifecycle: LifecycleDisposition {
                    state: LifecycleState::Active,
                    resolution: Resolution::Current,
                },
                native_tier: "pr_fast".to_string(),
                quarantine: None,
                detail: None,
            }],
            disposition_digest: DIGEST_B.to_string(),
            workflow_digest: DIGEST_C.to_string(),
            selectors: vec![GateSelectorInput {
                gate_id: "fmt_gate".to_string(),
                placement: SelectorPlacement::Selected,
                role: Some(SelectorRole::AlwaysOn),
                reason: "selected by selector".to_string(),
                proof: Some(SelectorProof::Applicable),
            }],
            selection: RouteSelectionEvidence {
                base: SHA_B.to_string(),
                scope_ok: true,
                fallback_used: false,
                fallback_reason: None,
                package_args: vec![],
                scope: None,
                selector_digest: DIGEST_A.to_string(),
            },
            execution: vec![RouteExecutionIdentity {
                gate_id: "fmt_gate".to_string(),
                command: "cargo fmt --check".to_string(),
                timeout_seconds: 300,
            }],
        }
    }

    fn compiled_bytes() -> Vec<u8> {
        CiRoutePlanV1::compile(input("merge_gate"))
            .expect("fixture compiles")
            .canonical_json()
            .expect("fixture encodes")
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "ci-route-plan-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn publication_writes_exact_canonical_bytes_and_cleans_temps() {
        let dir = temp_dir("success");
        let target = dir.join("plan.json");
        let bytes = compiled_bytes();
        publish_atomically(&target, &bytes).expect("publication succeeds");
        assert_eq!(fs::read(&target).expect("read back"), bytes);
        let residue: Vec<_> = fs::read_dir(&dir)
            .expect("list dir")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(residue, vec![std::ffi::OsString::from("plan.json")], "no temporary residue");
        drop(fs::remove_dir_all(&dir));
    }

    #[test]
    fn republish_replaces_previous_artifact_completely() {
        // Cross-platform overwrite: the second publication fully replaces
        // the first (std rename replaces on POSIX and Windows alike).
        let dir = temp_dir("overwrite");
        let target = dir.join("plan.json");
        let first = compiled_bytes();
        let second = CiRoutePlanV1::compile(input("commit"))
            .expect("second compiles")
            .canonical_json()
            .expect("second encodes");
        assert_ne!(first, second);
        publish_atomically(&target, &first).expect("first publication");
        publish_atomically(&target, &second).expect("second publication replaces");
        assert_eq!(fs::read(&target).expect("read back"), second);
        drop(fs::remove_dir_all(&dir));
    }

    #[test]
    fn concurrent_writers_never_interleave_a_final_artifact() {
        let dir = temp_dir("concurrent");
        let target = dir.join("plan.json");
        let left = compiled_bytes();
        let right = CiRoutePlanV1::compile(input("nightly"))
            .expect("right compiles")
            .canonical_json()
            .expect("right encodes");
        assert_ne!(left, right);
        let barrier = std::sync::Arc::new(Barrier::new(2));
        let left_path = target.clone();
        let left_barrier = barrier.clone();
        let left_payload = left.clone();
        let left_thread = std::thread::spawn(move || {
            left_barrier.wait();
            publish_atomically(&left_path, &left_payload)
        });
        let right_path = target.clone();
        let right_barrier = barrier.clone();
        let right_payload = right.clone();
        let right_thread = std::thread::spawn(move || {
            right_barrier.wait();
            publish_atomically(&right_path, &right_payload)
        });
        let left_result = left_thread.join().expect("left writer");
        let right_result = right_thread.join().expect("right writer");
        // The last writer to rename always reads back its own bytes, so at
        // least one publication succeeds. A writer that loses the read-back
        // race is a typed refusal — but it must never remove the winner's
        // completed artifact.
        let successes =
            [&left_result, &right_result].iter().filter(|result| result.is_ok()).count();
        assert!(
            successes >= 1,
            "at least one writer must publish: {left_result:?} {right_result:?}"
        );
        for result in [&left_result, &right_result] {
            if let Err(error) = result {
                assert!(
                    matches!(error, PublicationError::ReadBackMismatch { .. }),
                    "a lost race is a typed read-back refusal, got {error:?}"
                );
            }
        }
        let final_bytes = fs::read(&target).expect("final artifact exists");
        assert!(
            final_bytes == left || final_bytes == right,
            "final artifact must be exactly one writer's complete bytes"
        );
        drop(fs::remove_dir_all(&dir));
    }

    #[test]
    fn rename_failure_is_typed_and_leaves_no_artifact_or_residue() {
        // The final path occupied by a directory makes rename fail on both
        // platforms: the refusal must be typed, the temp cleaned, and no
        // file artifact left behind at the requested path.
        let dir = temp_dir("rename-failure");
        let target = dir.join("plan.json");
        fs::create_dir(&target).expect("directory occupies the target path");
        let bytes = compiled_bytes();
        let error = publish_atomically(&target, &bytes).expect_err("rename must fail");
        assert!(error.to_string().contains("atomic rename"), "{error}");
        let residue: Vec<_> = fs::read_dir(&dir)
            .expect("list dir")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(
            residue,
            vec![std::ffi::OsString::from("plan.json")],
            "no temporary residue after failure"
        );
        drop(fs::remove_dir_all(&dir));
    }

    #[test]
    fn directory_failure_is_typed() {
        // A file occupying the parent directory path makes directory
        // creation fail: typed refusal, no artifact.
        let dir = temp_dir("dir-failure");
        let blocker = dir.join("blocker");
        fs::write(&blocker, b"not a directory").expect("blocker");
        let target = blocker.join("nested").join("plan.json");
        let error = publish_atomically(&target, b"{}").expect_err("directory creation must fail");
        assert!(error.to_string().contains("create directory"), "{error}");
        drop(fs::remove_dir_all(&dir));
    }

    #[test]
    fn read_back_mismatch_is_refused() {
        let dir = temp_dir("readback");
        let target = dir.join("plan.json");
        publish_atomically(&target, b"{}").expect("publication succeeds");
        let error =
            verify_published(&target, b"differs").expect_err("mismatched read-back must refuse");
        assert!(error.to_string().contains("read-back mismatch"), "{error}");
        drop(fs::remove_dir_all(&dir));
    }

    #[test]
    fn pre_promotion_cleanup_removes_only_the_temporary() {
        // Corrected invariant: a refusal before promotion never deletes a
        // destination file this invocation cannot attribute to itself —
        // including a stale artifact from a previous subject. Consumers
        // must trust the typed refusal, never artifact presence/absence.
        let dir = temp_dir("stale-destination");
        let target = dir.join("plan.json");
        fs::write(&target, b"stale plan for another subject").expect("stale artifact");
        let temp = dir.join(".ci-route-plan.0.0.0.tmp");
        fs::write(&temp, b"partial").expect("temp artifact");
        let refusal = PublicationError::ReadBackMismatch {
            path: target.clone(),
            expected_len: 2,
            found_len: 7,
        };
        let returned = cleanup_failed_publication(&temp, refusal);
        assert!(
            matches!(returned, PublicationError::ReadBackMismatch { .. }),
            "the original refusal is preserved"
        );
        assert!(!temp.exists(), "temporary removed");
        assert!(target.is_file(), "a destination file is never removed by a failing writer");
        drop(fs::remove_dir_all(&dir));
    }

    #[test]
    fn pre_promotion_failure_never_removes_a_concurrent_writers_completed_artifact() {
        // Interleaving regression: writer B completes the destination,
        // then writer A hits a pre-promotion refusal (temp write, sync, or
        // temp read-back). A's cleanup must remove only A's temporary;
        // B's completed artifact must survive byte-for-byte.
        let dir = temp_dir("interleaved-pre-promotion");
        let target = dir.join("plan.json");
        let winner = compiled_bytes();
        publish_atomically(&target, &winner).expect("writer B completes first");
        // Writer A's pre-promotion failure: its own unique temporary plus
        // the typed refusal the write/sync/read-back step returns.
        let (temp, file) = create_unique_temp(&dir).expect("writer A temp");
        drop(file);
        let refusal = PublicationError::ReadBackMismatch {
            path: temp.clone(),
            expected_len: 0,
            found_len: 1,
        };
        let returned = cleanup_failed_publication(&temp, refusal);
        assert!(
            matches!(returned, PublicationError::ReadBackMismatch { .. }),
            "the original refusal is preserved: {returned:?}"
        );
        assert!(!temp.exists(), "the losing writer's temporary is removed");
        assert_eq!(
            fs::read(&target).expect("writer B's completed artifact survives"),
            winner,
            "a pre-promotion refusal must never delete a concurrent writer's completed artifact"
        );
        drop(fs::remove_dir_all(&dir));
    }

    #[test]
    fn explicit_null_optionals_are_refused_at_parse() {
        // The canonical contract spells absent optionals as omitted keys;
        // an explicit null is a second byte encoding of the same semantics
        // and must fail closed at the input adapter, not validate.
        let plan = CiRoutePlanV1::compile(input("merge_gate")).expect("compile");
        let mut payload = serde_json::to_value(&plan).expect("serialize");
        payload["subject"]["base_sha"] = serde_json::Value::Null;
        let error = serde_json::from_value::<CiRoutePlanV1>(payload)
            .expect_err("explicit null optional must refuse");
        assert!(error.to_string().contains("null"), "{error}");

        let mut compile_input = serde_json::to_value(&input("merge_gate")).expect("serialize");
        compile_input["expansion"]["detail"] = serde_json::Value::Null;
        let error = serde_json::from_value::<CompileRoutePlanInput>(compile_input)
            .expect_err("explicit null input optional must refuse");
        assert!(error.to_string().contains("null"), "{error}");
    }

    #[test]
    fn temp_names_are_unique_per_publication() {
        let dir = temp_dir("temp-names");
        let (first, _file_one) = create_unique_temp(&dir).expect("first temp");
        let (second, _file_two) = create_unique_temp(&dir).expect("second temp");
        assert_ne!(first, second, "two writers never share a temporary path");
        assert!(first.file_name().is_some_and(|name| name.to_string_lossy().ends_with(".tmp")));
        drop(fs::remove_dir_all(&dir));
    }

    // CLI-surface refusals (typed reasons for invalid profiles/inputs).

    #[test]
    fn compile_refuses_unknown_profile_with_typed_reason() {
        let mut payload = input("bogus_profile");
        payload.expansion.resolution = ExpansionStatus::Invalid;
        payload.expansion.detail = Some("unknown profile".to_string());
        let error = CiRoutePlanV1::compile(payload).expect_err("invalid profile must refuse");
        assert!(error.contains("not consumable"), "{error}");
    }

    #[test]
    fn compile_refuses_incomplete_expansion_with_typed_reason() {
        let mut payload = input("release");
        payload.expansion.resolution = ExpansionStatus::Unsupported;
        payload.expansion.detail = Some("no reviewed composition".to_string());
        let error = CiRoutePlanV1::compile(payload).expect_err("unsupported must refuse");
        assert!(error.contains("not consumable"), "{error}");
    }

    #[test]
    fn parse_refuses_unknown_input_fields() {
        let json = serde_json::to_string(&input("merge_gate")).expect("serialize");
        let tampered = json.replacen("\"workflow_digest\"", "\"legacy_workflow_digest\"", 1);
        let error = serde_json::from_str::<CompileRoutePlanInput>(&tampered)
            .expect_err("unknown input field must refuse");
        assert!(
            error.to_string().contains("unknown field"),
            "deny_unknown_fields refusal: {error}"
        );
    }

    #[test]
    fn baseline_row_shape_is_proof_backed_run() {
        // Fixture sanity for the falsifiers above: the compiled row is a
        // proof-backed run so `compiled_bytes` exercises the full payload.
        let plan = CiRoutePlanV1::compile(input("merge_gate")).expect("compile");
        assert_eq!(plan.rows.len(), 1);
        assert_eq!(plan.rows[0].applicability, Applicability::Applicable);
        assert!(matches!(&plan.rows[0].outcome, PlannedOutcome::Run { .. }));
    }
}
