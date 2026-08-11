# TODOs & Missing Features

> **Last Updated**: 2026-07-17
> **Sources of truth**: `docs/project/ROADMAP.md` (planning), `docs/project/CURRENT_STATUS.md` (evidence — current shipped line is `v0.17.0`), `features.toml` (capabilities)
> **Rule**: If this file conflicts with those sources, update this backlog file (not the canonical sources).

---

## How to Use This List

- Treat this as an actionable backlog, not a status report.
- Do not add benchmark/test metrics here; metrics and receipts belong in `docs/project/CURRENT_STATUS.md`.
- Capability truth belongs in `features.toml`; regenerate derived docs with `just status-update`.

---

## Now - TODOs

### Quality Cleanup

- [ ] Remove remaining debug `println!` usage from production library paths.
- [ ] Complete unused dependency cleanup across targeted crates.
- [ ] Finish production-code `unwrap()` / `expect()` audit and replacements.
- [ ] Close lingering integration test failures and ratchet regression coverage.

### Release Surface Consistency

- [ ] Keep release truth aligned across GitHub Releases, VS Code Marketplace, Open VSX, and crates.io.
- [ ] Verify docs references to the current shipped line (see `docs/project/CURRENT_STATUS.md`) remain consistent after each release PR.
- [ ] Track any channel split explicitly in release docs until crates.io catches up.
- [x] Add a short stability statement in `docs/README.md` describing the compatibility policy for CLI flags, LSP capability advertising, and DAP preview boundaries. (Implemented in #6446.)
- [ ] Expand `docs/how-to/UPGRADING.md` with deprecation notice windows and emergency-break guidance. Compatibility, patch/minor, and rollback/stale-state guidance are implemented in #6446.
- [ ] Complete the `Distribution Matrix` doc under `docs/project/` with supported OS/architecture combinations and their support tiers. Channel and editor-install guidance are implemented in #6446.
- [x] Add a receipt template in `benchmarks/results/README.md` that records command, machine/runtime metadata, before/after SHAs, and artifact location. (Implemented in #6446.)
- [ ] Ensure committed benchmark results link back to the initiating change or issue.
- [ ] Document config key renames/removals and migration steps for release-to-release upgrade paths.
- [ ] Add known behavior deltas for parser/LSP/DAP roll-forward guidance and rollback/cache-reset steps for editor clients.
- [x] Capture dependency ordering in `docs/project/CI_LOCAL_VALIDATION.md`.\n- [x] Assign owners for each validation stage in `docs/project/CI_LOCAL_VALIDATION.md`. (Implemented in #6446.)
- [ ] Add a short "what changed after #210 lands" checklist for contributors.

### Announcement Readiness

- [ ] Validate install flows end-to-end for all documented distribution channels.
- [ ] Finalize announcement/blog release notes content and links.
- [ ] Confirm demo assets and walkthrough artifacts are publish-ready.

---

## Missing Features (Derived from `features.toml`)

### DAP (Preview / Not Advertised)

- **`dap.breakpoints.logpoints`** (preview, not advertised)
  - [x] Implement logMessage parsing and output emission path.
  - [ ] Add dedicated E2E fixture for output + continue semantics.

- **`dap.exceptions.die`** (preview, not advertised)
  - [x] Implement `setExceptionBreakpoints` filter handling.
  - [ ] Add E2E fixture proving stop behavior changes when enabled.

### Forward-Looking Gaps

- [ ] Native DAP completeness: attach parity, variables/evaluate fidelity, safe eval hardening.
- [ ] Full LSP 3.18 compliance re-audit against evolving spec and client behavior.
- [ ] Distribution expansion (package-manager channels such as Homebrew/apt/choco).

---

## Later

- [ ] Stabilize API/versioning policy language for the path to `v1.0.0`.
- [ ] Harden large-workspace performance budgets and monitoring.
- [ ] Continue parser confidence ratchet against broader CPAN corpus slices.
- [ ] Expand security and operations runbooks with periodic audit receipts.

---

## Quick Receipts / Checks

```bash
# Canonical local gate
nix develop -c just ci-gate

# Status drift checks
just status-update
just status-check
```

---

## Notes

- If a TODO becomes a verified claim ("done", "complete"), move evidence-backed language to `docs/project/CURRENT_STATUS.md`.
- If a capability is missing or reclassified, update `features.toml` first, then regenerate derived docs.
