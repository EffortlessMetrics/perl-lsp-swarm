#!/usr/bin/env python3
"""Mechanical #7166 namespace migration with fail-closed text assertions."""

from __future__ import annotations

import re
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(rel: str) -> str:
    return (ROOT / rel).read_text(encoding="utf-8")


def write(rel: str, content: str) -> None:
    path = ROOT / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def replace_once(text: str, old: str, new: str, *, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one occurrence, found {count}: {old!r}")
    return text.replace(old, new, 1)


def replace_required(text: str, old: str, new: str, *, label: str) -> str:
    if old not in text:
        raise RuntimeError(f"{label}: required text not found: {old!r}")
    return text.replace(old, new)


def replace_old_crate_paths() -> None:
    """The old directory no longer exists; update every textual path reference."""
    needle = "crates/perl-kwalitee"
    replacement = "crates/perl-release-readiness"
    for path in ROOT.rglob("*"):
        if not path.is_file() or ".git" in path.parts:
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        if needle in text:
            path.write_text(text.replace(needle, replacement), encoding="utf-8")


def update_root_manifests() -> None:
    cargo = read("Cargo.toml")
    cargo = replace_required(
        cargo,
        'perl-kwalitee = { path = "crates/perl-release-readiness", version = "=0.17.0" }',
        'perl-release-readiness = { path = "crates/perl-release-readiness", version = "=0.17.0" }',
        label="workspace dependency",
    )
    cargo = replace_required(
        cargo,
        "[profile.release.package.perl-kwalitee]",
        "[profile.release.package.perl-release-readiness]",
        label="release profile",
    )
    cargo = replace_required(
        cargo,
        "[profile.release-opt.package.perl-kwalitee]",
        "[profile.release-opt.package.perl-release-readiness]",
        label="release-opt profile",
    )
    write("Cargo.toml", cargo)

    lock = read("Cargo.lock")
    if lock.count('name = "perl-kwalitee"') != 1:
        raise RuntimeError("Cargo.lock: expected exactly one perl-kwalitee package")
    lock = lock.replace('name = "perl-kwalitee"', 'name = "perl-release-readiness"')
    lock = lock.replace(' "perl-kwalitee",', ' "perl-release-readiness",')
    write("Cargo.lock", lock)

    xtask = read("xtask/Cargo.toml")
    xtask = replace_once(
        xtask,
        "perl-kwalitee = { workspace = true }",
        "perl-release-readiness = { workspace = true }",
        label="xtask dependency",
    )
    write("xtask/Cargo.toml", xtask)


def update_moved_crate() -> None:
    manifest_path = "crates/perl-release-readiness/Cargo.toml"
    manifest = read(manifest_path)
    manifest = replace_once(
        manifest,
        'name = "perl-kwalitee"',
        'name = "perl-release-readiness"',
        label="package name",
    )
    manifest = replace_once(
        manifest,
        'name = "perl_kwalitee"',
        'name = "perl_release_readiness"',
        label="library name",
    )
    manifest = manifest.replace(
        'description = "Legacy mixed repository/product release-readiness evaluator retained for perl_kwalitee.v1 compatibility"',
        'description = "Legacy mixed repository/product release-readiness evaluator and compatibility reader"',
    )
    manifest = manifest.replace(
        "# `perl-release-readiness`; the `perl-kwalitee` package name is reclaimed by\n# the native distribution analyser only after the migration train completes.",
        "# This package is the canonical code home for the historical mixed evaluator.\n# The `perl-kwalitee` package name is reserved for the native distribution analyser.",
    )
    write(manifest_path, manifest)

    for path in (ROOT / "crates/perl-release-readiness/src").rglob("*.rs"):
        text = path.read_text(encoding="utf-8")
        text = text.replace('"perl-kwalitee"', '"perl-release-readiness"')
        text = text.replace("`crates/perl-kwalitee`", "`crates/perl-release-readiness`")
        text = text.replace("use perl_kwalitee::{", "use perl_release_readiness::{")
        text = text.replace("[`perl_kwalitee`]", "[`perl_release_readiness`]")
        text = text.replace("the `perl_kwalitee` crate", "the `perl_release_readiness` crate")
        path.write_text(text, encoding="utf-8")

    readme = """# perl-release-readiness

`perl-release-readiness` is the canonical code home for the repository's
historical mixed release-readiness evaluator. It preserves the frozen
`perl_kwalitee.v1` receipt while the weighted catalog is decomposed into
independent native-product, engineering-evidence, release-integrity,
release-governance, and installed-acceptance rails.

This crate is **not** CPAN distribution Kwalitee. The `perl-kwalitee` package and
binary name are reserved for the native Rust CPANTS-compatible distribution
analyser being built under #4745.

## Compatibility contract

- historical receipt kind: `perl_kwalitee`;
- historical schema: `1`;
- canonical repository command: `cargo xtask release-readiness`;
- temporary compatibility alias: `cargo xtask perl-kwalitee`;
- canonical receipt directory: `target/receipts/release-readiness/`;
- legacy alias receipt directory: `target/receipts/kwalitee/`.

Every frozen indicator has one disposition in
[`legacy_indicator_migrations.toml`](legacy_indicator_migrations.toml). The
legacy catalog remains closed to new indicators.

## Library compatibility API

```rust
use perl_release_readiness::{evaluate, KwaliteeOptions, KwaliteeProfile};

let options = KwaliteeOptions::new("/path/to/repo", KwaliteeProfile::Pr);
let receipt = evaluate(&options);
println!("{}", receipt.to_markdown());
```

The type names intentionally remain stable in this mechanical move. The
scoreless rail model is owned by later PRs; #7166 changes authority and command
identity without changing observed evaluator results.

## License

MIT OR Apache-2.0 (workspace-inherited).
"""
    write("crates/perl-release-readiness/README.md", readme)


def canonical_task_from_legacy(source: str) -> str:
    text = source
    text = text.replace(
        "//! `cargo xtask perl-kwalitee` — Perl distribution Kwalitee evaluation.",
        "//! `cargo xtask release-readiness` — legacy mixed readiness evaluation.",
    )
    text = text.replace(
        "//! This is the repo-local wrapper around the [`perl_kwalitee`] crate.",
        "//! This is the canonical repo-local wrapper around [`perl_release_readiness`].",
    )
    text = text.replace("perl_kwalitee", "perl_release_readiness")
    text = text.replace("PerlKwaliteeProfile", "ReleaseReadinessProfile")
    text = text.replace("cargo xtask perl-kwalitee", "cargo xtask release-readiness")
    text = text.replace("Perl Kwalitee", "Release Readiness")
    text = text.replace(
        'const DEFAULT_JSON_REL: &str = "target/receipts/kwalitee/perl-kwalitee.json";',
        'const DEFAULT_JSON_REL: &str = "target/receipts/release-readiness/release-readiness.json";',
    )
    text = text.replace(
        'const DEFAULT_MARKDOWN_REL: &str = "target/receipts/kwalitee/perl-kwalitee.md";',
        'const DEFAULT_MARKDOWN_REL: &str = "target/receipts/release-readiness/release-readiness.md";',
    )
    return text


def legacy_task_wrapper() -> str:
    return """//! Deprecated `cargo xtask perl-kwalitee` compatibility alias.
//!
//! The historical mixed evaluator moved to [`crate::tasks::release_readiness`].
//! This wrapper preserves the old arguments and default output paths until the
//! native Rust distribution analyser claims the public `perl-kwalitee` name.

use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;

use crate::tasks::release_readiness::{self, ReleaseReadinessProfile};

const LEGACY_JSON_REL: &str = "target/receipts/kwalitee/perl-kwalitee.json";
const LEGACY_MARKDOWN_REL: &str = "target/receipts/kwalitee/perl-kwalitee.md";

fn warn() {
    eprintln!(
        "warning: `cargo xtask perl-kwalitee` is the deprecated legacy readiness alias; use `cargo xtask release-readiness`. The `perl-kwalitee` name will become the native Rust distribution analyser."
    );
}

/// Evaluate through the canonical release-readiness implementation.
pub fn check(
    profile: ReleaseReadinessProfile,
    dist: Option<PathBuf>,
    strict: bool,
    repo_root: Option<PathBuf>,
) -> Result<()> {
    warn();
    release_readiness::check(profile, dist, strict, repo_root)
}

/// Write the historical receipt through the canonical implementation.
pub fn report(
    profile: ReleaseReadinessProfile,
    dist: Option<PathBuf>,
    json: PathBuf,
    markdown: PathBuf,
    repo_root: Option<PathBuf>,
) -> Result<()> {
    warn();
    release_readiness::report(profile, dist, json, markdown, repo_root)
}

/// Explain a historical indicator through the canonical implementation.
pub fn explain(id: &str) -> Result<()> {
    warn();
    release_readiness::explain(id)
}

/// Historical JSON path retained only for the compatibility alias.
pub fn default_json_path(root: &Path) -> PathBuf {
    root.join(LEGACY_JSON_REL)
}

/// Historical Markdown path retained only for the compatibility alias.
pub fn default_markdown_path(root: &Path) -> PathBuf {
    root.join(LEGACY_MARKDOWN_REL)
}
"""


def update_xtask_tasks() -> None:
    old_path = ROOT / "xtask/src/tasks/perl_kwalitee.rs"
    source = old_path.read_text(encoding="utf-8")
    write("xtask/src/tasks/release_readiness.rs", canonical_task_from_legacy(source))
    old_path.write_text(legacy_task_wrapper(), encoding="utf-8")

    mod = read("xtask/src/tasks/mod.rs")
    mod = replace_once(
        mod,
        "pub mod perl_kwalitee;",
        "pub mod perl_kwalitee;\npub mod release_readiness;",
        label="task module export",
    )
    write("xtask/src/tasks/mod.rs", mod)


def update_xtask_main() -> None:
    path = "xtask/src/main.rs"
    text = read(path)
    old_variant = """    /// Evaluate Perl distribution Kwalitee indicators (measurable
    /// distribution quality) and emit a scored receipt.
    PerlKwalitee {
        #[command(subcommand)]
        command: PerlKwaliteeCommand,
    },
"""
    new_variant = """    /// Evaluate the historical mixed release-readiness catalog.
    #[command(name = "release-readiness")]
    ReleaseReadiness {
        #[command(subcommand)]
        command: ReleaseReadinessCommand,
    },

    /// Deprecated compatibility alias for the historical mixed evaluator.
    #[command(name = "perl-kwalitee", hide = true)]
    PerlKwalitee {
        #[command(subcommand)]
        command: ReleaseReadinessCommand,
    },
"""
    text = replace_once(text, old_variant, new_variant, label="top-level command variant")
    text = replace_required(
        text,
        "enum PerlKwaliteeCommand",
        "enum ReleaseReadinessCommand",
        label="subcommand enum",
    )
    text = text.replace("PerlKwaliteeCommand::", "ReleaseReadinessCommand::")
    text = text.replace(
        "perl_kwalitee::PerlKwaliteeProfile",
        "release_readiness::ReleaseReadinessProfile",
    )

    pattern = re.compile(
        r"        Commands::PerlKwalitee \{ command \} => match command \{\n(?P<body>.*?)\n        \},\n        Commands::SecurityHardening",
        re.DOTALL,
    )
    match = pattern.search(text)
    if match is None:
        raise RuntimeError("xtask main: legacy dispatch block not found")
    body = match.group("body")
    canonical_body = body.replace("perl_kwalitee::", "release_readiness::")
    replacement = (
        "        Commands::ReleaseReadiness { command } => match command {\n"
        + canonical_body
        + "\n        },\n"
        + "        Commands::PerlKwalitee { command } => match command {\n"
        + body
        + "\n        },\n"
        + "        Commands::SecurityHardening"
    )
    text = pattern.sub(replacement, text, count=1)
    write(path, text)


def update_cli_tests() -> None:
    source_path = ROOT / "xtask/tests/perl_kwalitee_cli.rs"
    text = source_path.read_text(encoding="utf-8")
    canonical = text.replace("cargo xtask perl-kwalitee", "cargo xtask release-readiness")
    canonical = canonical.replace("the `perl-kwalitee` crate", "the `perl-release-readiness` crate")
    canonical = canonical.replace('"perl-kwalitee"', '"release-readiness"')
    canonical = canonical.replace("Perl Kwalitee:", "Release Readiness:")
    canonical = canonical.replace("kwalitee.json", "release-readiness.json")
    canonical = canonical.replace("kwalitee.md", "release-readiness.md")
    canonical = canonical.replace("Perl Kwalitee", "Release Readiness")
    canonical = canonical.replace("perl_kwalitee", "release_readiness")
    write("xtask/tests/release_readiness_cli.rs", canonical)

    alias_test = """//! Compatibility tests for the deprecated `cargo xtask perl-kwalitee` alias.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use assert_cmd::Command;
use color_eyre::eyre::Result;

#[test]
fn legacy_alias_delegates_and_warns() -> Result<()> {
    let output = Command::cargo_bin("xtask")?
        .args(["perl-kwalitee", "explain", "release.no_external_tooling"])
        .output()?;
    assert!(output.status.success(), "legacy alias should delegate");
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stdout.contains("release.no_external_tooling"));
    assert!(stderr.contains("deprecated legacy readiness alias"), "{stderr}");
    assert!(stderr.contains("release-readiness"), "{stderr}");
    Ok(())
}
"""
    write("xtask/tests/perl_kwalitee_alias_cli.rs", alias_test)
    source_path.unlink()


def update_docs() -> None:
    source = ROOT / "docs/reference/PERL_KWALITEE.md"
    if source.exists():
        text = source.read_text(encoding="utf-8")
        text = text.replace("cargo xtask perl-kwalitee", "cargo xtask release-readiness")
        text = text.replace("perl_kwalitee::", "perl_release_readiness::")
        text = text.replace("crates/perl-kwalitee", "crates/perl-release-readiness")
        text = text.replace("# Perl Kwalitee", "# Perl release-readiness compatibility evaluator", 1)
        banner = (
            "> **Historical mixed evaluator.** This reference describes the compatibility\n"
            "> implementation now named `perl-release-readiness`. It is not the native Rust\n"
            "> CPANTS-compatible distribution analyser being built as `perl-kwalitee`.\n\n"
        )
        if banner not in text:
            lines = text.splitlines(keepends=True)
            lines.insert(2 if len(lines) > 1 else 1, "\n" + banner)
            text = "".join(lines)
        write("docs/reference/RELEASE_READINESS.md", text)
        source.write_text(
            "# Perl Kwalitee\n\n"
            "The historical mixed readiness evaluator moved to "
            "[`RELEASE_READINESS.md`](RELEASE_READINESS.md) and the canonical "
            "repository command is `cargo xtask release-readiness`.\n\n"
            "The `perl-kwalitee` name is reserved for the native Rust "
            "CPANTS-compatible Perl distribution analyser under #4745. "
            "Historical `perl_kwalitee.v1` receipts remain readable through "
            "`perl-release-readiness`.\n",
            encoding="utf-8",
        )

    migration = read("docs/reference/PERL_KWALITEE_MIGRATION.md")
    migration = migration.replace(
        "crates/perl-kwalitee/legacy_indicator_migrations.toml",
        "crates/perl-release-readiness/legacy_indicator_migrations.toml",
    )
    migration = migration.replace(
        "perl-kwalitee is a declared workspace member",
        "perl-release-readiness is a declared workspace member",
    )
    write("docs/reference/PERL_KWALITEE_MIGRATION.md", migration)


def update_hygiene_tests() -> None:
    path = "crates/perl-ci-hygiene/tests/test_quality_baseline_infrastructure.rs"
    text = read(path)
    text = text.replace("perl-kwalitee", "perl-release-readiness")
    text = text.replace("perl_kwalitee", "perl_release_readiness")
    write(path, text)


def verify_text_invariants() -> None:
    if (ROOT / "crates/perl-kwalitee").exists():
        raise RuntimeError("old crate directory still exists")
    if not (ROOT / "crates/perl-release-readiness").is_dir():
        raise RuntimeError("new crate directory is missing")

    cargo = read("Cargo.toml")
    for required in [
        '"crates/perl-release-readiness"',
        'perl-release-readiness = { path = "crates/perl-release-readiness"',
        "[profile.release.package.perl-release-readiness]",
        "[profile.release-opt.package.perl-release-readiness]",
    ]:
        if required not in cargo:
            raise RuntimeError(f"root Cargo.toml missing {required}")

    main = read("xtask/src/main.rs")
    if main.count("Commands::ReleaseReadiness { command }") != 1:
        raise RuntimeError("canonical release-readiness dispatch missing or duplicated")
    if main.count("Commands::PerlKwalitee { command }") != 1:
        raise RuntimeError("legacy perl-kwalitee dispatch missing or duplicated")

    legacy_receipt_sources = [
        read("crates/perl-release-readiness/src/receipt.rs"),
        read("crates/perl-release-readiness/src/legacy.rs"),
    ]
    if not all('"perl_kwalitee"' in source for source in legacy_receipt_sources):
        raise RuntimeError("frozen perl_kwalitee receipt kind was renamed")

    if "pub mod release_readiness;" not in read("xtask/src/tasks/mod.rs"):
        raise RuntimeError("canonical task module not exported")


def main() -> None:
    replace_old_crate_paths()
    update_root_manifests()
    update_moved_crate()
    update_xtask_tasks()
    update_xtask_main()
    update_cli_tests()
    update_docs()
    update_hygiene_tests()
    verify_text_invariants()


if __name__ == "__main__":
    main()
