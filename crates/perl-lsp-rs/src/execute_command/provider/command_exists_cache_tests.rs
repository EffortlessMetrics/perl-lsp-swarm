//! Discriminating proof for the memoized `command_exists` tool probe.
//!
//! The counting-probe seam follows the injected-probe pattern established in
//! the perl-lsp-rs-core config module (#12945/#12978): the test injects a
//! probe through [`super::command_exists_via`] and observes how many times
//! the underlying probe executes. Each test uses a unique fabricated command
//! name so its cache key stays disjoint from every other test running in
//! parallel within the same process.

use std::cell::Cell;

use super::{CommandExistsCacheKey, command_exists_cache_key, command_exists_via};

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
        command_exists_cache_key(command, path_env, path_ext)
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
/// absent, and the answer is stable across repeated lookups (cache-hit path
/// exercised for real through the production probe closure).
#[test]
fn real_command_exists_is_stable_for_a_fabricated_command() {
    const COMMAND: &str = "perl_lsp_test_definitely_missing_tool_unique_xyz";

    let first = super::command_exists(COMMAND);
    let second = super::command_exists(COMMAND);
    assert!(!first && !second, "a fabricated command must be absent on both lookups");
}
