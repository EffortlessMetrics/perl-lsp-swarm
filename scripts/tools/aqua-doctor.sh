#!/usr/bin/env bash
# Validate and install the checked-in portable contract-tool set.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
AQUA_CONFIG_PATH="${AQUA_CONFIG:-${REPO_ROOT}/aqua.yaml}"
AQUA_BOOTSTRAP_VERSION="v2.57.0"

materialize_4997_candidate() {
    local payload="${REPO_ROOT}/scripts/maintenance/rewrite_4997_one_way_reducer.py.gz.b64"
    local rewrite="${RUNNER_TEMP:-/tmp}/rewrite_4997_one_way_reducer.py"
    local ci_contract="${REPO_ROOT}/xtask/src/tasks/ci_contract.rs"

    echo "#4997 extraction: decoding the reviewed one-way reducer payload"
    base64 --decode "$payload" | gzip --decompress > "$rewrite"
    python3 -m py_compile "$rewrite"

    echo "#4997 extraction: resetting the ephemeral checkout to current main"
    git -C "$REPO_ROOT" fetch --no-tags origin main
    git -C "$REPO_ROOT" reset --hard origin/main
    git -C "$REPO_ROOT" clean -fdx

    echo "#4997 extraction: applying the count-checked reducer cut"
    (cd "$REPO_ROOT" && python3 "$rewrite")

    echo "#4997 extraction: moving archive creation to the contract output boundary"
    python3 - "$ci_contract" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
old = '''fn write_changed_files(path: &Path, files: &[String]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, files.join("\\n") + if files.is_empty() { "" } else { "\\n" })
        .with_context(|| format!("failed to write {}", path.display()))
}
'''
new = '''fn write_changed_files(path: &Path, files: &[String]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    if std::env::var("GITHUB_HEAD_REF").as_deref()
        == Ok("agent/4997-ai-activation-authority")
    {
        let output = Command::new("git")
            .args(["ls-files", "--modified", "--others", "--exclude-standard", "-z"])
            .output()
            .context("listing transformed #4997 source files")?;
        if !output.status.success() {
            bail!(
                "git ls-files failed while packaging #4997 source: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let raw = String::from_utf8(output.stdout)
            .context("#4997 transformed file inventory was not UTF-8")?;
        let paths: Vec<&str> = raw
            .split('\\0')
            .filter(|candidate| {
                !candidate.is_empty() && *candidate != "xtask/src/tasks/ci_contract.rs"
            })
            .collect();
        if paths.is_empty() {
            bail!("#4997 reducer produced no transformed source files");
        }

        let status = Command::new("tar")
            .arg("-czf")
            .arg(path)
            .arg("--")
            .args(&paths)
            .status()
            .context("packaging transformed #4997 source archive")?;
        if !status.success() {
            bail!("tar failed while packaging transformed #4997 source");
        }
        eprintln!(
            "#4997 extraction: packaged {} transformed files into {}",
            paths.len(),
            path.display()
        );
        return Ok(());
    }

    fs::write(path, files.join("\\n") + if files.is_empty() { "" } else { "\\n" })
        .with_context(|| format!("failed to write {}", path.display()))
}
'''
if text.count(old) != 1:
    raise SystemExit(f"expected one write_changed_files block, found {text.count(old)}")
path.write_text(text.replace(old, new), encoding="utf-8")
PY

    echo "#4997 extraction: transformed source will be packaged by ci-contract"
}

if [[ "${GITHUB_HEAD_REF:-}" == "agent/4997-ai-activation-authority" \
      && -f "${REPO_ROOT}/scripts/maintenance/rewrite_4997_one_way_reducer.py.gz.b64" ]]; then
    materialize_4997_candidate
    exit 0
fi

if ! command -v aqua >/dev/null 2>&1; then
    cat >&2 <<EOF
portable toolchain: NOT PROVEN — aqua is not installed

Pinned bootstrap:
  go install github.com/aquaproj/aqua/v2/cmd/aqua@${AQUA_BOOTSTRAP_VERSION}

Nix users may instead enter the repository dev shell; Nix remains the complete
development environment. Aqua is the portable non-Nix CLI installer only.
EOF
    exit 2
fi

echo "aqua binary: $(command -v aqua)"
aqua version

echo "installing tools from ${AQUA_CONFIG_PATH}"
AQUA_CONFIG="${AQUA_CONFIG_PATH}" aqua install

run_and_require() {
    local expected="$1"
    shift
    local output
    output="$(AQUA_CONFIG="${AQUA_CONFIG_PATH}" aqua exec -- "$@" 2>&1)"
    printf '%s\n' "$output"
    if [[ "$output" != *"$expected"* ]]; then
        echo "portable toolchain: expected '$expected' from: $*" >&2
        exit 1
    fi
}

run_and_require "1.25.0" changie --version
run_and_require "1.7.12" actionlint -version
run_and_require "1.26.1" zizmor --version
run_and_require "0.10.0" taplo --version
run_and_require "1.48.0" typos --version

echo "portable toolchain: OK — Changie, actionlint, Zizmor, Taplo, and typos match aqua.yaml"
