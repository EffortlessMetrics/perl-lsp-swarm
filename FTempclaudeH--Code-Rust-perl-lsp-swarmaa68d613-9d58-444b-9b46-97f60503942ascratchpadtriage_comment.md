<!-- issue-triage-research 2249 mode:already-fixed -->

## Current state

**Issue #2249 REQUEST_TIMEOUT fix is DONE.**

Checking origin/main HEAD (commit 25eaca807):
- `crates/perl-lsp-ux-tests/tests/ux_scenario_23_rename_workflow.rs:31-33`: Contains function `fn request_timeout(config: &ScenarioConfig) -> std::time::Duration { config.timeout }` ✓ 
- Replaces the original hardcoded `const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);` 
- All 3 rename calls (prepareRename, rename) now use `request_timeout(&config)` which respects `ScenarioConfig::default()` timeout (30s by default)

## Verification

**Fix was delivered in**: PR #3208 "test(ux): raise scenario_23 request timeout 10s→30s (#2860)"
- PR state: MERGED
- Commit: d080d8b35 on main-check branch

**Behavioral change**: 
- scenario_23 rename workflow now uses 30s default timeout (or custom via ScenarioConfig)
- Test `scenario_23_request_timeout_comes_from_scenario_config()` verifies the timeout is configurable

## Scope + plan

**Non-goals**: Scenarios 25, 26, 27 still have hardcoded 10s timeouts — those are separate issues (issue #2860 tracks these as a follow-up).

## Next-state triage

✓ **DONE-ON-MAIN** — Issue can be closed as completed. The fix is live on origin/main, and all three rename requests in scenario_23 now properly respect the ScenarioConfig timeout.

