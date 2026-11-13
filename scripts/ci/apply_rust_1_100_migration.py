#!/usr/bin/env python3
"""One-shot, exact-scope Rust 1.100 migration for #9864/#11234."""
from __future__ import annotations

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, text: str) -> None:
    (ROOT / path).write_text(text, encoding="utf-8")


def replace(path: str, old: str, new: str, *, count: int | None = None) -> None:
    text = read(path)
    found = text.count(old)
    if found == 0:
        raise SystemExit(f"expected text missing in {path}: {old!r}")
    if count is not None and found != count:
        raise SystemExit(f"unexpected replacement count in {path}: {found} != {count}")
    write(path, text.replace(old, new))


tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).split(b"\0")
paths = [Path(raw.decode("utf-8")) for raw in tracked if raw]
preserve_prefixes = (
    ".spec/",
    "benchmarks/results/",
    "docs/forensics/",
    "docs/learnings/",
    "docs/performance/",
    "docs/release/",
    "docs/releases/",
    "tests/agentic-task-battery/baselines/",
    "xtask/fixtures/",
)
preserve_exact = {
    "CHANGELOG.md",
    "docs/ci/perl-lsp-rust-1.95-rollout.md",
    "docs/ci/qualification-canonical-completion.md",
    "docs/ci/test-evidence-lanes.md",
    "docs/development/CI_UX_RAIL.md",
    "docs/development/FILE_POLICY_RAIL.md",
    "docs/development/PERL_ORACLE_RAIL.md",
    "docs/development/RUST_1_95_PROACTIVE_GUARDS.md",
    "docs/development/RUST_1_95_ROLLOUT.md",
    "docs/how-to/MULTI_WORKTREE_BUILD_CACHING.md",
    "docs/project/CODEBASE_HISTORY.md",
    "docs/project/RAIL_TEMPLATE.md",
    "docs/project/RELEASE_READINESS_2026-06.md",
    "docs/project/TREE_SITTER_INCREMENTAL_CROSSOVER.md",
    "docs/project/UX_CLOSEOUTS.md",
    "docs/project/metrics/RATCHET.md",
    "docs/reference/PROJECT_CHECK_SELECTION.md",
    "fixtures/clippy_repair_falsifiers/manifest.v1.json",
    ".ci/metrics/baselines/parser.json",
}

# Exact patch-version pins represent the live toolchain in source, CI, tests,
# and current guidance. Preserve dated evidence and old rollout authorities.
for relative in paths:
    path_s = relative.as_posix()
    if path_s in preserve_exact or path_s.startswith(preserve_prefixes):
        continue
    path = ROOT / relative
    try:
        text = path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, OSError):
        continue
    if "1.95.0" in text:
        path.write_text(text.replace("1.95.0", "1.100.0"), encoding="utf-8")

# Bare MSRV claims are updated only on current normative/user-facing surfaces.
for path_s in [
    ".cargo/config.local.toml.example",
    ".docker/perl-lsp/Dockerfile",
    ".docker/rust/Dockerfile",
    ".kiro/steering/tech.md",
    "Cargo.toml",
    "README.md",
    "CONTRIBUTING.md",
    "book/src/ci/local-validation.md",
    "book/src/ci/overview.md",
    "book/src/developer/development-workflow.md",
    "book/src/developer/test-infrastructure.md",
    "book/src/getting-started/installation.md",
    "book/src/quick-start.md",
    "book/src/reference/roadmap.md",
    "clippy.toml",
    "crates/perl-parser-pest/Cargo.toml",
    "docs/CLIPPY_POLICY.md",
    "docs/MAINTENANCE.md",
    "docs/NO_PANIC_POLICY.md",
    "docs/contributing/FIRST_PR.md",
    "docs/explanation/LSP_DOCUMENTATION.md",
    "docs/how-to/JENKINS.md",
    "docs/how-to/SEMVER_WORKFLOW.md",
    "docs/how-to/WORKSPACE_REFACTORING_GUIDE.md",
    "docs/issues/DEVELOPER_FRICTION.md",
    "docs/issues/KNOWN_ISSUES.md",
    "docs/project/CI.md",
    "docs/project/CI_LOCAL_VALIDATION.md",
    "docs/project/DEVELOPMENT.md",
    "docs/project/QUALITY_INFRASTRUCTURE.md",
    "docs/project/ROADMAP.md",
    "docs/project/WORKSPACE_ARCHITECTURE.md",
    "docs/reference/FAQ.md",
    "docs/reference/STABILITY.md",
    "docs/reference/TEST_INFRASTRUCTURE_GUIDE.md",
    "docs/tutorials/GETTING_STARTED.md",
    "flake.nix",
    "justfile",
    "vscode-extension/README.md",
]:
    path = ROOT / path_s
    if not path.exists():
        continue
    text = path.read_text(encoding="utf-8")
    text = text.replace("Rust 1.95", "Rust 1.100")
    text = text.replace("MSRV 1.95", "MSRV 1.100")
    text = text.replace("1.95+", "1.100+")
    text = text.replace("rust:1.95", "rust:1.100")
    path.write_text(text, encoding="utf-8")

# Local-only and hosted image identities are live execution contracts.
for relative in paths:
    path_s = relative.as_posix()
    if not path_s.startswith(".github/workflows/"):
        continue
    path = ROOT / relative
    text = path.read_text(encoding="utf-8")
    text = text.replace("em-ci-rust:1.95", "em-ci-rust:1.100")
    text = text.replace("rust:1.95", "rust:1.100")
    path.write_text(text, encoding="utf-8")

# Preserve an explicitly historical doctest observation embedded in live gate
# policy; it is provenance, not the current toolchain contract.
gate = read(".ci/gate-policy.yaml")
gate = gate.replace(
    "Measured on the pinned 1.100.0 toolchain, that field only removes doctests",
    "Measured on the pinned 1.95.0 toolchain, that field only removes doctests",
)
write(".ci/gate-policy.yaml", gate)

# Move newly due Clippy rows into explicit, owned deferrals rather than silently
# activating them during the compiler upgrade.
policy = read("policy/clippy-lints.toml")
policy = policy.replace('msrv = "1.95"', 'msrv = "1.100"', 1)
start = policy.index("# This lint belongs to the Rust/Clippy 1.96 rail")
end = policy.index("# These lints are already available", start)
deferrals = '''# Rust 1.100 provides these lints. Their activation remains explicitly owned
# by the semantic-equivalence work that predates the toolchain migration.
[[deferred_due]]
name = "clippy::manual_pop_if"
level = "deny"
activate_when_msrv = "1.96"
class = "reviewability"
owner = "#1803"
reason = "Rust 1.100 provides the lint, but #1803 must prove predicate, mutation, ownership, and drop-order equivalence before activation."
review_after = "2026-12-15"
next_status = "active"

[[deferred_due]]
name = "clippy::manual_clear"
level = "deny"
activate_when_msrv = "1.97"
class = "reviewability"
owner = "#9913"
reason = "Rust 1.100 provides the lint, but #9913 owns the buffer-reuse dispositions and activation proof."
review_after = "2026-12-15"
next_status = "active"

'''
policy = policy[:start] + deferrals + policy[end:]
write("policy/clippy-lints.toml", policy)

# Prove that the toolchain guard treats 99/100 numerically, not lexically.
toolchain = read("xtask/src/tasks/check_toolchain.rs")
needle = '''    #[test]
    fn compare_versions_treats_missing_patch_as_zero() {
        assert_eq!(compare_versions(&[1, 92], &[1, 92, 0]), std::cmp::Ordering::Equal);
    }
'''
addition = needle + '''
    #[test]
    fn compare_versions_orders_99_and_100_numerically() {
        assert_eq!(compare_versions(&[1, 99, 0], &[1, 100, 0]), Ordering::Less);
        assert_eq!(compare_versions(&[1, 100], &[1, 100, 0]), Ordering::Equal);
        assert_eq!(compare_versions(&[1, 101, 0], &[1, 100, 0]), Ordering::Greater);
    }
'''
if toolchain.count(needle) != 1:
    raise SystemExit("check_toolchain test insertion point changed")
write("xtask/src/tasks/check_toolchain.rs", toolchain.replace(needle, addition))

# Cargo 1.100 moved intermediate fingerprints from .fingerprint/<pkg>-<hash>
# to build/<pkg>/<hash>/fingerprint. Support both layouts during migration.
clippy = read("xtask/src/tasks/clippy_cost_measure.rs")
old = '''/// All fingerprint roots that can hold relevant member units: the base
/// `target/debug/.fingerprint` plus one root per platform subdirectory
/// (`target/<triple>/debug/.fingerprint`). Cargo writes cross-compilation
/// artifacts under `CARGO_BUILD_TARGET` / `[build] target` triples while
/// `cargo metadata` still reports the base target directory, so invalidating
/// only the base would miss every configured-triple unit and silently label
/// a warm run `members-cold`. A directory qualifies as a platform
/// subdirectory structurally — an immediate child of the target dir holding
/// its own `debug/.fingerprint` — which covers env-var and config-file
/// configuration without parsing cargo configuration.
fn fingerprint_roots(target_dir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![target_dir.join("debug").join(".fingerprint")];
    let mut platform_dirs: Vec<PathBuf> = match fs::read_dir(target_dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.is_dir() && path.join("debug").join(".fingerprint").is_dir())
            .collect(),
        Err(_) => Vec::new(),
    };
    platform_dirs.sort();
    roots.extend(platform_dirs.into_iter().map(|dir| dir.join("debug").join(".fingerprint")));
    roots
}

fn invalidate_member_fingerprints(target_dir: &Path, members: &[String]) -> Result<u32> {
    let mut removed = 0u32;
    for fp_dir in fingerprint_roots(target_dir) {
        let entries = match fs::read_dir(&fp_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("reading {}", fp_dir.display())),
        };
        for entry in entries {
            let entry = entry.with_context(|| format!("reading entry in {}", fp_dir.display()))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let owned = members.iter().any(|m| is_member_fingerprint_dir(&name, m));
            if owned {
                fs::remove_dir_all(entry.path()).with_context(|| {
                    format!("removing fingerprint directory {}", entry.path().display())
                })?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}
'''
new = '''/// Profile roots that can hold member fingerprints: the host `debug` profile
/// and configured-target `target/<triple>/debug` profiles. Cargo 1.100 stores
/// intermediate records under `build/<package>/<hash>/fingerprint`; older
/// Cargo stores them under `.fingerprint/<package>-<hash>`.
fn fingerprint_profile_roots(target_dir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![target_dir.join("debug")];
    let mut platform_roots: Vec<PathBuf> = match fs::read_dir(target_dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .map(|path| path.join("debug"))
            .filter(|profile| {
                profile.is_dir()
                    && (profile.join(".fingerprint").is_dir() || profile.join("build").is_dir())
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    platform_roots.sort();
    roots.extend(platform_roots);
    roots
}

fn invalidate_member_fingerprints(target_dir: &Path, members: &[String]) -> Result<u32> {
    let mut removed = 0u32;
    for profile in fingerprint_profile_roots(target_dir) {
        let legacy = profile.join(".fingerprint");
        match fs::read_dir(&legacy) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.with_context(|| format!("reading entry in {}", legacy.display()))?;
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if members.iter().any(|member| is_member_fingerprint_dir(&name, member)) {
                        fs::remove_dir_all(entry.path()).with_context(|| {
                            format!("removing fingerprint directory {}", entry.path().display())
                        })?;
                        removed += 1;
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).with_context(|| format!("reading {}", legacy.display())),
        }

        let build = profile.join("build");
        for member in members {
            let member_dir = build.join(member);
            let entries = match fs::read_dir(&member_dir) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| format!("reading {}", member_dir.display()));
                }
            };
            for entry in entries {
                let entry = entry.with_context(|| format!("reading entry in {}", member_dir.display()))?;
                let path = entry.path();
                if path.is_dir() && path.join("fingerprint").is_file() {
                    fs::remove_dir_all(&path).with_context(|| {
                        format!("removing Cargo 1.100 fingerprint directory {}", path.display())
                    })?;
                    removed += 1;
                }
            }
        }
    }
    Ok(removed)
}
'''
if clippy.count(old) != 1:
    raise SystemExit("clippy fingerprint implementation changed")
clippy = clippy.replace(old, new)
insert = '''    #[test]
    fn invalidation_handles_missing_fingerprint_root() -> Result<()> {
'''
new_test = '''    #[test]
    fn invalidation_removes_cargo_1_100_member_fingerprints_only() -> Result<()> {
        let target = tempfile::tempdir()?;
        let build = target.path().join("debug").join("build");
        let owned = build.join("perl-token").join("deadbeefdeadbeef");
        let owned_without_marker = build.join("perl-token").join("cafebabecafebabe");
        let dependency = build.join("serde").join("1111222233334444");
        let triple_owned = target
            .path()
            .join("x86_64-unknown-linux-gnu")
            .join("debug")
            .join("build")
            .join("perl-token")
            .join("aaaabbbbccccdddd");
        for path in [&owned, &owned_without_marker, &dependency, &triple_owned] {
            fs::create_dir_all(path)?;
        }
        fs::write(owned.join("fingerprint"), b"{}")?;
        fs::write(dependency.join("fingerprint"), b"{}")?;
        fs::write(triple_owned.join("fingerprint"), b"{}")?;

        let removed = invalidate_member_fingerprints(target.path(), &[member("perl-token")])?;
        assert_eq!(removed, 2);
        assert!(!owned.exists());
        assert!(!triple_owned.exists());
        assert!(owned_without_marker.exists());
        assert!(dependency.exists());
        Ok(())
    }

''' + insert
if clippy.count(insert) != 1:
    raise SystemExit("clippy fingerprint test insertion point changed")
write("xtask/src/tasks/clippy_cost_measure.rs", clippy.replace(insert, new_test))

# Storage evidence recognizes both layouts, while retaining the same bounded
# JSON read and redaction behavior.
storage = read("scripts/storage_observation.py")
anchor = '''def inventory_roots(paths, *, max_entries=MAX_ENTRIES, max_depth=MAX_DEPTH,
'''
helper = '''def is_cargo_fingerprint(path, root):
    """Recognize legacy and Cargo 1.100 intermediate fingerprint records."""
    relative = path.relative_to(root)
    parts = relative.parts
    legacy = ".fingerprint" in parts and path.suffix == ".json"
    current = len(parts) >= 4 and parts[-4] == "build" and parts[-1] == "fingerprint"
    return legacy or current


''' + anchor
if storage.count(anchor) != 1:
    raise SystemExit("storage helper insertion point changed")
storage = storage.replace(anchor, helper)
old_condition = '''                elif (collect_fingerprints and name in ("target", "build")
                      and ".fingerprint" in path.relative_to(root).parts
                      and path.suffix == ".json"):
'''
new_condition = '''                elif (collect_fingerprints and name in ("target", "build")
                      and is_cargo_fingerprint(path, root)):
'''
if storage.count(old_condition) != 1:
    raise SystemExit("storage fingerprint condition changed")
write("scripts/storage_observation.py", storage.replace(old_condition, new_condition))

tests = read("scripts/tests/test_storage_doctor.py")
insert = '''    def test_clippy_descriptor_scope_accepts_bounded_clippy_setup(self):
'''
new_test = '''    def test_cargo_1_100_fingerprint_layout_is_collected(self):
        path = (self.paths["build"] / "debug" / "build" / "perl-parser-core"
                / "deadbeefdeadbeef" / "fingerprint")
        path.parent.mkdir(parents=True)
        path.write_text(json.dumps({"rustc": 42, "path": 99, "private": "SENSITIVE"}),
                        encoding="utf-8")
        result = self.observe()
        self.assertTrue(result["inventory_complete"], result)
        self.assertTrue(result["fingerprints_complete"], result)
        self.assertEqual(len(result["fingerprints"]), 1)
        self.assertEqual(result["fingerprints"][0]["hashes"], {"rustc": "42", "path": "99"})
        self.assertNotIn("SENSITIVE", json.dumps(result))

''' + insert
if tests.count(insert) != 1:
    raise SystemExit("storage test insertion point changed")
write("scripts/tests/test_storage_doctor.py", tests.replace(insert, new_test))

# Current migration authority; old 1.95 rollout documents remain immutable.
upgrade_doc = '''# Rust 1.100 pre-release upgrade

Issues: [#9864](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9864), [#11234](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11234)

The release line moves the compiler pin and MSRV together from Rust 1.95.0 to
Rust 1.100.0. Rust 1.95 through 1.99 are no longer supported after this change.
Edition 2024, package versions, dependency resolution, and historical receipts
remain unchanged.

## Why this upgrade lands before release

Rust 1.100 stabilizes the never type, custom allocator APIs, `DropGuard`, and
several library APIs. The compiler series also includes correctness fixes from
1.96.1 through 1.99.1 and performance improvements such as faster integer
formatting. The upgrade does not adopt optional allocator or API refactors;
those require independent evidence and review.

The material compatibility change for this repository is Cargo 1.100's new
intermediate build-directory layout. Cargo documents that layout as internal,
so tools should prefer JSON messages. The retained-output census and the
Clippy cost instrument still need bounded fingerprint visibility, and therefore
support both the legacy `.fingerprint/<package>-<hash>` layout and the 1.100
`build/<package>/<hash>/fingerprint` layout.

## Synchronized contracts

- `rust-toolchain.toml`, Cargo `rust-version`, Clippy MSRV, Nix, Docker, `just`,
  CI policy, hosted workflows, and source-build guidance use 1.100.
- The local-only `em-ci-rust` route uses the `1.100` image identity and retains
  hosted fallback when that image has not been provisioned on a runner.
- `manual_pop_if` and `manual_clear` become explicit owned deferrals under
  #1803 and #9913 instead of silently activating during the compiler bump.
- Toolchain checks include the 1.99/1.100 numeric boundary.
- Coverage, PGO, formatter, Clippy, and LLVM evidence must be regenerated with
  compiler-matched 1.100 tools; old receipts remain historical evidence only.

## Compatibility review

The migration must review never-type/`Infallible` inference, removed `box`
patterns, closure capture, trait-object dyn compatibility, symbol/backtrace
expectations, FFI/runtime symbols, ARM ABI-affecting target features, rustdoc
lints, Cargo warnings, and workspace `default-features` behavior. A focused
1.100 probe compiled the parser, core LSP, server, and `xtask` all-target
surfaces and passed the repository-wide rustfmt check before the policy pin was
changed. Full PR qualification remains the merge authority.
'''
write("docs/development/RUST_1_100_UPGRADE.md", upgrade_doc)

changelog = read("CHANGELOG.md")
changed = "### Changed\n\n"
entry = '''### Changed

- **Rust 1.100 toolchain and MSRV.** The pre-release line now builds and tests
  with Rust/Cargo 1.100.0 across Cargo metadata, Clippy, Nix, Docker, CI, and
  source-build guidance. Cargo 1.100 fingerprint consumers support both the
  legacy and current intermediate layouts; historical 1.95 evidence is retained.

'''
if changelog.count(changed) < 1:
    raise SystemExit("CHANGELOG Changed section missing")
write("CHANGELOG.md", changelog.replace(changed, entry, 1))

# The audit/apply machinery is intentionally absent from the production diff.
for path_s in [
    ".github/workflows/rust-1-100-inventory.yml",
    ".github/workflows/rust-1-100-migration.yml",
    ".github/workflows/rust-1-100-apply.yml",
    "scripts/ci/apply_rust_1_100_migration.py",
]:
    path = ROOT / path_s
    if path.exists():
        path.unlink()
