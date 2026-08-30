# Acceptance: #11633 — typed writer-preflight subject and decision core

Every item is proven by the focused suite
`cargo test -p xtask --lib --locked writer_preflight` (45 tests after the
#12059 review repair) unless a different proof is named. Falsifier numbers
refer to #11633's shift-left list.

- [x] One pure typed domain owns writer-preflight subjects, observations,
  decisions, and reasons (`xtask/src/writer_preflight/`; no I/O imports;
  purity auditable by reading the module).
- [x] Read-only/create/resume/mutate requirements are mechanically distinct
  (per-operation evaluation paths in `decide`; operation participates in
  every rule — falsifier 2).
- [x] Required uncertainty cannot become `PASS` (falsifiers 1 and 10:
  stale/unavailable/absent required facts → NOT_PROVEN with
  `provider_unavailable_or_stale` or `base_or_remote_not_proven`).
- [x] Ambient and executor-owned Cargo configuration remain distinct
  (separate facts and reasons; falsifiers 6 and 7 both directions).
- [x] Collision, unique-state, repository/base/candidate, and selected-
  capacity hazards are explicit blocking reasons (falsifiers 3–5;
  unknown-mutation-subject; capacity selected-vs-unselected).
- [x] Behind-only and unrelated load remain non-authorizing advisories
  rather than blanket blocks (falsifiers 8 and 9; shared stash likewise).
- [x] Human/JSON/explain projections derive from one decision object and
  agree (tokens identical to serde forms; round-trip equality; falsifier
  13).
- [x] Deterministic fixtures cover every outcome and reason family; input
  ordering preserves decision identity (digest equality under reversal;
  falsifier 12); unknown variants rejected at deserialization (falsifier
  14); machine path conventions stay out of policy (falsifier 11).
- [x] No live Git/filesystem/process/network call, worktree mutation,
  command adapter, or compatibility retirement enters this PR (module has
  no I/O surface; scripts/Just untouched).

Proof commands:

```bash
cargo fmt -p xtask -- --check
cargo test -p xtask --lib --locked writer_preflight   # 45 passed
cargo clippy -p xtask --lib --locked -- -D warnings
```

Known boundaries (honest): this PR proves the domain core only. Live
evidence collection (#11634), mutation consumption (#11635), parity/race
proof on real hosts (#11636), and deprecation decisions (#9569/#9576)
remain unproven here and are NOT_PROVEN by construction.
