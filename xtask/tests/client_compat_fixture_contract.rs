//! Contract for the shared fixture and expectation-set identity consumed by
//! `agent_client_compat.v1` and `editor_client_compat.v1`.
//!
//! Both receipt contracts bind a receipt to the exact fixture and expectation
//! set it ran against. These cases prove the identity actually discriminates,
//! rather than trusting that the canonical constants happen to be well formed.

use anyhow::{Result, ensure};
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use xtask::client_compat_fixture::{
    CANONICAL_EXPECTATION_IDS, CANONICAL_EXPECTATION_SET_ID, canonical_expectation_set_digest,
    expectation_set_digest, fixture_digest, is_reason_token,
};

const SET: &str = "test-set";

#[test]
fn expectation_set_rejects_empty_duplicate_and_untokenized_ids() -> Result<()> {
    ensure!(expectation_set_digest(SET, &[]).is_err(), "empty expectation set was accepted");
    ensure!(
        expectation_set_digest(SET, &["definition.widget_new", "definition.widget_new"]).is_err(),
        "duplicate expectation ids were accepted"
    );
    ensure!(
        expectation_set_digest(SET, &["Definition.Widget"]).is_err(),
        "uppercase expectation id was accepted"
    );
    ensure!(
        expectation_set_digest(SET, &["hover widget"]).is_err(),
        "expectation id containing a space was accepted"
    );
    ensure!(
        expectation_set_digest("Not A Token", &["definition.widget_new"]).is_err(),
        "expectation set id that is not a reason token was accepted"
    );
    ensure!(
        expectation_set_digest(SET, &["definition.widget_new"]).is_ok(),
        "a valid expectation set was rejected"
    );
    Ok(())
}

/// Ordering must not change identity, but membership and set id must.
///
/// These are paired deliberately: a digest that ignored ordering *and*
/// membership would satisfy the first case alone.
#[test]
fn expectation_set_digest_is_order_independent_and_subject_bound() -> Result<()> {
    ensure!(
        expectation_set_digest(SET, &["a.one", "b.two"])?
            == expectation_set_digest(SET, &["b.two", "a.one"])?,
        "expectation digest depended on input order"
    );
    ensure!(
        expectation_set_digest(SET, &["a.one", "b.two"])?
            != expectation_set_digest(SET, &["a.one", "b.three"])?,
        "expectation digest ignored a changed member"
    );
    ensure!(
        expectation_set_digest(SET, &["a.one"])?
            != expectation_set_digest("other-set", &["a.one"])?,
        "expectation digest ignored the set id"
    );
    Ok(())
}

#[test]
fn canonical_expectation_set_is_stable_and_well_formed() -> Result<()> {
    let first = canonical_expectation_set_digest()?;
    ensure!(
        first == canonical_expectation_set_digest()?,
        "canonical expectation digest was not deterministic"
    );
    ensure!(
        first == expectation_set_digest(CANONICAL_EXPECTATION_SET_ID, CANONICAL_EXPECTATION_IDS)?,
        "canonical digest diverged from the shared primitive"
    );
    ensure!(
        first.starts_with("sha256:") && first.len() == "sha256:".len() + 64,
        "canonical expectation digest had the wrong identity shape"
    );
    ensure!(
        is_reason_token(CANONICAL_EXPECTATION_SET_ID),
        "canonical expectation set id is not a reason token"
    );
    for id in CANONICAL_EXPECTATION_IDS {
        ensure!(is_reason_token(id), "canonical expectation id is not a reason token: {id}");
    }
    Ok(())
}

#[test]
fn reason_tokens_reject_leading_punctuation_and_non_ascii() -> Result<()> {
    for accepted in ["lifecycle.shutdown", "utf-16", "0abc"] {
        ensure!(is_reason_token(accepted), "reason token was rejected: {accepted}");
    }
    for rejected in ["", "_leading", ".leading", "-leading", "Upper", "héllo"] {
        ensure!(!is_reason_token(rejected), "reason token was accepted: {rejected}");
    }
    Ok(())
}

/// `fixture_digest`'s guards are error paths the receipt contracts never take,
/// because they always point it at the real canonical fixture.
#[test]
fn fixture_digest_rejects_a_missing_or_empty_root() -> Result<()> {
    ensure!(
        fixture_digest(Path::new("this/path/does/not/exist")).is_err(),
        "non-directory fixture root was accepted"
    );
    let empty = TempDir::new()?;
    ensure!(fixture_digest(empty.path()).is_err(), "fixture root with no files was accepted");
    Ok(())
}

/// The digest must separate the two things a fixture can change independently.
#[test]
fn fixture_digest_binds_relative_path_and_content() -> Result<()> {
    let root = TempDir::new()?;
    fs::write(root.path().join("a.pl"), b"one")?;
    let before = fixture_digest(root.path())?;

    fs::write(root.path().join("a.pl"), b"two")?;
    ensure!(before != fixture_digest(root.path())?, "fixture digest ignored changed content");

    fs::write(root.path().join("a.pl"), b"one")?;
    ensure!(before == fixture_digest(root.path())?, "fixture digest was not restored by content");

    fs::rename(root.path().join("a.pl"), root.path().join("b.pl"))?;
    ensure!(before != fixture_digest(root.path())?, "fixture digest ignored the relative path");
    Ok(())
}
