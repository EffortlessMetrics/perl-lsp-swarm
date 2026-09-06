//! GitHub Actions and npm Dependabot titles must carry one dependency scope
//! (#14180).
//!
//! GitHub's `include: "scope"` appends `(deps)` or `(deps-dev)` after the
//! configured prefix. Combining `prefix: "chore(deps)"` with that include
//! produced duplicated titles such as `chore(deps)(deps): bump ...`
//! (#12212, #12210) and `chore(deps)(deps-dev): bump ...` (#12207, #12206).
//!
//! This suite is the source contract for those two remaining rows. It accepts
//! `prefix: "chore"` plus `include: "scope"` on github-actions `/` and npm
//! `/vscode-extension`, and it rejects the doubled combination on either row.
//! It does not govern Cargo — #13482 already repaired that row and #14898 owns
//! its contract — and it does not replace the three-ecosystem validator on
//! #13478.
//!
//! Live repaired GitHub Actions and npm titles remain `NOT_PROVEN` until
//! Dependabot's next scheduled run. This suite cannot call GitHub; it asserts
//! the committed composition that produces that rendering.

use anyhow::{Result, anyhow, bail};

#[path = "support/dependabot_gha_npm.rs"]
mod dependabot_gha_npm;

use dependabot_gha_npm::{
    DEPENDABOT_YML, GITHUB_ACTIONS, GOVERNED_ROWS, NPM, assert_committed_governed_rows,
    assert_governed_rows, rendered_title_prefix,
};

fn parse_yaml(yaml: &str) -> Result<serde_yaml_ng::Value> {
    serde_yaml_ng::from_str(yaml).map_err(|err| anyhow!("parsing fixture YAML: {err}"))
}

fn check_yaml(yaml: &str) -> Result<()> {
    assert_governed_rows(&parse_yaml(yaml)?)
}

fn must_err_containing(yaml: &str, needle: &str) -> Result<()> {
    match check_yaml(yaml) {
        Ok(()) => {
            bail!("expected the GitHub Actions/npm commit-message contract to reject this document")
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

/// One governed row's `commit-message` block. `include: None` omits the key.
fn commit_block(prefix: &str, include: Option<&str>) -> String {
    match include {
        Some(include) => format!(
            "    commit-message:\n      prefix: \"{prefix}\"\n      include: \"{include}\"\n"
        ),
        None => format!("    commit-message:\n      prefix: \"{prefix}\"\n"),
    }
}

fn row_yaml(ecosystem: &str, directory: &str, prefix: &str, include: Option<&str>) -> String {
    format!(
        "  - package-ecosystem: {ecosystem}\n    directory: {directory}\n{}",
        commit_block(prefix, include)
    )
}

fn fixture(
    gha_prefix: &str,
    gha_include: Option<&str>,
    npm_prefix: &str,
    npm_include: Option<&str>,
) -> String {
    format!(
        "updates:\n{}{}",
        row_yaml(GITHUB_ACTIONS.ecosystem, GITHUB_ACTIONS.directory, gha_prefix, gha_include),
        row_yaml(NPM.ecosystem, NPM.directory, npm_prefix, npm_include),
    )
}

/// The committed GitHub Actions and npm rows are the combination GitHub renders once.
#[test]
fn committed_gha_and_npm_rows_use_chore_plus_scope() -> Result<()> {
    assert_committed_governed_rows()
}

/// Restoring `prefix: "chore(deps)"` on GitHub Actions while `include: "scope"`
/// remains must fail. This is the pre-repair combination observed on #12212
/// and #12210.
#[test]
fn github_actions_chore_deps_plus_scope_is_rejected() -> Result<()> {
    must_err_containing(
        &fixture("chore(deps)", Some("scope"), "chore", Some("scope")),
        "chore(deps)(deps)",
    )?;
    must_err_containing(
        &fixture("chore(deps)", Some("scope"), "chore", Some("scope")),
        "github-actions `/`",
    )
}

/// Restoring `prefix: "chore(deps)"` on npm while `include: "scope"` remains
/// must fail. This is the pre-repair combination observed on #12207 / #12206
/// and still live on #14849.
#[test]
fn npm_chore_deps_plus_scope_is_rejected() -> Result<()> {
    must_err_containing(
        &fixture("chore", Some("scope"), "chore(deps)", Some("scope")),
        "chore(deps)(deps)",
    )?;
    must_err_containing(
        &fixture("chore", Some("scope"), "chore(deps)", Some("scope")),
        "npm `/vscode-extension`",
    )
}

/// Repairing only one of the two rows is not this claim. Either remaining
/// doubled prefix must keep the suite red.
#[test]
fn partial_repair_of_one_ecosystem_is_rejected() -> Result<()> {
    must_err_containing(
        &fixture("chore", Some("scope"), "chore(deps)", Some("scope")),
        "npm `/vscode-extension`",
    )?;
    must_err_containing(
        &fixture("chore(deps)", Some("scope"), "chore", Some("scope")),
        "github-actions `/`",
    )
}

/// Dropping `include: "scope"` instead of narrowing the prefix is the other
/// realistic wrong repair: titles become `chore: ...` and lose deps /
/// deps-dev discrimination. npm still emits `deps-dev` live (#14849).
#[test]
fn omitting_include_scope_is_rejected_on_each_row() -> Result<()> {
    must_err_containing(&fixture("chore", None, "chore", Some("scope")), "include")?;
    must_err_containing(&fixture("chore", Some("scope"), "chore", None), "include")
}

/// GitHub documents only `include: "scope"`. A different string must not
/// silently skip the composition contract.
#[test]
fn include_other_than_scope_is_rejected() -> Result<()> {
    must_err_containing(
        &fixture("chore", Some("not-scope"), "chore", Some("scope")),
        "found `not-scope`",
    )?;
    must_err_containing(
        &fixture("chore", Some("scope"), "chore", Some("not-scope")),
        "found `not-scope`",
    )
}

/// Keeping `prefix: "chore(deps)"` and deleting `include` also yields a
/// single-looking `chore(deps): ...` title, but by the wrong mechanism, and
/// it still cannot emit `deps-dev`.
#[test]
fn chore_deps_without_include_is_rejected() -> Result<()> {
    must_err_containing(&fixture("chore(deps)", None, "chore", Some("scope")), "include")?;
    must_err_containing(&fixture("chore", Some("scope"), "chore(deps)", None), "include")
}

/// A second github-actions `/` row with the old prefix must not hide behind
/// `.find()`.
#[test]
fn duplicate_github_actions_rows_fail_closed() -> Result<()> {
    must_err_containing(
        &format!(
            "updates:\n{}{}{}",
            row_yaml("github-actions", "/", "chore", Some("scope")),
            row_yaml("github-actions", "/", "chore(deps)", Some("scope")),
            row_yaml("npm", "/vscode-extension", "chore", Some("scope")),
        ),
        "2 github-actions `/` update entries",
    )
}

/// A second npm `/vscode-extension` row with the old prefix must not hide
/// behind `.find()`.
#[test]
fn duplicate_npm_rows_fail_closed() -> Result<()> {
    must_err_containing(
        &format!(
            "updates:\n{}{}{}",
            row_yaml("github-actions", "/", "chore", Some("scope")),
            row_yaml("npm", "/vscode-extension", "chore", Some("scope")),
            row_yaml("npm", "/vscode-extension", "chore(deps)", Some("scope")),
        ),
        "2 npm `/vscode-extension` update entries",
    )
}

/// Missing either governed row is a defect, not a vacuously empty pass.
#[test]
fn missing_governed_row_fails_closed() -> Result<()> {
    must_err_containing(
        &format!("updates:\n{}", row_yaml("npm", "/vscode-extension", "chore", Some("scope")),),
        "must keep a github-actions `/` update entry",
    )?;
    must_err_containing(
        &format!("updates:\n{}", row_yaml("github-actions", "/", "chore", Some("scope")),),
        "must keep a npm `/vscode-extension` update entry",
    )
}

/// A github-actions row at a directory other than `/` does not satisfy the
/// governed row. Counting any github-actions entry would let the live `/`
/// defect hide behind an extra row.
#[test]
fn github_actions_at_another_directory_does_not_count() -> Result<()> {
    must_err_containing(
        &format!(
            "updates:\n{}{}",
            row_yaml("github-actions", "/.github", "chore", Some("scope")),
            row_yaml("npm", "/vscode-extension", "chore", Some("scope")),
        ),
        "must keep a github-actions `/` update entry",
    )
}

/// An npm row at a directory other than `/vscode-extension` does not satisfy
/// the governed row.
#[test]
fn npm_at_another_directory_does_not_count() -> Result<()> {
    must_err_containing(
        &format!(
            "updates:\n{}{}",
            row_yaml("github-actions", "/", "chore", Some("scope")),
            row_yaml("npm", "/", "chore", Some("scope")),
        ),
        "must keep a npm `/vscode-extension` update entry",
    )
}

/// Doubled Cargo scope is out of this claim. Scanning every row would turn
/// this suite red on a Cargo-only regression that #14898 owns, or keep it
/// green by accident if Cargo were later reverted first.
#[test]
fn cargo_doubled_scope_does_not_fail_the_gha_npm_contract() -> Result<()> {
    check_yaml(&format!(
        "updates:\n{}{}{}",
        row_yaml("cargo", "/", "chore(deps)", Some("scope")),
        row_yaml("github-actions", "/", "chore", Some("scope")),
        row_yaml("npm", "/vscode-extension", "chore", Some("scope")),
    ))
}

/// Unreadable config is a defect, not an empty pass.
#[test]
fn missing_updates_sequence_fails_closed() -> Result<()> {
    must_err_containing("{}", "updates")?;
    must_err_containing("updates: {}\n", "updates")
}

/// The composition oracle itself must still produce the doubled form when
/// fed the old prefix. A "helpful" renderer that stripped an existing
/// `(deps)` would make the negative controls assert against the wrong string.
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
             #12212 / #12210 failure mode"
        );
    }
    if rendered_title_prefix("chore(deps)", "scope", "deps-dev") != "chore(deps)(deps-dev)" {
        bail!(
            "prefix `chore(deps)` plus include `scope` must still render \
             `chore(deps)(deps-dev)` for npm development updates (#12207 / #14849)"
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

/// Both governed identities are actually scanned. A checker that only loaded
/// github-actions would leave npm doubled and still pass the committed-file
/// test after a one-row YAML edit.
#[test]
fn both_governed_row_identities_are_distinct() -> Result<()> {
    if GOVERNED_ROWS.len() != 2 {
        bail!(
            "this claim governs exactly two rows (github-actions `/` and npm `/vscode-extension`)"
        );
    }
    if GITHUB_ACTIONS.ecosystem == NPM.ecosystem && GITHUB_ACTIONS.directory == NPM.directory {
        bail!("github-actions and npm row selectors must stay distinct");
    }
    if GITHUB_ACTIONS.directory != "/" {
        bail!("github-actions is governed at directory `/`");
    }
    if NPM.directory != "/vscode-extension" {
        bail!("npm is governed at directory `/vscode-extension`");
    }
    Ok(())
}

const REQUIRED_STEP: &str = "      - name: Dependabot GitHub Actions and npm commit-message contract (required merge surface)\n        run: cargo test -p xtask --test dependabot_gha_npm_commit_message --locked -- --nocapture";

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
///
/// The step is appended after the changelog fragment gate so it does not
/// occupy the insertion site #14898 uses after the Cargo dependency-name
/// contract.
#[test]
fn required_merge_surface_executes_this_contract() -> Result<()> {
    let path = dependabot_gha_npm::project_root().join(".github/workflows/ci.yml");
    let workflow = std::fs::read_to_string(&path)
        .map_err(|err| anyhow!("reading {}: {err}", path.display()))?
        .replace("\r\n", "\n");
    if !required_wiring_is_present(&workflow) {
        bail!(
            "{DEPENDABOT_YML} edits do not reach the advisory routed lane, so this \
             contract must stay as a required `check-all-targets` step in \
             `.github/workflows/ci.yml` (#14180)"
        );
    }
    let commented = workflow.replace(
        REQUIRED_STEP,
        "      # cargo test -p xtask --test dependabot_gha_npm_commit_message --locked -- --nocapture",
    );
    if required_wiring_is_present(&commented) {
        bail!(
            "a comment containing the cargo test command must not satisfy the \
             wiring contract (#14180)"
        );
    }
    Ok(())
}
