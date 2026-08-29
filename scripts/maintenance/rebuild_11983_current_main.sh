#!/usr/bin/env bash
set -euo pipefail

# This entrypoint invokes cargo (fmt/test below), so it must run the shared
# toolchain guard first — otherwise a cargo older than the workspace
# rust-version surfaces as a manifest parse error instead of a typed refusal
# (#12593). Ported from the verified patch on #12997: it is the only remaining
# failure in the guard self-test on current main, and no PR carries it yet.
. "$(dirname -- "${BASH_SOURCE[0]}")/../lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

first_commit="d174ec1e9845056b8e1a193001ce88a2ea9eaebe"
first_parent="470277161c18cd5cfa00e31ea6545e2e7baee461"
second_commit="0f6a4334eb5a53df54a5ed40103659a63578b6f5"

# Identity gate (#12045 review): prove this lane executed exactly the triggering
# pull-request revision before any local reconstruction mutates the workspace.
if [ -z "${REBUILD_EVENT_HEAD_SHA:-}" ]; then
  echo "REBUILD_EVENT_HEAD_SHA must identify the checked-out event revision" >&2
  exit 1
fi
checked_out_head="$(git rev-parse HEAD)"
if [ "$checked_out_head" != "$REBUILD_EVENT_HEAD_SHA" ]; then
  echo "checked-out head $checked_out_head does not match event revision $REBUILD_EVENT_HEAD_SHA" >&2
  exit 1
fi

# Local-only reconstruction identity for throwaway cherry-pick commits.
evidence_dir="target/receipts/rebuild-11983"
mkdir -p "$evidence_dir"

git config user.name EffortlessSteven
git config user.email git@effortlesssteven.com

# Cherry-pick sources survive via the durable merged pull-request ref; the
# original source branch fix/11955-withdraw-secondary-format-routes was deleted.
git fetch --no-tags origin main "+refs/pull/11983/head:refs/remotes/origin/pr-11983-source"
git merge --no-edit origin/main

# The merge may have raised the workspace floor: re-run the guard against the
# merged tree so every later cargo invocation is validated by the toolchain
# contract actually being built (#12593). Sourcing after the merge validates
# against the merged tree's own guard contract.
. "$(dirname -- "${BASH_SOURCE[0]}")/../lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

git cat-file -e "${first_commit}^{commit}"
git cat-file -e "${second_commit}^{commit}"

if ! git cherry-pick "$first_commit"; then
  mapfile -t conflicts < <(git diff --name-only --diff-filter=U | sort)
  expected=(
    crates/perl-lsp-rs/src/runtime/language/formatting_policy/tests.rs
    crates/perl-lsp-rs/src/runtime/text_sync.rs
    crates/perl-lsp-rs/src/runtime/text_sync/lifecycle.rs
    crates/perl-lsp-rs/tests/lsp_batteries_e2e_workflow_test.rs
  )
  mapfile -t expected_sorted < <(printf '%s\n' "${expected[@]}" | sort)
  if ! diff -u \
    <(printf '%s\n' "${expected_sorted[@]}") \
    <(printf '%s\n' "${conflicts[@]}") > "$evidence_dir/conflict-denominator.diff"; then
    echo "current-main moved: conflict denominator diverged from the reviewed set." >&2
    echo "No hunks were resolved, deleted, or committed; nothing below is trusted." >&2
    cat "$evidence_dir/conflict-denominator.diff" >&2
    printf 'BLOCKER: re-discovery required; reviewed conflict set must be re-owned.\n' \
      > "$evidence_dir/reconstruction-summary.txt"
    exit 1
  fi

  reject_manifest="$(mktemp)"
  export REBUILD_REJECT_MANIFEST="$reject_manifest"
  for path in "${conflicts[@]}"; do
    git checkout --ours -- "$path"
    patch="/tmp/$(printf '%s' "$path" | tr '/' '_').patch"
    git diff --binary "$first_parent" "$first_commit" -- "$path" > "$patch"
    apply_log="/tmp/$(printf '%s' "$path" | tr '/' '_').apply.log"
    git apply --reject --recount --whitespace=nowarn "$patch" 2> "$apply_log" || true
    printf '%s\t%s\t%s\t%s\n' \
      "$path" "$patch" "$apply_log" "${path}.rej" >> "$reject_manifest"
  done

  python3 - <<'PY'
import os
import re
from pathlib import Path


def hunk_segments(text: str) -> list[str]:
    segments = []
    current = None
    for line in text.splitlines():
        if line.startswith("@@ "):
            if current is not None:
                segments.append("\n".join(current))
            current = [line]
        elif current is not None:
            current.append(line)
    if current is not None:
        segments.append("\n".join(current))
    return segments


def reject_evidence_dir() -> Path:
    evidence = Path("target/receipts/rebuild-11983/rejected-hunks")
    evidence.mkdir(parents=True, exist_ok=True)
    return evidence


manifest_path = os.environ.get("REBUILD_REJECT_MANIFEST")
if not manifest_path:
    raise SystemExit("reject manifest environment variable is missing")

verified_rejects: list[Path] = []
for entry in Path(manifest_path).read_text(encoding="utf-8").splitlines():
    path, patch_file, apply_log_file, reject_name = entry.split("\t")
    log_text = Path(apply_log_file).read_text(encoding="utf-8")
    rejected_hunks = [int(n) for n in re.findall(r"Rejected hunk #(\d+)\.", log_text)]
    patch_segments = set(hunk_segments(Path(patch_file).read_text(encoding="utf-8")))
    reject = Path(reject_name)

    def retain(reason: str) -> None:
        evidence_dir = reject_evidence_dir()
        if reject.exists():
            (evidence_dir / reject.name).write_text(
                reject.read_text(encoding="utf-8"), encoding="utf-8"
            )
        raise SystemExit(
            f"{path}: unverified rejected hunks retained under {evidence_dir}: {reason}"
        )

    if not rejected_hunks:
        if reject.exists():
            retain("apply reported no rejection but a reject artifact exists")
        continue

    if not reject.exists():
        retain(f"apply recorded rejected hunks {rejected_hunks} but no reject file exists")
    found_segments = sorted(set(hunk_segments(reject.read_text(encoding="utf-8"))))
    foreign = [s for s in found_segments if s not in patch_segments]
    if foreign:
        retain(f"{len(foreign)} reject hunks do not come from the reviewed patch")
    if len(found_segments) != len(rejected_hunks):
        retain(
            f"expected {len(rejected_hunks)} rejected hunks, found {len(found_segments)}"
        )
    verified_rejects.append(reject)


def replace_exact(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one current-main anchor, found {count}")
    file.write_text(text.replace(old, new), encoding="utf-8")


text_sync = "crates/perl-lsp-rs/src/runtime/text_sync.rs"
replace_exact(
    text_sync,
    "    Arc, AtomicBool, AtomicU32, CodeFormatter, DocumentState, FormattingOptions, HashMap,\n"
    "    JsonRpcError, LspServer, Mutex, Node, NonZeroU32, Ordering, Parser, Value,\n",
    "    Arc, AtomicBool, AtomicU32, DocumentState, HashMap, JsonRpcError, LspServer, Mutex,\n"
    "    Node, NonZeroU32, Ordering, Parser, Value,\n",
)

lifecycle = "crates/perl-lsp-rs/src/runtime/text_sync/lifecycle.rs"
replace_exact(
    lifecycle,
    "    Arc, AtomicU32, CodeFormatter, FormattingOptions, JsonRpcError, LspServer, NonZeroU32, Value,\n"
    "    invalid_params, json, source_path_from_uri,\n",
    "    Arc, AtomicU32, JsonRpcError, LspServer, NonZeroU32, Value, invalid_params, json,\n"
    "    source_path_from_uri,\n",
)

e2e = Path("crates/perl-lsp-rs/tests/lsp_batteries_e2e_workflow_test.rs")
e2e_text = e2e.read_text(encoding="utf-8")
required = (
    "// True-EOF policy (#8048/#11873): whole-document replace edits extend through\n"
    "    // true EOF"
)
if required not in e2e_text:
    raise SystemExit("current-main true-EOF formatting oracle is missing")
if '"my $result = calculate(5, 3);\\n",\n            "\\n",' in e2e_text:
    raise SystemExit("stale extra-newline expectation returned")

verified_names = {str(reject) for reject in verified_rejects}
for expected_reject in [
    Path(text_sync + ".rej"),
    Path(lifecycle + ".rej"),
    Path(str(e2e) + ".rej"),
]:
    if str(expected_reject) not in verified_names:
        raise SystemExit(
            f"reject evidence not verified against reviewed patch: {expected_reject}"
        )
for verified in verified_rejects:
    if verified.exists():
        verified.unlink()
PY

  if find . -name '*.rej' -print -quit | grep -q .; then
    echo "Unreviewed rejected hunks remain:" >&2
    find . -name '*.rej' -print -exec sh -c 'echo "--- $1"; cat "$1"' _ {} \;
    exit 1
  fi

  git add -- "${conflicts[@]}"
  git cherry-pick --continue
fi

git cherry-pick "$second_commit"

# Strengthen the containment proof: refusal must leave accepted document source,
# client version, and generation unchanged, not merely return no edits.
python3 - <<'PY'
from pathlib import Path


def replace_exact(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one proof anchor, found {count}")
    file.write_text(text.replace(old, new), encoding="utf-8")


policy = "crates/perl-lsp-rs/src/runtime/language/formatting_policy/tests.rs"
replace_exact(
    policy,
    '    server.test_apply_did_open(document_uri, "my$x=1;\\nmy$y=2;\\n", 1)?;\n'
    '    server.test_apply_did_open(on_type_uri, "if ($ok) {\\n\\n", 1)?;\n\n'
    '    // Withdrawn surfaces (#11955): every default-profile request must receive\n',
    '    server.test_apply_did_open(document_uri, "my$x=1;\\nmy$y=2;\\n", 1)?;\n'
    '    server.test_apply_did_open(on_type_uri, "if ($ok) {\\n\\n", 1)?;\n\n'
    '    let snapshot_documents = || {\n'
    '        let documents = server.documents.lock();\n'
    '        [document_uri, on_type_uri].map(|uri| {\n'
    '            let document = documents.get(uri).expect("opened document must remain present");\n'
    '            (\n'
    '                document.text.clone(),\n'
    '                document.version,\n'
    '                document\n'
    '                    .generation\n'
    '                    .load(std::sync::atomic::Ordering::SeqCst),\n'
    '            )\n'
    '        })\n'
    '    };\n'
    '    let before_withdrawn_requests = snapshot_documents();\n\n'
    '    // Withdrawn surfaces (#11955): every default-profile request must receive\n',
)
replace_exact(
    policy,
    '        assert_eq!(error.code, crate::protocol::METHOD_NOT_FOUND, "{method} refusal code");\n'
    '        assert!(response.result.is_none(), "{method} refusal cannot carry edits");\n'
    '    }\n\n'
    '    // The proven manual surface stays live through one receipt policy.\n',
    '        assert_eq!(error.code, crate::protocol::METHOD_NOT_FOUND, "{method} refusal code");\n'
    '        assert!(response.result.is_none(), "{method} refusal cannot carry edits");\n'
    '        assert_eq!(\n'
    '            snapshot_documents(),\n'
    '            before_withdrawn_requests,\n'
    '            "{method} must not change source bytes, client version, or document generation",\n'
    '        );\n'
    '        assert!(\n'
    '            server.provider_decision_traces.lock().get(PROVIDER).is_none(),\n'
    '            "{method} must refuse before formatter admission",\n'
    '        );\n'
    '    }\n\n'
    '    // The proven manual surface stays live through one receipt policy.\n',
)

lifecycle = Path("crates/perl-lsp-rs/src/runtime/text_sync/lifecycle.rs")
text = lifecycle.read_text(encoding="utf-8")
marker = "withdrawn_will_save_wait_until_preserves_source_and_generation"
if marker in text:
    raise SystemExit("will-save withdrawal proof already exists")
addition = r'''

#[cfg(test)]
mod withdrawal_tests {
    use super::*;

    #[test]
    fn withdrawn_will_save_wait_until_preserves_source_and_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = LspServer::new();
        let uri = "file:///withdrawn-will-save-state.pl";
        server.test_apply_did_open(uri, "sub test{my$foo=42;return$foo;}\n", 7)?;

        let snapshot = || {
            let documents = server.documents.lock();
            let document = documents.get(uri).expect("opened document must remain present");
            (
                document.text.clone(),
                document.version,
                document
                    .generation
                    .load(std::sync::atomic::Ordering::SeqCst),
            )
        };
        let before = snapshot();

        let error = server
            .handle_will_save_wait_until(Some(json!({
                "textDocument": { "uri": uri, "version": 7 },
                "reason": 1
            })))
            .err()
            .ok_or("withdrawn willSaveWaitUntil must refuse")?;

        assert_eq!(error.code, crate::protocol::METHOD_NOT_FOUND);
        assert_eq!(snapshot(), before, "refusal must preserve source/version/generation");
        Ok(())
    }
}
'''
lifecycle.write_text(text + addition, encoding="utf-8")
PY

cargo fmt --all
cargo fmt --all -- --check

# Proof surface regenerated against current main (#12045 adoption):
# the former `lsp_secondary_format_routes_withdrawn` integration target never
# existed; its boundary proof lives inside lsp_formatting_e2e (second reviewed
# commit), which is exercised below. lsp_3_17_compliance_tests and
# server_capabilities_snapshot_test continue under their current names.
cargo test -p perl-lsp-rs --lib formatting_policy --locked
cargo test -p perl-lsp-rs --lib withdrawn_will_save_wait_until_preserves_source_and_generation --locked
cargo test -p perl-lsp-rs --test lsp_lifecycle_events_test --locked
cargo test -p perl-lsp-rs --test lsp_formatting_e2e --locked
cargo test -p perl-lsp-rs --test lsp_batteries_e2e_workflow_test --locked
cargo test -p perl-lsp-rs --test lsp_3_17_formatting_tests --locked
cargo test -p perl-lsp-rs --test lsp_capabilities_snapshot --locked

cargo run -q -p xtask -- provider-confidence-matrix > /dev/null
cargo xtask check-support-claims
cargo xtask check-test-wiring
cargo xtask check-architecture
cargo clippy -p perl-lsp-rs -p xtask --all-targets --all-features --locked -- -D warnings
git diff --check

# Publication boundary (#12045 review): this PR-triggered lane is read-only.
# It reconstructs and proves locally, records durable evidence as an artifact,
# and never mutates repository refs from untrusted candidate code. Candidate
# refs are published only by an explicitly authorized writer from trusted code.
if ! git diff --quiet; then
  git diff --binary > "$evidence_dir/strengthened-edits.patch"
  echo "throwaway reconstruction edits (proof-exercised) preserved in strengthened-edits.patch" \
    >> "$evidence_dir/reconstruction-summary.txt"
  git checkout -- .
fi
worktree_status="$(git status --porcelain=v1)"
{
  echo "event-head: $REBUILD_EVENT_HEAD_SHA"
  echo "reconstructed-head: $(git rev-parse HEAD)"
  printf '%s\n' "$worktree_status"
} > "$evidence_dir/reconstruction-summary.txt"
if [ -n "$worktree_status" ]; then
  echo "verification left unaccounted workspace changes:" >&2
  printf '%s\n' "$worktree_status" >&2
  exit 1
fi
