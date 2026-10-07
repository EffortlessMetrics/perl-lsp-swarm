# Graded agentic task-completion battery

Eight real-repo workflow tasks (clean build, focused test, bench+baseline,
bisect, draft PR, hygiene gate, owner-docs lookup, CI-failure repro), each
with a machine-readable spec and a step-by-step point rubric. The runner
executes every automatable rubric step and prints a per-task score table.

## Layout

- `tasks/task-0N-*.toml` — one spec per task: goal, starting state,
  instruction given to the agent, allowed commands, point rubric, pass
  threshold (6/10), max duration, most-likely failure mode.
- `run-battery.py` — stdlib-only (Python 3.11+) runner. No third-party deps.
- `baselines/` — measured reference runs pinned to a base SHA.

## Usage

```bash
# Validate specs without running anything
python3 tests/agentic-task-battery/run-battery.py --check

# Full battery (executes builds, tests, benches, gates)
python3 tests/agentic-task-battery/run-battery.py --results-out /tmp/results.toml

# One task, with grader params
python3 tests/agentic-task-battery/run-battery.py --task task-04-bisect \
  --param failing-test='perl-token fmt::tests::x' --param good-sha=<sha>
python3 tests/agentic-task-battery/run-battery.py --task task-06-hygiene-gate \
  --param change-type=must-migration --param pkg=perl-token
python3 tests/agentic-task-battery/run-battery.py --task task-07-owner-docs \
  --param file-path=crates/perl-dap/src/lib.rs
```

Exit codes: 0 = measurement complete (step failures are data, not runner
errors); 1 = runner error; 2 = spec validation failure.

## Honesty rules (load-bearing)

- `mode = "auto"` steps really execute their check. `mode = "manual"` steps
  are never scored by the runner: they print as MANUAL with the exact
  verification command and what to record.
- Tasks needing live GitHub/CI (`task-05`, `task-08`) are `manual-only`.
  The runner never opens, edits, merges, or re-triggers anything remote.
- Tasks needing a grader instance (`task-04`) degrade to NEEDS-PARAM when
  `--param` values are absent. Task 4 additionally refuses to bisect a dirty
  tree and always resets (verified post-condition, not just the command).
- Write-side recipes (`status-update`, `public-api-update`, `bench-baseline`)
  are excluded from automated gate runs; the specs name them as MANUAL steps
  gated on a human finding drift first.
- `just` recipes run via Git Bash on Windows (CONTRIBUTING: recipes are bash;
  bare PowerShell lacks `cygpath`). Where the task instruction names a
  direct-cargo fallback, the runner takes it and says so.
- Child output is never truncated from a live pipe: every child runs to
  completion via `communicate()` into a temp log, which is then parsed.
  Temp logs live outside the repo; the only repo paths a run writes are
  gitignored (`target/`, `benchmarks/results/`). Known exception handled
  explicitly: `bench-run` rewrites the tracked generated file
  `docs/project/status/parser_performance_scorecard.json` with local
  numbers; the runner snapshots tracked status before the bench step and
  restores exactly the files that step dirtied (recorded in the step
  evidence), so the tree is clean afterward.

## Spec schema (`agentic_task_battery.v1`)

Top level: `schema_version`, `task_id`, `title`, `goal`, `starting_state`,
`instruction`, `max_duration`, `pass_threshold`, `max_points`,
`failure_mode`, `automation` (`full`|`partial`|`manual-only`),
`allowed_commands`, optional `[params]` (grader-supplied, with `name =
"description"`), plus task-specific tables (`[gate_table]` on task 6,
`[local_equivalents]` on task 8).

Each `[[rubric]]` step: `step_id`, `description`, `points`, `mode`. Auto
steps add `check` (a key the runner implements — see `KNOWN_CHECKS` in
`run-battery.py`). Manual steps add `verify` (exact verification command)
and `evidence` (what the grader records). Rubric points must sum to
`max_points`; `--check` enforces this and the rest of the schema.

## Grading agents with this battery

1. Give the agent the task `instruction` + `starting_state` only (not the rubric).
2. Score each rubric step from the agent's transcript and artifacts, using the
   spec's `verify` commands for manual steps.
3. Compare against `baselines/` for the same base SHA: a reference run that
   scores below a baseline on automatable steps is a regression in the
   environment or the battery, not in the agent.
