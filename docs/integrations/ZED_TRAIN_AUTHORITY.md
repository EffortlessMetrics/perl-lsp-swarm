# Zed implementation train authority

The repository has one active Zed implementation train:

```text
.ci/fixtures/zed-perl-upstream/train-authority.v1.json
  -> .ci/fixtures/zed-perl-upstream/train-v2/manifest.json
  -> docs/integrations/ZED_CODEX_IMPLEMENTATION_TRAIN_V2.md
```

The version-2 manifest owns stable topology: stage identity, issue, dependencies,
actor, route, evidence boundary, and close authority. Current issue, pull-request,
check, merge, manual-checkpoint, and external-publication state comes from a
separate typed read-only observation.

The current frontier is derived from:

```text
stable version-2 train
+ one typed observation bound to current main
= generated frontier
```

Unknown, ambiguous, stale, unavailable, or instrument-failed observation stays
fail closed. GitHub state cannot create product or behavioral evidence.

## Historical version 1

These remain in history as migration and review subjects:

```text
.ci/fixtures/zed-perl-upstream/codex-train.v1.json
docs/integrations/ZED_CODEX_IMPLEMENTATION_TRAIN.md
```

They are not current routing authority. Version 1 mixed stable architecture with
mutable issue, PR, and frontier state, so ordinary repository events made the
checked train stale. Unique work may be mined through the successor ledger, but
Codex must not execute from its hand-maintained frontier.

## Delivery boundary

Every Codex or read-only acceptance stage ends by delivering one internal draft
PR and its handoff. External extension, Zed-core, and registry submissions remain
maintainer-only checkpoints. A delivered PR, merged repository substrate, packet,
or checked template does not satisfy host evidence or public support.

Zed remains planned/not-proven until the official-registry host receipt, public
compatibility row, and exact support projection pass for the same bounded public
subjects.
