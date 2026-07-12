## Current state (origin/main @ 25eaca807)

**Extraction (done)**: PR #3218 (commit d93d8aa66, merged 2026-06-30) landed `try_extract_native_class_fields` and shared field-extraction logic. `ClassModel.fields` now holds NativeClass field records with attributes (`:param`, `:reader`, `:writer`, `:accessor`, `:mutator`).

**Surfacing (done, pending merge)**: PR #3434 (commit 20f1b0d69, OPEN, CI green) solves this issue completely:
- Extends `add_object_pad_constructor_completions` framework gate from `== Framework::ObjectPad` to `matches!(Framework::ObjectPad | Framework::NativeClass)`
- Framework-aware detail/documentation strings (shows "native class constructor parameter" for NativeClass vs Object::Pad copy)
- 2 unit tests: native class :param completion with/without prefix filtering
- 2 integration tests: field declaration hover + field reference hover inside method bodies
- 2 integration tests: goto-def on field reference in method body + on :reader accessor call site
- All CI checks PASS (Perl LSP Rust Small Result: ✓, ripr+ New Gap Gate: ✓)

## Claim check

- **"native `class { field $x :param :reader }`"** → CONFIRMED via [perldoc.perl.org/perlclass](https://perldoc.perl.org/perlclass)
- **"#1651 extraction done"** → CONFIRMED (PR #3218, merged)
- **"provider path reads ClassModel.fields generically"** → REFUTED (explicit framework gate), BUT **FIXED in PR #3434** (gate extended to NativeClass)
- **"Object::Pad fields already surface"** → CONFIRMED by proxy (code path is tested; PR #3434 adds parallel tests for NativeClass)
- **"Array/hash fields deferred"** → CONFIRMED (issue explicitly defers; PR #3434 does not attempt)

## Scope (PR #3434)

✓ Completion: `Package->new(field_name => ...)` now completes for native class :param fields  
✓ Hover: field declarations and references inside methods both hover-enabled (tests added)  
✓ Goto-def: field references + :reader accessors navigate to field site (tests added)  
✓ Test coverage: 6 new tests (2 unit + 4 integration), all passing

**Non-scope**: parsing, extraction, array/hash fields (deferred per issue).

## Next-state triage

**`already-done-in-PR-3434`** — The fix is complete, passing all required CI checks, and waiting for review/merge. No builder work remains on origin/main; the issue is solved by PR #3434.

For closure: review + approve PR #3434 → merge to main → close issue #3220 as fixed-by-PR.
