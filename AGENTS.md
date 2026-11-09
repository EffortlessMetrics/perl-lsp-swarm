# Repository operating contract

## Product direction

perl-lsp is becoming a compiler-backed Perl toolchain whose parser, compiler facts,
workspace model, LSP, DAP, packaging, and editor behavior remain honest about source,
freshness, confidence, fallback, and dynamic boundaries.

Optimize for user-visible closure, semantic ownership, deterministic proof, and
maintainable current-main behavior—not local component completion or workflow
compliance.

## Start or resume

Confirm the checked-out repository, branch, and current issue or PR before choosing
work. Preserve unrelated changes and establish the candidate's mutation owner before
editing. Read applicable `AGENTS.md` files and package-local `CLAUDE.md` where present;
package ownership and architecture guidance apply to both providers. An absent
package-local `AGENTS.md` does not mean there is no owner guidance. If a package has
neither local file, establish ownership and focused proof from its manifest, source,
tests, and any README.

| Need | Read |
| --- | --- |
| Set up a checkout or choose the host shell | [Contributor quick start](CONTRIBUTING.md#quick-start), including the Windows shell guidance |
| Find the owning crate | [Current repository map](CONTRIBUTING.md#current-repository-map), then its local guidance |
| Resume an issue, candidate, or review | [Session operations](docs/how-to/SESSION_OPERATIONS.md) |
| Find tools installed by an earlier setup | [Task-shell activation](docs/how-to/AGENT_CONTRIBUTING.md#activate-tools-in-the-task-shell) |
| Run compiled proof | [Cargo storage admission](docs/agents/CARGO_STORAGE.md), then the package's focused proof |

Use these task-specific entrypoints as needed; setup and repository maps stay with
their existing owners. The route below selects the work, and admission applies to the
exact command that will execute it.

## Sources of truth

This ordering resolves repository facts and document status. It does not override
current task instructions, higher-priority runtime instructions, or permissions.

Use the highest applicable current authority:

1. current `origin/main`, live GitHub issues, PRs, reviews, checks, rulesets, and actual
   repository behavior;
2. accepted specifications, ADRs, policies, generated contracts, and independent proof;
3. this file, `.agents/skills/`, and applicable package-local `AGENTS.md`/`CLAUDE.md`;
4. shared method/reference docs under `docs/agents/`;
5. runtime plans, workers, worktrees, memory, and historical conversation.

Document applicability and status are governed by
[`docs/agents/AUTHORITY_STATUS.md`](docs/agents/AUTHORITY_STATUS.md) and its
[machine registry](docs/agents/authority_status.toml). Before using an agent,
review, queue, branch, or orchestration spec/ADR/doc, check that registry. A path listed
as `transitional`, `historical`, or `superseded` does not re-enter the hierarchy merely
because its own old header says “accepted,” “active doctrine,” “north star,” or
“current.”
The registry classifies documentation authority; current source and live GitHub evidence still
govern the underlying facts and transaction state.

This file is Codex's route map. `.agents/skills/` contains the executable provider-
native procedures. Shared docs define invariants and GitHub surface ownership; they do
not replace a named `$skill`.

GitHub owns durable live transaction state. Runtime topology, claim-frame ordering,
liveness, retries, and temporary plans are not repository authority and must not be
written to tracked state files.

Detailed cross-provider contracts remain in
[`docs/agents/DEVELOPMENT_METHOD.md`](docs/agents/DEVELOPMENT_METHOD.md),
[`docs/agents/GITHUB_SURFACES.md`](docs/agents/GITHUB_SURFACES.md),
[`docs/agents/REVIEW_CURRENTNESS.md`](docs/agents/REVIEW_CURRENTNESS.md), and
[`docs/agents/SKILL_CONTRACT.md`](docs/agents/SKILL_CONTRACT.md).

## Select and run the route

Choose the narrowest applicable public flow:

- `$deliver-goal` — [durable multi-PR outcome or umbrella](.agents/skills/deliver-goal/SKILL.md);
- `$deliver-pr` — [one issue, PR, branch, candidate, or coherent claim](.agents/skills/deliver-pr/SKILL.md);
- `$prepare-issue` — [problem, owner, scope, proof seam, or plan](.agents/skills/prepare-issue/SKILL.md);
- `$prepare-proof` — [discriminating executable proof](.agents/skills/prepare-proof/SKILL.md);
- `$build-candidate` — [implementation, test hardening, simplification, candidate
  challenge](.agents/skills/build-candidate/SKILL.md);
- `$finish-pr` — [publication/resume, repair, substantive review, integration, merge,
  reconciliation](.agents/skills/finish-pr/SKILL.md).

Read the selected skill's `SKILL.md` before executing it. If the runtime has not
preloaded the repository skills, open the linked file directly and follow its named
transitions; lack of automatic discovery does not replace the repository procedure.

Enter at the earliest absent or stale useful judgment. Existing coherent work enters
midstream. Selecting a route is not completion: invoke it, follow its named normal and
material backward edges, and do not invent a parallel lifecycle or run a stage locator.

## Operating posture

**Default-complete, recovery-forward.** Continue through every applicable judgment in
the selected route until the claim is reconciled, reaches a real remote-owned wait, or
returns a precise blocker or `NOT_PROVEN` boundary. Do not stop at research, a plan, a
worker result, or green checks when the route still contains useful work.

Make reasonable documented engineering decisions and proceed. Missing historical
ceremony, labels, receipts, or named-agent handoffs is not a reason to discard coherent
work; perform the cheapest still-useful repair and continue.

## Root orchestration and claim frames

The **main/root Codex thread is the accountable orchestrator**. It owns both goal-level
and claim-level orchestration:

- user goal, current interpretation, constraints, and acceptance predicates;
- selection and ordering of coherent claims;
- a runtime-local frame for each active or resumable claim;
- cross-claim prerequisites and contradictions;
- joined evidence and review sufficiency;
- writer allocation and mutation ownership;
- remote waits and wake events;
- GitHub publication, merge judgment, reconciliation, and continuation.

A claim or lane is a **logical frame held by the root**, not normally another
orchestrator agent. These logical claim frames keep claim orchestration in the root
while bounded contexts execute research, mutation, and review. A claim frame may contain:

```text
claim
acceptance predicate
controlling issue / PR / merge subject
current candidate and writer, if any
current route or missing judgment
proof / review status and limitations
external wait and wake event
```

Keep that frame runtime-local and reconstruct it from GitHub/repository artifacts after
replacement. Do not serialize agent liveness or create a second claim-state database.

The root should not personally absorb every high-volume leaf operation. It delegates
bounded **programmes** when another context materially helps:

- research / archaeology / external truth / CI evidence → researcher or focused worker;
- candidate or proof mutation → one writer/builder context;
- fixed-subject adversarial judgment → reviewer context;
- tightly coupled work needing provider-native lateral communication → a team only when
  communication changes the result.

Recursive orchestration is an optional physical execution technique, not the logical
architecture. A child does not become a subordinate orchestrator merely because it owns
a substantial task or because the claim is long-lived.

## Codex orchestration

Use `$orchestrate-work` after selecting a public flow or substantive atomic skill.

Normal shape:

```text
main/root orchestrator
├── logical claim frame A
│   ├── researcher programme(s), when useful
│   ├── one candidate writer / builder programme
│   └── reviewer programme(s)
├── logical claim frame B
│   └── remote wait; no live agent required
└── logical claim frame C
    └── bounded evidence work
```

`$deliver-goal` manages the goal and claim set. `$deliver-pr` focuses the root on one
claim frame and carries that claim through the named SDLC flows. The root may switch
between claim frames when one reaches a genuine GitHub-owned wait; it does not need one
orchestrator process per frame.

For bounded work, name the `$skill`, exact subject, accepted authority and facts,
read/write boundary, falsifiers, sufficient return, stop conditions, and non-goals.
Require a child to consume the named skill when supplied. Do not restate the skill's
method in the brief.

Choose agents when they preserve useful context, compress high-output evidence, change
source/oracle/tool/environment/threat model, reduce elapsed time, improve recovery, or
avoid expensive CI cycles. Stop adding agents when another result cannot change a
decision.

A programme may intentionally span several ordered atomic skills. Do not fork once per
skill when the same subject and artifact context remain load-bearing. Atomic skills
change attention; they do not automatically justify a new identity.

Keep claim frames and wake events in runtime memory only. Reconstruct them from issues,
PRs, reviews, checks, merges, and repository artifacts after replacement. Do not poll
unchanged remote state.

For the selected candidate, retain the same useful owning lead through in-session
proof/review, exact-H conditional landing authorization, native queue qualification on
actual G, protected landing and cheap M read-back. Authorization precedes enqueue;
native landing does not wait for a second manual merge or compulsory post-green review.
Use supported events or bounded checks without a polling child or scheduled monitor.
A material finding returns to repair; a real interruption gets an exact resumable
handoff. Preserve durable evidence independently of the warm worker.

Read `docs/agents/DEVELOPMENT_METHOD.md` and `docs/ci/qualification-cutover.md` for the
desired/staged/live boundary. Ordinary Draft, Ready and publication start no Actions,
including cheap wrappers. Missing admission, G producer or protection capability is
`NOT_PROVEN`; do not invent a request command, push to manufacture CI, bypass protection
or treat a local source fixture as a deployed lifecycle.

## Useful GitHub handoffs

Post or update GitHub only when information remains useful after the current context
disappears:

- claim, authority, plan, proof obligation, route, prerequisite, support, risk, or
  rollback meaning changed;
- source-backed evidence would otherwise be rediscovered;
- a localized finding belongs in an inline review;
- a finding receives an evidence-backed disposition;
- a real external wait and wake event need to survive handoff;
- a useful cumulative review, merged effect, or goal synthesis is ready.

Use issues for durable research/rulings/plans/dependencies/goal synthesis; PR bodies or
comments for candidate-wide route/proof/limitation summaries; inline reviews for
localized findings; review replies for dispositions; submitted reviews for cumulative
judgment; and issue closeout for landed effects and residual claims.

Keep agent identity, topology, liveness, retries, task state, provisional reasoning,
raw logs, unchanged polling, and routine skill transitions runtime-local.

When another context benefits and the route is not obvious, post one compact route
declaration with parent goal, claim, entry flow, current named transition, reason,
durable subject, and wake event. Update it only when the material route changes. It is
a resumability aid, not lifecycle authority.

## Codex-native PR review

For substantive PRs the native route is:

```text
`$finish-pr`
→ `$address-review-comments` for existing findings
→ `$final-challenge`
→ `$orchestrate-work` for differentiated review lenses
→ cumulative `$review-pr`
→ REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN |
  BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE
→ REVIEW_CURRENT plus exact-H conditional landing authorization enters `$verify-live-ci`
→ INTEGRATION_READY | PR_IN_FLIGHT | MERGE_BLOCKED | NOT_PROVEN
→ `$merge-reconcile`
```

The root owns the cumulative judgment and disposition. Review is not diff reading, green
CI, mergeability, zero threads, bot approval, or a worker verdict. It must
proportionately challenge proof discrimination, production reachability, external
truth, claim honesty, semantic authority/complexity, and risk/rollback.

The construction context must not be the only detection surface supporting a
substantive merge. Independence comes from changed evidence, oracle, method, threat
model, environment, or attention—not identity alone.

Read-only review is non-exclusive. Multiple sessions may inspect the same candidate and
publish useful findings concurrently without writer allocation, a claim, or a wait.
Exclusivity begins at mutation: a reviewer that wants to change the candidate must take
the one writer role or leave the finding for the current writer.

A clean review is valid. Do not manufacture findings or edits to demonstrate that the
review happened.

Substantive review and integration posture remain separate. Pending remote checks leave
review current and return `PR_IN_FLIGHT`.

## Proof and currentness

Keep candidate, integration, and landed evidence distinct.

- material candidate change → rerun affected proof and review;
- actual conflict or combined-tree interaction → repair and review the affected seam;
- unrelated `main` movement with a conflict-free candidate → no rebase, branch refresh,
  full CI replay, or review churn;
- head SHA change alone → no review invalidation;
- merge uses current head only as compare-and-swap protection.

Never weaken a test, ratchet, support claim, or required proof merely to obtain green
status. Missing, partial, stale, contradictory, or instrument-failed evidence is
`NOT_PROVEN`.

## Self-authored correction and disclosure

Follow the canonical correction and disclosure contract in
[`docs/agents/DEVELOPMENT_METHOD.md`](docs/agents/DEVELOPMENT_METHOD.md): repair an
in-scope reversible defect, rerun affected proof, disclose the correction, and request
approval only for a separate non-derivable or protected decision.

## Hard stops

Stop only for concrete hazards:

- two writers would mutate the same candidate concurrently;
- destructive cleanup would lose unsalvaged work;
- repository/candidate/material-claim identity or authority cannot be established;
- a secret or unsafe release would be published;
- a durable contract is structurally invalid;
- substantive findings remain unresolved or review is missing/`NOT_PROVEN` at merge;
- current rulesets, required checks, mergeability, or queue state block integration.

Otherwise detect, explain, repair, and continue.

## Repository hygiene and local proof

- use the host shell and direct Git, GitHub, Cargo, and repository commands; do not
  require a token-saving wrapper or wrapper initialization. Cargo admission below
  still applies to resource-bearing work;
- preserve the direct command and its relevant output in evidence and handoffs;
- production code must not use `unwrap`, `expect`, `panic!`, `todo!`,
  `unimplemented!`, `abort`, or `dbg!` outside documented narrow exceptions;
- migrating a test's `.expect("…")` onto the `perl-test-must` helpers uses the
  context-preserving `must_with`/`must_some_with`/`must_err_with`; the bare
  `must`/`must_some`/`must_err` are only correct when the call site carried no
  explanation (`cargo xtask ci-hygiene check-must-context` reports the drop);
- never use `git stash` in this shared multi-worktree repository; `refs/stash` is
  repository-global, so another worktree can pop or drop your entry without knowing it
  is yours; use scoped restore or a branch-local WIP commit;
- immediately before candidate mutation, establish the exact candidate, head, and
  mutation owner from live evidence, using checks applicable to that operation.
  New-candidate admission is not a resume check; an open PR alone does not establish
  another writer. Missing, stale, or contradictory ownership evidence is `NOT_PROVEN`;
  read-only research and review need no writer admission;
- if another current writer is established, stop mutating that candidate; continue as
  a reviewer or take a different claim;
- rewrite published candidate history only with established sole mutation ownership,
  applicable user and repository authorization, and an explicit expected remote SHA.
  Ownership alone grants no rebase or force-push permission;
- if commits from multiple contexts have already reached the branch, first establish
  one writer; that writer preserves the commits through non-rewriting integration,
  subject to the applicable authorization;
- stage intended paths explicitly;
- use one worktree per genuine concurrent write claim, not per lifecycle pass;
- regenerate generated projections only through their owning writer and publication
  policy; never hand-merge or hand-edit them. Feature branches do not regenerate or
  stage `docs/policy/NON_RUST_INVENTORY.md`, a default-branch publication. Follow
  [file policy](docs/FILE_POLICY.md) and [non-Rust policy](docs/policy/NON_RUST_POLICY.md)
  for current-tree evidence under `target/policy/`;
- never add temporary workflows that fetch, execute, or push on behalf of a branch
  (`contents: write` self-repair jobs); apply the edit locally and remove such files
  when found on a branch;
- before treating an older branch as an unrelated-history port, check its merge base
  with current `main`; shared history calls for claim-level comparison;
- keep both Cargo target and intermediate build paths private to the canonical
  worktree; a shared lock or Git version stamp does not prevent stale sibling
  libraries (#11650). Use the host-admitted `scripts/cargo-admitted` route or an
  explicitly admitted equivalent; legacy `cargo-safe` does not establish isolation.
  See [Cargo storage admission](docs/agents/CARGO_STORAGE.md). Preserve old resources;
- bind behavioral proof to an immutable executable copied before releasing its
  build ownership, and verify actual behavior after candidate changes;
- run focused proof, then affected package proof, then broader proof only when risk or
  the merge gate selects it;
- do not run repository-wide Clippy or tests after every edit.

### Choose local proof

Replace `PACKAGE` with the owning Cargo package. Formatting does not compile it:

```bash
cargo fmt -p PACKAGE -- --check
```

For compiled proof, choose the smallest package/target command that can falsify the
claim, then preflight and execute that same request from the same worktree and task
shell. A package-test example, after host setup:

```bash
bash scripts/cargo-admitted --preflight test -p PACKAGE --locked &&
  bash scripts/cargo-admitted test -p PACKAGE --locked
```

Preserve any selected target, feature, profile, and test filters in both commands.
Follow the separately supported [staged native Clippy route](docs/agents/CARGO_STORAGE.md#staged-native-clippy)
for linting; do not mechanically prefix unsupported Cargo commands with the wrapper.
The `xtask` alias is not admitted: use the documented `run -p xtask --locked -- ...`
route. `scripts/xtask-quiet.sh` also builds through admission, even for a query ending
in `--help`; quiet output is not evidence of build-free execution.

`just doctor` diagnoses repository health. `just devex` diagnoses tool setup but can
bootstrap `xtask`. Neither establishes build admission, and `just pr-fast` still uses
the legacy `cargo-safe` route. These compatibility commands do not replace the
[Cargo storage contract](docs/agents/CARGO_STORAGE.md) for new agent work.

For documentation-only changes, check links and run the existing narrow contracts
that govern the edited surface. Record passed, failed, and unavailable proof honestly.
Current GitHub protection remains authoritative at merge; the
[late-qualification contract](docs/agents/DEVELOPMENT_METHOD.md#late-qualification-and-publication)
distinguishes local authoring proof, staged policy, and live workflow triggers.
