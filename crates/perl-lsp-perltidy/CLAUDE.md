# CLAUDE.md (perl-lsp-perltidy)

## Role and ownership

Deprecated compatibility facade for the published mode-bearing formatter API.
It retains the original configuration, UTF-16 DTO/result shapes and trait/method
signatures while delegating native computation to `perl-tidy` and external text
execution to `perl-tidy-perltidy`. There is no formatter algorithm here.

## Boundaries

- `src/native/projection.rs` translates through canonical `perl-position-tracking`
  conversion. Require exact user endpoints; named legacy whole-line widening is
  explicit request policy, never an endpoint clamp or a second geometry scanner.
- `Off` performs no native work. Native `ExternalLegacy` requests refuse instead
  of silently invoking native or discovering an executable.
- `BuiltInFormatter` delegates to the same parse-gated native owner. Do not restore
  its retired delimiter scanner to preserve speculative edits on malformed input.
- Legacy cache, file-in-place and line helpers remain only in this facade; new
  first-party consumers use the canonical packages directly.

The migration/removal target is 0.18.0. Issue #9292 owns actual removal after
downstream migration and external acceptance; that target does not itself permit
removal. Previously withdrawn runtime methods remain withdrawn.

## Focused proof

Use the repository's admitted Cargo route for `test -p perl-lsp-perltidy
--lib --tests`. Retained compatibility tests prove signatures, selection refusal,
strict geometry and canonical delegation. Native layout/benchmark ownership is
in `perl-tidy`; the independent legacy edit-application helper remains a projection
oracle. External subprocess tests do not establish installed-version output parity.

Read `README.md` before changing compatibility behavior. Preserve original public
result shapes; disclose numeric admission, exact unsupported targets and other
intentional refusal changes instead of claiming blanket historical byte parity.
