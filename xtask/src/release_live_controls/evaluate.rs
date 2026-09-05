//! Pure laws over an already-collected `release_live_controls.v1` snapshot
//! (#9403).
//!
//! Nothing here reads the network, runs a command, or mutates anything.
//! Every function is a total function of the typed model in
//! [`super::model`], so each law is falsifiable from a fixture alone.

use std::collections::{BTreeMap, BTreeSet};

use super::model::{
    ClassicProtection, Environment, IdentityMatch, Instrument, ObservationState, Observed,
    RepositoryControls, RepositoryIdentity, RepositorySubject, RequiredContextsUnion, Ruleset,
    UnionContext, Verdict,
};

/// Merge classic branch protection's required contexts with every branch
/// ruleset's `required_status_checks` contexts.
///
/// LAW: classic protection and rulesets are enforced *additively* by
/// GitHub — a required check that either half names is actually required.
/// The union is therefore [`ObservationState::Observed`] only when **both**
/// halves are conclusive (observed, or a corroborated absence); if either
/// half — or a contributing ruleset's `rules` — is
/// [`ObservationState::NotProven`], the union is `NOT_PROVEN` and `detail`
/// names exactly which half was missing. The missing half is never inferred
/// from the other.
pub fn required_contexts_union(
    classic: &Observed<ClassicProtection>,
    branch_rulesets: &Observed<Vec<Ruleset>>,
) -> RequiredContextsUnion {
    let mut missing: Vec<String> = Vec::new();
    let mut contributed: Vec<(String, String)> = Vec::new();

    match classic.state {
        ObservationState::Absent => {}
        ObservationState::Observed => match classic.value() {
            Some(protection) => {
                collect_classic_contexts(protection, &mut contributed, &mut missing)
            }
            None => missing.push("classic_branch_protection".to_string()),
        },
        ObservationState::NotProven => missing.push("classic_branch_protection".to_string()),
    }

    match branch_rulesets.state {
        ObservationState::Absent => {}
        ObservationState::Observed => match branch_rulesets.value() {
            Some(rulesets) => {
                for ruleset in rulesets {
                    collect_ruleset_contexts(ruleset, &mut contributed, &mut missing);
                }
            }
            None => missing.push("branch_rulesets".to_string()),
        },
        ObservationState::NotProven => missing.push("branch_rulesets".to_string()),
    }

    if !missing.is_empty() {
        missing.sort();
        missing.dedup();
        return RequiredContextsUnion {
            state: ObservationState::NotProven,
            detail: Some(format!(
                "required-contexts union is not proven: {} not conclusively observed",
                missing.join(", ")
            )),
            contexts: Vec::new(),
        };
    }

    let mut merged: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (name, source) in contributed {
        merged.entry(name).or_default().insert(source);
    }
    let contexts = merged
        .into_iter()
        .map(|(name, sources)| UnionContext { name, sources: sources.into_iter().collect() })
        .collect();

    RequiredContextsUnion { state: ObservationState::Observed, detail: None, contexts }
}

fn collect_classic_contexts(
    protection: &ClassicProtection,
    contributed: &mut Vec<(String, String)>,
    missing: &mut Vec<String>,
) {
    match protection.required_status_checks.state {
        ObservationState::Absent => {}
        ObservationState::Observed => match protection.required_status_checks.value() {
            Some(checks) => {
                for row in &checks.contexts {
                    contributed.push((row.context.clone(), "branch_protection".to_string()));
                }
            }
            None => missing.push("classic_branch_protection.required_status_checks".to_string()),
        },
        ObservationState::NotProven => {
            missing.push("classic_branch_protection.required_status_checks".to_string());
        }
    }
}

fn collect_ruleset_contexts(
    ruleset: &Ruleset,
    contributed: &mut Vec<(String, String)>,
    missing: &mut Vec<String>,
) {
    // A ruleset GitHub is not enforcing (`evaluate`/`disabled`) or whose
    // ref conditions do not select this branch must not inflate the union.
    // Whether it selects the branch must itself be conclusive: an unreadable
    // condition set is a gap, not an exclusion.
    if ruleset.enforcement != "active" {
        return;
    }
    match ruleset.applies_to_branch.value() {
        Some(true) => {}
        Some(false) => return,
        None => {
            missing.push(format!("branch_rulesets[{}].applies_to_branch", ruleset.id));
            return;
        }
    }
    match ruleset.rules.state {
        ObservationState::Absent => {}
        ObservationState::Observed => match ruleset.rules.value() {
            Some(rules) => {
                for rule in rules {
                    if rule.rule_type == "required_status_checks" {
                        for context in &rule.required_contexts {
                            contributed.push((context.clone(), format!("ruleset:{}", ruleset.id)));
                        }
                    }
                }
            }
            None => missing.push(format!("branch_rulesets[{}].rules", ruleset.id)),
        },
        ObservationState::NotProven => {
            missing.push(format!("branch_rulesets[{}].rules", ruleset.id));
        }
    }
}

/// Whether the observed identity is the repository that was requested.
///
/// `Matched` only when the observed `full_name` equals `owner/name`
/// case-insensitively **and** `database_id != 0` **and** `node_id` is
/// non-empty. A payload naming a different repository is `Mismatched`. An
/// unobserved identity is `NotProven` — never guessed to be either.
pub fn identity_match(
    requested: &RepositorySubject,
    identity: &Observed<RepositoryIdentity>,
) -> IdentityMatch {
    if !identity.is_observed() {
        return IdentityMatch::NotProven {
            detail: identity
                .detail
                .clone()
                .unwrap_or_else(|| "repository identity was not observed".to_string()),
        };
    }
    let Some(observed) = identity.value() else {
        return IdentityMatch::NotProven {
            detail: "identity observation carries no value".to_string(),
        };
    };

    let expected = requested.render();
    if observed.database_id == 0 || observed.node_id.trim().is_empty() {
        return IdentityMatch::Mismatched {
            detail: format!("identity for {expected} is missing a database_id or node_id"),
        };
    }
    if observed.full_name.eq_ignore_ascii_case(&expected) {
        IdentityMatch::Matched
    } else {
        IdentityMatch::Mismatched {
            detail: format!("requested {expected} but the API returned {}", observed.full_name),
        }
    }
}

/// Whether `observed` and, when present, every field nested inside its
/// [`ClassicProtection`] value are conclusive.
///
/// A wrapper that is itself `OBSERVED` still does not make the *plane*
/// conclusive if one of its own sub-fields (e.g. `enforce_admins`) could not
/// be read — that sub-field's ambiguity must not be laundered away by the
/// outer wrapper's success.
fn classic_protection_conclusive(observed: &Observed<ClassicProtection>) -> bool {
    if !observed.is_conclusive() {
        return false;
    }
    match observed.value() {
        None => true,
        Some(protection) => {
            protection.required_status_checks.is_conclusive()
                && protection.enforce_admins.is_conclusive()
                && protection.required_pull_request_reviews.is_conclusive()
                && protection.required_conversation_resolution.is_conclusive()
                && protection.restrictions_present.is_conclusive()
        }
    }
}

fn ruleset_row_conclusive(ruleset: &Ruleset) -> bool {
    ruleset.applies_to_branch.is_conclusive()
        && ruleset.bypass_actors.is_conclusive()
        && ruleset.rules.is_conclusive()
}

/// Whether a ruleset list observation, and every ruleset row inside it, is
/// conclusive.
///
/// A ruleset whose `bypass_actors` the API omitted is exactly the "we cannot
/// see who bypasses this" gap #9403 exists to surface: the list itself
/// having been read successfully does not paper over it.
fn ruleset_list_conclusive(observed: &Observed<Vec<Ruleset>>) -> bool {
    if !observed.is_conclusive() {
        return false;
    }
    match observed.value() {
        None => true,
        Some(rulesets) => rulesets.iter().all(ruleset_row_conclusive),
    }
}

fn environment_row_conclusive(environment: &Environment) -> bool {
    environment.protection_rules.is_conclusive()
        && environment.deployment_branch_policy.is_conclusive()
        && environment.secret_count.is_conclusive()
}

fn environment_list_conclusive(observed: &Observed<Vec<Environment>>) -> bool {
    if !observed.is_conclusive() {
        return false;
    }
    match observed.value() {
        None => true,
        Some(environments) => environments.iter().all(environment_row_conclusive),
    }
}

/// The overall verdict: `OBSERVED` only when every repository's every plane
/// was conclusively established.
///
/// Requires, for **every** repository: `identity` observed, `identity_match`
/// `Matched`, every plane (`classic_branch_protection`, `branch_rulesets`,
/// `tag_rulesets`, `environments`, `release_posture`) conclusive — including
/// every row nested inside a plane, not merely the plane's own wrapper — and
/// `required_contexts_union.state` `Observed`. An empty repository list is
/// also `NOT_PROVEN`: nothing was actually established.
pub fn verdict(repositories: &[RepositoryControls]) -> Verdict {
    if repositories.is_empty() {
        return Verdict::NotProven;
    }
    let every_plane_conclusive = repositories.iter().all(|repository| {
        repository.identity.is_observed()
            && matches!(repository.identity_match, IdentityMatch::Matched)
            && classic_protection_conclusive(&repository.classic_branch_protection)
            && ruleset_list_conclusive(&repository.branch_rulesets)
            && ruleset_list_conclusive(&repository.tag_rulesets)
            && environment_list_conclusive(&repository.environments)
            && repository.release_posture.immutable_releases.is_conclusive()
            && repository.release_posture.tag_rulesets_present.is_conclusive()
            && repository.required_contexts_union.state == ObservationState::Observed
    });
    if every_plane_conclusive { Verdict::Observed } else { Verdict::NotProven }
}

/// The receipt-level verdict: [`verdict`] over the repositories, gated on
/// the instrument itself having been observed.
///
/// A receipt whose `gh` could not be established is `NOT_PROVEN` even if
/// every plane happened to read cleanly: an unobserved instrument cannot
/// vouch for what it read.
pub fn receipt_verdict(instrument: &Instrument, repositories: &[RepositoryControls]) -> Verdict {
    if instrument.state != ObservationState::Observed {
        return Verdict::NotProven;
    }
    verdict(repositories)
}

/// One deterministic, sorted line per non-conclusive plane, naming the
/// repository and the plane; plus one line for an unobserved instrument.
pub fn receipt_limitations(
    instrument: &Instrument,
    repositories: &[RepositoryControls],
) -> Vec<String> {
    let mut lines = limitations(repositories);
    if instrument.state != ObservationState::Observed {
        lines.push("instrument: gh is not observed".to_string());
        lines.sort();
    }
    lines
}

/// One deterministic, sorted line per non-conclusive plane, naming the
/// repository and the plane.
pub fn limitations(repositories: &[RepositoryControls]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for repository in repositories {
        let label = repository.requested.render();
        if !repository.identity.is_observed() {
            lines.push(format!("{label}: identity is not observed"));
        }
        if !matches!(repository.identity_match, IdentityMatch::Matched) {
            lines.push(format!("{label}: identity_match is not Matched"));
        }
        if !classic_protection_conclusive(&repository.classic_branch_protection) {
            lines.push(format!("{label}: classic_branch_protection is not conclusive"));
        }
        if !ruleset_list_conclusive(&repository.branch_rulesets) {
            lines.push(format!("{label}: branch_rulesets is not conclusive"));
        }
        if !ruleset_list_conclusive(&repository.tag_rulesets) {
            lines.push(format!("{label}: tag_rulesets is not conclusive"));
        }
        if !environment_list_conclusive(&repository.environments) {
            lines.push(format!("{label}: environments is not conclusive"));
        }
        if !repository.release_posture.immutable_releases.is_conclusive()
            || !repository.release_posture.tag_rulesets_present.is_conclusive()
        {
            lines.push(format!("{label}: release_posture is not conclusive"));
        }
        if repository.required_contexts_union.state != ObservationState::Observed {
            lines.push(format!("{label}: required_contexts_union is not observed"));
        }
    }
    lines.sort();
    lines
}
