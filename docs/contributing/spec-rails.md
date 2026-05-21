# Contributor guide: spec rails

Use this guide when creating or editing repo-native spec artifacts.

## Rule of ownership

Place durable method artifacts in `.<repo>-spec/`:

- `proposals/` for why work exists
- `specs/` for behavior contracts
- `adr/` for architecture decisions
- `lanes/` for focused implementation trackers
- `templates/` for reusable forms
- `closeouts/` for landed scope and residual follow-up

## Rule of non-ownership

Do **not** create, migrate, rewrite, or validate tool/session state directories as part of this lane:

- `.spec/`
- `.codex/`
- `.claude/`
- `.jules/`

These may consume or mirror parts of the method, but they are not the durable repo-native source of truth.

## Practical workflow

1. Create or update a proposal for purpose and scope.
2. Add/update a spec for required behavior and expected evidence.
3. Record any durable architecture decision in ADR form.
4. Track execution with focused lane trackers under `.<repo>-spec/lanes/`.
5. Publish closeout evidence in `.<repo>-spec/closeouts/`.

## Minimal proof for docs-only rail updates

Run:

```bash
git diff --check
```
