//! Cargo Dependabot titles must carry one dependency scope (#13477).
//!
//! GitHub's `include: "scope"` appends `(deps)` or `(deps-dev)` after the
//! configured prefix. Combining `prefix: "chore(deps)"` with that include
//! produced duplicated titles such as `chore(deps)(deps): bump ...`
//! (#12208, #12209). #13482 changed only the Cargo row's prefix to `chore`.
//!
//! This suite is the source contract that PR deferred: it accepts
//! `prefix: "chore"` plus `include: "scope"`, and it rejects the doubled
//! combination. It does not govern GitHub Actions or npm — those remain
//! #14180 — and it does not replace the three-ecosystem validator on #13478.
//!
//! The live rendering after #13482 is already observed on Cargo Dependabot
//! PRs (#14225, #14229, #14231, #14403, #14706). This suite cannot call
//! GitHub; it asserts the committed composition that produces that rendering.

use anyhow::{Result, anyhow, bail};

#[path = "support/dependabot_yaml.rs"]
mod dependabot_yaml;

use dependabot_yaml::{
    DEPENDABOT_YML, assert_single_cargo_scope, cargo_commit_message, cargo_update_entry,
    cargo_update_entry_in, rendered_title_prefix,
};

fn check_yaml(yaml: &str) -> Result<()> {
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml)?;
    let entry = cargo_update_entry_in(&doc)?;
    let msg = cargo_commit_message(&entry)?;
    assert_single_cargo_scope(&msg)
}

fn must_err_containing(yaml: &str, needle: &str) -> Result<()> {
    match check_yaml(yaml) {
        Ok(()) => {
            bail!("expected the cargo commit-message contract to reject this document")
        }
        Err(err) => {
            let text = format!("{err:#}");
            if !text.contains(needle) {
                bail!("rejection did not name `{needle}`: {text}");
            }
            Ok(())
        }
    }
}

/// The committed Cargo row is the combination GitHub renders once.
#[test]
fn committed_cargo_row_uses_chore_plus_scope() -> Result<()> {
    let entry = cargo_update_entry()?;
    let msg = cargo_commit_message(&entry)?;
    assert_single_cargo_scope(&msg)
}

/// Restoring `prefix: "chore(deps)"` while `include: "scope"` remains must fail.
///
/// This is the pre-#13482 combination. A checker that only required the
/// rendered title to *contain* `chore(deps)` would stay green on it.
#[test]
fn chore_deps_plus_scope_is_rejected() -> Result<()> {
    must_err_containing(
        r#"
updates:
  - package-ecosystem: cargo
    directory: /
    commit-message:
      prefix: "chore(deps)"
      include: "scope"
"#,
        "chore(deps)(deps)",
    )
}

/// Dropping `include: "scope"` instead of narrowing the prefix is the other
/// realistic wrong repair: titles become `chore: ...` and lose deps /
/// deps-dev discrimination.
#[test]
fn omitting_include_scope_is_rejected() -> Result<()> {
    must_err_containing(
        r#"
updates:
  - package-ecosystem: cargo
    directory: /
    commit-message:
      prefix: "chore"
"#,
        "include",
    )
}

/// Keeping `prefix: "chore(deps)"` and deleting `include` also yields a
/// single-looking `chore(deps): ...` title, but by the wrong mechanism, and
/// it still cannot emit `deps-dev`.
#[test]
fn chore_deps_without_include_is_rejected() -> Result<()> {
    must_err_containing(
        r#"
updates:
  - package-ecosystem: cargo
    directory: /
    commit-message:
      prefix: "chore(deps)"
"#,
        "include",
    )
}

/// A second cargo `/` row with the old prefix must not hide behind `.find()`.
#[test]
fn duplicate_cargo_workspace_rows_fail_closed() -> Result<()> {
    must_err_containing(
        r#"
updates:
  - package-ecosystem: cargo
    directory: /
    commit-message:
      prefix: "chore"
      include: "scope"
  - package-ecosystem: cargo
    directory: /
    commit-message:
      prefix: "chore(deps)"
      include: "scope"
"#,
        "2 cargo update entries",
    )
}

/// Missing the cargo `/` row is a defect, not a vacuously empty pass.
#[test]
fn missing_cargo_row_fails_closed() -> Result<()> {
    must_err_containing(
        r#"
updates:
  - package-ecosystem: github-actions
    directory: /
    commit-message:
      prefix: "chore"
      include: "scope"
"#,
        "must keep a cargo update entry",
    )
}

/// Doubled scopes on other ecosystems are out of this claim. Scanning every
/// row would turn this suite red until #14180, or keep it green by accident
/// if those rows were later repaired first.
#[test]
fn other_ecosystem_doubled_scope_does_not_fail_the_cargo_contract() -> Result<()> {
    check_yaml(
        r#"
updates:
  - package-ecosystem: cargo
    directory: /
    commit-message:
      prefix: "chore"
      include: "scope"
  - package-ecosystem: github-actions
    directory: /
    commit-message:
      prefix: "chore(deps)"
      include: "scope"
  - package-ecosystem: npm
    directory: /vscode-extension
    commit-message:
      prefix: "chore(deps)"
      include: "scope"
"#,
    )
}

/// The composition oracle itself must still produce the doubled form when
/// fed the old prefix. A "helpful" renderer that stripped an existing
/// `(deps)` would make the negative control in
/// `chore_deps_plus_scope_is_rejected` assert against the wrong string.
#[test]
fn composition_oracle_renders_github_scope_once() -> Result<()> {
    if rendered_title_prefix("chore", "scope", "deps") != "chore(deps)" {
        bail!("prefix `chore` plus include `scope` must render `chore(deps)`");
    }
    if rendered_title_prefix("chore", "scope", "deps-dev") != "chore(deps-dev)" {
        bail!("prefix `chore` plus include `scope` must render `chore(deps-dev)`");
    }
    if rendered_title_prefix("chore(deps)", "scope", "deps") != "chore(deps)(deps)" {
        bail!(
            "prefix `chore(deps)` plus include `scope` must still render the doubled \
             form; rewriting the oracle to strip an existing scope would hide the \
             #12208 / #12209 failure mode"
        );
    }
    if rendered_title_prefix("chore", "not-scope", "deps") != "chore" {
        return Err(anyhow!(
            "an include other than `scope` must not invent a parenthetical; \
             GitHub only documents `scope`"
        ));
    }
    Ok(())
}

const REQUIRED_STEP: &str = "      - name: Dependabot Cargo commit-message contract (required merge surface)\n        run: cargo test -p xtask --test dependabot_cargo_commit_message --locked -- --nocapture";

fn check_all_targets_job(workflow: &str) -> Result<&str> {
    let start = workflow
        .find("\n  check-all-targets:")
        .ok_or_else(|| anyhow!("`.github/workflows/ci.yml` must keep job `check-all-targets`"))?;
    let section = &workflow[start + 1..];
    let end = section
        .match_indices('\n')
        .find(|(idx, _)| {
            let line = &section[idx + 1..];
            line.starts_with("  ") && !line.starts_with("   ") && !line.starts_with("  #")
        })
        .map(|(idx, _)| idx)
        .unwrap_or(section.len());
    Ok(&section[..end])
}

fn required_wiring_is_present(workflow: &str) -> bool {
    let Ok(job) = check_all_targets_job(workflow) else {
        return false;
    };
    job.contains(REQUIRED_STEP) && !job.contains("continue-on-error: true")
}

/// `.github/dependabot.yml` is not in ci-scope's `CI_CONFIG_PATHS`, so this
/// suite has to stay on the required merge surface or it compiles and never
/// executes — the #14585 / #14178 failure mode. A substring mention in a
/// comment is not enough.
#[test]
fn required_merge_surface_executes_this_contract() -> Result<()> {
    let path = dependabot_yaml::project_root().join(".github/workflows/ci.yml");
    let workflow = std::fs::read_to_string(&path)
        .map_err(|err| anyhow!("reading {}: {err}", path.display()))?
        .replace("\r\n", "\n");
    if !required_wiring_is_present(&workflow) {
        bail!(
            "{DEPENDABOT_YML} edits do not reach the advisory routed lane, so this \
             contract must stay as a required `check-all-targets` step in \
             `.github/workflows/ci.yml` (#13477)"
        );
    }
    let commented = workflow.replace(
        REQUIRED_STEP,
        "      # cargo test -p xtask --test dependabot_cargo_commit_message --locked -- --nocapture",
    );
    if required_wiring_is_present(&commented) {
        bail!(
            "a comment containing the cargo test command must not satisfy the \
             wiring contract (#13477)"
        );
    }
    Ok(())
}
