#!/usr/bin/env python3
"""Fixture proof for the current-main 11983 reject-identity contract."""

from __future__ import annotations

import ast
import importlib.util
import os
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).parents[2]
MODULE_PATH = ROOT / "scripts/maintenance/verify_11983_reject_identities.py"
SPEC = importlib.util.spec_from_file_location("verify_11983", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load {MODULE_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def load_rebuild_import_helper():
    script = (ROOT / "scripts/maintenance/rebuild_11983_current_main.sh").read_text(encoding="utf-8")
    start = script.index("python3 - <<'PY'\n") + len("python3 - <<'PY'\n")
    end = script.index("\nPY\n", start)
    module = ast.parse(script[start:end])
    function = next(node for node in module.body if isinstance(node, ast.FunctionDef) and node.name == "replace_stale_import")
    namespace = {"Path": Path}
    exec(compile(ast.Module(body=[function], type_ignores=[]), str(ROOT), "exec"), namespace)
    return namespace["replace_stale_import"]


def exercise_empty_cherry_pick_guard(
    *,
    cherry_pick_head: str,
    expected_commit: str,
    cherry_pick_status: int,
    dirty_tree: bool,
    expect_skip: bool,
) -> None:
    script = (ROOT / "scripts/maintenance/rebuild_11983_current_main.sh").read_text(encoding="utf-8")
    start = script.index("reconstruction_tree_is_clean() {\n")
    end = script.index("\n# Identity gate", start)
    helper = script[start:end]
    required_fragments = (
        "capture_file=\"$(mktemp)\"",
        "if \"$@\" >\"$capture_file\" 2>&1; then",
        "else\n    status=$?\n  fi",
        "previous[[:space:]]+cherry-pick[[:space:]]+is[[:space:]]+now[[:space:]]+empty",
        "cherry_pick_head=\"$(git rev-parse --verify CHERRY_PICK_HEAD",
        "tree-status: clean",
    )
    if any(fragment not in script for fragment in required_fragments):
        raise RuntimeError("empty cherry-pick guard lost its status, identity, or tree checks")
    if os.name == "nt":
        return
    with tempfile.TemporaryDirectory(prefix="rebuild-11983-empty-guard-") as directory:
        root = Path(directory)
        evidence = root / "evidence"
        skipped = root / "skipped"
        fake_git = root / "git"
        fake_git.write_text(
            """#!/usr/bin/env bash
case "$*" in
  "cherry-pick --continue")
    printf '%s\\n' 'The previous cherry-pick is now empty, possibly due to conflict resolution.' >&2
    exit "${CHERRY_PICK_STATUS}"
    ;;
  "rev-parse --verify CHERRY_PICK_HEAD")
    printf '%s\\n' "${FAKE_CHERRY_PICK_HEAD}"
    ;;
  "rev-parse HEAD")
    printf '%s\\n' 'current-tree-head'
    ;;
  "diff --cached --name-only"|"diff --name-only --diff-filter=U")
    exit 0
    ;;
  "diff --quiet")
    exit "${DIRTY_TREE}"
    ;;
  "ls-files --others --exclude-standard")
    exit 0
    ;;
  "cherry-pick --skip")
    : > "${SKIPPED}"
    ;;
  *)
    printf 'unexpected fake git call: %s\\n' "$*" >&2
    exit 97
    ;;
esac
""",
            encoding="utf-8",
        )
        fake_git.chmod(0o755)
        harness = f"""\
set -u
evidence_dir={root.as_posix()}/evidence
mkdir -p "$evidence_dir"
export PATH="{root.as_posix()}:$PATH"
{helper}
if run_cherry_pick_or_skip_empty 'first cherry-pick --continue' "$EXPECTED_COMMIT" git cherry-pick --continue; then
  result=0
else
  result=$?
fi
test "$result" -eq {0 if expect_skip else 1}
"""
        result = subprocess.run(
            ["bash", "-c", harness],
            cwd=ROOT,
            env={
                **os.environ,
                "EXPECTED_COMMIT": expected_commit,
                "FAKE_CHERRY_PICK_HEAD": cherry_pick_head,
                "CHERRY_PICK_STATUS": str(cherry_pick_status),
                "DIRTY_TREE": str(int(dirty_tree)),
                "SKIPPED": f"{root.as_posix()}/skipped",
            },
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode:
            raise RuntimeError(
                "empty cherry-pick guard fixture failed: "
                f"status={result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}"
            )
        if expect_skip:
            receipt = evidence / "empty-cherry-pick-first_cherry-pick_--continue.txt"
            if not skipped.is_file() or not receipt.is_file():
                raise RuntimeError("verified empty cherry-pick did not skip with a receipt")
            receipt_text = receipt.read_text(encoding="utf-8")
            for line in (
                f"expected-commit: {expected_commit}",
                f"cherry-pick-head: {cherry_pick_head}",
                "current-head: current-tree-head",
                "tree-status: clean",
            ):
                if line not in receipt_text:
                    raise RuntimeError(f"empty cherry-pick receipt omitted {line!r}")
        elif skipped.exists():
            raise RuntimeError("unverified empty cherry-pick was incorrectly skipped")


# Independent oracle for the artifacts reproduced from d174ec1e9 on current
# origin/main. This must not be copied from MODULE.EXPECTED_REJECT_IDENTITIES:
# otherwise a table edit could make the positive fixture agree with itself.
FIXTURE_IDENTITIES: dict[str, tuple[tuple[str, str], ...]] = {
    "crates/perl-dap/features_sot.toml": (
        ('@@ -103,27 +103,27 @@ id = "lsp.range_formatting"', "advertised = true"),
    ),
    "crates/perl-lsp-rs-core/features_sot.toml": (
        ('@@ -141,27 +141,27 @@ id = "lsp.range_formatting"', "advertised = true"),
    ),
    "crates/perl-lsp-rs/features_sot.toml": (
        ('@@ -106,27 +106,27 @@ id = "lsp.range_formatting"', "advertised = true"),
    ),
    "crates/perl-parser/features_sot.toml": (
        ('@@ -103,27 +103,27 @@ id = "lsp.range_formatting"', "advertised = true"),
    ),
    "crates/perl-lsp-rs/src/runtime/language/formatting_policy/tests.rs": (
        (
            "@@ -555,7 +555,7 @@ fn stale_unknown_range_decision_preserves_unknown_receipt_engine()",
            "fn live_dispatch_routes_all_four_surfaces_through_one_receipt_policy()",
        ),
        (
            "@@ -564,14 +564,9 @@ fn live_dispatch_routes_all_four_surfaces_through_one_receipt_policy()",
            "let cases = [",
        ),
        (
            "@@ -611,65 +606,53 @@ fn live_dispatch_routes_all_four_surfaces_through_one_receipt_policy()",
            "for (offset, (method, params)) in cases.into_iter().enumerate() {",
        ),
        (
            "@@ -688,13 +671,16 @@ fn live_external_partial_range_returns_typed_refusal_not_native_edits()",
            'assert!(response.error.is_none(), "range formatting should return a typed refusal");',
        ),
    ),
    "crates/perl-lsp-rs/src/runtime/text_sync.rs": (
        (
            "@@ -11,9 +11,8 @@",
            "Arc, AtomicBool, AtomicU32, CodeFormatter, DocumentState, FormattingOptions, HashMap,",
        ),
    ),
    "crates/perl-lsp-rs/src/runtime/text_sync/lifecycle.rs": (
        ("@@ -1,7 +1,4 @@", "use super::{"),
        (
            "@@ -242,77 +239,17 @@ impl LspServer {",
            "params: Option<Value>,",
        ),
    ),
    "crates/perl-lsp-rs/tests/lsp_batteries_e2e_workflow_test.rs": (
        (
            "@@ -149,6 +149,8 @@ my$result=calculate(5,3);",
            "let formatted = apply_text_edits(messy_code, edits);",
        ),
        ('@@ -158,7 +160,6 @@ my$result=calculate(5,3);', '"\\n",'),
    ),
    "crates/perl-lsp-rs/tests/lsp_formatting_e2e.rs": (
        (
            "@@ -48,8 +48,14 @@ fn native_default_document_formatting() -> Result<(), Box<dyn std::error::Error>",
            "fn native_default_range_formatting() -> Result<(), Box<dyn std::error::Error>> {",
        ),
        (
            "@@ -70,18 +76,31 @@ fn native_default_range_formatting() -> Result<(), Box<dyn std::error::Error>> {",
            "let edits =",
        ),
        (
            "@@ -167,23 +186,26 @@ fn native_default_formatting_honors_lsp_tab_size() -> Result<(), Box<dyn std::er",
            "fn native_default_ranges_formatting_formats_selected_ranges()",
        ),
        ("@@ -205,23 +227,9 @@ sub third{my$c=3;return$c;}", "let edits ="),
    ),
    "docs/specs/lsp-318-conformance-matrix.md": (
        (
            '@@ -19,7 +19,7 @@ Status vocabulary:',
            "| Multi-range formatting | LSP 3.18 | range-formatting client support | `documentRangeFormattingProvider.rangesSupport` | `textDocument/rangesFormatting` | implemented+tested+documented | `lsp_caps_contract_shapes`; `lsp_disabled_features_tests`; `lsp_formatting_e2e`; `lsp_capabilities_snapshot`; `lsp_cap_snap` | `crates/perl-lsp-rs/src/runtime/language/formatting.rs`; `crates/perl-lsp-rs-core/src/protocol/capabilities.rs` | P0 | `documentRangesFormattingProvider` is not a valid capability and remains forbidden. |",
        ),
    ),
    "features.toml": (
        ('@@ -172,30 +172,30 @@ id = "lsp.range_formatting"', "advertised = true"),
    ),
    "xtask/src/tasks/lsp_318_matrix.rs": (
        (
            "@@ -63,11 +63,11 @@ const ROWS: &[MatrixRow] = &[",
            'status: "implemented+tested+documented",',
        ),
    ),
}
EXPECTED_FILE_COUNT = 12
EXPECTED_HUNK_COUNT = 20


def _segments(identities: tuple[tuple[str, str], ...]) -> list[str]:
    return [f"{hunk}\n {anchor}\n" for hunk, anchor in identities]


def write_manifest(
    root: Path,
    *,
    mutation: str | None = None,
    extra_artifact: bool = False,
) -> Path:
    root.mkdir(parents=True, exist_ok=True)
    manifest = root / "manifest.tsv"
    rows = []
    for index, (path, identities) in enumerate(FIXTURE_IDENTITIES.items()):
        patch = root / f"patch-{index}.diff"
        log = root / f"log-{index}.txt"
        reject = root / f"reject-{index}.rej"
        patch_segments = _segments(identities)
        reject_segments = list(patch_segments)
        if mutation == "mismatch" and index == 0:
            reject_segments[0] = reject_segments[0].replace(
                "advertised = true", "advertised = false"
            )
        elif mutation == "omission" and index == 4:
            reject_segments.pop()
        elif mutation == "hunk_header" and index == 0:
            patch_segments[0] = patch_segments[0].replace("@@ -103,27", "@@ -104,27")
            reject_segments[0] = patch_segments[0]
        elif mutation == "duplicate" and index == 0:
            reject_segments.append(reject_segments[0])

        patch_path = "foreign/path" if mutation == "patch_path" and index == 0 else path
        patch.write_text(
            f"diff --git a/{patch_path} b/{patch_path}\n"
            f"--- a/{patch_path}\n"
            f"+++ b/{patch_path}\n"
            + "\n".join(patch_segments),
            encoding="utf-8",
        )
        reject.write_text("\n".join(reject_segments), encoding="utf-8")
        log_count = len(reject_segments) if mutation == "duplicate" else len(patch_segments)
        log_numbers = list(range(1, log_count + 1))
        if mutation == "reorder" and index == 4:
            log_numbers.reverse()
        log.write_text(
            "".join(f"Rejected hunk #{number}.\n" for number in log_numbers),
            encoding="utf-8",
        )
        rows.append(f"{path}\t{patch}\t{log}\t{reject}\n")
    manifest.write_text("".join(rows), encoding="utf-8")
    if extra_artifact:
        (root / "unlisted.rej").write_text("@@ -1,1 +1,1 @@\n forged\n", encoding="utf-8")
    return manifest


def expect_rejection(
    manifest: Path,
    evidence: Path,
    phrase: str,
    retained: Path | None = None,
) -> None:
    try:
        MODULE.validate_manifest(manifest, evidence, reject_scope=manifest.parent)
    except ValueError as error:
        if phrase not in str(error):
            raise RuntimeError(f"unexpected rejection: {error}") from error
        if retained is not None and not (evidence / retained).is_file():
            raise RuntimeError(f"rejected artifact was not retained: {evidence / retained}")
    else:
        raise RuntimeError(f"fixture unexpectedly passed: {phrase}")


def main() -> None:
    actual_hunk_count = sum(len(identities) for identities in FIXTURE_IDENTITIES.values())
    if len(FIXTURE_IDENTITIES) != EXPECTED_FILE_COUNT:
        raise RuntimeError("independent fixture file count drifted")
    if actual_hunk_count != EXPECTED_HUNK_COUNT:
        raise RuntimeError("independent fixture hunk count drifted")

    exercise_empty_cherry_pick_guard(
        cherry_pick_head="d174ec1e9845056b8e1a193001ce88a2ea9eaebe",
        expected_commit="d174ec1e9845056b8e1a193001ce88a2ea9eaebe",
        cherry_pick_status=1,
        dirty_tree=False,
        expect_skip=True,
    )
    exercise_empty_cherry_pick_guard(
        cherry_pick_head="0f6a4334eb5a53df54a5ed40103659a63578b6f5",
        expected_commit="d174ec1e9845056b8e1a193001ce88a2ea9eaebe",
        cherry_pick_status=1,
        dirty_tree=False,
        expect_skip=False,
    )
    exercise_empty_cherry_pick_guard(
        cherry_pick_head="d174ec1e9845056b8e1a193001ce88a2ea9eaebe",
        expected_commit="d174ec1e9845056b8e1a193001ce88a2ea9eaebe",
        cherry_pick_status=1,
        dirty_tree=True,
        expect_skip=False,
    )
    exercise_empty_cherry_pick_guard(
        cherry_pick_head="d174ec1e9845056b8e1a193001ce88a2ea9eaebe",
        expected_commit="d174ec1e9845056b8e1a193001ce88a2ea9eaebe",
        cherry_pick_status=2,
        dirty_tree=False,
        expect_skip=False,
    )

    replace_stale_import = load_rebuild_import_helper()
    rebuild_script = (ROOT / "scripts/maintenance/rebuild_11983_current_main.sh").read_text(encoding="utf-8")
    if "git ls-files --others --exclude-standard" not in rebuild_script:
        raise RuntimeError("empty cherry-pick guard does not reject untracked files")
    if '"$@" >"$capture_file" 2>&1' not in rebuild_script:
        raise RuntimeError("empty cherry-pick helper does not capture combined output")
    if "previous[[:space:]]+cherry-pick[[:space:]]+is[[:space:]]+now[[:space:]]+empty" not in rebuild_script:
        raise RuntimeError("empty cherry-pick helper lacks the expected diagnostic matcher")
    classification_start = rebuild_script.index("  local cherry_pick_head")
    classification_end = rebuild_script.index("\n\n# Identity gate", classification_start)
    classification = rebuild_script[classification_start:classification_end]
    receipt_start = classification.index("printf 'command-output:\\n'")
    receipt_cat = classification.index('cat "$capture_file"', receipt_start)
    receipt_cleanup = classification.index('rm -f "$capture_file"', receipt_cat)
    if receipt_cat > receipt_cleanup:
        raise RuntimeError("empty cherry-pick evidence removes capture before reading it")
    if 'run_cherry_pick_or_skip_empty "first cherry-pick" "$first_commit" git cherry-pick "$first_commit"' not in rebuild_script:
        raise RuntimeError("initial cherry-pick bypasses the guarded helper")
    if "-- provider-confidence-matrix" in rebuild_script:
        raise RuntimeError("rebuild invokes the obsolete provider-confidence command")
    if "check-provider-confidence-matrix" not in rebuild_script:
        raise RuntimeError("rebuild omits the current provider-confidence command")
    with tempfile.TemporaryDirectory(prefix="rebuild-11983-imports-") as directory:
        path = Path(directory) / "text_sync.rs"
        old = "old import\n"
        new = "new import\n"
        current = "already current import\n"
        path.write_text(current, encoding="utf-8")
        replace_stale_import(str(path), old, new, "CodeFormatter", current)
        if path.read_text(encoding="utf-8") != current:
            raise RuntimeError("exact already-current import was not preserved")
        path.write_text("arbitrary marker-free state\n", encoding="utf-8")
        try:
            replace_stale_import(str(path), old, new, "CodeFormatter", current)
        except SystemExit as error:
            if "exact already-current import" not in str(error):
                raise RuntimeError(f"unexpected import-helper rejection: {error}") from error
        else:
            raise RuntimeError("arbitrary marker-free import state was accepted")

    with tempfile.TemporaryDirectory(prefix="rebuild-11983-identities-") as directory:
        root = Path(directory)
        positive = root / "positive"
        positive_manifest = write_manifest(positive)
        positive_rejects = list(positive.rglob("*.rej"))
        if len(positive_rejects) != EXPECTED_FILE_COUNT:
            raise RuntimeError("positive fixture artifact count drifted")
        positive_hunks = sum(
            len(MODULE.hunk_segments(path.read_text(encoding="utf-8")))
            for path in positive_rejects
        )
        if positive_hunks != EXPECTED_HUNK_COUNT:
            raise RuntimeError("positive fixture hunk count drifted")
        MODULE.validate_manifest(
            positive_manifest,
            positive / "evidence",
            reject_scope=positive,
            delete_verified=True,
        )
        if list(positive.rglob("*.rej")):
            raise RuntimeError("verified reject artifacts were not deleted")

        expect_rejection(
            write_manifest(root / "mismatch", mutation="mismatch"),
            root / "mismatch" / "evidence",
            "absent from or ambiguous",
            Path("reject-0.rej"),
        )
        expect_rejection(
            write_manifest(root / "hunk-header", mutation="hunk_header"),
            root / "hunk-header" / "evidence",
            "hunk identity omitted or reused",
            Path("reject-0.rej"),
        )
        expect_rejection(
            write_manifest(root / "reorder", mutation="reorder"),
            root / "reorder" / "evidence",
            "reject hunk ordinals",
        )
        table_mismatch = write_manifest(root / "table-mismatch")
        table_path = "crates/perl-dap/features_sot.toml"
        authored = MODULE.EXPECTED_REJECT_IDENTITIES[table_path]
        MODULE.EXPECTED_REJECT_IDENTITIES[table_path] = (
            MODULE.RejectIdentity(authored[0].hunk, "advertised = false"),
        )
        try:
            expect_rejection(
                table_mismatch,
                root / "table-mismatch" / "evidence",
                "hunk identity omitted or reused",
            )
        finally:
            MODULE.EXPECTED_REJECT_IDENTITIES[table_path] = authored
        table_order = write_manifest(root / "table-order")
        order_path = "crates/perl-lsp-rs/src/runtime/language/formatting_policy/tests.rs"
        ordered = MODULE.EXPECTED_REJECT_IDENTITIES[order_path]
        MODULE.EXPECTED_REJECT_IDENTITIES[order_path] = (ordered[1], ordered[0], *ordered[2:])
        try:
            expect_rejection(
                table_order,
                root / "table-order" / "evidence",
                "exactly match the authored per-file table",
            )
        finally:
            MODULE.EXPECTED_REJECT_IDENTITIES[order_path] = ordered
        expect_rejection(
            write_manifest(root / "omission", mutation="omission"),
            root / "omission" / "evidence",
            "expected 4 rejected hunks, found 3",
        )
        expect_rejection(
            write_manifest(root / "duplicate", mutation="duplicate"),
            root / "duplicate" / "evidence",
            "absent from or ambiguous",
        )
        expect_rejection(
            write_manifest(root / "patch-path", mutation="patch_path"),
            root / "patch-path" / "evidence",
            "patch artifact path mismatch",
            Path("reject-0.rej"),
        )
        extra = root / "extra"
        extra_evidence = extra / "evidence"
        extra_manifest = write_manifest(extra, extra_artifact=True)
        expect_rejection(
            extra_manifest,
            extra_evidence,
            "canonical artifact scope mismatch",
        )
        if not (extra_evidence / "unlisted.rej").exists():
            raise RuntimeError("unaccounted reject artifact was not retained")
        expect_rejection(
            extra_manifest,
            extra_evidence,
            "canonical artifact scope mismatch",
        )
    print("11983 reject-identity fixtures passed")


if __name__ == "__main__":
    main()
