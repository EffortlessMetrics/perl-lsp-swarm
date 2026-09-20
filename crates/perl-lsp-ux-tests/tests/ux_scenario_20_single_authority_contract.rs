//! Recurrence guard for Scenario 20's single-authority evidence model.
//!
//! The semantic role of each retained case is reviewed in Scenario 20 and will
//! move into `ux_case_policy.v1` under #10020. This narrow source contract only
//! prevents the exact status-only twins removed by #10012 from returning before
//! that compiled policy exists.

const TARGET: &str = include_str!("ux_scenario_20_real_workspace_providers.rs");

const RETIRED_TESTS: &[&str] = &[
    "scenario_20_completion_module_prefix_surfaces_real_baseline_app",
    "scenario_20_completion_imported_symbol_helper_in_app_pm",
    "scenario_20_goto_definition_parent_class_resolves_to_base_pm",
    "scenario_20_goto_definition_inherited_method_shared_resolves_to_base_pm",
    "scenario_20_goto_definition_imported_helper_resolves_to_util_pm",
    "scenario_20_goto_definition_static_method_call_new_resolves_to_app_pm",
    "scenario_20_hover_sub_shared_in_base_pm_does_not_error",
    "scenario_20_hover_module_import_in_app_pm_does_not_crash",
    "scenario_20_hover_inherited_method_call_in_app_pm",
    "scenario_20_hover_result_has_valid_contents_shape",
    "scenario_20_diagnostics_known_modules_do_not_fire_pl701",
    "scenario_20_diagnostics_missing_module_fires_pl701",
    "scenario_20_diagnostics_typeglob_alias_no_false_positive",
    "scenario_20_diagnostics_notification_received_for_all_files",
    "scenario_20_goto_definition_inherited_shared_to_base_pm_hard_assert",
    "scenario_20_references_app_module_cross_file",
    "scenario_20_references_util_module_cross_file",
];

const RETAINED_TESTS: &[&str] = &[
    "scenario_20_fixture_exists_on_disk",
    "scenario_20_completion_items_valid_shape_in_base_pm",
    "scenario_20_completion_module_prefix_surfaces_real_baseline_app_hard_assert",
    "scenario_20_completion_imported_symbol_helper_hard_assert",
    "scenario_20_goto_definition_parent_class_resolves_to_base_pm_hard_assert",
    "scenario_20_goto_definition_inherited_method_shared_base_pm_hard_assert",
    "scenario_20_goto_definition_imported_helper_to_util_pm_hard_assert",
    "scenario_20_goto_definition_static_new_to_app_pm_hard_assert",
    "scenario_20_goto_definition_typeglob_alias_dynamic_boundary",
    "scenario_20_hover_sub_shared_in_base_pm_hard_assert",
    "scenario_20_hover_module_import_in_app_pm_hard_assert",
    "scenario_20_hover_inherited_method_call_hard_assert",
    "scenario_20_hover_sub_helper_valid_shape_hard_assert",
    "scenario_20_diagnostics_no_false_pl701_hard_assert",
    "scenario_20_diagnostics_missing_module_fires_pl701_hard_assert",
    "scenario_20_diagnostics_typeglob_alias_no_false_positive_hard_assert",
    "scenario_20_diagnostics_notification_received_for_all_files_hard_assert",
];

fn function_signature(name: &str) -> String {
    format!("fn {name}(")
}

/// Names recorded in the target's `CASE_DISPOSITIONS` table.
fn disposition_names() -> Vec<&'static str> {
    let Some(start) = TARGET.find("const CASE_DISPOSITIONS") else {
        panic!("Scenario 20 disposition table disappeared from the target");
    };
    let rest = TARGET.get(start..).unwrap_or("");
    let Some(length) = rest.find("\n];") else {
        panic!("Scenario 20 disposition table is not terminated");
    };
    rest.get(..length)
        .unwrap_or("")
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|name| name.starts_with("scenario_20_"))
        .collect()
}

#[test]
fn scenario_20_disposition_table_matches_retained_authorities() {
    let mut recorded = disposition_names();
    recorded.sort_unstable();
    let mut retained = RETAINED_TESTS.to_vec();
    retained.sort_unstable();
    assert_eq!(
        recorded, retained,
        "CASE_DISPOSITIONS and RETAINED_TESTS disagree about the reviewed Scenario 20 cells"
    );
}

#[test]
fn scenario_20_has_one_current_authority_per_reviewed_cell() {
    let forbidden_soft_phrase = ["known", "gap"].join(" ");
    assert!(
        !TARGET.contains(&forbidden_soft_phrase),
        "Scenario 20 must not convert a missing or wrong result into a status-only pass"
    );
    assert!(
        !TARGET.contains("status:"),
        "Scenario 20 status logging must not act as a second executable verdict"
    );

    for retired in RETIRED_TESTS {
        assert!(
            !TARGET.contains(&function_signature(retired)),
            "retired Scenario 20 authority returned: {retired}"
        );
    }

    for retained in RETAINED_TESTS {
        let signature = function_signature(retained);
        let Some(position) = TARGET.find(&signature) else {
            panic!("reviewed Scenario 20 authority disappeared: {retained}");
        };
        assert!(
            TARGET[..position].trim_end().ends_with("#[test]"),
            "reviewed Scenario 20 authority is no longer annotated with #[test]: {retained}"
        );
    }
}
