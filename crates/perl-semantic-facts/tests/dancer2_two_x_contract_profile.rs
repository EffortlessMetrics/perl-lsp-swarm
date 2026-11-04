//! Bounded Dancer2 1.1.1 / 2.0.0 / 2.1.0 semantic-profile remainder (#13616).
//!
//! #14408 landed L1+L2 (activation/import and the pinned 2.x core-DSL
//! registry). This suite commits the research artifact that issue still
//! lacked on `main`: a machine-readable per-cell classification, the
//! terminal 1.x 1.1.2 identity, explicit blocked/dynamic boundaries, and
//! the L3–L5 / support-policy remainder graph.
//!
//! Expected rows are derived from the pinned upstream sources named in
//! [`PINNED_IDENTITIES`], not from perl-lsp output. Live adapters are the
//! negative controls: a wrong classification fails against current facts.

#![deny(clippy::map_err_ignore)]

use perl_semantic_facts::framework::version_constraint_matches;
use perl_semantic_facts::framework_adapters::dancer2::{
    DANCER2_DSL_KEYWORDS, DANCER2_VERSION_CONSTRAINT, DslKeywordScope, dancer2_descriptor,
};
use perl_semantic_facts::framework_adapters::dancer2_hooks::normalize_dancer2_hook_name;
use perl_semantic_facts::framework_adapters::dancer2_two_x::{
    DANCER2_TWO_X_DSL_KEYWORDS, DANCER2_TWO_X_IMPORT_DIE_MESSAGE, DANCER2_TWO_X_KEYWORD_GLOBAL,
    DANCER2_TWO_X_KEYWORD_ROUTE_ONLY, DANCER2_TWO_X_KEYWORD_TOTAL,
    DANCER2_TWO_X_PINNED_COMMIT_2_0_0, DANCER2_TWO_X_PINNED_COMMIT_2_0_1,
    DANCER2_TWO_X_PINNED_COMMIT_2_1_0, DANCER2_TWO_X_VERSION_CONSTRAINT, dancer2_two_x_descriptor,
};
use perl_semantic_facts::hook::HookNameNormalization;
use perl_test_must::{must, must_some_with};

/// Pinned upstream identities for the research matrix (#13616).
const PINNED_IDENTITIES: &[(&str, &str)] = &[
    ("1.1.1", "36bdd7aa4e9b97585ad545180a2135fe26278ad1"),
    ("1.1.2", "126be702b5628a8ab91e8ec6883f7c975c8898f1"),
    ("2.0.0", "09f316678b8dd237d4c4ea0242e70e32591f5d64"),
    ("2.0.1", "674837ce095db3bffb5acfccd50fdac57771d50b"),
    ("2.1.0", "09e9376288ddc3571f226883eb99753fecce818d"),
];

/// One classified semantic cell. `same` means the fact surface is unchanged
/// across the named pins; `changed` means a 2.x delta exists; `blocked`
/// means the cell is recorded and must not become an exact fact; `dynamic`
/// means runtime/unmodeled.
struct ProfileCell {
    family: &'static str,
    cell: &'static str,
    pin_1_1_1: &'static str,
    pin_2_0_0: &'static str,
    pin_2_1_0: &'static str,
    classification: &'static str,
    authority_path: &'static str,
    authority_symbol: &'static str,
    leaf: &'static str,
    status: &'static str,
    negative_control: &'static str,
}

/// Independently provable implementation leaves and blocked follow-ups.
struct ImplementationLeaf {
    id: &'static str,
    claim: &'static str,
    status: &'static str,
    landed_in: &'static str,
    depends_on: &'static str,
    rollback: &'static str,
}

const CONTRACT_PROFILE: &[ProfileCell] = &[
    ProfileCell {
        family: "activation",
        cell: "use_dancer2_import",
        pin_1_1_1: "sub import as reviewed",
        pin_2_0_0: "byte-identical to 1.1.1",
        pin_2_1_0: "byte-identical to 1.1.1",
        classification: "same",
        authority_path: "lib/Dancer2.pm",
        authority_symbol: "sub import",
        leaf: "L1",
        status: "landed",
        negative_control: "odd-args die; empty import list; name-only identity",
    },
    ProfileCell {
        family: "activation",
        cell: "version_gate",
        pin_1_1_1: ">=1.0.0,<2.0.0",
        pin_2_0_0: ">=2.0.0,<2.2.0",
        pin_2_1_0: ">=2.0.0,<2.2.0",
        classification: "changed",
        authority_path: "lib/Dancer2.pm",
        authority_symbol: "$VERSION",
        leaf: "L1",
        status: "landed",
        negative_control: "1.x identity fails 2.x constraint; 2.2.0 unreviewed; no newest-profile fallback",
    },
    ProfileCell {
        family: "keywords",
        cell: "dsl_keywords_map",
        pin_1_1_1: "82 keywords; 43 global / 39 route-handler-only",
        pin_2_0_0: "byte-identical map",
        pin_2_1_0: "byte-identical map (Path::Tiny dirname/path does not reach facts)",
        classification: "same",
        authority_path: "lib/Dancer2/Core/DSL.pm",
        authority_symbol: "dsl_keywords",
        leaf: "L2",
        status: "landed",
        negative_control: "unchanged spelling is not whole-contract compatibility",
    },
    ProfileCell {
        family: "keywords",
        cell: "cookie_redirect_scope",
        pin_1_1_1: "is_global => 0",
        pin_2_0_0: "is_global => 0",
        pin_2_1_0: "is_global => 0",
        classification: "same",
        authority_path: "lib/Dancer2/Core/DSL.pm",
        authority_symbol: "cookie/redirect",
        leaf: "L2",
        status: "landed",
        negative_control: "cookie/redirect must not be classified Global",
    },
    ProfileCell {
        family: "routes",
        cell: "declaration_grammar",
        pin_1_1_1: "2/3/4-arity; GET→HEAD; del→DELETE; bare-any default set",
        pin_2_0_0: "same grammar",
        pin_2_1_0: "same grammar",
        classification: "same",
        authority_path: "lib/Dancer2/Core/DSL.pm",
        authority_symbol: "any/get/post/put/del/options/patch",
        leaf: "L3",
        status: "partial",
        negative_control: "2.x route facts stay comparison-only (RouteFactsContract::TwoX)",
    },
    ProfileCell {
        family: "hooks",
        cell: "core_alias_table",
        pin_1_1_1: "reviewed 1.x aliases",
        pin_2_0_0: "1.x table plus on_hook_exception",
        pin_2_1_0: "1.x table plus on_hook_exception",
        classification: "changed",
        authority_path: "lib/Dancer2/Core/App.pm",
        authority_symbol: "hook_aliases",
        leaf: "L4",
        status: "remaining",
        negative_control: "1.x hook contract must not treat on_hook_exception as canonical",
    },
    ProfileCell {
        family: "hooks",
        cell: "on_hook_exception",
        pin_1_1_1: "absent",
        pin_2_0_0: "core.app.hook_exception; non-reentrant",
        pin_2_1_0: "core.app.hook_exception; non-reentrant",
        classification: "changed",
        authority_path: "lib/Dancer2/Core/App.pm",
        authority_symbol: "on_hook_exception",
        leaf: "L4",
        status: "remaining",
        negative_control: "accepting on_hook_exception under the 1.x constraint",
    },
    ProfileCell {
        family: "config",
        cell: "strict_config_and_bootstrap",
        pin_1_1_1: "no ConfigReader",
        pin_2_0_0: "Dancer2::ConfigReader; strict_config default 0",
        pin_2_1_0: "same 2.x config surface",
        classification: "blocked",
        authority_path: "lib/Dancer2/ConfigReader.pm",
        authority_symbol: "strict_config",
        leaf: "blocked",
        status: "blocked",
        negative_control: "1.x fixture reused as 2.x config proof; exact strict_config diagnostics",
    },
    ProfileCell {
        family: "templates",
        cell: "engine_defaults",
        pin_1_1_1: "TemplateToolkit + Tiny + Simple/ForkedTiny",
        pin_2_0_0: "Simple and ForkedTiny removed",
        pin_2_1_0: "TemplateToolkit + Tiny only",
        classification: "blocked",
        authority_path: "lib/Dancer2/Template",
        authority_symbol: "Dancer2::Template::Simple",
        leaf: "blocked",
        status: "blocked",
        negative_control: "removed engines assumed available",
    },
    ProfileCell {
        family: "plugins",
        cell: "app_specific_dsl_ownership",
        pin_1_1_1: "shared plugin DSL",
        pin_2_0_0: "caller-aware dsl() (pre-2.1 isolation incomplete)",
        pin_2_1_0: "app-specific plugin keyword ownership",
        classification: "blocked",
        authority_path: "lib/Dancer2/Plugin.pm",
        authority_symbol: "dsl()",
        leaf: "blocked",
        status: "blocked",
        negative_control: "plugin keywords minted as core DSL facts; leak across apps/roots",
    },
    ProfileCell {
        family: "callable",
        cell: "runtime_deprecation_croaks",
        pin_1_1_1: "header/headers/push_header/context bodies",
        pin_2_0_0: "same croak replacements",
        pin_2_1_0: "same croak replacements",
        classification: "same",
        authority_path: "lib/Dancer2/Core/DSL.pm",
        authority_symbol: "DEPRECATED:",
        leaf: "L2",
        status: "landed",
        negative_control: "dropping deprecated exports because the registry has no deprecated flag",
    },
    ProfileCell {
        family: "lifecycle",
        cell: "generation_and_missing_version",
        pin_1_1_1: "fail-closed on missing/unresolved identity",
        pin_2_0_0: "fail-closed; no newest-profile fallback",
        pin_2_1_0: "fail-closed; 2.0 rules must not apply as 2.1-only facts",
        classification: "same",
        authority_path: "lib/Dancer2.pm",
        authority_symbol: "$VERSION",
        leaf: "L1",
        status: "landed",
        negative_control: "missing source/version evidence falling back to the newest profile",
    },
    ProfileCell {
        family: "identity",
        cell: "multi_app_scoping",
        pin_1_1_1: "appname default caller; per-app routes/hooks",
        pin_2_0_0: "same",
        pin_2_1_0: "same; custom dsl_class remains dynamic",
        classification: "same",
        authority_path: "lib/Dancer2.pm",
        authority_symbol: "appname",
        leaf: "L5",
        status: "partial",
        negative_control: "second package sharing first package exclusions; dynamic dsl_class minting default keywords",
    },
];

const IMPLEMENTATION_LEAVES: &[ImplementationLeaf] = &[
    ImplementationLeaf {
        id: "L1",
        claim: "2.x activation/import facts",
        status: "landed",
        landed_in: "60268c2327 (#14408)",
        depends_on: "#8914 1.x activation shape",
        rollback: "revert the 2.x adapter; 1.x constraint stays <2.0.0",
    },
    ImplementationLeaf {
        id: "L2",
        claim: "pinned 82-keyword 2.x core-DSL registry",
        status: "landed",
        landed_in: "60268c2327 (#14408)",
        depends_on: "L1",
        rollback: "revert DANCER2_TWO_X_DSL_KEYWORDS; 1.x table is independently owned",
    },
    ImplementationLeaf {
        id: "L3",
        claim: "2.x route-declaration facts under the #12334 grammar",
        status: "partial",
        landed_in: "ae795a276 (#15006) comparison-only RouteFactsContract::TwoX",
        depends_on: "L1+L2; #14989 remaining Shadow→Production / rendering",
        rollback: "2.x route facts remain Shadow; drop TwoX contract marker",
    },
    ImplementationLeaf {
        id: "L4",
        claim: "2.x hook registry with version-gated on_hook_exception",
        status: "remaining",
        landed_in: "none",
        depends_on: "L1",
        rollback: "keep the 1.x hook contract; do not alias on_hook_exception",
    },
    ImplementationLeaf {
        id: "L5",
        claim: "multi-app / multi-root identity",
        status: "partial",
        landed_in: "60268c2327 package-scoped activations",
        depends_on: "L1; multi-root isolation still NOT_PROVEN",
        rollback: "package scoping stays activation-local; no shared root table",
    },
    ImplementationLeaf {
        id: "blocked-config-templates-plugins",
        claim: "2.x config/template/plugin fact classes",
        status: "blocked",
        landed_in: "none — recorded boundaries only",
        depends_on: "own leaves after source-backed authority",
        rollback: "keep cells blocked; never widen from keyword-table similarity",
    },
    ImplementationLeaf {
        id: "support-policy-2x-row",
        claim: "independent 2.x support-policy row for #8955",
        status: "remaining",
        landed_in: "none — 1.x framework-adapters.toml row stays <2.0.0",
        depends_on: "policy/framework-adapters.toml writer (busy in other open PRs)",
        rollback: "do not promote a broad Dancer2 2.x support claim",
    },
];

fn one_x_adapter_source() -> String {
    must(std::fs::read_to_string(format!(
        "{}/src/framework_adapters/dancer2.rs",
        env!("CARGO_MANIFEST_DIR")
    )))
}

fn two_x_skeleton_sources() -> (String, String) {
    let root = format!(
        "{}/../../test_corpus/real_projects/dancer2_2x_skeleton",
        env!("CARGO_MANIFEST_DIR")
    );
    let module = must(std::fs::read_to_string(format!("{root}/lib/Dancer2.pm")));
    let app = must(std::fs::read_to_string(format!("{root}/t/basic.t")));
    (module, app)
}

fn cell(family: &str, name: &str) -> &'static ProfileCell {
    must_some_with(
        CONTRACT_PROFILE.iter().find(|row| row.family == family && row.cell == name),
        format!("missing profile cell {family}.{name}"),
    )
}

fn leaf(id: &str) -> &'static ImplementationLeaf {
    must_some_with(
        IMPLEMENTATION_LEAVES.iter().find(|row| row.id == id),
        format!("missing implementation leaf {id}"),
    )
}

#[test]
fn pinned_identities_cover_1_1_1_through_2_1_0_and_terminal_1_1_2() {
    let names: Vec<_> = PINNED_IDENTITIES.iter().map(|(release, _)| *release).collect();
    assert_eq!(names, ["1.1.1", "1.1.2", "2.0.0", "2.0.1", "2.1.0"]);
    assert_eq!(PINNED_IDENTITIES[2].1, DANCER2_TWO_X_PINNED_COMMIT_2_0_0);
    assert_eq!(PINNED_IDENTITIES[3].1, DANCER2_TWO_X_PINNED_COMMIT_2_0_1);
    assert_eq!(PINNED_IDENTITIES[4].1, DANCER2_TWO_X_PINNED_COMMIT_2_1_0);
    for (_, sha) in PINNED_IDENTITIES {
        assert_eq!(sha.len(), 40, "pinned identity must be a full SHA-1");
        assert!(sha.chars().all(|ch| ch.is_ascii_hexdigit()));
    }
}

#[test]
fn one_x_adapter_records_the_terminal_1_1_2_identity() {
    let source = one_x_adapter_source();
    assert!(
        source.contains("36bdd7aa4e9b97585ad545180a2135fe26278ad1"),
        "the 1.x profile must keep the semantically anchored 1.1.1 identity"
    );
    assert!(
        source.contains("126be702b5628a8ab91e8ec6883f7c975c8898f1"),
        "the 1.x profile must record Dancer2 1.1.2 rather than describing 1.1.1 as the final 1.x release"
    );
    assert!(
        !source.contains("A Dancer v2.0+ release has not been reviewed"),
        "the 1.x adapter must not imply 2.x is unreviewed; 2.x is a separate bounded profile"
    );
}

#[test]
fn comparison_covers_every_research_matrix_family() {
    let families: Vec<_> = CONTRACT_PROFILE.iter().map(|row| row.family).collect();
    for required in [
        "activation",
        "keywords",
        "routes",
        "hooks",
        "config",
        "templates",
        "plugins",
        "callable",
        "lifecycle",
        "identity",
    ] {
        assert!(families.contains(&required), "profile missing family {required}");
    }
}

#[test]
fn same_versus_changed_is_classified_per_cell() {
    assert_eq!(cell("activation", "use_dancer2_import").classification, "same");
    assert_eq!(cell("keywords", "dsl_keywords_map").classification, "same");
    assert_eq!(cell("hooks", "on_hook_exception").classification, "changed");
    assert_eq!(cell("config", "strict_config_and_bootstrap").classification, "blocked");
    assert_eq!(cell("templates", "engine_defaults").classification, "blocked");
    assert_eq!(cell("plugins", "app_specific_dsl_ownership").classification, "blocked");
    assert_ne!(
        cell("keywords", "dsl_keywords_map").classification,
        cell("hooks", "on_hook_exception").classification,
        "unchanged keyword spelling must not be treated as whole-contract compatibility"
    );
}

#[test]
fn one_x_constraint_is_not_widened_past_2_0_0() {
    assert_eq!(DANCER2_VERSION_CONSTRAINT, ">=1.0.0,<2.0.0");
    assert_eq!(DANCER2_TWO_X_VERSION_CONSTRAINT, ">=2.0.0,<2.2.0");
    assert_eq!(version_constraint_matches(DANCER2_VERSION_CONSTRAINT, "1.1.2"), Some(true));
    assert_eq!(version_constraint_matches(DANCER2_VERSION_CONSTRAINT, "2.0.0"), Some(false));
    assert_eq!(version_constraint_matches(DANCER2_TWO_X_VERSION_CONSTRAINT, "1.1.2"), Some(false));
    assert_eq!(version_constraint_matches(DANCER2_TWO_X_VERSION_CONSTRAINT, "2.0.0"), Some(true));
    assert_eq!(version_constraint_matches(DANCER2_TWO_X_VERSION_CONSTRAINT, "2.1.0"), Some(true));
    assert_eq!(version_constraint_matches(DANCER2_TWO_X_VERSION_CONSTRAINT, "2.2.0"), Some(false));
    assert_ne!(dancer2_descriptor().adapter_id, dancer2_two_x_descriptor().adapter_id);
}

#[test]
fn cookie_and_redirect_are_route_handler_only_on_both_contracts() {
    let one_x_cookie = DANCER2_DSL_KEYWORDS.iter().find(|keyword| keyword.name == "cookie");
    let two_x_cookie = DANCER2_TWO_X_DSL_KEYWORDS.iter().find(|keyword| keyword.name == "cookie");
    assert!(
        matches!(one_x_cookie, Some(keyword) if keyword.scope == DslKeywordScope::RouteHandlerOnly)
    );
    assert!(
        matches!(two_x_cookie, Some(keyword) if keyword.scope == DslKeywordScope::RouteHandlerOnly)
    );
    let one_x_redirect = DANCER2_DSL_KEYWORDS.iter().find(|keyword| keyword.name == "redirect");
    let two_x_redirect =
        DANCER2_TWO_X_DSL_KEYWORDS.iter().find(|keyword| keyword.name == "redirect");
    assert!(
        matches!(one_x_redirect, Some(keyword) if keyword.scope == DslKeywordScope::RouteHandlerOnly)
    );
    assert!(
        matches!(two_x_redirect, Some(keyword) if keyword.scope == DslKeywordScope::RouteHandlerOnly)
    );
    for absent in ["route", "before", "after", "body", "register", "plugin_setting"] {
        assert!(
            !DANCER2_TWO_X_DSL_KEYWORDS.iter().any(|keyword| keyword.name == absent),
            "`{absent}` must not mint as a 2.x core DSL keyword"
        );
        assert!(
            !DANCER2_DSL_KEYWORDS.iter().any(|keyword| keyword.name == absent),
            "`{absent}` must not mint as a 1.x core DSL keyword"
        );
    }
}

#[test]
fn on_hook_exception_is_not_a_reviewed_1_x_hook() {
    let normalized = normalize_dancer2_hook_name("on_hook_exception");
    assert!(
        matches!(normalized, HookNameNormalization::Unresolved { .. }),
        "on_hook_exception must stay unresolved on the 1.x contract, got {normalized:?}"
    );
    if let HookNameNormalization::Unresolved { reason } = normalized {
        assert!(
            reason.contains("dancer2-hooks.1-1.v1"),
            "1.x hook contract must refuse the 2.x-only alias, got {reason}"
        );
    }
}

#[test]
fn blocked_config_template_plugin_cells_have_no_exact_adapter_claim() {
    let two_x_source = must(std::fs::read_to_string(format!(
        "{}/src/framework_adapters/dancer2_two_x.rs",
        env!("CARGO_MANIFEST_DIR")
    )));
    assert!(
        two_x_source.contains("blocked cells"),
        "the 2.x adapter must keep config/template/plugin cells blocked in its claim boundary"
    );
    assert!(
        !two_x_source.contains("strict_config"),
        "strict_config must not become an exact 2.x fact"
    );
    assert!(
        !two_x_source.contains("Template::Simple"),
        "removed template engines must not be assumed available"
    );
    assert_eq!(cell("config", "strict_config_and_bootstrap").status, "blocked");
    assert_eq!(cell("templates", "engine_defaults").status, "blocked");
    assert_eq!(cell("plugins", "app_specific_dsl_ownership").status, "blocked");
}

#[test]
fn two_x_skeleton_is_not_config_template_or_plugin_proof() {
    let (module, app) = two_x_skeleton_sources();
    for source in [&module, &app] {
        assert!(
            source.contains("must never be cited as proof")
                && source.contains("2.x config")
                && source.contains("plugin behavior"),
            "the 2.x skeleton must refuse config/template/plugin citation"
        );
    }
    let one_x_skeleton = format!(
        "{}/../../test_corpus/real_projects/dancer2_skeleton/lib/Dancer2.pm",
        env!("CARGO_MANIFEST_DIR")
    );
    let one_x_module = must(std::fs::read_to_string(one_x_skeleton));
    assert!(
        one_x_module.contains("1.1.1") || one_x_module.contains("$VERSION"),
        "1.x skeleton remains a separate fixture"
    );
    assert_ne!(module, one_x_module, "a 1.x fixture must not be reused as 2.x proof");
}

#[test]
fn implementation_graph_splits_leaves_with_rollback() {
    assert_eq!(leaf("L1").status, "landed");
    assert_eq!(leaf("L2").status, "landed");
    assert_eq!(leaf("L3").status, "partial");
    assert_eq!(leaf("L4").status, "remaining");
    assert_eq!(leaf("L5").status, "partial");
    assert_eq!(leaf("blocked-config-templates-plugins").status, "blocked");
    assert_eq!(leaf("support-policy-2x-row").status, "remaining");
    for row in IMPLEMENTATION_LEAVES {
        assert!(!row.rollback.is_empty(), "{} needs rollback meaning", row.id);
        assert!(!row.depends_on.is_empty(), "{} needs a dependency", row.id);
        assert!(!row.claim.is_empty(), "{} needs a claim", row.id);
        assert!(!row.landed_in.is_empty(), "{} needs a landed-in record", row.id);
    }
}

#[test]
fn every_supported_cell_names_authority_and_a_negative_control() {
    for row in CONTRACT_PROFILE {
        assert!(
            !row.authority_path.is_empty(),
            "{}.{} missing authority path",
            row.family,
            row.cell
        );
        assert!(
            !row.authority_symbol.is_empty(),
            "{}.{} missing authority symbol",
            row.family,
            row.cell
        );
        assert!(
            !row.negative_control.is_empty(),
            "{}.{} missing negative control",
            row.family,
            row.cell
        );
        assert!(
            !row.pin_1_1_1.is_empty(),
            "{}.{} missing 1.1.1 classification",
            row.family,
            row.cell
        );
        assert!(
            !row.pin_2_0_0.is_empty(),
            "{}.{} missing 2.0.0 classification",
            row.family,
            row.cell
        );
        assert!(!row.leaf.is_empty(), "{}.{} missing leaf id", row.family, row.cell);
        if row.status == "landed" {
            assert!(
                matches!(row.classification, "same" | "changed"),
                "landed cell {}.{} must be a classified fact, not a blocked guess",
                row.family,
                row.cell
            );
        }
    }
}

#[test]
fn adapters_stay_shadow_and_do_not_promote_broad_2_x_support() {
    use perl_semantic_facts::framework::AdapterDisposition;
    assert_eq!(dancer2_descriptor().disposition, AdapterDisposition::Shadow);
    assert_eq!(dancer2_two_x_descriptor().disposition, AdapterDisposition::Shadow);
    assert_eq!(cell("activation", "version_gate").pin_2_1_0, ">=2.0.0,<2.2.0");
}

#[test]
fn every_landed_profile_row_anchors_on_live_adapter_surfaces() {
    const LANDED_CELLS: &[(&str, &str)] = &[
        ("activation", "use_dancer2_import"),
        ("activation", "version_gate"),
        ("keywords", "dsl_keywords_map"),
        ("keywords", "cookie_redirect_scope"),
        ("callable", "runtime_deprecation_croaks"),
        ("lifecycle", "generation_and_missing_version"),
    ];
    let landed_in_profile = CONTRACT_PROFILE.iter().filter(|row| row.status == "landed").count();
    assert_eq!(
        LANDED_CELLS.len(),
        landed_in_profile,
        "update LANDED_CELLS when landed profile rows change"
    );

    let two_x_source = must(std::fs::read_to_string(format!(
        "{}/src/framework_adapters/dancer2_two_x.rs",
        env!("CARGO_MANIFEST_DIR")
    )));

    let global_from_registry = DANCER2_TWO_X_DSL_KEYWORDS
        .iter()
        .filter(|keyword| keyword.scope == DslKeywordScope::Global)
        .count();
    let route_only_from_registry = DANCER2_TWO_X_DSL_KEYWORDS
        .iter()
        .filter(|keyword| keyword.scope == DslKeywordScope::RouteHandlerOnly)
        .count();
    let deprecated_names: Vec<_> = DANCER2_TWO_X_DSL_KEYWORDS
        .iter()
        .filter(|keyword| keyword.is_deprecated())
        .map(|keyword| keyword.name)
        .collect();

    for &(family, name) in LANDED_CELLS {
        let row = cell(family, name);
        assert_eq!(row.status, "landed", "{}.{} must stay landed", family, name);
        match (family, name) {
            ("activation", "use_dancer2_import") => {
                assert_eq!(row.authority_path, "lib/Dancer2.pm");
                assert_eq!(row.authority_symbol, "sub import");
                assert!(
                    two_x_source.contains(DANCER2_TWO_X_IMPORT_DIE_MESSAGE),
                    "import die must stay wired to the live 2.x adapter"
                );
            }
            ("activation", "version_gate") => {
                assert_eq!(row.pin_1_1_1, DANCER2_VERSION_CONSTRAINT);
                assert_eq!(row.pin_2_0_0, DANCER2_TWO_X_VERSION_CONSTRAINT);
                assert_eq!(row.pin_2_1_0, DANCER2_TWO_X_VERSION_CONSTRAINT);
            }
            ("keywords", "dsl_keywords_map") => {
                assert_eq!(DANCER2_DSL_KEYWORDS.len(), DANCER2_TWO_X_DSL_KEYWORDS.len());
                assert_eq!(DANCER2_TWO_X_DSL_KEYWORDS.len(), DANCER2_TWO_X_KEYWORD_TOTAL);
                assert_eq!(DANCER2_TWO_X_KEYWORD_TOTAL, 82);
                assert_eq!(global_from_registry, DANCER2_TWO_X_KEYWORD_GLOBAL);
                assert_eq!(DANCER2_TWO_X_KEYWORD_GLOBAL, 43);
                assert_eq!(route_only_from_registry, DANCER2_TWO_X_KEYWORD_ROUTE_ONLY);
                assert_eq!(DANCER2_TWO_X_KEYWORD_ROUTE_ONLY, 39);
                assert_eq!(
                    row.pin_1_1_1, "82 keywords; 43 global / 39 route-handler-only",
                    "profile narrative must stay aligned with live registry counts"
                );
            }
            ("keywords", "cookie_redirect_scope") => {
                for keyword_name in ["cookie", "redirect"] {
                    let one_x = must_some_with(
                        DANCER2_DSL_KEYWORDS.iter().find(|keyword| keyword.name == keyword_name),
                        format!("1.x registry missing `{keyword_name}`"),
                    );
                    let two_x = must_some_with(
                        DANCER2_TWO_X_DSL_KEYWORDS
                            .iter()
                            .find(|keyword| keyword.name == keyword_name),
                        format!("2.x registry missing `{keyword_name}`"),
                    );
                    assert_eq!(one_x.scope, DslKeywordScope::RouteHandlerOnly);
                    assert_eq!(two_x.scope, DslKeywordScope::RouteHandlerOnly);
                }
            }
            ("callable", "runtime_deprecation_croaks") => {
                assert_eq!(
                    deprecated_names,
                    ["context", "header", "headers", "push_header"],
                    "landed deprecation row must track the live registry croak replacements"
                );
            }
            ("lifecycle", "generation_and_missing_version") => {
                assert!(
                    two_x_source.contains("fail closed"),
                    "2.x detection must document fail-closed lifecycle without newest-profile fallback"
                );
                assert!(
                    !two_x_source.to_ascii_lowercase().contains("newest-profile"),
                    "profile must not admit a newest-profile fallback path"
                );
            }
            _ => unreachable!("LANDED_CELLS and match arms must stay in sync"),
        }
    }
}
