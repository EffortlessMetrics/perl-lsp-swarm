//! Discriminating proof for the memoized `command_exists` tool probe.
//!
//! The counting-probe seam follows the injected-probe pattern established in
//! the perl-lsp-rs-core config module (#12945/#12978): the test injects a
//! probe through [`super::command_exists_via`] and observes how many times
//! the underlying probe executes. Each test uses a unique fabricated command
//! name so its cache key stays disjoint from every other test running in
//! parallel within the same process.

use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::{
    CommandExistsCacheEntry, CommandExistsCacheKey, MAX_COMMAND_EXISTS_CACHE_ENTRIES,
    command_exists_cache_key, command_exists_candidate_paths, command_exists_via,
    command_exists_via_key, insert_bounded,
};

/// The second lookup with an unchanged environment must be served from the
/// cache, not re-execute the underlying probe. This is the load-bearing
/// behavior claim: repeated initialize-time / diagnostics-time tool detection
/// stops re-walking PATH.
#[test]
fn second_command_exists_lookup_does_not_reexecute_probe() {
    const COMMAND: &str = "perl_lsp_test_counting_probe_tool_unique_xyz";

    let probe_runs = Cell::new(0u32);
    let probe = |command: &str| {
        probe_runs.set(probe_runs.get() + 1);
        assert_eq!(command, COMMAND, "probe must only see its own command");
        true
    };

    let first = command_exists_via(probe, COMMAND);
    assert!(first, "injected probe should answer true on the cold lookup");
    assert_eq!(probe_runs.get(), 1, "cold lookup must execute the probe exactly once");

    let second = command_exists_via(probe, COMMAND);
    assert!(second, "memoized answer must match the probe result");
    assert_eq!(
        probe_runs.get(),
        1,
        "second lookup with unchanged environment must not re-execute the probe"
    );
}

/// A missing-tool answer must be memoized too — this is the case the
/// cold-start discriminator measured amplifying under AV pressure (a 767 ms
/// single `which` spike repeated on every diagnostics-cycle guard).
#[test]
fn negative_probe_result_is_memoized() {
    const COMMAND: &str = "perl_lsp_test_negative_probe_tool_unique_xyz";

    let probe_runs = Cell::new(0u32);
    let probe = |_command: &str| {
        probe_runs.set(probe_runs.get() + 1);
        false
    };

    let first = command_exists_via(probe, COMMAND);
    let second = command_exists_via(probe, COMMAND);
    assert!(!first && !second, "missing-tool answers must stay consistent");
    assert_eq!(
        probe_runs.get(),
        1,
        "a missing-tool answer must also be served from the cache on re-lookup"
    );
}

/// Fixed working directory for hermetic key construction: [`command_exists_via_key`]
/// takes explicit keys, so no test mutates process-global state to prove key
/// sensitivity.
const TEST_CWD: &str = "/test/workdir";

fn key(command: &str, path_env: &str, path_ext: Option<&str>) -> CommandExistsCacheKey {
    command_exists_cache_key(
        command,
        true,
        OsStr::new(path_env),
        Path::new(TEST_CWD),
        path_ext.map(OsStr::new),
    )
}

/// The cache key must separate entries by command and by the environment
/// inputs that can change a `which` answer, so an environment change
/// re-probes instead of serving a stale entry (keyed invalidation).
#[test]
fn cache_key_tracks_command_and_path_environment() {
    let base = key("perltidy", "/usr/bin:/bin", None);
    let same = key("perltidy", "/usr/bin:/bin", None);
    assert_eq!(base, same, "identical inputs must share one cache entry");

    let other_command = key("perlcritic", "/usr/bin:/bin", None);
    assert_ne!(base, other_command, "different commands must not share entries");

    let other_path = key("perltidy", "/opt/tools/bin", None);
    assert_ne!(base, other_path, "a PATH change must invalidate the entry");

    #[cfg(windows)]
    {
        let other_ext = key("perltidy", "/usr/bin:/bin", Some(".BAT"));
        assert_ne!(base, other_ext, "a PATHEXT change must invalidate the entry on Windows");
    }
    #[cfg(not(windows))]
    {
        let ignored_ext = key("perltidy", "/usr/bin:/bin", Some(".BAT"));
        assert_eq!(base, ignored_ext, "PATHEXT must not participate off-Windows");
    }
}

/// Live wiring through the public `command_exists`: a fabricated command is
/// absent. The cache-hit guarantee is carried by the injected counting-probe
/// test above; this is only a production-probe smoke test.
#[test]
fn real_command_exists_returns_false_for_fabricated_command() {
    const COMMAND: &str = "perl_lsp_test_definitely_missing_tool_unique_xyz";

    let first = super::command_exists(COMMAND);
    let second = super::command_exists(COMMAND);
    assert!(!first && !second, "a fabricated command must be absent on both lookups");
}

#[test]
fn filesystem_creation_reprobes_a_cached_negative_answer() {
    let directory = tempfile::tempdir().expect("temporary directory must be created");
    let command = "perl_lsp_test_filesystem_create_unique_xyz";
    let key = command_exists_cache_key(
        command,
        true,
        directory.path().as_os_str(),
        directory.path(),
        None,
    );
    let probe_runs = Cell::new(0u32);
    let probe = |_command: &str| {
        probe_runs.set(probe_runs.get() + 1);
        false
    };

    assert!(!command_exists_via_key(probe, key.clone()));
    assert_eq!(probe_runs.get(), 1, "cold negative lookup must probe once");

    std::fs::write(directory.path().join(command), b"tool")
        .expect("candidate file must be created");
    assert!(!command_exists_via_key(probe, key));
    assert_eq!(
        probe_runs.get(),
        2,
        "creating a candidate in an unchanged PATH directory must invalidate a negative answer"
    );
}

#[test]
fn filesystem_removal_reprobes_a_cached_positive_answer() {
    let directory = tempfile::tempdir().expect("temporary directory must be created");
    let command = "perl_lsp_test_filesystem_remove_unique_xyz";
    let candidate = directory.path().join(command);
    std::fs::write(&candidate, b"tool").expect("candidate file must be created");
    let key = command_exists_cache_key(
        command,
        true,
        directory.path().as_os_str(),
        directory.path(),
        None,
    );
    let probe_runs = Cell::new(0u32);
    let probe = |_command: &str| {
        probe_runs.set(probe_runs.get() + 1);
        true
    };

    assert!(command_exists_via_key(probe, key.clone()));
    assert_eq!(probe_runs.get(), 1, "cold positive lookup must probe once");

    std::fs::remove_file(candidate).expect("candidate file must be removed");
    assert!(command_exists_via_key(probe, key));
    assert_eq!(
        probe_runs.get(),
        2,
        "removing a candidate from an unchanged PATH directory must invalidate a positive answer"
    );
}

#[cfg(unix)]
#[test]
fn cache_key_preserves_invalid_unicode_path_environment() {
    use std::os::unix::ffi::OsStringExt;

    let invalid_path = OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]);
    let key = command_exists_cache_key(
        "perl_lsp_invalid_unicode",
        true,
        &invalid_path,
        Path::new(TEST_CWD),
        None,
    );
    assert_eq!(key.path_env, invalid_path);
}

#[cfg(windows)]
#[test]
fn cache_key_preserves_invalid_unicode_path_environment() {
    use std::os::windows::ffi::OsStringExt;

    let invalid_path = OsString::from_wide(&[b'C' as u16, b':' as u16, b'\\' as u16, 0xd800]);
    let key = command_exists_cache_key(
        "perl_lsp_invalid_unicode",
        true,
        &invalid_path,
        Path::new(TEST_CWD),
        None,
    );
    assert_eq!(key.path_env, invalid_path);
}

/// A missing `PATH` and an explicitly empty `PATH` must never share an entry:
/// the probe delegates a missing `PATH` to ambient `which` but fails a
/// present-but-empty `PATH` closed. Structural inequality plus a behavioral
/// no-alias check in both transition directions.
#[test]
fn path_absence_and_empty_path_do_not_share_entries() {
    const COMMAND: &str = "perl_lsp_test_path_presence_split_unique_xyz";
    let cwd = Path::new(TEST_CWD);

    let absent = command_exists_cache_key(COMMAND, false, OsStr::new(""), cwd, None);
    let empty = command_exists_cache_key(COMMAND, true, OsStr::new(""), cwd, None);
    assert_ne!(absent, empty, "missing PATH and empty PATH must key different entries");

    let probe_runs = Cell::new(0u32);
    let probe = |_command: &str| {
        probe_runs.set(probe_runs.get() + 1);
        true
    };
    assert!(command_exists_via_key(probe, empty.clone()));
    assert_eq!(probe_runs.get(), 1, "cold lookup must probe once");
    assert!(command_exists_via_key(probe, absent.clone()));
    assert_eq!(
        probe_runs.get(),
        2,
        "missing PATH must re-probe rather than reuse the empty-PATH entry"
    );
    assert!(command_exists_via_key(probe, empty));
    assert_eq!(probe_runs.get(), 2, "returning to empty PATH must hit its own entry, not re-probe");
}

/// A `cwd` change must invalidate the entry: `which` resolves relative or
/// separator-containing command text against the working directory, so the
/// old directory's answer must never serve the new one.
#[test]
fn path_containing_command_reprobes_after_working_directory_change() {
    const COMMAND: &str = "perl_lsp_test_cwd_split_unique_xyz";
    let path = OsStr::new("/usr/bin:/bin");

    let here = command_exists_cache_key(COMMAND, true, path, Path::new("/work/here"), None);
    let there = command_exists_cache_key(COMMAND, true, path, Path::new("/work/there"), None);
    assert_ne!(here, there, "a cwd change must key a different entry");

    let probe_runs = Cell::new(0u32);
    let probe = |_command: &str| {
        probe_runs.set(probe_runs.get() + 1);
        true
    };
    assert!(command_exists_via_key(probe, here));
    assert_eq!(probe_runs.get(), 1, "cold lookup must probe once");
    assert!(command_exists_via_key(probe, there));
    assert_eq!(
        probe_runs.get(),
        2,
        "lookup from another directory must re-probe rather than reuse the first directory's answer"
    );
}

/// Class-level falsifier for cache identity: every identity dimension the key
/// claims (command, PATH presence, PATH value, working directory) must
/// separate entries, so dropping any one dimension fails this test.
#[test]
fn cache_key_distinguishes_full_identity() {
    const COMMAND: &str = "perl_lsp_test_identity_grid_unique_xyz";

    let states = [(false, ""), (true, ""), (true, "/usr/bin:/bin")];
    let cwds = ["/work/a", "/work/b"];
    let mut keys = Vec::new();
    for (present, path) in states {
        for cwd in cwds {
            keys.push(command_exists_cache_key(
                COMMAND,
                present,
                OsStr::new(path),
                Path::new(cwd),
                None,
            ));
        }
    }
    for (index, left) in keys.iter().enumerate() {
        for right in &keys[index + 1..] {
            assert_ne!(
                left, right,
                "every (PATH presence, PATH value, cwd) combination must key a distinct entry"
            );
        }
    }
    let again = command_exists_cache_key(
        COMMAND,
        true,
        OsStr::new("/usr/bin:/bin"),
        Path::new("/work/a"),
        None,
    );
    assert_eq!(
        keys.into_iter().find(|key| key.path_present
            && key.path_env == "/usr/bin:/bin"
            && key.cwd == Path::new("/work/a")),
        Some(again),
        "identical identity inputs must share one cache entry"
    );
}

/// The process-lifetime map is hard-bounded: inserting past capacity evicts
/// instead of growing. Exercised against a local map (never the shared
/// static) so the test is deterministic under parallel execution.
#[test]
fn cache_insert_stays_bounded_over_capacity() {
    fn entry() -> CommandExistsCacheEntry {
        CommandExistsCacheEntry { result: true, filesystem_state: Vec::new() }
    }
    fn key_for(index: usize) -> CommandExistsCacheKey {
        command_exists_cache_key(
            &format!("perl_lsp_test_capacity_tool_{index}_unique_xyz"),
            true,
            OsStr::new("/usr/bin:/bin"),
            Path::new(TEST_CWD),
            None,
        )
    }

    let mut cache = HashMap::new();
    for index in 0..MAX_COMMAND_EXISTS_CACHE_ENTRIES {
        insert_bounded(&mut cache, key_for(index), entry());
    }
    assert_eq!(cache.len(), MAX_COMMAND_EXISTS_CACHE_ENTRIES, "map must fill exactly to capacity");

    let overflow = key_for(MAX_COMMAND_EXISTS_CACHE_ENTRIES);
    insert_bounded(&mut cache, overflow.clone(), entry());
    assert_eq!(
        cache.len(),
        MAX_COMMAND_EXISTS_CACHE_ENTRIES,
        "inserting past capacity must evict instead of growing"
    );
    assert!(cache.contains_key(&overflow), "the newest entry must survive its own insertion");

    insert_bounded(&mut cache, overflow, entry());
    assert_eq!(
        cache.len(),
        MAX_COMMAND_EXISTS_CACHE_ENTRIES,
        "refreshing a present key must not evict"
    );
}

/// The validation walk covers exactly one candidate per PATH entry (plus
/// PATHEXT expansions on Windows): this pins the per-hit walk surface the
/// memoization claim prices against.
#[test]
fn candidate_paths_cover_each_path_entry_once() {
    const COMMAND: &str = "perl_lsp_test_walk_surface_unique_xyz";
    let key = command_exists_cache_key(
        COMMAND,
        true,
        OsStr::new("/x/bin:/y/bin"),
        Path::new(TEST_CWD),
        None,
    );
    let paths = command_exists_candidate_paths(&key);
    #[cfg(not(windows))]
    assert_eq!(
        paths,
        vec![PathBuf::from("/x/bin").join(COMMAND), PathBuf::from("/y/bin").join(COMMAND),],
        "each PATH entry must contribute exactly one candidate off-Windows"
    );
    #[cfg(windows)]
    {
        let _ = paths;
        let ext_key = command_exists_cache_key(
            COMMAND,
            true,
            OsStr::new("/x/bin"),
            Path::new(TEST_CWD),
            Some(OsStr::new(".EXE")),
        );
        let ext_paths = command_exists_candidate_paths(&ext_key);
        assert_eq!(
            ext_paths,
            vec![
                PathBuf::from("/x/bin").join(COMMAND),
                PathBuf::from("/x/bin").join(format!("{COMMAND}.EXE")),
            ],
            "each PATH entry must contribute the bare candidate plus one PATHEXT expansion on Windows"
        );
    }
}
