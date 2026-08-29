//! Discriminating proof for the memoized `command_exists` tool probe.
//!
//! The counting-probe seam follows the injected-probe pattern established in
//! the perl-lsp-rs-core config module (#12945/#12978): the test injects a
//! probe through [`super::command_exists_via`] and observes how many times
//! the underlying probe executes. Each test uses a unique fabricated command
//! name so its cache key stays disjoint from every other test running in
//! parallel within the same process.

use std::cell::Cell;
use std::ffi::{OsStr, OsString};

use super::{
    CommandExistsCacheKey, command_exists_cache_key, command_exists_via, command_exists_via_key,
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

/// The cache key must separate entries by command and by the environment
/// inputs that can change a `which` answer, so an environment change
/// re-probes instead of serving a stale entry (keyed invalidation).
#[test]
fn cache_key_tracks_command_and_path_environment() {
    fn key(command: &str, path_env: &str, path_ext: Option<&str>) -> CommandExistsCacheKey {
        command_exists_cache_key(command, OsStr::new(path_env), path_ext.map(OsStr::new))
    }

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
    let key = command_exists_cache_key(command, directory.path().as_os_str(), None);
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
    let key = command_exists_cache_key(command, directory.path().as_os_str(), None);
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
    let key = command_exists_cache_key("perl_lsp_invalid_unicode", &invalid_path, None);
    assert_eq!(key.path_env, invalid_path);
}

#[cfg(windows)]
#[test]
fn cache_key_preserves_invalid_unicode_path_environment() {
    use std::os::windows::ffi::OsStringExt;

    let invalid_path = OsString::from_wide(&[b'C' as u16, b':' as u16, b'\\' as u16, 0xd800]);
    let key = command_exists_cache_key("perl_lsp_invalid_unicode", &invalid_path, None);
    assert_eq!(key.path_env, invalid_path);
}
