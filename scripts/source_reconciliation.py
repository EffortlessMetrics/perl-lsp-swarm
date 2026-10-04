#!/usr/bin/env python3
"""Compose sync-divergence v2 with complete, current source reconciliation.

This is a reconciliation preflight, not product qualification, authenticated
producer evidence, source admission, or release authority. See the runbook.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from contextlib import contextmanager
from pathlib import Path

LEGACY_LEDGER = "source_reconciliation_ledger.v1"
LEDGER = "source_reconciliation_ledger.v2"
PACKET = "source_reconciliation.v1"
TERMINAL = {
    "port_to_swarm", "already_equivalent_in_swarm",
    "superseded_by_swarm_architecture", "deliberately_abandoned",
    "publication_lineage_only", "publication_context_translation",
    "merge_ancestry",
}
PRIMITIVE_MAP = {
    "port_to_swarm": "port_to_swarm",
    "already_equivalent_in_swarm": "already_equivalent_in_swarm",
    "superseded_by_swarm_architecture": "superseded_by_newer_architecture",
    "deliberately_abandoned": "deliberately_abandoned",
    "publication_lineage_only": "release_lineage_only",
    "publication_context_translation": None,
    "merge_ancestry": None,
}
# An explicit control-test exception, not a global exemption for tests/scripts.
CONTROL_PATHS = {
    "scripts/publication_sync_check.py",
    "scripts/tests/test-publication-sync-contract.py",
    "schemas/publication_sync.v2.schema.json",
    ".github/workflows/publication-sync-contract.yml",
}
# These are the reviewed, dated/publication documents in the fixed source
# cut. A new path needs its own semantic or projection decision; extension or
# directory alone never makes executable work or active guidance lineage-only.
LINEAGE_ONLY_PATHS = {
    "CHANGELOG.md", "RELEASE_HISTORY.md", "docs/project/RELEASE_CHECKLIST.md",
    "docs/releases/0.15.2-closeout-audit.md", "docs/releases/README.md",
    "docs/releases/v0.13.0-rc1.md", "docs/releases/v0.13.1.md",
    "docs/releases/v0.13.4.md", "docs/releases/v0.14.0.md",
    "docs/releases/v0.15.1.md", "docs/releases/v0.15.2.md",
    "docs/releases/v0.16.0.md", "docs/releases/v0.17.0.md",
    "docs/swarm/source-syncs/2026-07-15-final-closeout-bdfde90d7.md",
    "docs/swarm/source-syncs/2026-07-15-modernization-b4b55aa3e.md",
    "docs/swarm/source-syncs/2026-07-15-workspace-capabilities-bd3eb11b2.md",
}
MAX_MERGE_SCRATCH_BYTES = 64 * 1024 * 1024
ROLES = {"source": "patch_equivalence_upstream", "boundary": "history_limit",
         "target": "release_head"}
PRIMITIVE_KEYS = {
    "schema_version", "subjects", "ledger", "verdict", "population_digest",
    "target_unique_commits", "excluded_merge_commits", "excluded_merge_ancestry",
    "excluded_release_lineage_commits", "accepted_commits", "unresolved_commits", "errors",
}
GIT_LOCATION_ENV = {
    "GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_PREFIX",
    "GIT_NAMESPACE", "GIT_CEILING_DIRECTORIES", "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_SHALLOW_FILE", "GIT_CONFIG",
}


def git_environment():
    env = {key: value for key, value in os.environ.items() if key not in GIT_LOCATION_ENV}
    env["GIT_NO_LAZY_FETCH"] = "1"
    env["GIT_NO_REPLACE_OBJECTS"] = "1"
    return env


def isolated_merge_environment():
    """Exclude ambient configuration and object redirection from merge-tree."""
    env = {key: value for key, value in git_environment().items()
           if not key.startswith("GIT_CONFIG") and key != "GIT_TEMPLATE_DIR"}
    env["GIT_CONFIG_NOSYSTEM"] = "1"
    env["GIT_CONFIG_GLOBAL"] = os.devnull
    env["GIT_ATTR_NOSYSTEM"] = "1"
    return env


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=True,
                                    separators=(",", ":")).encode("ascii")).hexdigest()


class Git:
    def __init__(self, repo):
        self.repo = Path(repo).resolve()
        self.current_trees = {}

    def run(self, *args):
        proc = subprocess.run(["git", "--literal-pathspecs", "-C", str(self.repo), *args],
                              env=git_environment(), capture_output=True)
        if proc.returncode:
            raise ValueError(proc.stderr.decode("utf-8", "replace").strip())
        return proc.stdout

    def text(self, *args):
        return self.run(*args).decode("utf-8", "surrogateescape").strip()

    def ancestor(self, older, newer):
        try:
            self.run("merge-base", "--is-ancestor", older, newer)
            return True
        except ValueError:
            return False

    def original_graph(self):
        # Replacement refs are disabled on every call. Legacy graft files are
        # separate overlays, so reject their presence rather than trusting them.
        if "GIT_GRAFT_FILE" in os.environ:
            raise ValueError("legacy graft environment cannot establish original ancestry")
        grafts = Path(self.text("rev-parse", "--path-format=absolute", "--git-path", "info/grafts"))
        if grafts.exists():
            raise ValueError("legacy graft file cannot establish original ancestry")

    def exact_commit(self, value):
        return isinstance(value, str) and bool(re.fullmatch("[0-9a-f]{40}", value)) and self.text(
            "rev-parse", value + "^{commit}") == value

    def entry(self, commit, path):
        if commit in self.current_trees:
            return self.current_trees[commit].get(path)
        raw = self.run("ls-tree", "-z", commit, "--", path)
        if not raw:
            return None
        records = raw.rstrip(b"\0").split(b"\0")
        if len(records) != 1:
            raise ValueError("declared literal path resolved to multiple entries")
        metadata, name = records[0].split(b"\t", 1)
        if name.decode("utf-8", "surrogateescape") != path:
            raise ValueError("declared path did not resolve literally")
        mode, kind, sha = metadata.decode("ascii").split()
        return {"mode": mode, "type": kind, "sha": sha}

    def prime_current_tree(self, commit):
        entries = {}
        for record in self.run("ls-tree", "-r", "-t", "-z", commit).rstrip(b"\0").split(b"\0"):
            if not record:
                continue
            metadata, path = record.split(b"\t", 1)
            mode, kind, sha = metadata.decode("ascii").split()
            entries[path.decode("utf-8", "surrogateescape")] = {"mode": mode, "type": kind, "sha": sha}
        self.current_trees[commit] = entries

    def paths(self, commit, parents):
        args = ["diff-tree", "--no-renames", "--no-commit-id", "--name-only", "-r", "-z"]
        args += [parents[0], commit] if parents else ["--root", commit]
        return sorted(set(self.run(*args).decode("utf-8", "surrogateescape").rstrip("\0").split("\0")) - {""})

    @contextmanager
    def isolated_merge_store(self):
        """Use original objects read-only, writing automatic trees only to scratch."""
        source_objects = Path(self.text("rev-parse", "--path-format=absolute", "--git-path", "objects"))
        if not source_objects.is_dir():
            raise ValueError("source object directory is unavailable for isolated merge proof")
        with tempfile.TemporaryDirectory(prefix="source-merge-baseline-") as root:
            bare = Path(root) / "proof.git"
            proc = subprocess.run(["git", "init", "--bare", "--quiet", str(bare)],
                                  env=isolated_merge_environment(), capture_output=True)
            if proc.returncode:
                raise ValueError("cannot initialize isolated merge proof: " + proc.stderr.decode("utf-8", "replace"))
            alternate = bare / "objects" / "info" / "alternates"
            alternate.write_bytes((source_objects.as_posix() + "\n").encode("utf-8"))
            self.merge_store = bare
            try:
                yield
            finally:
                del self.merge_store

    def merge_run(self, *args):
        if not hasattr(self, "merge_store"):
            raise ValueError("isolated merge proof store is absent")
        return subprocess.run(["git", "--literal-pathspecs", "-C", str(self.merge_store), *args],
                              env=isolated_merge_environment(), capture_output=True)

    def merge_baseline(self, first, side, actual):
        proc = self.merge_run("merge-tree", "--write-tree", "--name-only", "-z",
                              "--no-messages", first, side)
        if proc.returncode not in (0, 1):
            raise ValueError("automatic merge baseline unavailable: " + proc.stderr.decode("utf-8", "replace"))
        fields = proc.stdout.split(b"\0")
        if len(fields) < 2 or fields[-1] != b"" or not re.fullmatch(rb"[0-9a-f]{40}", fields[0]):
            detail = proc.stderr.decode("utf-8", "replace").strip()[:240]
            raise ValueError(f"malformed automatic merge baseline output (status {proc.returncode}): {detail}")
        conflicts = fields[1:-1]
        if (proc.returncode == 0 and conflicts) or (proc.returncode == 1 and not conflicts):
            raise ValueError("automatic merge baseline status/path mismatch")
        if any(not path for path in conflicts) or len(conflicts) != len(set(conflicts)):
            raise ValueError("malformed automatic merge conflict paths")
        tree = fields[0].decode("ascii")
        kind = self.merge_run("cat-file", "-t", tree)
        if kind.returncode or kind.stdout.strip() != b"tree":
            raise ValueError("automatic merge baseline is not a tree")
        diff = self.merge_run("diff-tree", "--no-renames", "--no-commit-id", "--name-only",
                              "-r", "-z", tree, actual)
        if diff.returncode:
            raise ValueError("cannot compare automatic and actual merge trees")
        differences = set(diff.stdout.rstrip(b"\0").split(b"\0")) - {b""}
        paths = sorted((differences | set(conflicts)))
        scratch_bytes = sum(path.stat().st_size for path in (self.merge_store / "objects").rglob("*")
                            if path.is_file())
        if scratch_bytes > MAX_MERGE_SCRATCH_BYTES:
            raise ValueError("isolated merge proof exceeds bounded scratch budget")
        version = self.merge_run("version")
        if version.returncode or not version.stdout.startswith(b"git version "):
            raise ValueError("unbound automatic merge Git version")
        return {"tool": version.stdout.decode("ascii", "replace").strip(),
                "policy": "isolated-bare-alternate; merge-tree --write-tree --name-only -z --no-messages; diff-tree --no-renames",
                "ordered_parents": [first, side], "tree": tree,
                "status": "clean" if proc.returncode == 0 else "conflicted",
                "conflict_paths": [p.decode("utf-8", "surrogateescape") for p in sorted(conflicts)],
                "resolution_paths": [p.decode("utf-8", "surrogateescape") for p in paths]}

    def population(self, subjects):
        # Cache only the two current trees; do not retain every historical tree.
        self.prime_current_tree(subjects["source"])
        self.prime_current_tree(subjects["target"])
        lines = self.text("rev-list", "--reverse", "--topo-order", "--parents",
                          subjects["target"], "^" + subjects["boundary"]).splitlines()
        rows = []
        for line in lines:
            commit, *parents = line.split()
            paths = self.paths(commit, parents)
            rows.append({"commit": commit, "parents": parents,
                         "subject": self.text("show", "-s", "--format=%s", commit),
                         "changed_paths": paths,
                         "current_bindings": [{"path": p,
                             "target": self.entry(subjects["target"], p),
                             "source": self.entry(subjects["source"], p)} for p in paths]})
        return rows


def product_path(path):
    return bool(set(path.replace("\\", "/").split("/")) & {
        "src", "lib", "bin", "t", "test", "tests", "testing", "examples", "xt"
    }) or Path(path).suffix.lower() in {".rs", ".c", ".h", ".hpp", ".cpp", ".pm", ".pl", ".t"}


def context_path(path):
    return path in CONTROL_PATHS or (not product_path(path) and (
        path.startswith(".github/") or path == ".ci/policies/required-checks.toml"))


def merge_effects(git, subjects, row):
    if len(row["parents"]) != 2:
        raise ValueError("merge ancestry inspection requires exactly two parents")
    first, side = row["parents"]
    side_work = git.text("rev-list", "--parents", side, "^" + first).splitlines()
    side_paths = set()
    for line in side_work:
        commit, *parents = line.split()
        side_paths.update(git.paths(commit, parents))
    baseline = git.merge_baseline(first, side, row["commit"])
    paths = baseline["resolution_paths"]
    return {"side_work": sorted(line.split()[0] for line in side_work),
            "side_work_paths": sorted(side_paths), "baseline": baseline,
            "resolution_paths": paths,
            "resolution_current_bindings": [{"path": path,
                "target": git.entry(subjects["target"], path),
                "source": git.entry(subjects["source"], path)} for path in paths]}


def cherry(git, subjects):
    output = git.text("cherry", subjects["source"], subjects["target"], subjects["boundary"])
    result = {}
    for line in output.splitlines():
        mark, sha = line.split()
        if mark not in {"+", "-"} or not re.fullmatch("[0-9a-f]{40}", sha):
            raise ValueError("malformed git cherry result")
        result[sha] = mark
    return result


def primitive_check(receipt, subjects, rows, patches):
    errors = []
    if not isinstance(receipt, dict) or set(receipt) != PRIMITIVE_KEYS or receipt.get("schema_version") != 2:
        return ["missing or foreign sync-divergence v2 receipt"]
    if not isinstance(receipt["subjects"], dict):
        return ["malformed primitive subjects"]
    for name, role in ROLES.items():
        item = receipt["subjects"].get(name, {})
        if not isinstance(item, dict):
            errors.append("malformed primitive subject: " + name)
            continue
        if item.get("commit") != subjects[name] or item.get("role") != role or not item.get("input"):
            errors.append("primitive subject/role mismatch: " + name)
    unique = {r["commit"]: r for r in rows if len(r["parents"]) <= 1 and patches.get(r["commit"]) == "+"}
    native_rows = receipt["target_unique_commits"]
    if not isinstance(native_rows, list) or any(not isinstance(r, dict) for r in native_rows):
        return errors + ["malformed primitive population"]
    ids = [r.get("commit") for r in native_rows]
    if len(ids) != len(set(ids)) or set(ids) != set(unique):
        errors.append("primitive population differs from independent git cherry")
    for row in native_rows:
        if row.get("classification") is not None and row.get("classification") not in set(PRIMITIVE_MAP.values()):
            errors.append("foreign primitive classification")
        if row.get("commit") in unique and " ".join(row.get("subject", "").split()) != " ".join(unique[row["commit"]]["subject"].split()):
            errors.append("primitive subject text differs from Git")
    classified = {r["commit"]: r.get("classification") for r in native_rows}
    expected = {
        "unresolved_commits": {c for c, kind in classified.items() if kind is None},
        "excluded_release_lineage_commits": {c for c, kind in classified.items() if kind == "release_lineage_only"},
        "accepted_commits": {c for c, kind in classified.items() if kind is not None and kind != "release_lineage_only"},
    }
    for field, values in expected.items():
        if not isinstance(receipt[field], list) or len(receipt[field]) != len(values) or set(receipt[field]) != values:
            errors.append("primitive derived-list mismatch: " + field)
    derived_verdict = "blocked" if expected["unresolved_commits"] else "pass"
    if receipt["verdict"] != derived_verdict:
        errors.append("primitive verdict inconsistent with its classifications")
    raw = "".join(sha + " " + " ".join(unique[sha]["subject"].split()) + "\n" for sha in sorted(unique))
    if receipt["population_digest"] != hashlib.sha256(raw.encode("utf-8", "surrogateescape")).hexdigest():
        errors.append("primitive population digest mismatch")
    if receipt["errors"] or receipt["verdict"] not in {"pass", "blocked"}:
        errors.append("primitive reports an error or NOT_PROVEN")
    merges = {r["commit"]: r for r in rows if len(r["parents"]) > 1}
    if set(receipt["excluded_merge_commits"]) != set(merges):
        errors.append("primitive excluded merge denominator mismatch")
    ancestry = receipt["excluded_merge_ancestry"]
    if len(ancestry) != len(merges) or any(
        r.get("commit") not in merges or r.get("parents") != merges[r["commit"]]["parents"] for r in ancestry
    ):
        errors.append("primitive merge ancestry mismatch")
    return errors


def semantic_proof(git, subjects, original, bindings, item, projection):
    """Apply the same source proof rules to a work unit or one merge effect."""
    disposition = item["disposition"]
    if disposition not in TERMINAL - {"merge_ancestry"} or item["blocking_decisions"] or not item["authority"]:
        raise ValueError("unsupported disposition or unresolved semantic proof")
    paths = [binding["path"] for binding in bindings]
    if disposition in {"port_to_swarm", "already_equivalent_in_swarm"}:
        proof = item["source_commit"]
        if not git.exact_commit(proof) or not git.ancestor(proof, subjects["source"]):
            raise ValueError("required port/equivalent is not reachable from S")
        if disposition == "port_to_swarm":
            parents = git.text("show", "-s", "--format=%P", proof).split()
            if not set(paths).issubset(git.paths(proof, parents)):
                raise ValueError("credited port did not change every required source path")
            for path in paths:
                if git.entry(proof, path) is None and (not parents or git.entry(parents[0], path) is None):
                    raise ValueError("credited port absence is not an actual deletion")
        for binding in bindings:
            if binding["source"] != git.entry(proof, binding["path"]):
                raise ValueError("port/equivalent has been displaced in current S")
        if disposition == "already_equivalent_in_swarm" and any(
            b["source"] != git.entry(original, b["path"]) for b in bindings
        ):
            raise ValueError("equivalent patch does not survive in current S")
    elif disposition == "publication_lineage_only":
        if not all(path in LINEAGE_ONLY_PATHS for path in paths):
            raise ValueError("unreviewed executable, control or product path cannot be lineage-only")
    elif disposition == "superseded_by_swarm_architecture":
        if not git.exact_commit(item["source_commit"]) or not git.ancestor(item["source_commit"], subjects["source"]):
            raise ValueError("architecture successor is not reachable from S")
    elif disposition == "publication_context_translation":
        if not all(context_path(path) for path in paths):
            raise ValueError("shared product disguised as public context")
        if not projection:
            raise ValueError("public-context row lacks an actual projection tree")
        translated = item["projection_bindings"]
        if sorted(b["path"] for b in translated) != sorted(paths):
            raise ValueError("public context is not bound to every projection path")
        for binding in translated:
            if set(binding) != {"path", "row_id", "row_digest", "entry"} or not binding["row_id"] or not re.fullmatch("[0-9a-f]{64}", binding["row_digest"]):
                raise ValueError("missing projection row identity/digest")
            if git.entry(projection, binding["path"]) != binding["entry"]:
                raise ValueError("projection entry/mode identity mismatch")


def merge_resolution_proof(git, subjects, row, item, projection, legacy):
    effect = row["merge_effects"]
    paths = effect["resolution_paths"]
    if legacy:
        if paths:
            raise ValueError("legacy ledger cannot adjudicate merge resolution effects")
        return False
    entries = item["merge_resolution_dispositions"]
    if not isinstance(entries, list) or any(not isinstance(entry, dict) for entry in entries):
        raise ValueError("malformed merge resolution effects")
    names = [entry.get("path") for entry in entries]
    if any(not isinstance(path, str) for path in names) or len(names) != len(set(names)) or set(names) != set(paths):
        raise ValueError("omitted, duplicate or extra merge resolution effects")
    current = {binding["path"]: binding for binding in effect["resolution_current_bindings"]}
    unresolved = False
    for entry in entries:
        if set(entry) != {"path", "disposition", "authority", "source_commit", "projection_bindings", "blocking_decisions"}:
            raise ValueError("foreign merge resolution disposition shape")
        if entry["disposition"] is None:
            unresolved = True
        else:
            semantic_proof(git, subjects, row["commit"], [current[entry["path"]]], entry, projection)
    return unresolved


def reconcile(git, subjects, ledger=None, primitive=None, projection=None):
    packet = {"schema_version": PACKET, "subjects": subjects,
              "scope": "source_reconciliation_preflight",
              "acceptance_ceiling": ["no_product_execution", "no_authenticated_producer_origin",
                                     "no_source_admission", "no_release_authority"],
              "verdict": "not_proven", "population": [], "errors": [],
              "unresolved_commits": [], "primitive_receipt_sha256": None,
              "projection_tree": projection}
    errors = packet["errors"]
    try:
        git.original_graph()
        if git.text("rev-parse", "--is-shallow-repository") != "false":
            raise ValueError("shallow graph cannot establish complete reconciliation")
        for name, sha in subjects.items():
            if not git.exact_commit(sha):
                raise ValueError("unresolved exact subject: " + name)
        if projection is not None and (not isinstance(projection, str) or not re.fullmatch(
            "[0-9a-f]{40}", projection) or git.text("cat-file", "-t", projection) != "tree"):
            raise ValueError("projection must be an immutable exact tree object")
        if not git.ancestor(subjects["boundary"], subjects["target"]):
            raise ValueError("boundary is not contained in target")
        rows = git.population(subjects)
        packet["population"] = rows
        packet["population_digest"] = digest(rows)
        merges = [row for row in rows if len(row["parents"]) > 1]
        if merges:
            with git.isolated_merge_store():
                for row in merges:
                    row["merge_effects"] = merge_effects(git, subjects, row)
        # Merge evidence participates in the full population digest.
        packet["population_digest"] = digest(rows)
        try:
            patches = cherry(git, subjects)
            packet["patch_equivalence"] = patches
        except ValueError as error:
            patches = {}
            errors.append("patch equivalence NOT_PROVEN: " + str(error))
        if primitive is None:
            errors.append("sync-divergence v2 producer receipt not supplied")
        elif "patch_equivalence" in packet:
            errors.extend(primitive_check(primitive, subjects, rows, patches))
            packet["primitive_receipt_sha256"] = digest(primitive)
        if ledger is None:
            packet["unresolved_commits"] = [r["commit"] for r in rows]
        else:
            if not isinstance(ledger, dict) or set(ledger) != {
                "schema_version", "subjects", "population_digest", "entries"
            } or ledger.get("schema_version") not in {LEDGER, LEGACY_LEDGER}:
                raise ValueError("foreign source reconciliation ledger")
            if ledger["subjects"] != subjects or ledger["population_digest"] != packet["population_digest"]:
                raise ValueError("stale source reconciliation ledger identity")
            entries = ledger["entries"]
            ids = [r["commit"] for r in entries]
            if len(ids) != len(set(ids)) or set(ids) != {r["commit"] for r in rows}:
                raise ValueError("omitted, duplicate or extra full-population work unit")
            by_id = {r["commit"]: r for r in entries}
            legacy = ledger["schema_version"] == LEGACY_LEDGER
            primitive_rows = {r["commit"]: r.get("classification")
                              for r in primitive["target_unique_commits"]} if primitive else {}
            for row in rows:
                item = by_id[row["commit"]]
                expected = {"commit", "disposition", "current_bindings", "authority",
                            "source_commit", "projection_bindings", "blocking_decisions"}
                if not legacy:
                    expected.add("merge_resolution_dispositions")
                if set(item) != expected:
                    raise ValueError("foreign source row shape")
                if item["current_bindings"] != row["current_bindings"]:
                    raise ValueError("changed-path omission or stale current behavior binding")
                disposition = item["disposition"]
                if disposition is None:
                    packet["unresolved_commits"].append(row["commit"])
                    continue
                if disposition not in TERMINAL or item["blocking_decisions"] or not item["authority"]:
                    raise ValueError("unsupported disposition or unresolved terminal row")
                if row["commit"] in primitive_rows and primitive_rows[row["commit"]] != PRIMITIVE_MAP[disposition]:
                    raise ValueError("source/native disposition mapping mismatch")
                if (len(row["parents"]) > 1) != (disposition == "merge_ancestry"):
                    raise ValueError("merge unit hidden as ordinary work")
                if disposition == "merge_ancestry":
                    if merge_resolution_proof(git, subjects, row, item, projection, legacy):
                        packet["unresolved_commits"].append(row["commit"])
                else:
                    if not legacy and item["merge_resolution_dispositions"]:
                        raise ValueError("non-merge work carries foreign merge resolution effects")
                    semantic_proof(git, subjects, row["commit"], row["current_bindings"], item, projection)
            packet["ledger_sha256"] = digest(ledger)
        packet["verdict"] = "not_proven" if errors else "blocked" if packet["unresolved_commits"] else "pass"
    except (ValueError, KeyError, TypeError, AttributeError, IndexError) as error:
        errors.append(str(error))
        packet["verdict"] = "not_proven"
    packet["packet_digest"] = digest(packet)
    return packet


def skeleton(packet):
    return {"schema_version": LEDGER, "subjects": packet["subjects"],
            "population_digest": packet.get("population_digest"),
            "entries": [{"commit": row["commit"], "disposition": None,
                "current_bindings": row["current_bindings"], "authority": [],
                "source_commit": None, "projection_bindings": [],
                "merge_resolution_dispositions": [{"path": path, "disposition": None,
                    "authority": [], "source_commit": None, "projection_bindings": [],
                    "blocking_decisions": ["merge resolution effect requires current semantic proof"]}
                    for path in row.get("merge_effects", {}).get("resolution_paths", [])],
                "blocking_decisions": ["current semantic survival not yet reviewed"]}
                for row in packet["population"]]}


@contextmanager
def exclusive_output(path):
    """Create through no-follow parents, keeping traversal bound until close."""
    if os.name == "nt":
        import ctypes
        import msvcrt
        from ctypes import wintypes
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        native = ctypes.WinDLL("ntdll")
        class UnicodeString(ctypes.Structure):
            _fields_ = [("length", wintypes.USHORT), ("maximum", wintypes.USHORT),
                        ("buffer", wintypes.LPWSTR)]
        class ObjectAttributes(ctypes.Structure):
            _fields_ = [("length", wintypes.ULONG), ("root", wintypes.HANDLE),
                        ("name", ctypes.POINTER(UnicodeString)), ("attributes", wintypes.ULONG),
                        ("security", ctypes.c_void_p), ("quality", ctypes.c_void_p)]
        class IoStatus(ctypes.Structure):
            _fields_ = [("status", ctypes.c_void_p), ("information", ctypes.c_size_t)]
        create = native.NtCreateFile
        create.argtypes = [ctypes.POINTER(wintypes.HANDLE), wintypes.ULONG,
                           ctypes.POINTER(ObjectAttributes), ctypes.POINTER(IoStatus),
                           ctypes.c_void_p, wintypes.ULONG, wintypes.ULONG, wintypes.ULONG,
                           wintypes.ULONG, ctypes.c_void_p, wintypes.ULONG]
        create.restype = wintypes.LONG
        error_code = native.RtlNtStatusToDosError
        error_code.argtypes = [wintypes.LONG]
        error_code.restype = wintypes.ULONG
        close = kernel.CloseHandle
        close.argtypes = [wintypes.HANDLE]
        close.restype = wintypes.BOOL
        def open_native(name, root, access, disposition, options):
            buffer = ctypes.create_unicode_buffer(name)
            size = len(name.encode("utf-16-le"))
            if size > 65532:
                raise ValueError("receipt native path is too long")
            string = UnicodeString(size, size + 2, ctypes.cast(buffer, wintypes.LPWSTR))
            # CASE_INSENSITIVE | DONT_REPARSE applies to the entire name parse.
            attributes = ObjectAttributes(ctypes.sizeof(ObjectAttributes), root,
                                          ctypes.pointer(string), 0x1040, None, None)
            handle, status = wintypes.HANDLE(), IoStatus()
            result = create(ctypes.byref(handle), access, ctypes.byref(attributes),
                            ctypes.byref(status), None, 0x80, 1, disposition,
                            options, None, 0)
            if result < 0:
                raise ctypes.WinError(error_code(result))
            return handle
        # Open the existing parent without following any reparse component.
        # Final FILE_CREATE is relative to this handle, never an absolute path
        # re-lookup after validation. Parent sharing denies its rename/delete.
        parent = open_native("\\??\\" + str(path.parent), None, 0x1000A0, 1, 0x200021)
        try:
            handle = open_native(path.name, parent, 0x40100000, 2, 0x200060)
            try:
                descriptor = msvcrt.open_osfhandle(handle.value, os.O_WRONLY)
            except BaseException:
                close(handle)
                raise
            try:
                output = os.fdopen(descriptor, "w", encoding="ascii", newline="\n")
            except BaseException:
                os.close(descriptor)
                raise
            with output:
                yield output
        finally:
            close(parent)
    else:
        if not hasattr(os, "O_NOFOLLOW") or os.open not in os.supports_dir_fd:
            raise ValueError("platform lacks no-follow receipt creation")
        descriptors = []
        try:
            flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
            parent = os.open(path.anchor, flags)
            descriptors.append(parent)
            for part in path.parent.parts[1:]:
                parent = os.open(part, flags, dir_fd=parent)
                descriptors.append(parent)
            descriptor = os.open(path.name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                                 0o600, dir_fd=parent)
            try:
                output = os.fdopen(descriptor, "w", encoding="ascii", newline="\n")
            except BaseException:
                os.close(descriptor)
                raise
            with output:
                yield output
        finally:
            for descriptor in reversed(descriptors):
                os.close(descriptor)


def write_new(git, path, value, inputs):
    path = Path(path).resolve()
    if os.name == "nt" and (not re.fullmatch(r"[A-Za-z]:\\", path.anchor) or any(
        ":" in part or part.endswith((".", " ")) or re.fullmatch(
            r"(?i)(CON|PRN|AUX|NUL|COM[1-9¹²³]|LPT[1-9¹²³])(?:\..*)?", part)
        for part in path.parts[1:]
    )):
        raise ValueError("receipt path uses a Windows stream, device, or ambiguous component")
    if path.exists() or path in {Path(p).resolve() for p in inputs if p}:
        raise ValueError("refusing to overwrite a historical packet or input")
    private = Path(git.text("rev-parse", "--absolute-git-dir")).resolve()
    common = Path(git.text("rev-parse", "--path-format=absolute", "--git-common-dir")).resolve()
    if path == git.repo / ".git" or any(path == p or path.is_relative_to(p) for p in (private, common)):
        raise ValueError("receipt destination is Git metadata")
    with exclusive_output(path) as output:
        json.dump(value, output, ensure_ascii=True, indent=2)
        output.write("\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("repo", "source", "boundary", "target", "receipt"):
        parser.add_argument("--" + name, required=True)
    for name in ("ledger", "primitive-receipt", "projection-tree", "scaffold"):
        parser.add_argument("--" + name)
    args = parser.parse_args()
    git = Git(args.repo)
    subjects = {n: getattr(args, n) for n in ROLES}
    try:
        load = lambda p: json.loads(Path(p).read_text(encoding="utf-8")) if p else None
        ledger, primitive = load(args.ledger), load(args.primitive_receipt)
    except (ValueError, OSError) as error:
        packet = reconcile(git, subjects)
        packet["errors"].append("unreadable input evidence: " + str(error))
        packet["verdict"] = "not_proven"
        packet.pop("packet_digest", None)
        packet["packet_digest"] = digest(packet)
    else:
        packet = reconcile(git, subjects, ledger, primitive, args.projection_tree)
    try:
        write_new(git, args.receipt, packet, [args.ledger, args.primitive_receipt])
        if args.scaffold:
            write_new(git, args.scaffold, skeleton(packet), [args.ledger, args.primitive_receipt, args.receipt])
    except (ValueError, OSError) as error:
        parser.error(str(error))
    print(json.dumps({"verdict": packet["verdict"], "population": len(packet["population"]),
                      "unresolved": len(packet["unresolved_commits"]), "errors": packet["errors"]}))
    return {"pass": 0, "blocked": 3, "not_proven": 4}[packet["verdict"]]


if __name__ == "__main__":
    sys.exit(main())
