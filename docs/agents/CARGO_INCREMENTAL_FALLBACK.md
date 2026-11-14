# Raw Cargo incremental fallback

The canonical agent build route remains `scripts/cargo-admitted`, normally through
`just agent-check PACKAGE ...` or `just agent-test PACKAGE SELECTOR ...` after an
exact-command preflight. That route owns private worktree target/build paths,
capacity admission, the lease, the pinned agent profile, and child settlement.

Tracked `.cargo/config.toml` also sets `build.incremental = false`. This is a
containment fallback for direct `cargo check`, `cargo test`, and other raw Cargo
callers that still exist while migration to the admitted route continues. It
prevents those callers from creating path-crate incremental payloads by default in
every retained worktree. It does not make raw Cargo admitted, move its output,
reclaim existing files, or establish a performance result.

A deliberate single-worktree experiment may override the fallback for one command:

```bash
CARGO_INCREMENTAL=1 cargo check -p PACKAGE
```

That override is outside the default agent posture. It does not bypass a capacity
refusal, authorize shared target/build state, or grant cleanup authority. Hold the
source, package graph, profile, jobs, features, output roots, compiler wrapper, and
test threads constant when comparing it with the default; #19217 owns the
cold/warm/edit adjudication and its storage/speed threshold.

Run the source-backed configuration proof with:

```bash
bash scripts/tests/test-target-isolation.sh
```

The test creates throwaway worktrees from the checked-out candidate, verifies their
default target roots remain distinct, then compiles a dependency-free standalone
fixture below one worktree. With CI and incremental overrides removed, the default
cell must leave no non-empty `incremental` payload; `CARGO_INCREMENTAL=1` must create
one. All fixture outputs and worktrees are removed on exit.
