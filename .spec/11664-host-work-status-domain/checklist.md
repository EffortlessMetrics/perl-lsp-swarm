# Checklist — typed host work status domain (#11664)

## Falsifier IDs → proof map

| ID | Falsifier | Test |
| --- | --- | --- |
| F1 | Agent/lane return makes compute terminal | `host_work_status_f1_agent_return_not_terminal` |
| F2 | Parent exit makes descendants/lock/reservation terminal | `host_work_status_f2_parent_exit_not_terminal` |
| F3 | Process API unavailable becomes zero processes | `host_work_status_f3_api_unavailable_is_not_proven` |
| F4 | Age alone creates `ORPHAN_CANDIDATE` | `host_work_status_f4_age_alone_never_orphan` |
| F5 | Executable name/path attributes another repository's process | `host_work_status_f5_basename_attribution_rejected` |
| F6 | Count-based universal dispatch denial | `host_work_status_f6_no_dispatch_verdict` |
| F7 | Open PR forces physical KEEP despite reconstructible clean state | `host_work_status_f7_open_pr_allows_reconstructible_terminal` |
| F8 | Closed issue makes unique dirty work removable | `host_work_status_f8_closed_issue_keeps_salvage` |
| F9 | Shared cache state becomes candidate authority | `host_work_status_f9_shared_cache_not_candidate_authority` |
| F10 | Dimensions collapse into one status | `host_work_status_f10_dimensions_stay_independent` |
| F11 | Provider exit zero overrides typed blocked/unknown state | `host_work_status_f11_exit_zero_cannot_override_typed_fact` |
| F12 | Provider human wording parsed as authority | `host_work_status_f12_no_prose_parsing_surface` (type-level: adapters take enums only) |
| F13 | Unknown provider variant disappears | `host_work_status_f13_unknown_variant_visible` |
| F14 | One subject's resource satisfies another | `host_work_status_f14_subject_mismatch_unrepresentable` |
| F15 | Aggregate HEALTHY hides ambiguous dimension | `host_work_status_f15_healthy_never_hides_uncertainty` |
| F16 | Cleanup eligibility represented as cleanup authorization | `host_work_status_f16_readiness_is_not_authorization` |
| F17 | Input ordering changes semantic status identity | `host_work_status_f17_ordering_does_not_change_identity` |
| F18 | PR performs live observation or mutation | review map + module imports (no `std::process`/fs/net in `host_work_status/`) |

## Review map

- Review A (false terminal/orphan/healthy from weak evidence): falsifiers F1–F5, F15.
- Review B (scheduler/cleanup authority/duplicated provider fact hidden in model):
  F6, F16, F18 + adapter contract audit.

## Rollback

Single-module revert: delete `xtask/src/host_work_status/`, drop the two `lib.rs`/
`mod.rs` lines, remove `.spec/11664-host-work-status-domain/`. No other surface
changes; no data migration.

## Successor handoffs

- #11666 consumes `HostWorkObservationSet` constructors to publish live rows.
- #11667/#11669 consume cleanup-readiness fields as plan inputs.
- #10256/#10263 keep worktree cleanup ownership; this domain only reports readiness.
- #11650/#11653/#11659/#11661 owners replace the declared-missing providers with
  typed adapters over their landed schemas.
