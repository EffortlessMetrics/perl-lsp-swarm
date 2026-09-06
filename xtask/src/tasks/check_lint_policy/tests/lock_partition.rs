//! Behavioral proof for the two `let_underscore_lock` rows promoted by #14444.
//!
//! The ledger claims the rustc row and the Clippy row cover *different* lock
//! types and together leave no gap. That claim is about the selected toolchain,
//! not about the TOML, so asserting it against the ledger alone would be
//! circular: the rows would prove themselves. These tests compile one fixture
//! that acquires standard-library and `parking_lot` guards side by side and
//! isolate each lint in turn, so the recorded partition is measured rather than
//! asserted.
//!
//! The same fixture carries the three discards both rows miss, and #14579's
//! ruling on them is measured the same way: the owned and mapped guard forms
//! are seen by `clippy::let_underscore_must_use`, and the explicit `drop`
//! form is seen by no Clippy lint at any group level. Each boundary is asserted
//! in the direction it was measured, so a toolchain that moves one fails the
//! matching test and the ruling gets revisited rather than silently going
//! stale.
//!
//! Both lock lints are deny-by-default upstream, so the fixture runs them under
//! `--force-warn`: the compile completes, every site is reported in one pass,
//! and a non-zero exit means the instrument failed rather than that the lint
//! fired. Instrument failure is reported as failure, never as a pass.

use color_eyre::eyre::{Result, bail};
use serde_json::Value as JsonValue;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

const RUST_LOCK_LINT: &str = "let_underscore_lock";
const CLIPPY_LOCK_LINT: &str = "clippy::let_underscore_lock";
/// The tracked row #14579 assigns the `let _ =` discards the lock rows miss:
/// owned `*_arc` guards and mapped guards are `#[must_use]` types, so this lint
/// sees them. It is a deny row in the ledger already and activates through
/// #11240 once #11236 has defined deliberate-discard semantics.
const CLIPPY_MUST_USE_LINT: &str = "clippy::let_underscore_must_use";
/// Every Clippy group, for the sweep that establishes "no lint on this
/// toolchain" rather than "not these rows".
const CLIPPY_GROUPS: [&str; 4] =
    ["clippy::all", "clippy::pedantic", "clippy::nursery", "clippy::restriction"];

/// Discarded standard-library guards. Only the rustc row sees these.
const STD_DISCARD_BINDINGS: [&str; 2] = ["dropped_std_mutex", "dropped_std_rwlock"];
/// Discarded borrowed `parking_lot` guards. Only the Clippy row sees these.
const PARKING_LOT_DISCARD_BINDINGS: [&str; 2] = ["dropped_pl_mutex", "dropped_pl_rwlock"];
/// Discards outside both lock rows that #14579 assigns to
/// [`CLIPPY_MUST_USE_LINT`]: owned `parking_lot` guards from
/// `lock_arc`/`write_arc`, and a mapped guard. Their guard types are
/// `#[must_use]`, which is what that lint keys on; the lock rows never see them.
const OWNED_AND_MAPPED_DISCARD_BINDINGS: [&str; 3] =
    ["dropped_arc_mutex", "dropped_arc_rwlock", "dropped_mapped_pl"];
/// The explicit `drop(m.lock())` discards of either family, by binding name.
/// Used where the lint under measurement reports on the statement line, so a
/// binding-name match cannot be confused with the signature that declares it.
const EXPLICIT_DROP_DISCARD_BINDINGS: [&str; 2] = ["dropped_via_std_drop", "dropped_via_pl_drop"];
/// The same two discards as exact statements. The every-group sweep sees
/// whole-function findings whose primary span starts on the signature naming
/// these parameters, so a finding *on the discard* has to be matched by the
/// statement itself.
const EXPLICIT_DROP_DISCARD_STATEMENTS: [&str; 2] =
    ["drop(dropped_via_std_drop.lock());", "drop(dropped_via_pl_drop.lock());"];
/// Compliant acquisitions of every flavor, held by named guards.
const HELD_BINDING_MARKER: &str = "held_";

const MAX_DIAGNOSTIC_LINES: usize = 20;
const MAX_DIAGNOSTIC_CHARS_PER_LINE: usize = 500;

/// Build the fixture manifest against the workspace's own `parking_lot`.
///
/// The version is read from the workspace `Cargo.lock` and pinned exactly. A
/// floating requirement would let a runner with a newer compatible release
/// cached measure *that* release instead, so the recorded coverage could change
/// with no toolchain or repository change — which would quietly undo the point
/// of measuring it. Pinning also keeps `--offline` safe: `xtask` depends on
/// `perl-workspace`, which depends on `parking_lot` unconditionally, so the
/// exact locked version is already fetched in any environment that can build
/// this test binary.
///
/// `arc_lock` is enabled because the workspace enables it and the owned guards
/// it unlocks are production-reachable (`perl-workspace`'s `workspace_index`
/// holds an `ArcMutexGuard`). Measuring the borrowed guards alone would leave
/// the ledger claiming coverage over a guard family the fixture never exercised.
fn fixture_manifest(parking_lot_version: &str) -> String {
    format!(
        r#"[package]
name = "let-underscore-lock-partition-fixture"
version = "0.0.0"
edition = "2024"
rust-version = "1.95"
publish = false

[dependencies]
parking_lot = {{ version = "={parking_lot_version}", features = ["arc_lock"] }}

[workspace]
"#
    )
}

/// Read the exact `parking_lot` version the workspace resolves to.
fn workspace_parking_lot_version(root: &Path) -> Result<String> {
    let lockfile = root.join("Cargo.lock");
    let text = fs::read_to_string(&lockfile)
        .map_err(|err| color_eyre::eyre::eyre!("failed to read {}: {err}", lockfile.display()))?;
    let parsed: toml::Value = toml::from_str(&text)
        .map_err(|err| color_eyre::eyre::eyre!("failed to parse {}: {err}", lockfile.display()))?;
    let packages = parsed
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| color_eyre::eyre::eyre!("Cargo.lock must contain a package array"))?;

    let mut versions = packages
        .iter()
        .filter(|package| package.get("name").and_then(toml::Value::as_str) == Some("parking_lot"))
        .filter_map(|package| package.get("version").and_then(toml::Value::as_str));

    let Some(version) = versions.next() else {
        bail!("Cargo.lock does not lock parking_lot; the fixture cannot pin the measured release");
    };
    if versions.next().is_some() {
        bail!("Cargo.lock locks multiple parking_lot versions; the measured release is ambiguous");
    }
    Ok(version.to_owned())
}

/// Every discard form and its compliant counterpart in one compilation.
///
/// The compliant halves are the negative control: they use the same lock types
/// and the same call sites as the discards and differ only in whether the guard
/// is bound. A lint that fired on them, or one that fired on nothing at all,
/// would not discriminate, and the assertions below would catch either.
const FIXTURE_SOURCE: &str = r#"use parking_lot::{Mutex as PlMutex, MutexGuard as PlMutexGuard, RwLock as PlRwLock};
use std::sync::{Arc, Mutex, RwLock};

pub fn discards_std_guards(dropped_std_mutex: &Mutex<u32>, dropped_std_rwlock: &RwLock<u32>) {
    let _ = dropped_std_mutex.lock();
    let _ = dropped_std_rwlock.read();
}

pub fn discards_parking_lot_guards(dropped_pl_mutex: &PlMutex<u32>, dropped_pl_rwlock: &PlRwLock<u32>) {
    let _ = dropped_pl_mutex.lock();
    let _ = dropped_pl_rwlock.write();
}

pub fn discards_arc_guards(dropped_arc_mutex: &Arc<PlMutex<u32>>, dropped_arc_rwlock: &Arc<PlRwLock<u32>>) {
    let _ = dropped_arc_mutex.lock_arc();
    let _ = dropped_arc_rwlock.write_arc();
}

pub fn discards_by_explicit_drop(dropped_via_std_drop: &Mutex<u32>, dropped_via_pl_drop: &PlMutex<u32>) {
    drop(dropped_via_std_drop.lock());
    drop(dropped_via_pl_drop.lock());
}

pub fn discards_mapped_guard(dropped_mapped_pl: &PlMutex<(u32, u32)>) {
    let dropped_mapped_pl_guard = dropped_mapped_pl.lock();
    let _ = PlMutexGuard::map(dropped_mapped_pl_guard, |value| &mut value.0);
}

pub fn holds_mapped_guard(held_mapped_pl: &PlMutex<(u32, u32)>) -> u32 {
    let held_mapped_pl_guard = held_mapped_pl.lock();
    let mut mapped = PlMutexGuard::map(held_mapped_pl_guard, |value| &mut value.0);
    *mapped = mapped.wrapping_add(1);
    *mapped
}

pub fn holds_arc_guards(held_arc_mutex: &Arc<PlMutex<u32>>, held_arc_rwlock: &Arc<PlRwLock<u32>>) -> u32 {
    let mut guard = held_arc_mutex.lock_arc();
    *guard = guard.wrapping_add(1);
    let mut writer = held_arc_rwlock.write_arc();
    *writer = guard.wrapping_add(1);
    *writer
}

pub fn holds_std_guards(held_std_mutex: &Mutex<u32>, held_std_rwlock: &RwLock<u32>) -> Option<u32> {
    let mut guard = held_std_mutex.lock().ok()?;
    *guard = guard.wrapping_add(1);
    let reader = held_std_rwlock.read().ok()?;
    Some(guard.wrapping_add(*reader))
}

pub fn holds_parking_lot_guards(held_pl_mutex: &PlMutex<u32>, held_pl_rwlock: &PlRwLock<u32>) -> u32 {
    let mut guard = held_pl_mutex.lock();
    *guard = guard.wrapping_add(1);
    let mut writer = held_pl_rwlock.write();
    *writer = guard.wrapping_add(1);
    *writer
}
"#;

/// One reported finding: which lint fired, and on which source line it fired.
#[derive(Debug)]
struct LintFinding {
    lint: String,
    source_line: String,
}

/// Which findings a measurement keeps from the diagnostic stream.
#[derive(Clone, Copy)]
enum Keep {
    /// Only the named lints; a kept finding without a primary span is
    /// instrument failure, because these lints always report on a statement.
    Lints(&'static [&'static str]),
    /// Every lint. Findings without a primary span (group-level advice such as
    /// `blanket_clippy_restriction_lints`) are skipped: they cannot sit on a
    /// fixture line and so cannot bear on any assertion below.
    Every,
}

impl Keep {
    fn admits(self, lint: &str) -> bool {
        match self {
            Keep::Lints(lints) => lints.contains(&lint),
            Keep::Every => true,
        }
    }
}

const LOCK_LINTS: &[&str] = &[RUST_LOCK_LINT, CLIPPY_LOCK_LINT];

#[test]
fn rustc_row_covers_standard_library_guards_and_not_parking_lot() -> Result<()> {
    // Isolate the rustc row: the Clippy row is silenced, so anything reported
    // here is coverage the compiler lint supplies on its own.
    let findings = measure_fixture(
        &["-A", CLIPPY_LOCK_LINT, "--force-warn", RUST_LOCK_LINT],
        Keep::Lints(LOCK_LINTS),
    )?;

    assert_every_finding_is(&findings, RUST_LOCK_LINT)?;
    assert_covers(&findings, &STD_DISCARD_BINDINGS)?;
    assert_silent_on(&findings, &PARKING_LOT_DISCARD_BINDINGS, PARTITION_WRONG)?;
    assert_silent_on(&findings, &OWNED_AND_MAPPED_DISCARD_BINDINGS, LOCK_ROW_GREW)?;
    assert_silent_on(&findings, &EXPLICIT_DROP_DISCARD_BINDINGS, LOCK_ROW_GREW)?;
    assert_silent_on_held_guards(&findings)?;
    Ok(())
}

#[test]
fn clippy_row_covers_parking_lot_guards_and_not_standard_library() -> Result<()> {
    // Isolate the Clippy row the same way. If Clippy had kept covering the
    // standard-library guards it uplifted to rustc, the two rows would overlap
    // and the ledger's non-overlap note would be wrong; the silence assertion
    // below is what would fail.
    let findings = measure_fixture(
        &["-A", RUST_LOCK_LINT, "--force-warn", CLIPPY_LOCK_LINT],
        Keep::Lints(LOCK_LINTS),
    )?;

    assert_every_finding_is(&findings, CLIPPY_LOCK_LINT)?;
    assert_covers(&findings, &PARKING_LOT_DISCARD_BINDINGS)?;
    assert_silent_on(&findings, &STD_DISCARD_BINDINGS, PARTITION_WRONG)?;
    assert_silent_on(&findings, &OWNED_AND_MAPPED_DISCARD_BINDINGS, LOCK_ROW_GREW)?;
    assert_silent_on(&findings, &EXPLICIT_DROP_DISCARD_BINDINGS, LOCK_ROW_GREW)?;
    assert_silent_on_held_guards(&findings)?;
    Ok(())
}

#[test]
fn discards_outside_the_governed_forms_are_covered_by_neither_row() -> Result<()> {
    // The measured edge of the two lock rows' union, recorded so the policy
    // states a boundary rather than implying the rows cover every discard. On
    // the pinned toolchain both lints recognise exactly one shape: `let _ =
    // <expr>` whose type is a known borrowed guard. Three discards with
    // identical meaning fall outside it:
    //
    //   let _ = m.lock_arc();              owned guard
    //   let _ = MutexGuard::map(g, ...);   mapped guard
    //   drop(m.lock());                    same discard, different syntax
    //
    // #14579 ruled on each. The first two are `#[must_use]` types and belong to
    // `clippy::let_underscore_must_use` — proved by
    // `let_underscore_must_use_owns_owned_and_mapped_guard_discards`. The third
    // is seen by no Clippy lint at all and stays with #11236's deliberate-
    // discard contract — proved by
    // `explicit_drop_discards_are_covered_by_no_clippy_lint_at_any_level`.
    //
    // This test keeps the lock rows honest about their own edge. If a future
    // toolchain teaches either lock lint one of these forms, it fails, and the
    // failure means the ledger reason, `docs/CLIPPY_POLICY.md`, and the ruling
    // should say the lock row now owns that form — not that anything regressed.
    let findings = measure_fixture(
        &["--force-warn", RUST_LOCK_LINT, "--force-warn", CLIPPY_LOCK_LINT],
        Keep::Lints(LOCK_LINTS),
    )?;

    assert_silent_on(&findings, &OWNED_AND_MAPPED_DISCARD_BINDINGS, LOCK_ROW_GREW)?;
    assert_silent_on(&findings, &EXPLICIT_DROP_DISCARD_BINDINGS, LOCK_ROW_GREW)?;
    Ok(())
}

#[test]
fn let_underscore_must_use_owns_owned_and_mapped_guard_discards() -> Result<()> {
    // #14579's ruling for two of the three forms the lock rows miss. An owned
    // `ArcMutexGuard`/`ArcRwLock*Guard` and a `MappedMutexGuard` are
    // `#[must_use]` types, so `clippy::let_underscore_must_use` — already a
    // tracked deny row in this ledger, activated by #11240 after #11236 — is
    // the lint that sees `let _ = m.lock_arc()` and a discarded mapped guard.
    // Nothing lock-specific is missing from that path. Both lock rows are
    // silenced here so the coverage measured is this row's own.
    //
    // The row also reports the standard-library discards, which the rustc row
    // owns; that overlap is harmless and not asserted. It does not report the
    // borrowed `parking_lot` discards (Clippy routes those to the lock lint
    // alone), and it cannot see `drop(m.lock())`, which has no `let _` to
    // inspect. Held guards must stay silent, or the row would not discriminate.
    let findings = measure_fixture(
        &["-A", RUST_LOCK_LINT, "-A", CLIPPY_LOCK_LINT, "--force-warn", CLIPPY_MUST_USE_LINT],
        Keep::Lints(&[CLIPPY_MUST_USE_LINT]),
    )?;

    assert_every_finding_is(&findings, CLIPPY_MUST_USE_LINT)?;
    assert_covers(&findings, &OWNED_AND_MAPPED_DISCARD_BINDINGS)?;
    assert_silent_on(&findings, &EXPLICIT_DROP_DISCARD_BINDINGS, MUST_USE_SEES_DROP)?;
    assert_silent_on_held_guards(&findings)?;
    Ok(())
}

#[test]
fn explicit_drop_discards_are_covered_by_no_clippy_lint_at_any_level() -> Result<()> {
    // #14579's ruling for the third form. `drop(m.lock())` acquires and
    // releases in one expression and offers no `let _` for either lock row or
    // the must-use row to inspect. This sweep enables every Clippy group, so
    // the measurement says "no lint on this toolchain", not merely "not those
    // rows". That is why the form stays with #11236's deliberate-discard
    // contract, which already names `drop(lock())`-as-synchronization a
    // rejected shape and will need a non-Clippy instrument for it: nothing
    // here can be activated to close the gap.
    //
    // A change detector in the other direction: a future Clippy that gains
    // such a lint fails this test, and the failure means the ruling can be
    // revisited with a real lint to activate.
    let mut flags = vec!["--force-warn", RUST_LOCK_LINT, "--force-warn", CLIPPY_LOCK_LINT];
    for group in CLIPPY_GROUPS {
        flags.extend(["-W", group]);
    }
    let findings = measure_fixture(&flags, Keep::Every)?;

    // Instrument liveness: the sweep has to be seeing the discards on the
    // neighbouring lines, or silence on the drop lines would prove nothing.
    assert_covers(&findings, &PARKING_LOT_DISCARD_BINDINGS)?;
    assert_covers(&findings, &OWNED_AND_MAPPED_DISCARD_BINDINGS)?;

    for statement in EXPLICIT_DROP_DISCARD_STATEMENTS {
        if let Some(finding) = findings.iter().find(|finding| finding.source_line == statement) {
            bail!(
                "{} now reports `{statement}`; a Clippy lint covers the explicit-drop discard, so #14579's ruling that no lint can be activated for it should be revisited",
                finding.lint
            );
        }
    }
    Ok(())
}

#[test]
fn the_two_rows_jointly_cover_every_borrowed_guard_discard() -> Result<()> {
    // The reason both rows are pinned: neither tool covers the whole invariant,
    // so only the union is the contract. Running them together must reach every
    // borrowed-guard discard in the fixture and still leave every held guard
    // alone. Owned (`*_arc`) guards are outside this union by measurement, not
    // by omission — see
    // `discards_outside_the_governed_forms_are_covered_by_neither_row`.
    let findings = measure_fixture(
        &["--force-warn", RUST_LOCK_LINT, "--force-warn", CLIPPY_LOCK_LINT],
        Keep::Lints(LOCK_LINTS),
    )?;

    assert_covers(&findings, &STD_DISCARD_BINDINGS)?;
    assert_covers(&findings, &PARKING_LOT_DISCARD_BINDINGS)?;
    assert_silent_on_held_guards(&findings)?;

    let std_lints = lints_reported_for(&findings, &STD_DISCARD_BINDINGS);
    let parking_lot_lints = lints_reported_for(&findings, &PARKING_LOT_DISCARD_BINDINGS);
    if std_lints != vec![RUST_LOCK_LINT.to_owned()] {
        bail!("standard-library discards should be owned by {RUST_LOCK_LINT} alone: {std_lints:?}");
    }
    if parking_lot_lints != vec![CLIPPY_LOCK_LINT.to_owned()] {
        bail!(
            "parking_lot discards should be owned by {CLIPPY_LOCK_LINT} alone: {parking_lot_lints:?}"
        );
    }
    Ok(())
}

/// Compile the fixture with the given lint flags and return the findings it
/// reported, filtered by `keep`.
fn measure_fixture(lint_flags: &[&str], keep: Keep) -> Result<Vec<LintFinding>> {
    let manifest = fixture_manifest(&workspace_parking_lot_version(super::test_root())?);
    let fixture = tempdir()?;
    write_fixture(fixture.path(), &manifest)?;

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut command = Command::new(cargo);
    command
        .current_dir(fixture.path())
        .args(["clippy", "--offline", "--quiet", "--lib", "--no-deps", "--message-format=json"])
        .arg("--");
    command.args(lint_flags);
    let output = command
        .env("CARGO_TARGET_DIR", fixture.path().join("target"))
        .env("CARGO_TERM_COLOR", "never")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS")
        .output()?;

    // Every lint here is warned or force-warned, so a clean instrument exits
    // zero. A non-zero exit means the fixture did not build — a missing
    // registry entry, a toolchain mismatch — and that is instrument failure,
    // not evidence about lint coverage. Reporting it as a pass would be the
    // vacuous-oracle failure this proof exists to avoid.
    if !output.status.success() {
        let stdout = bounded_diagnostic(&output.stdout);
        let stderr = bounded_diagnostic(&output.stderr);
        bail!(
            "lock-partition fixture failed to build under {lint_flags:?}; the lint measurement is not valid:\nstdout (bounded):\n{stdout}\nstderr (bounded):\n{stderr}"
        );
    }

    collect_findings(&output.stdout, keep)
}

fn write_fixture(root: &Path, manifest: &str) -> Result<()> {
    let source_dir = root.join("src");
    fs::create_dir(&source_dir)?;
    fs::write(root.join("Cargo.toml"), manifest)?;
    fs::write(source_dir.join("lib.rs"), FIXTURE_SOURCE)?;
    Ok(())
}

/// Parse the strict JSON diagnostic stream, keeping the findings `keep` admits.
///
/// `--message-format=json` makes stdout a machine channel, so a non-JSON line
/// is instrument failure and is not skipped as unrelated chatter.
fn collect_findings(stdout: &[u8], keep: Keep) -> Result<Vec<LintFinding>> {
    let mut findings = Vec::new();
    let lines = stdout.split(|byte| *byte == b'\n').collect::<Vec<_>>();

    for (index, line) in lines.iter().enumerate() {
        if line.is_empty() {
            let is_single_terminal_newline = index + 1 == lines.len();
            if is_single_terminal_newline {
                continue;
            }
            bail!("blank JSON record in strict stdout stream");
        }
        let event: JsonValue = serde_json::from_slice(line)?;
        let Some(lint) = event.pointer("/message/code/code").and_then(JsonValue::as_str) else {
            continue;
        };
        if !keep.admits(lint) {
            continue;
        }

        // Key findings on the source text of the primary span rather than on a
        // line number, so the fixture stays editable without silently
        // retargeting the assertions at the wrong statement.
        let Some(source_line) =
            event.pointer("/message/spans/0/text/0/text").and_then(JsonValue::as_str)
        else {
            if matches!(keep, Keep::Every) {
                continue;
            }
            bail!("{lint} finding carried no primary span text");
        };
        findings.push(LintFinding {
            lint: lint.to_owned(),
            source_line: source_line.trim().to_owned(),
        });
    }

    Ok(findings)
}

fn assert_every_finding_is(findings: &[LintFinding], expected: &str) -> Result<()> {
    if findings.is_empty() {
        bail!("{expected} reported nothing; an always-silent lint proves no coverage");
    }
    for finding in findings {
        if finding.lint != expected {
            bail!(
                "expected only {expected} while the other rows were silenced, got {} on `{}`",
                finding.lint,
                finding.source_line
            );
        }
    }
    Ok(())
}

fn assert_covers(findings: &[LintFinding], bindings: &[&str]) -> Result<()> {
    for binding in bindings {
        if !findings.iter().any(|finding| finding.source_line.contains(binding)) {
            bail!("no finding reported the discarded `{binding}` guard");
        }
    }
    Ok(())
}

/// Why an unexpected finding on one of `bindings` is a defect, named per
/// boundary so the failure says which recorded fact moved.
const PARTITION_WRONG: &str = "the recorded rustc/Clippy partition is wrong";
const LOCK_ROW_GREW: &str = "a lock row has grown past its recorded boundary — widen the ledger reason, the Clippy policy doc, and the #14579 ruling to say it now owns this form";
const MUST_USE_SEES_DROP: &str = "let_underscore_must_use now sees an explicit-drop discard, which has no let-underscore to inspect; the #14579 ruling that this form has no lint owner should be revisited";

fn assert_silent_on(findings: &[LintFinding], bindings: &[&str], why: &str) -> Result<()> {
    for binding in bindings {
        if let Some(finding) = findings.iter().find(|finding| finding.source_line.contains(binding))
        {
            bail!("{} unexpectedly covered `{binding}`; {why}", finding.lint);
        }
    }
    Ok(())
}

fn assert_silent_on_held_guards(findings: &[LintFinding]) -> Result<()> {
    if let Some(finding) =
        findings.iter().find(|finding| finding.source_line.contains(HELD_BINDING_MARKER))
    {
        bail!(
            "{} fired on a guard that is actually held (`{}`); the lint is not discriminating between discarded and held acquisitions",
            finding.lint,
            finding.source_line
        );
    }
    Ok(())
}

fn lints_reported_for(findings: &[LintFinding], bindings: &[&str]) -> Vec<String> {
    let mut lints = findings
        .iter()
        .filter(|finding| bindings.iter().any(|binding| finding.source_line.contains(binding)))
        .map(|finding| finding.lint.clone())
        .collect::<Vec<_>>();
    lints.sort();
    lints.dedup();
    lints
}

fn bounded_diagnostic(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let mut output = String::new();

    for (index, line) in lines.by_ref().take(MAX_DIAGNOSTIC_LINES).enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let mut chars = line.chars();
        output.extend(chars.by_ref().take(MAX_DIAGNOSTIC_CHARS_PER_LINE));
        if chars.next().is_some() {
            output.push('…');
        }
    }
    if lines.next().is_some() {
        output.push_str("\n… additional lines omitted");
    }
    if output.is_empty() {
        output.push_str("<empty>");
    }

    output
}
