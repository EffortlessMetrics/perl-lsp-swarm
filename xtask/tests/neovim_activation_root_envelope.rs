//! Contract proof for the actual-host Neovim activation/root envelope (#10502).
//!
//! The valid fixture is the verbatim output of one real
//! `scripts/ux/neovim_activation_root_smoke.sh` run against Neovim 0.11.3 and a
//! release `perllsp`, so these tests validate what the harness actually emits
//! rather than a hand-written idealization of it.
//!
//! Each falsifier below is one of the ways #10502 says this envelope must not
//! be allowed to look green.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::path::PathBuf;
use xtask::neovim_activation_root_envelope::{
    EnvelopeValidationError, ISOLATION_ROOT_CELLS, REQUIRED_FILE_FAMILIES, REQUIRED_ROOT_CELLS,
    validate_envelope,
};

fn valid_envelope() -> Result<Value, serde_json::Error> {
    serde_json::from_str(include_str!(
        "fixtures/neovim_activation_root_envelopes/valid-linux-nvim-0-11-3.json"
    ))
}

fn failed_run_envelope() -> Result<Value, serde_json::Error> {
    serde_json::from_str(include_str!(
        "fixtures/neovim_activation_root_envelopes/invalid-server-never-attached.json"
    ))
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask always has a parent directory")
        .to_path_buf()
}

fn sha256_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Freshness, not just well-formedness.
///
/// The envelope records the canonical config's digest but nothing inside the
/// envelope can tell whether that digest still describes the file on disk. Both
/// captured runs must therefore pin the *current* `perllsp.lua`, so editing the
/// canonical filetypes or root markers turns this red and forces a fresh
/// capture instead of leaving stale evidence green.
#[test]
fn both_captured_runs_pin_the_current_canonical_config() -> Result<(), Box<dyn Error>> {
    let relative = "scripts/ux/neovim/perllsp.lua";
    let config = std::fs::read(repository_root().join(relative))?;
    let digest = sha256_of(&config);

    for (label, envelope) in [("valid", valid_envelope()?), ("failed-run", failed_run_envelope()?)]
    {
        assert_eq!(
            envelope["config"]["path"],
            json!(relative),
            "the {label} fixture should name the canonical config"
        );
        assert_eq!(
            envelope["config"]["sha256"],
            json!(digest),
            "the {label} fixture was captured against a different {relative}; re-run \
             scripts/ux/neovim_activation_root_smoke.sh and recapture it"
        );
    }
    Ok(())
}

fn rejection(result: Result<(), EnvelopeValidationError>) -> Result<String, Box<dyn Error>> {
    match result {
        Ok(()) => Err("envelope unexpectedly validated".into()),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn captured_actual_host_envelope_is_accepted() -> Result<(), Box<dyn Error>> {
    validate_envelope(&valid_envelope()?)?;
    Ok(())
}

/// The producer and the judge have to agree about failure, not just success.
///
/// This fixture is the verbatim envelope from a real run against a server that
/// exits immediately: the harness still emitted the complete denominator (10
/// root cells, 17 families) and exited non-zero, and the validator must reject
/// what it produced rather than accept a run in which nothing was established.
#[test]
fn a_real_failed_run_is_rejected_even_though_it_is_well_formed() -> Result<(), Box<dyn Error>> {
    let envelope = failed_run_envelope()?;

    // It is a complete, structurally valid envelope: the denominator survived.
    for family in REQUIRED_FILE_FAMILIES {
        assert!(
            envelope["file_families"].get(*family).is_some(),
            "the failed run should still carry family `{family}`"
        );
    }
    for cell in REQUIRED_ROOT_CELLS {
        assert!(
            envelope["roots"].get(*cell).is_some(),
            "the failed run should still carry root cell `{cell}`"
        );
    }

    // ...and it is still rejected. The first required row that the run failed
    // to establish is the one reported.
    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.file_families.shebang.bin_tool.disposition: required family `shebang.bin_tool` \
         is `not_proven`; the run did not establish it"
    );

    // Every root cell degraded too, so the root rule would reject it just as
    // surely once the families are set aside.
    let mut roots_only = envelope.clone();
    roots_only["file_families"] = valid_envelope()?["file_families"].clone();
    assert_eq!(
        rejection(validate_envelope(&roots_only))?,
        "envelope.roots.conflict.competing_markers_at_depth.semantic.outcome: required root cell \
         `conflict.competing_markers_at_depth` is `not_proven`; it must reach `proven` on its own \
         evidence"
    );
    Ok(())
}

#[test]
fn every_required_file_family_is_present_in_the_captured_run() -> Result<(), Box<dyn Error>> {
    let envelope = valid_envelope()?;
    for family in REQUIRED_FILE_FAMILIES {
        assert!(
            envelope["file_families"].get(*family).is_some(),
            "captured run is missing required file family `{family}`"
        );
    }
    Ok(())
}

#[test]
fn every_required_root_cell_is_present_in_the_captured_run() -> Result<(), Box<dyn Error>> {
    let envelope = valid_envelope()?;
    for cell in REQUIRED_ROOT_CELLS {
        assert!(
            envelope["roots"].get(*cell).is_some(),
            "captured run is missing required root cell `{cell}`"
        );
    }
    Ok(())
}

#[test]
fn a_dropped_file_family_cannot_shrink_the_denominator() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["file_families"]
        .as_object_mut()
        .ok_or("file_families is not an object")?
        .remove("template.mason");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.file_families: required family `template.mason` is missing"
    );
    Ok(())
}

#[test]
fn a_dropped_root_cell_cannot_shrink_the_matrix() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["roots"]
        .as_object_mut()
        .ok_or("roots is not an object")?
        .remove("fallback.git_file_linked_worktree");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.roots: required root cell `fallback.git_file_linked_worktree` is missing"
    );
    Ok(())
}

#[test]
fn root_dir_equality_alone_cannot_prove_a_root() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    // The root still matches and the structural target is still right; only the
    // content-bearing marker is gone.
    envelope["roots"]["isolation.sibling_same_relative_path"]["semantic"]["expected_marker"] =
        json!("");
    envelope["roots"]["isolation.sibling_same_relative_path"]["semantic"]["observed_marker"] =
        json!("");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.roots.isolation.sibling_same_relative_path.semantic: outcome=proven requires a \
         root-specific expected_marker; `root_dir` equality is not a semantic result"
    );
    Ok(())
}

#[test]
fn an_isolation_cell_must_name_the_facts_it_rejected() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    for cell in ISOLATION_ROOT_CELLS {
        let mut candidate = envelope.clone();
        candidate["roots"][*cell]["semantic"]["rejected_symbols"] = json!([]);
        assert_eq!(
            rejection(validate_envelope(&candidate))?,
            format!(
                "envelope.roots.{cell}.semantic.rejected_symbols: `{cell}` claims root isolation \
                 and must name the competing facts it rejected"
            )
        );
    }
    // Guard against the fixture silently losing its isolation cells entirely.
    envelope["roots"]["isolation.sibling_same_relative_path"]["semantic"]["rejected_symbols"] =
        json!([]);
    assert!(validate_envelope(&envelope).is_err());
    Ok(())
}

#[test]
fn a_wrong_root_fact_cannot_be_the_observed_result() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    let cell = &mut envelope["roots"]["conflict.nearest_perl_marker_beats_farther"]["semantic"];
    cell["observed_marker"] = json!("outerperl");
    cell["expected_marker"] = json!("outerperl");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.roots.conflict.nearest_perl_marker_beats_farther.semantic: `probe_outerperl` \
         cannot be both the observed result and a rejected wrong-root fact"
    );
    Ok(())
}

#[test]
fn a_target_outside_the_selected_root_cannot_pass() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    let cell = &mut envelope["roots"]["conflict.perl_marker_beats_git"]["semantic"];
    cell["expected_target_role"] = json!("fixture:roots/perl-beats-git/lib/RootProbe.pm");
    cell["observed_target_role"] = json!("fixture:roots/perl-beats-git/lib/RootProbe.pm");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.roots.conflict.perl_marker_beats_git.semantic: resolved target \
         `fixture:roots/perl-beats-git/lib/RootProbe.pm` lies outside the selected root \
         `fixture:roots/perl-beats-git/app`"
    );
    Ok(())
}

#[test]
fn root_match_cannot_disagree_with_the_roles_it_summarizes() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["roots"]["marker.build_pl"]["actual_role"] = json!("fixture:roots/somewhere-else");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.roots.marker.build_pl.root_match: `true` contradicts expected \
         `fixture:roots/marker-build` against actual `fixture:roots/somewhere-else`"
    );
    Ok(())
}

/// The observation cell exists to withhold a root claim, so every way of
/// turning it into one has to fail.
#[test]
fn the_observation_cell_cannot_become_a_single_file_root_claim() -> Result<(), Box<dyn Error>> {
    let cell = "boundary.no_marker_single_file";

    let mut claims_match = valid_envelope()?;
    claims_match["roots"][cell]["root_match"] = json!(true);
    assert_eq!(
        rejection(validate_envelope(&claims_match))?,
        format!(
            "envelope.roots.{cell}.root_match: \
             the observation cell asserts no root and cannot claim a match"
        )
    );

    // Making both roles equal would satisfy the generic invariant and smuggle
    // the claim back in through `root_match=true`.
    let mut equal_roles = valid_envelope()?;
    equal_roles["roots"][cell]["actual_role"] = json!("observation_only");
    equal_roles["roots"][cell]["root_match"] = json!(true);
    assert!(validate_envelope(&equal_roles).is_err());

    let mut borrowed_marker = valid_envelope()?;
    borrowed_marker["roots"][cell]["marker"] = json!("cpanfile");
    assert_eq!(
        rejection(validate_envelope(&borrowed_marker))?,
        format!("envelope.roots.{cell}.marker: the observation cell must record `none`")
    );
    Ok(())
}

#[test]
fn a_non_proven_cell_must_say_why() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["roots"]["marker.build_pl"]["semantic"]["outcome"] = json!("not_proven");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.roots.marker.build_pl.semantic.reason: missing required field"
    );
    Ok(())
}

/// Adding a marker to the canonical config without giving it a winning
/// actual-host case must fail: the envelope would otherwise claim a marker it
/// never exercised.
#[test]
fn a_configured_marker_that_never_won_a_cell_is_rejected() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["config"]["root_marker_groups"] = json!([
        [".perl-lsp.toml", "Makefile.PL", "Build.PL", "cpanfile", "dist.ini", "META.json"],
        ".git"
    ]);

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.roots: configured root marker `META.json` never won a proven cell"
    );
    Ok(())
}

/// Several cells legitimately share one marker (three cells use `cpanfile`,
/// two use `.git`), so marker coverage alone would let a degraded conflict or
/// isolation cell hide behind a sibling cell naming the same marker.
#[test]
fn a_shared_marker_cannot_cover_for_a_degraded_isolation_cell() -> Result<(), Box<dyn Error>> {
    for cell in ["conflict.perl_marker_beats_git", "fallback.git_file_linked_worktree"] {
        let mut envelope = valid_envelope()?;
        let semantic = &mut envelope["roots"][cell]["semantic"];
        semantic["outcome"] = json!("not_applicable");
        semantic["reason"] = json!("degraded by this test");

        // Marker coverage still holds: another cell names the same marker.
        assert_eq!(
            rejection(validate_envelope(&envelope))?,
            format!(
                "envelope.roots.{cell}.semantic.outcome: required root cell `{cell}` is \
                 `not_applicable`; it must reach `proven` on its own evidence"
            )
        );
    }
    Ok(())
}

#[test]
fn the_observation_only_boundary_cell_is_the_sole_exemption() -> Result<(), Box<dyn Error>> {
    // It is already `not_applicable` in the captured run, and that must stay legal.
    let envelope = valid_envelope()?;
    assert_eq!(
        envelope["roots"]["boundary.no_marker_single_file"]["semantic"]["outcome"],
        json!("not_applicable")
    );
    validate_envelope(&envelope)?;
    Ok(())
}

#[test]
fn a_required_family_that_failed_to_establish_itself_is_rejected() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    let family = &mut envelope["file_families"]["source.psgi"];
    family["disposition"] = json!("not_proven");
    family["reason"] = json!("degraded by this test");
    family["attached"] = json!(false);
    family["language_id"] = json!("");
    family["config_eligible"] = json!(false);
    family["native_filetype"] = json!("");
    family["opened_filetype"] = json!("");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.file_families.source.psgi.disposition: required family `source.psgi` is \
         `not_proven`; the run did not establish it"
    );
    Ok(())
}

#[test]
fn the_harness_cannot_test_a_marker_the_canonical_config_does_not_declare()
-> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["roots"]["marker.build_pl"]["marker"] = json!("META.json");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.roots.marker.build_pl.marker: `META.json` is not one of the configured root \
         markers"
    );
    Ok(())
}

#[test]
fn an_applied_override_cannot_be_recorded_as_native_support() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["file_families"]["source.pl"]["override_applied"] = json!(true);

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.file_families.source.pl: an applied override cannot be recorded as native support"
    );
    Ok(())
}

#[test]
fn a_template_family_cannot_be_promoted_to_native_perl_support() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["file_families"]["template.tt"]["disposition"] = json!("native_perl_and_attached");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.file_families.template.tt: disposition `native_perl_and_attached` requires \
         native filetype `perl`, found ``"
    );
    Ok(())
}

#[test]
fn broadening_the_canonical_filetypes_breaks_the_adjacent_rows() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    // Adding `mason` to the canonical config to make a matrix row look better
    // immediately contradicts the recorded eligibility of the mason families.
    envelope["config"]["filetypes"] = json!(["perl", "mason"]);

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.file_families.template.ep.config_eligible: `false` contradicts opened filetype \
         `mason` against the recorded activating filetypes"
    );
    Ok(())
}

/// Widening the config so an adjacent family becomes activating contradicts a
/// disposition that says the config leaves that family alone — whether or not
/// the row happened to attach.
#[test]
fn an_adjacent_family_cannot_stay_adjacent_once_the_config_activates_it()
-> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["config"]["filetypes"] = json!(["perl", "mason"]);
    for family in ["template.ep", "template.mason"] {
        envelope["file_families"][family]["config_eligible"] = json!(true);
    }

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.file_families.template.ep: disposition `intentionally_adjacent_or_mixed` says \
         the canonical config does not activate this family, but config_eligible=true"
    );
    Ok(())
}

/// `unsupported` is a statement about the host, not about the configuration,
/// so a family whose filetype the config does match may still be unsupported.
#[test]
fn unsupported_stays_orthogonal_to_configured_eligibility() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["config"]["filetypes"] = json!(["perl", "mason"]);
    for family in ["template.ep", "template.mason"] {
        let row = &mut envelope["file_families"][family];
        row["config_eligible"] = json!(true);
        row["disposition"] = json!("unsupported");
        row["reason"] = json!("this host cannot establish the family");
    }

    validate_envelope(&envelope)?;
    Ok(())
}

#[test]
fn attaching_without_eligibility_is_a_contradiction() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["file_families"]["template.mason"]["attached"] = json!(true);

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.file_families.template.mason: attached=true contradicts config_eligible=false"
    );
    Ok(())
}

#[test]
fn command_registration_cannot_stand_in_for_a_sent_language_id() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["file_families"]["source.pm"]["language_id"] = json!("");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.file_families.source.pm.language_id: an attached buffer must record the \
         language id it sent"
    );
    Ok(())
}

#[test]
fn a_private_absolute_path_cannot_enter_a_durable_envelope() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["roots"]["marker.perl_lsp_toml"]["actual_role"] = json!("/home/someone/fixture/roots");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.roots.marker.perl_lsp_toml.actual_role: `/home/someone/fixture/roots` is an \
         absolute path; roles must be normalized identities"
    );
    Ok(())
}

#[test]
fn a_windows_absolute_path_cannot_enter_a_durable_envelope() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["roots"]["marker.dist_ini"]["actual_role"] = json!(r"C:\build\fixture\roots");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        concat!(
            r"envelope.roots.marker.dist_ini.actual_role: `C:\build\fixture\roots` ",
            "is an absolute path; roles must be normalized identities"
        )
    );
    Ok(())
}

#[test]
fn the_config_path_must_stay_repository_relative() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["config"]["path"] = json!(r"C:\checkout\scripts\ux\neovim\perllsp.lua");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        concat!(
            r"envelope.config.path: `C:\checkout\scripts\ux\neovim\perllsp.lua` ",
            "must be repository-relative"
        )
    );
    Ok(())
}

/// A role legitimately contains a colon (`fixture:roots/...`), so the
/// drive-letter guard must not swallow the ordinary case.
#[test]
fn a_namespaced_role_is_not_mistaken_for_a_drive_letter() -> Result<(), Box<dyn Error>> {
    validate_envelope(&valid_envelope()?)?;
    let envelope = valid_envelope()?;
    assert_eq!(
        envelope["roots"]["marker.dist_ini"]["actual_role"],
        json!("fixture:roots/marker-dist"),
        "the captured run should still use namespaced fixture roles"
    );
    Ok(())
}

#[test]
fn an_unknown_field_cannot_be_smuggled_into_a_row() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["roots"]["fallback.git_only"]["passed"] = json!(true);

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.roots.fallback.git_only: unknown field `passed`"
    );
    Ok(())
}

#[test]
fn a_foreign_schema_version_is_rejected() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["schema_version"] = json!("neovim_activation_root_envelope.v2");

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.schema_version: expected `neovim_activation_root_envelope.v1`, found \
         `neovim_activation_root_envelope.v2`"
    );
    Ok(())
}

#[test]
fn the_envelope_must_keep_its_limitations_and_claim_boundary() -> Result<(), Box<dyn Error>> {
    let mut envelope = valid_envelope()?;
    envelope["limitations"] = json!([]);

    assert_eq!(
        rejection(validate_envelope(&envelope))?,
        "envelope.limitations: at least one limitation is required"
    );
    Ok(())
}
