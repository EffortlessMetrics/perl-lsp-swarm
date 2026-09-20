#!/usr/bin/env python3
"""Offline deterministic validator for the vim/vim-lsp subject+config authority (#11369).

Checks the governed artifacts under .ci/editor-clients/ against the #11369
contract: content-bound subject pinning, exact perllsp --stdio command identity,
consumed (not copied) root/filetype policy, disabled experimental workspace
folders, source-bound public-surface classifications, and redirect of copied
pin/config surfaces. Standard library only; no network access.

Usage:
    python scripts/ux/validate_vim_vim_lsp_contract.py [--quiet]

Exit codes: 0 = contract holds, 1 = violation found, 64 = usage error.
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
EDITOR_CLIENTS = REPO_ROOT / ".ci" / "editor-clients"
SUBJECT_PATH = EDITOR_CLIENTS / "vim-vim-lsp-subject.v1.json"
CONFIG_PATH = EDITOR_CLIENTS / "vim-vim-lsp-configuration.v1.json"
SURFACE_PATH = EDITOR_CLIENTS / "vim-vim-lsp-public-surface.v1.json"
ACTIVATION_ROOT_PATH = EDITOR_CLIENTS / "vim-vim-lsp-activation-root.v1.json"
SMOKE_SCRIPT = REPO_ROOT / "scripts" / "ux" / "vim_vim_lsp_smoke.sh"
DRIVER_SCRIPT = REPO_ROOT / "scripts" / "ux" / "vim_vim_lsp_driver.vim"
ADAPTER_SCRIPT = REPO_ROOT / "scripts" / "test" / "vim-clients" / "vim-lsp-adapter.vim"

HEX40 = re.compile(r"^[0-9a-f]{40}$")

CLASSIFICATIONS = {
    "stable_public_action_or_event",
    "public_but_version_sensitive_autoload",
    "instrument_only_hook_requiring_justification",
    "not_exposed_by_pinned_client",
    "unknown_not_proven",
}

FORBIDDEN_KEY_SUBSTRINGS = (
    # Host behavior / support / public / release state must never become a key
    # on the subject or configuration objects (#11369 negative control 9).
    "support_tier",
    "supported_version_row",
    "maintained_row",
    "host_receipt",
    "public_artifact",
    "release_channel",
    "readiness",
    "behavior_proof",
)

REQUIRED_CONFIG_LAWS = (
    "perllsp --stdio",
    "#7762 remains the sole filetype/root authority",
    ".perl-lsp.toml remains the preferred shared project configuration source",
    "client channel, not a second server schema",
    "workspace-contained relative paths unless #4998 explicitly admits a trusted channel",
    "no blanket .t/.pm/.cgi/.fcgi/POD/XS/template activation rule",
    "#10960 directly earns a compatible optional cell",
)


class Violations:
    def __init__(self) -> None:
        self.items: list[str] = []

    def add(self, message: str) -> None:
        self.items.append(message)

    @property
    def ok(self) -> bool:
        return not self.items


def load_json(path: Path) -> dict:
    try:
        with path.open("r", encoding="utf-8") as handle:
            return json.load(handle)
    except FileNotFoundError:
        raise SystemExit(f"FAIL: required artifact missing: {path.relative_to(REPO_ROOT)}")
    except json.JSONDecodeError as exc:
        raise SystemExit(f"FAIL: {path.name} is not valid JSON: {exc}")


def canonical_digest(document: dict) -> str:
    encoded = json.dumps(document, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def iter_keys(node: object):
    if isinstance(node, dict):
        for key, value in node.items():
            yield key
            yield from iter_keys(value)
    elif isinstance(node, list):
        for item in node:
            yield from iter_keys(item)


def check_forbidden_keys(document: dict, name: str, violations: Violations) -> None:
    for key in iter_keys(document):
        lowered = str(key).lower()
        for forbidden in FORBIDDEN_KEY_SUBSTRINGS:
            if forbidden in lowered:
                violations.add(
                    f"{name}: forbidden state key '{key}' (host/support/public "
                    "state may not live on this object)"
                )


def check_evidence_blocks(document: dict, name: str, violations: Violations) -> None:
    """Every evidence/source citation must retain path + blob identity (#NC8)."""

    def walk(node: object, trail: str) -> None:
        if isinstance(node, dict):
            if "git_blob_sha1" in node:
                blob = node.get("git_blob_sha1")
                if not isinstance(blob, str) or not HEX40.match(blob):
                    violations.add(f"{name}: evidence at {trail} has non-SHA1 blob '{blob}'")
                if not node.get("path"):
                    violations.add(f"{name}: evidence at {trail} missing source path")
                if "line" in node and not isinstance(node["line"], int):
                    violations.add(f"{name}: evidence at {trail} has non-integer line")
                if not any(key in node for key in ("note", "line", "role")):
                    violations.add(f"{name}: evidence at {trail} carries neither line, note, nor role")
            for key, value in node.items():
                walk(value, f"{trail}.{key}")
        elif isinstance(node, list):
            for index, item in enumerate(node):
                walk(item, f"{trail}[{index}]")

    walk(document, "$")


def validate_subject(subject: dict, violations: Violations) -> None:
    if subject.get("schema_version") != "vim_lsp_subject.v1":
        violations.add("subject: unexpected schema_version")

    upstream = subject.get("upstream") or {}
    commit = upstream.get("selected_commit")
    if not isinstance(commit, str) or not HEX40.match(commit):
        violations.add("subject: selected_commit must be an exact 40-hex commit (no floating refs)")
    digest = upstream.get("tree_digest") or {}
    if digest.get("algorithm") != "git-tree-sha1":
        violations.add("subject: tree_digest.algorithm must be git-tree-sha1")
    tree_value = digest.get("value")
    if not isinstance(tree_value, str) or not HEX40.match(tree_value):
        violations.add("subject: tree_digest.value must be an exact 40-hex tree SHA")
    if not upstream.get("commit_author_date"):
        violations.add("subject: commit date missing")

    governance = subject.get("pin_governance") or {}
    policy = str(governance.get("floating_branch_policy", ""))
    if "floating" not in policy or "invalid" not in policy:
        violations.add("subject: floating_branch_policy must declare floating refs invalid")
    replacement = str(governance.get("replacement_law", ""))
    if "does not replace this pin" not in replacement:
        violations.add("subject: replacement law must forbid silent pin movement")

    prerequisites = subject.get("upstream_theoretical_prerequisites") or {}
    status = str(prerequisites.get("status", ""))
    if "never_a_maintained_support_floor" not in status:
        violations.add("subject: theoretical prerequisites must disclaim support-floor status")
    if "#10966" not in str(prerequisites.get("maintained_floor_authority", "")):
        violations.add("subject: maintained floor rows must point at #10966, not here")

    identity = subject.get("expected_content_identity") or {}
    entries = identity.get("entry_files") or []
    if not entries:
        violations.add("subject: expected_content_identity.entry_files must not be empty")

    verification = subject.get("last_deliberate_verification") or {}
    if not verification.get("date_utc") or not verification.get("method"):
        violations.add("subject: last_deliberate_verification incomplete")


def validate_configuration(config: dict, violations: Violations) -> None:
    if config.get("schema_version") != "vim_lsp_configuration.v1":
        violations.add("config: unexpected schema_version")

    claim_boundary = str(config.get("claim_boundary", ""))
    if "proves no behavior" not in claim_boundary:
        violations.add("config: claim_boundary must deny behavior proof")

    reference = config.get("subject_reference") or {}
    if reference.get("manifest") != SUBJECT_PATH.relative_to(REPO_ROOT).as_posix():
        violations.add("config: subject_reference.manifest must point at the subject artifact path")

    registration = config.get("registration") or {}
    argv = (registration.get("command_identity") or {}).get("argv") or []
    if argv != ["perllsp", "--stdio"]:
        violations.add(f"config: command identity must be exactly ['perllsp','--stdio'], got {argv}")

    if registration.get("allowlist_filetypes") != ["perl"]:
        violations.add("config: allowlist must be exactly ['perl']")

    allowlist_text = json.dumps(registration.get("allowlist_filetypes"))
    if any(ext in allowlist_text for ext in (".t", ".pm", ".cgi", ".fcgi", "pod", "xs")):
        violations.add("config: blanket extension activation leaked into the shared client config")

    root_contract = registration.get("root_uri_contract") or {}
    if root_contract.get("authority_issue") != 7762:
        violations.add("config: root contract must cite #7762 as sole authority")
    if root_contract.get("authority_manifest") != ACTIVATION_ROOT_PATH.relative_to(REPO_ROOT).as_posix():
        violations.add("config: root contract must reference the activation-root manifest by path")
    if "markers" in root_contract:
        violations.add(
            "config: root_uri_contract must not carry its own marker policy (#7762 owns markers)"
        )

    workspace_channel = registration.get("workspace_configuration_channel") or {}
    shape_text = json.dumps(workspace_channel.get("shape") or {})
    if "includePaths" not in shape_text:
        violations.add("config: positive example must exercise perl.workspace.includePaths")
    example_law = str(workspace_channel.get("positive_example_law") or "")
    if "workspace-contained relative" not in example_law or "#4998" not in example_law:
        violations.add(
            "config: positive include-path example must be workspace-contained relative per #4998"
        )

    folders = registration.get("experimental_workspace_folders") or {}
    if folders.get("g:lsp_experimental_workspace_folders_default") is not False:
        violations.add("config: experimental workspace folders must default to false")
    if "#10960" not in str(folders.get("law", "")):
        violations.add("config: workspace-folder enablement must require #10960 authorization")

    hooks = registration.get("logging_and_status_hooks") or {}
    if str(hooks.get("model", "")) != "bounded instrument configuration only":
        violations.add("config: logging/status hooks must stay bounded instrument configuration")

    laws = [str(law) for law in (config.get("laws") or [])]
    for required in REQUIRED_CONFIG_LAWS:
        if not any(required in law for law in laws):
            violations.add(f"config: laws missing required clause: {required!r}")


def validate_public_surface(surface: dict, violations: Violations) -> None:
    if surface.get("schema_version") != "vim_lsp_public_surface.v1":
        violations.add("surface: unexpected schema_version")

    declared = surface.get("classification_values") or []
    if set(declared) != CLASSIFICATIONS:
        violations.add("surface: classification_values drifted from the fixed vocabulary")

    surfaces = surface.get("surfaces") or []
    if not surfaces:
        violations.add("surface: inventory must not be empty")
    for row in surfaces:
        label = row.get("surface", "<unnamed>")
        classification = row.get("classification")
        if classification not in CLASSIFICATIONS:
            violations.add(f"surface: '{label}' has unknown classification {classification!r}")
        evidence = row.get("evidence") or []
        if not evidence:
            violations.add(f"surface: '{label}' classified without retained source evidence")
        if (
            classification == "instrument_only_hook_requiring_justification"
            and not row.get("justification")
        ):
            violations.add(f"surface: '{label}' instrument hook lacks explicit justification")

    binding = surface.get("consumer_binding") or {}
    if binding.get("canonical_driver") != DRIVER_SCRIPT.relative_to(REPO_ROOT).as_posix():
        violations.add("surface: consumer_binding.canonical_driver must name the canonical driver")


def semantic_marker(marker: str) -> str:
    """Collapse vim-lsp's directory spelling onto the semantic marker name.

    The cross-editor activation contract stores semantic names (`.git`). The
    pinned vim-lsp helper treats a marker as a directory only when its spelling
    ends in `/`, so the canonical driver may legitimately project one semantic
    marker onto both client spellings (`.git/` directory, `.git` file for
    linked worktrees/submodules). The manifest itself keeps only the semantic
    spelling.
    """
    return marker[:-1] if marker.endswith("/") else marker


def validate_activation_root_consumption(violations: Violations) -> None:
    """Root markers consumed from #7762, never re-declared with drift (#NC5).

    The driver may only carry an authority marker verbatim or as its
    trailing-slash directory spelling, and its semantic projection must equal
    the authority list exactly.
    """
    activation = load_json(ACTIVATION_ROOT_PATH)
    markers = ((activation.get("root") or {}).get("markers")) or []
    if not markers:
        raise SystemExit(f"FAIL: {ACTIVATION_ROOT_PATH.name} lost its marker list")
    authority = sorted(markers)
    authority_set = set(authority)

    driver_text = DRIVER_SCRIPT.read_text(encoding="utf-8")
    adapter_text = ADAPTER_SCRIPT.read_text(encoding="utf-8")
    required_adapter_fragments = (
        "function! VimLspHostClientRootMarkers() abort",
        "call extend(l:markers, ['.git/', '.git'])",
        "expand('%:p'), VimLspHostClientRootMarkers()",
    )
    for fragment in required_adapter_fragments:
        if fragment not in adapter_text:
            violations.add(f"adapter: canonical marker projection is missing {fragment!r}")

    if "execute 'source ' . fnameescape(expand('$PERLLSP_VIM_ADAPTER'))" not in driver_text:
        violations.add("driver: integration rail must source the canonical Vim adapter")
    if "VimLspHostRegister()" not in driver_text:
        violations.add("driver: integration rail must use adapter-owned registration")

    if "VimLspHostRegister()" in driver_text:
        # The adapter-owned registration reaches its root callback through the
        # canonical projection checked above. There is intentionally no second
        # nearest-parent call in this rail to inspect.
        return

    direct_calls = re.findall(
        r"find_nearest_parent_file_directory\((.*?)\)", driver_text, re.DOTALL
    )
    if direct_calls and all("VimLspHostClientRootMarkers()" in call for call in direct_calls):
        return

    call_match = re.search(
        r"find_nearest_parent_file_directory\([\s\\]*[^,]+,[\s\\]*([^\)]+)\)",
        driver_text,
        re.DOTALL,
    )
    if not call_match:
        violations.add("driver: nearest-parent marker call not found for consumption check")
        return
    marker_expression = call_match.group(1).strip()
    if marker_expression == "VimLspHostClientRootMarkers()":
        # The deep rail delegates to the canonical adapter projection. Its
        # source-level contract is checked below; there is no second marker
        # list to compare here.
        return
    literal_match = re.fullmatch(r"\[([^\]]*)\]", marker_expression, re.DOTALL)
    if not literal_match:
        violations.add("driver: nearest-parent call must consume VimLspHostClientRootMarkers()")
        return
    driver_markers = re.findall(r"'([^']+)'", literal_match.group(1))

    unsanctioned = sorted(
        marker
        for marker in driver_markers
        if marker not in authority_set and semantic_marker(marker) not in authority_set
    )
    if unsanctioned:
        violations.add(
            "driver: root markers outside #7762 authority and its directory spelling: "
            f"{unsanctioned} (authority={authority})"
        )

    semantic_projection = sorted({semantic_marker(marker) for marker in driver_markers})
    if semantic_projection != authority:
        violations.add(
            "driver: root marker copy drifted from #7762 activation-root manifest "
            f"(driver_semantic={semantic_projection}, authority={authority})"
        )


def validate_smoke_redirect(subject_commit: str, violations: Violations) -> None:
    """The smoke script must consume the governed pin, not keep its own (#NC10)."""
    text = SMOKE_SCRIPT.read_text(encoding="utf-8")
    if SUBJECT_PATH.name not in text:
        violations.add("smoke: does not reference the governed subject manifest")
    hardcoded = re.findall(r"^expected_vim_lsp_ref=([0-9a-f]{40})\s*$", text, re.MULTILINE)
    if hardcoded:
        violations.add(
            "smoke: keeps an independent hard-coded vim-lsp ref instead of consuming the subject manifest"
        )
    if subject_commit in text:
        violations.add(
            "smoke: embeds the pinned commit literal; it must extract the pin from the subject manifest"
        )


def main(argv: list[str]) -> int:
    quiet = False
    for arg in argv:
        if arg == "--quiet":
            quiet = True
        else:
            print(__doc__)
            return 64

    violations = Violations()

    subject = load_json(SUBJECT_PATH)
    config = load_json(CONFIG_PATH)
    surface = load_json(SURFACE_PATH)

    for name, document in (
        ("subject", subject),
        ("config", config),
        ("surface", surface),
    ):
        check_forbidden_keys(document, name, violations)
        check_evidence_blocks(document, name, violations)

    validate_subject(subject, violations)
    validate_configuration(config, violations)
    validate_public_surface(surface, violations)
    validate_activation_root_consumption(violations)
    validate_smoke_redirect(str((subject.get("upstream") or {}).get("selected_commit")), violations)

    if not violations.ok:
        for item in violations.items:
            print(f"FAIL: {item}")
        print(f"vim/vim-lsp contract validation FAILED with {len(violations.items)} violation(s)")
        return 1

    if not quiet:
        print("vim/vim-lsp contract validation PASSED")
        print(f"subject.digest        sha256:{canonical_digest(subject)}")
        print(f"configuration.digest  sha256:{canonical_digest(config)}")
        print(f"public_surface.digest sha256:{canonical_digest(surface)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
