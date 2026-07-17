# Issue #1715 — Implementation Checklist

- [x] Read the existing PR diff and review feedback before changing the seam.
- [x] Rebase the PR onto the current `origin/main` without broadening the diff.
- [x] Remove the incidental `doctor.rs` change.
- [x] Fix empty WorkspaceEdit return paths.
- [x] Parse the prepare-rename capability without numeric wraparound.
- [x] Reject reserved keyword positions for default-behavior delegation.
- [x] Preserve WorkspaceEdit metadata and open-document versions.
- [x] Release the document lock before version-aware conversion.
- [x] Strengthen prepare-rename and WorkspaceEdit integration assertions.
- [x] Add focused conversion/version/metadata regression coverage.
- [x] Run formatter, diff hygiene, and rename integration tests locally.
- [ ] Verify exact-head hosted required contexts.
- [ ] Merge only after required checks and review threads are green/resolved.
- [ ] Leave change-annotation generation as a follow-up rather than widening
      this PR.
