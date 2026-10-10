# Acceptance and falsifiers

This prerequisite is complete when the real pinned driver's bounded dispatch is
observable, retained refusal is tested, the proposed finite canonical route has a
renderer decision table and authority map, and all missing production qualification
is named. It does not
close #3230, #11660, #11663 or any upstream executor issue.

| ID | Cheapest falsifier/control | Current evidence |
| --- | --- | --- |
| C01 | allowlist Clippy/aliases/external commands in storage | production entrypoint refuses before resource planning/spawn |
| C02 | admit path/config/job override | existing validator negative controls retained |
| C03 | drop literal `clippy` token | real driver drops first Cargo argument; exact dispatch oracle detects mismatch |
| C04 | put Cargo global config before token | real driver misroutes config text as child argument |
| C05 | use ambient Cargo instead of bound identity | no `CARGO` uses controlled ambient PATH spy |
| C06 | lose package/profile/lints/private target/build | exact child argv/environment spy with Unicode/space paths |
| C07 | treat `--fix` as proof | real driver maps to mutating `fix`; route must exclude it |
| C08 | treat version-success as product | actual zero-child version control |
| C09 | flatten child failures to pass | 0/101/17 propagation controls, no product/tree claim |
| C10 | forward no-deps to Cargo | real driver separates input into CLIPPY_ARGS |

Still required before production enablement, through accepted owners:

- #11642 model and #11647/#11650/#11653/#11659 APIs accepted, then #11660 composed;
- exact provenance/host/target and missing/wrong/tampered executable refusals;
- typed renderer rejects unknown flags, aliases, fixes, lint encoding injection,
  conflicting Cargo `[env]`, config, wrappers and executable selectors;
- deliberate Clippy-only failure + clean package control (ordinary check cannot pass
  as Clippy), exact selected-work observation, both private paths and actual volumes;
- contention, cancellation at every phase, descendants after parent exit, abnormal
  status/timeout, retained ambiguous lease and release postcondition;
- cold/warm matched-subject and cross-worktree parity under #11663, no fewer-work gain;
- native Windows/MSVC and supported POSIX cells separately, spaces/Unicode paths;
- exact hook/resolver candidate Clippy only after route qualification and a conservative
  workload budget covering all actual destination volumes; native Windows ongoing
  storage evidence elsewhere does not fill this Clippy cell.

Stop on missing accepted authorities, wrong executable/source identity, a need to
weaken capacity, or any change to the storage writer's branch. Return to the
semantic owner rather than creating a Python executor or bypass route.
