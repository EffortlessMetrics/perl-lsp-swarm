# Acceptance — typed host work status domain (#11664)

## Lifecycle / reason truth table

A dimension classification is `(lifecycle, reasons[])`. Laws are mechanical; each row
names the enforcing function.

| Evidence supplied | Resulting lifecycle | Enforced by |
| --- | --- | --- |
| Active mutation owner or capacity reservation or owned live process tree | `ACTIVE` | `classify_dimension` |
| Exact operation queued for capacity, no product work started | `QUEUED` | `classify_dimension` |
| Initiator returned / cancellation began AND ≥1 unsettled descendant/reservation/lock/output/terminality fact | `STOPPING` (persists until every fact settles) | `ComputeWorkObservation` settlement set + `classify_dimension` |
| Durable remote GitHub/integration wait, no proven local mutation/compute | `REMOTE_IN_FLIGHT` | `LogicalWorkObservation` |
| Positive current ownership + targetability + fully observable cleanup premises | `ORPHAN_CANDIDATE` (only via explicit `OrphanPremises` with all fields proven) | `classify_dimension` orphan gate |
| Ownership/subject/ancestry/currentness/uniqueness/targetability incomplete or contradictory | `AMBIGUOUS` | `classify_dimension`, contradiction detector |
| No remaining ownership; local state retained/reconstructible/reconciled | `TERMINAL` | `classify_dimension` terminal gate |

Negative laws (each has a falsifier test):

1. Agent/lane return alone ⇒ never compute `TERMINAL`; return without settled facts ⇒
   `STOPPING`. (F1)
2. Parent exit alone ⇒ never descendant `TERMINAL`. (F2)
3. Process API unavailable ⇒ compute row `NOT_PROVEN`-backed, never zero-processes
   `TERMINAL`. (F3)
4. Age alone ⇒ never `ORPHAN_CANDIDATE`; missing positive ownership evidence ⇒
   `AMBIGUOUS`. (F4)
5. Executable name/path resemblance without exact subject binding ⇒ attribution
   `UNATTRIBUTED`, dimension `AMBIGUOUS`; it cannot satisfy another subject. (F5, F14)
6. Aggregate never returns a dispatch verdict; only observation tokens. (F6)
7. Open PR + clean pushed worktree may classify reconstructible (`TERMINAL` disposition
   allowed); open PR does not force physical retention. (F7)
8. Closed issue + dirty/unpushed/unique state ⇒ salvage-required stays. (F8)
9. Shared cache facts never establish candidate/product completion. (F9)
10. Dimensions never collapse: aggregate is computed from four independent
    classifications. (F10)
11. Provider instrument failure/exit-zero cannot override a typed blocked/not-proven
    fact. (F11)
12. Adapters consume typed structs only; no prose parsing surface exists. (F12)
13. Unknown provider variants surface as visible rows (`UNKNOWN_PROVIDER_VARIANT`,
    lifecycle `AMBIGUOUS`), never dropped. (F13)
15. Any ambiguous/not-proven required dimension forces the aggregate to carry
    `AMBIGUOUS`/`NOT_PROVEN`; `HEALTHY` requires every dimension decided and benign.
    (F15)
16. Cleanup-readiness fields are descriptive; no field implies authorization to run a
    plan. (F16)
17. Canonical ordering: status identity is independent of input insertion order
    (canonical sort before compare/serialize). (F17)

## Provider adapter contract

Each adapter: names provider/schema version, binds the exact subject key, carries
observation identity/currentness, consumes the provider-owned typed result, records
instrument availability and limitations, and maps exhaustively into dimension rows.
Adapters shipped over landed typed owners:

- `worktree_plan_v1` (#10256/#10263 `WorktreeCleanupPlan`) → logical/mutation/storage;
- `admission_report_v1` (#3957/#11617 writer-admission report) → logical/mutation/
  storage incl. collision + disk floor;
- generic typed inputs for future #11650/#11653/#11659 owners (reservation/process/
  executor-storage) whose providers are declared `Missing` until they land.

## Aggregate observation semantics

Severity order (ascending): `HEALTHY < NOT_PROVEN < AMBIGUOUS < LOW_DISK < SATURATED <
COLLISION < SALVAGE_REQUIRED`. The aggregate is the deterministically sorted,
deduplicated set of triggered observations; empty ⇒ `HEALTHY`. Uncertainty is
contagious: any `AMBIGUOUS`/not-proven-required dimension injects its token regardless
of stronger findings.

## Cleanup readiness boundaries

`NotTargetable | RequiresSalvage | ReadOnlyObservationComplete |
EligibleForProcessReapPlan | EligibleForCacheReclaimPlan | WorktreeCleanupOwnedBy |
NotProven` — descriptive handoffs to #11667/#11669/#10256/#10263. This PR creates no
plan, kills nothing, deletes nothing.

## Determinism / serialization

One canonical row ordering (dimension rank, then subject key, then provider id, then
row id); JSON projection derives from the sorted form so identical evidence in any
insertion order serializes byte-identically.
