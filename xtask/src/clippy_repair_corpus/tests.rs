//! Corpus falsifier probes (#11649 "first falsifiers"): each designated
//! corruption must fail validation with its named error class, and the
//! checked-in corpus itself must stay green.

use super::validate::{FIXTURE_DIR, validate_corpus, violations_for};
use color_eyre::eyre::{Result, WrapErr, bail};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Recursively copy the checked-in fixture tree into an isolated temp repo
/// layout that still carries the authority files validators resolve against.
fn isolated_repo() -> Result<(TempDir, PathBuf)> {
    let temp = TempDir::new().wrap_err("creating isolated corpus repo")?;
    let root = temp.path().to_path_buf();
    let src_fixtures = repo_root().join(FIXTURE_DIR);
    let dst_fixtures = root.join(FIXTURE_DIR);
    copy_tree(&src_fixtures, &dst_fixtures)?;

    // Authority files referenced by bound cases must exist relative to root.
    for authority in [
        ".ci/gate-policy.yaml",
        "Cargo.toml",
        "rust-toolchain.toml",
        "docs/CLIPPY_POLICY.md",
        "policy/clippy-lints.toml",
        "policy/clippy-debt.toml",
        "policy/generated-status-contract.toml",
        ".cargo-semver-checks.toml",
        "scripts/check_features_invariants.py",
        "deny.toml",
        "xtask/src/rust_hygiene.rs",
        "xtask/src/publication_drift/mod.rs",
    ] {
        let source = repo_root().join(authority);
        let target = root.join(authority);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .wrap_err_with(|| format!("creating {}", parent.display()))?;
        }
        fs::copy(&source, &target)
            .wrap_err_with(|| format!("copying authority {authority} into isolation"))?;
    }
    // The lint catalog directory backs governed-lint resolution.
    copy_tree(&repo_root().join("policy/clippy-lints.d"), &root.join("policy/clippy-lints.d"))?;
    Ok((temp, root))
}

fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target).wrap_err_with(|| format!("creating {}", target.display()))?;
    let entries = fs::read_dir(source).wrap_err_with(|| format!("reading {}", source.display()))?;
    for entry in entries {
        let entry = entry.wrap_err_with(|| format!("walking {}", source.display()))?;
        let entry_path = entry.path();
        let destination = target.join(entry.file_name());
        if entry_path.is_dir() {
            copy_tree(&entry_path, &destination)?;
        } else {
            fs::copy(&entry_path, &destination).wrap_err_with(|| {
                format!("copying {} to {}", entry_path.display(), destination.display())
            })?;
        }
    }
    Ok(())
}

fn case_path(root: &Path, case_id: &str) -> PathBuf {
    root.join(FIXTURE_DIR).join("cases").join(format!("{case_id}.json"))
}

fn patch_file(path: &Path, from: &str, to: &str) -> Result<()> {
    let raw = fs::read_to_string(path).wrap_err_with(|| format!("reading {}", path.display()))?;
    if !raw.contains(from) {
        bail!("probe precondition failed: `{from}` absent from {}", path.display());
    }
    fs::write(path, raw.replacen(from, to, 1))
        .wrap_err_with(|| format!("writing {}", path.display()))
}

#[test]
fn checked_in_corpus_validates_and_reports_counts() -> Result<()> {
    let report = validate_corpus(&repo_root())?;
    if report.case_count != 50 {
        bail!("expected 50 frozen cases, found {}", report.case_count);
    }
    if report.bound_count == 0 || report.pending_count == 0 {
        bail!(
            "corpus must carry both bound ({}) and pending-owner ({}) authorities",
            report.bound_count,
            report.pending_count
        );
    }
    Ok(())
}

/// Read one quoted string field out of the manifest text.
fn manifest_field(raw: &str, field: &str) -> Result<String> {
    let needle = format!("\"{field}\": \"");
    let after = raw
        .split(&needle)
        .nth(1)
        .ok_or_else(|| color_eyre::eyre::eyre!("{field} not found in manifest"))?;
    let value = after
        .split('"')
        .next()
        .ok_or_else(|| color_eyre::eyre::eyre!("{field} value not terminated"))?;
    Ok(value.to_owned())
}

/// The corpus records the toolchain it was authored under as history and the
/// release line it was validated under as the field the tree can invalidate.
/// A producer record that no longer matches the tree is history, not a
/// defect, so it must not raise a violation.
#[test]
fn producer_toolchain_history_is_not_compared_against_the_tree() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let manifest_raw = fs::read_to_string(root.join(FIXTURE_DIR).join("manifest.v1.json"))?;
    let producer = manifest_field(&manifest_raw, "producer_toolchain")?;
    let validated = manifest_field(&manifest_raw, "validated_under_toolchain")?;
    let channel_raw = fs::read_to_string(root.join("rust-toolchain.toml"))?;
    if producer == validated {
        bail!("the corpus must record a producer release distinct from the validated one");
    }
    if !channel_raw.contains(&validated) {
        bail!("validated release {validated} is not the tree's pinned channel");
    }
    let violations = violations_for(&root)?;
    if violations.iter().any(|v| v.contains("producer_toolchain")) {
        bail!("producer provenance was compared against the tree: {violations:?}");
    }
    Ok(())
}

/// A release line that is a prefix of the tree's release names a different
/// release. `1.10` is a substring of `1.100.0`, so a substring comparison would
/// accept it; the validator must not.
#[test]
fn toolchain_validation_pin_rejects_a_prefix_release() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let manifest = root.join(FIXTURE_DIR).join("manifest.v1.json");
    patch_file(
        &manifest,
        "\"validated_under_toolchain\": \"1.100\"",
        "\"validated_under_toolchain\": \"1.10\"",
    )?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("validated_under_toolchain")) {
        bail!("a prefix release was accepted as the validation pin: {violations:?}");
    }
    Ok(())
}

/// A validation pin that names a different release must be rejected.
#[test]
fn toolchain_validation_pin_rejects_a_different_release() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let manifest = root.join(FIXTURE_DIR).join("manifest.v1.json");
    patch_file(
        &manifest,
        "\"validated_under_toolchain\": \"1.100\"",
        "\"validated_under_toolchain\": \"1.99\"",
    )?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("validated_under_toolchain")) {
        bail!("a different release was accepted as the validation pin: {violations:?}");
    }
    Ok(())
}

/// The validation pin is compared against the tree's pinned channel, not
/// against a constant: moving the tree's channel must invalidate the pin.
#[test]
fn toolchain_validation_pin_is_compared_against_the_tree_channel() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    patch_file(&root.join("rust-toolchain.toml"), "channel = \"1.100.0\"", "channel = \"1.99.0\"")?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("validated_under_toolchain")) {
        bail!("moving the tree's channel did not invalidate the validation pin: {violations:?}");
    }
    Ok(())
}

/// A malformed producer record is still a defect: the history field is shaped
/// even though it is never compared against the tree.
#[test]
fn malformed_producer_toolchain_is_rejected() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let manifest = root.join(FIXTURE_DIR).join("manifest.v1.json");
    patch_file(
        &manifest,
        "\"producer_toolchain\": \"1.95\"",
        "\"producer_toolchain\": \"not-a-release\"",
    )?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("producer_toolchain")) {
        bail!("a malformed producer record was accepted: {violations:?}");
    }
    Ok(())
}

#[test]
fn digest_tampering_without_digest_movement_fails() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let path = case_path(&root, "A01-file-wide-suppression-carveout");
    patch_file(&path, "\"content\": \"#!", "\"content\": \"X#!")?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("digest does not match")) {
        bail!("digest tamper was not detected: {violations:?}");
    }
    Ok(())
}

#[test]
fn duplicate_manifest_identity_fails() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let manifest = root.join(FIXTURE_DIR).join("manifest.v1.json");
    // Repoint A02's identity onto A01's ID while keeping its own file name,
    // which must trip uniqueness before layout checks.
    patch_file(
        &manifest,
        "\"case_id\": \"A02-dead-code-baseline-absorption\"",
        "\"case_id\": \"A01-file-wide-suppression-carveout\"",
    )?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("duplicate case id")) {
        bail!("duplicate identity was not detected: {violations:?}");
    }
    Ok(())
}

#[test]
fn deleted_required_case_cannot_make_validation_green() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    fs::remove_file(case_path(&root, "F39-lib-only-helper-deletion"))?;
    // Removing the file alone must fail even though the manifest still lists it;
    // removing it from the manifest too must still fail on the frozen denominator.
    let violations_after_file_loss = violations_for(&root)?;
    if !violations_after_file_loss
        .iter()
        .any(|v| v.contains("reading case file failed") || v.contains("is missing"))
    {
        bail!("silent file deletion was not detected: {violations_after_file_loss:?}");
    }
    let manifest = root.join(FIXTURE_DIR).join("manifest.v1.json");
    let raw = fs::read_to_string(&manifest)?;
    let key = raw.find("\"case_id\": \"F39-lib-only-helper-deletion\"").ok_or_else(|| {
        color_eyre::eyre::eyre!("F39 manifest entry not found for deletion probe")
    })?;
    let start =
        raw[..key].rfind('{').ok_or_else(|| color_eyre::eyre::eyre!("entry brace not found"))?;
    let end = raw[start..]
        .find('}')
        .map(|offset| start + offset + 1)
        .ok_or_else(|| color_eyre::eyre::eyre!("entry closing brace not found"))?;
    let mut patched = String::new();
    patched.push_str(&raw[..start]);
    if raw[end..].starts_with(',') {
        patched.push_str(&raw[end + 1..]);
    } else {
        patched.push_str(&raw[end..]);
    }
    fs::write(&manifest, patched)?;
    let violations = violations_for(&root)?;
    if !violations
        .iter()
        .any(|v| v.contains("required corpus case F39-lib-only-helper-deletion is missing"))
    {
        bail!("denominator deletion was not detected: {violations:?}");
    }
    Ok(())
}

#[test]
fn identical_negative_and_counterpart_content_fails() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let path = case_path(&root, "C17-ok-erasure-of-result");
    let raw = fs::read_to_string(&path)?;
    let mutation_digest = raw
        .split("\"dishonest_mutation\"")
        .nth(1)
        .and_then(|section| section.split("\"sha256\": \"").nth(1))
        .and_then(|rest| rest.split('"').next())
        .ok_or_else(|| color_eyre::eyre::eyre!("mutation digest not found"))?
        .to_owned();
    // Overwrite the LAST digest occurrence (the counterpart's) with the
    // mutation digest so the two recorded identities collide.
    let last_digest_start = raw
        .rfind("\"sha256\": \"")
        .ok_or_else(|| color_eyre::eyre::eyre!("counterpart digest position not found"))?;
    let value_start = last_digest_start + "\"sha256\": \"".len();
    let patched = format!(
        "{}\"sha256\": \"{mutation_digest}{}",
        &raw[..last_digest_start],
        &raw[value_start + 64..]
    );
    fs::write(&path, patched)?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("lacks a distinct positive counterpart")) {
        bail!("identical negative/control identity was not detected: {violations:?}");
    }
    Ok(())
}

#[test]
fn mutation_missing_trigger_evidence_fails() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let path = case_path(&root, "A03-cfg-test-attr-general-carveout");
    patch_file(
        &path,
        "#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]",
        "#![cfg_attr(test, warn(clippy::unwrap_used))]",
    )?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("absent from its own content")) {
        bail!("missing trigger evidence was not detected: {violations:?}");
    }
    Ok(())
}

#[test]
fn sanctioned_control_leaking_into_mutation_fails() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let path = case_path(&root, "B11-same-total-finding-swap");
    patch_file(
        &path,
        "// aggregate total still reads 2; the historical finding vanished.",
        "// aggregate total still reads 2; review_after keeps the row honest.",
    )?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("leaked into dishonest mutation")) {
        bail!("control leak was not detected: {violations:?}");
    }
    Ok(())
}

#[test]
fn pending_authority_claiming_packet_ready_fails() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let manifest = root.join(FIXTURE_DIR).join("manifest.v1.json");
    // A05 is pending_owner; flipping its manifest readiness must contradict.
    let raw = fs::read_to_string(&manifest)?;
    let needle = "\"case_id\": \"A05-command-missing-docs-reintroduction\"";
    let position = raw
        .find(needle)
        .ok_or_else(|| color_eyre::eyre::eyre!("A05 manifest entry missing for readiness probe"))?;
    let window_end = (position + needle.len()).min(raw.len());
    let after = &raw[window_end..];
    let ready_offset = after
        .find("\"packet_ready\": false")
        .ok_or_else(|| color_eyre::eyre::eyre!("A05 packet_ready flag missing"))?;
    let patched = format!(
        "{}{}{}",
        &raw[..window_end + ready_offset],
        "\"packet_ready\": true",
        &after[ready_offset + "\"packet_ready\": false".len()..]
    );
    fs::write(&manifest, patched)?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("packet_ready=true contradicts")) {
        bail!("pending-as-ready claim was not detected: {violations:?}");
    }
    Ok(())
}

#[test]
fn unknown_projection_fields_fail_closed() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let path = case_path(&root, "G45-restating-documentation");
    let raw = fs::read_to_string(&path)?;
    let patched = raw.replace(
        "{\n  \"schema_version\": 1,",
        "{\n  \"provider_projection\": \"gpt-x\",\n  \"schema_version\": 1,",
    );
    fs::write(&path, patched)?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("parsing case document failed")) {
        bail!("unknown projection field was accepted: {violations:?}");
    }
    if !violations.iter().any(|v| v.contains("unknown field")) {
        bail!("serde denial did not name the unknown field: {violations:?}");
    }
    Ok(())
}

#[test]
fn mutable_state_as_stable_identity_fails() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let long_hex = "a".repeat(40);
    let renamed = format!("C20-{long_hex}");
    patch_file(
        case_path(&root, "C20-log-only-error-consumption").as_path(),
        "\"case_id\": \"C20-log-only-error-consumption\"",
        &format!("\"case_id\": \"{renamed}\""),
    )?;
    // The manifest must agree with the embedded identity for the case to load;
    // both sides carrying the mutable hash then trips the grammar law.
    patch_file(
        root.join(FIXTURE_DIR).join("manifest.v1.json").as_path(),
        "\"case_id\": \"C20-log-only-error-consumption\"",
        &format!("\"case_id\": \"{renamed}\""),
    )?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("embeds a raw long hash as identity")) {
        bail!("mutable hash-like identity was not detected: {violations:?}");
    }
    Ok(())
}

#[test]
fn unresolvable_bound_authority_fails() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let path = case_path(&root, "A06-required-target-omission");
    patch_file(
        &path,
        "\"reference\": \"gate:compile_all_targets\"",
        "\"reference\": \"gate:no_such_gate\"",
    )?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("names no gate")) {
        bail!("fabricated gate authority was not detected: {violations:?}");
    }

    let (_temp2, root2) = isolated_repo()?;
    let path2 = case_path(&root2, "E38-generated-output-edited-generator-stale");
    patch_file(
        &path2,
        "\"reference\": \"file:policy/generated-status-contract.toml#\"",
        "\"reference\": \"file:policy/generated-status-contract.toml#no-such-anchor-text\"",
    )?;
    let violations2 = violations_for(&root2)?;
    if !violations2.iter().any(|v| v.contains("no longer contains pinned text")) {
        bail!("drifted file anchor was not detected: {violations2:?}");
    }
    Ok(())
}

#[test]
fn self_referential_authority_is_rejected() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let path = case_path(&root, "B13-consumed-finding-identity-reintroduction");
    // Pending case: convert to a bound citation pointing back into the corpus.
    patch_file(
        &path,
        "\"status\": \"pending_owner\",\n    \"owner_issue\": 11407,\n    \"unresolved_reason\": \"Consumptive finding-admission transitions (#11407) are unlanded; identity vocabulary exists only in issue prose.\"",
        "\"status\": \"bound\",\n    \"authority_kind\": \"file_contract\",\n    \"reference\": \"file:fixtures/clippy_repair_falsifiers/manifest.v1.json#corpus\"",
    )?;
    let violations = violations_for(&root)?;
    if !violations.iter().any(|v| v.contains("cites the corpus itself")) {
        bail!("self-referential authority was not detected: {violations:?}");
    }
    Ok(())
}

#[test]
fn discrimination_pairs_keep_their_semantic_identity() -> Result<()> {
    let (_temp, root) = isolated_repo()?;
    let a01 = fs::read_to_string(case_path(&root, "A01-file-wide-suppression-carveout"))?;
    if !a01.contains("#![allow(clippy::unwrap_used") || !a01.contains("ok_or_else") {
        bail!("A01 lost its carveout-vs-typed-refusal discrimination pair");
    }
    let f39 = fs::read_to_string(case_path(&root, "F39-lib-only-helper-deletion"))?;
    if !f39.contains("\"incident_reference\": \"#10600\"") {
        bail!("F39 lost its #10600 regression provenance");
    }
    Ok(())
}
