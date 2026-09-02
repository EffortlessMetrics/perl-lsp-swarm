use color_eyre::eyre::{Context, Result, bail};
use perl_lsp_rs_core::providers::inline_completion::{
    NextEditFeatureGate, NextEditProvider, NextEditRequest, NextEditResponse, NextEditStatus,
    PreparedInlineCompletionContext,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const ROUTES: &[SupportedEditorRouteRequirement] = &[
    SupportedEditorRouteRequirement {
        route: "stdio_cli_smoke",
        claim: "the generic stdio LSP smoke covers static, dynamic, and disabled inline-completion clients",
        proof_planes: &[proof_surface_plane(&[ProofSurfaceRequirement {
            path: "xtask/src/tasks/inline_completion_smoke.rs",
            markers: &[
                "fn run_static_client",
                "fn run_dynamic_client",
                "fn run_disabled_client",
                "textDocument/inlineCompletion",
                "perl-inlineCompletion",
                "assert_inline_completion_runtime",
            ],
        }])],
    },
    SupportedEditorRouteRequirement {
        route: "lsp4ij_upstream_integration",
        claim: "the JetBrains path documents the `perllsp --stdio` launch identity, executably proves the static/dynamic inline-completion protocol split, and keeps actual IntelliJ/LSP4IJ host support explicitly unproven here",
        proof_planes: &[
            ProofPlaneRequirement {
                plane: "launch_identity",
                owner: None,
                declared_status: None,
                proof_surfaces: &[ProofSurfaceRequirement {
                    path: "docs/EDITORS/INTELLIJ_IDEA_SETUP.md",
                    markers: &["Recommended: LSP4IJ Upstream Integration", "perllsp --stdio"],
                }],
            },
            ProofPlaneRequirement {
                plane: "static_protocol",
                owner: None,
                declared_status: None,
                proof_surfaces: &[ProofSurfaceRequirement {
                    path: "xtask/src/tasks/inline_completion_smoke.rs",
                    markers: &["fn run_static_client", "inlineCompletionProvider"],
                }],
            },
            ProofPlaneRequirement {
                plane: "dynamic_protocol",
                owner: None,
                declared_status: None,
                proof_surfaces: &[
                    ProofSurfaceRequirement {
                        path: "xtask/src/tasks/inline_completion_smoke.rs",
                        markers: &[
                            "fn run_dynamic_client",
                            "client/registerCapability",
                            "textDocument/inlineCompletion",
                        ],
                    },
                    ProofSurfaceRequirement {
                        path: "docs/EDITORS/INTELLIJ_IDEA_SETUP.md",
                        markers: &["client/registerCapability", "textDocument/inlineCompletion"],
                    },
                ],
            },
            ProofPlaneRequirement {
                plane: "host_support_boundary",
                owner: None,
                declared_status: None,
                proof_surfaces: &[ProofSurfaceRequirement {
                    path: "docs/EDITORS/INTELLIJ_IDEA_SETUP.md",
                    markers: &["not the same thing as IntelliJ/LSP4IJ host support", "#7719"],
                }],
            },
            ProofPlaneRequirement {
                plane: "actual_host_evidence",
                owner: Some("#7719/#7977/#8179"),
                declared_status: Some("not_proven"),
                proof_surfaces: &[],
            },
        ],
    },
    SupportedEditorRouteRequirement {
        route: "vscode_extension_path",
        claim: "the VS Code route uses the managed extension path with configurable perllsp binary resolution",
        proof_planes: &[proof_surface_plane(&[
            ProofSurfaceRequirement {
                path: "docs/EDITORS/VS_CODE_SETUP.md",
                markers: &[
                    "EffortlessMetrics.perl-lsp-rs",
                    "The extension auto-downloads the matching `perllsp` server by default.",
                    "\"perl-lsp.serverPath\"",
                    "\"perl-lsp.autoDownload\"",
                ],
            },
            ProofSurfaceRequirement {
                path: "vscode-extension/package.json",
                markers: &[
                    "\"perl-lsp.serverPath\"",
                    "\"perl-lsp.autoDownload\"",
                    "Absolute path to the `perllsp` binary. Leave empty to auto-download a release build.",
                    "Automatically download the `perllsp` binary if it is not found locally.",
                ],
            },
        ])],
    },
    SupportedEditorRouteRequirement {
        route: "release_built_binary_smoke",
        claim: "the release gate requires building perllsp and running the inline-completion stdio smoke against that binary",
        proof_planes: &[proof_surface_plane(&[
            ProofSurfaceRequirement {
                path: "docs/development/INLINE_COMPLETION_RELEASE_GATE.md",
                markers: &[
                    "./scripts/cargo-safe build -p perllsp --profile agent --locked",
                    "./scripts/cargo-safe xtask inline-completion-smoke --binary target/agent/perllsp",
                    "static clients receive top-level `inlineCompletionProvider`",
                    "dynamic clients omit the static provider",
                    "disabledFeatures: [\"lsp.inline_completion\"]",
                ],
            },
            ProofSurfaceRequirement {
                path: "xtask/src/tasks/inline_completion_smoke.rs",
                markers: &[
                    "pub fn run(binary: PathBuf) -> Result<()>",
                    "resolve_binary_path",
                    "run_static_client(&binary)?;",
                    "run_dynamic_client(&binary)?;",
                    "run_disabled_client(&binary)?;",
                ],
            },
        ])],
    },
];

#[derive(Debug, Clone, Copy)]
struct SupportedEditorRouteRequirement {
    route: &'static str,
    claim: &'static str,
    proof_planes: &'static [ProofPlaneRequirement],
}

/// One named proof plane of a route. Planes with proof surfaces are executable/
/// documented claims that must validate for the route to register. A plane with
/// `declared_status` reports an explicit non-registered disposition (for example
/// actual-host evidence owned elsewhere) without failing the bundle.
#[derive(Debug, Clone, Copy)]
struct ProofPlaneRequirement {
    plane: &'static str,
    owner: Option<&'static str>,
    declared_status: Option<&'static str>,
    proof_surfaces: &'static [ProofSurfaceRequirement],
}

const fn proof_surface_plane(
    proof_surfaces: &'static [ProofSurfaceRequirement],
) -> ProofPlaneRequirement {
    ProofPlaneRequirement {
        plane: "proof_surface",
        owner: None,
        declared_status: None,
        proof_surfaces,
    }
}

#[derive(Debug, Clone, Copy)]
struct ProofSurfaceRequirement {
    path: &'static str,
    markers: &'static [&'static str],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct SupportedEditorInlineSmokeReceipt {
    schema_version: &'static str,
    provider: &'static str,
    provider_action: &'static str,
    claim_boundary: &'static str,
    route_count: usize,
    all_supported_routes_registered: bool,
    supported_editor_routes: BTreeMap<&'static str, SupportedEditorRouteReceipt>,
    next_edit_boundary: NextEditSupportedEditorBoundaryReceipt,
    future_gated: BTreeMap<&'static str, &'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct SupportedEditorRouteReceipt {
    status: &'static str,
    claim: &'static str,
    proof_plane_count: usize,
    proof_planes: BTreeMap<&'static str, ProofPlaneReceipt>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct ProofPlaneReceipt {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<&'static str>,
    proof_surface_count: usize,
    proof_surfaces: Vec<ProofSurfaceReceipt>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct ProofSurfaceReceipt {
    path: &'static str,
    required_marker_count: usize,
    required_markers: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct NextEditSupportedEditorBoundaryReceipt {
    claim_boundary: &'static str,
    enabled_by_default: bool,
    explicit_dev_gate_enabled: bool,
    runtime_provider_registered: bool,
    editor_visible_suggestions: bool,
    ai_candidate_source_enabled: bool,
    default_response: NextEditResponse,
    explicit_gate_response: NextEditResponse,
}

pub fn run(receipt: PathBuf) -> Result<()> {
    let root = crate::utils::project_root()?;
    let receipt_data = summarize_supported_editor_routes(&root, ROUTES)?;

    write_receipt(&receipt, &receipt_data)?;
    println!(
        "supported editor inline smoke receipt OK: {} routes, {}",
        receipt_data.route_count,
        receipt.display()
    );
    Ok(())
}

fn summarize_supported_editor_routes(
    root: &Path,
    requirements: &[SupportedEditorRouteRequirement],
) -> Result<SupportedEditorInlineSmokeReceipt> {
    let mut supported_editor_routes = BTreeMap::new();
    for route in requirements {
        let mut proof_planes = BTreeMap::new();
        for plane in route.proof_planes {
            let status = match (plane.declared_status, plane.proof_surfaces.is_empty()) {
                (Some(status), true) => {
                    validate_declared_status(route.route, plane.plane, status)?;
                    status
                }
                (Some(_), false) | (None, true) => {
                    bail!(
                        "supported editor inline smoke route `{}` plane `{}` must either declare a status with no proof surfaces or carry proof surfaces with no declared status",
                        route.route,
                        plane.plane
                    );
                }
                (None, false) => "registered",
            };
            let mut proof_surfaces = Vec::new();
            for surface in plane.proof_surfaces {
                validate_proof_surface(root, route.route, plane.plane, surface)?;
                proof_surfaces.push(ProofSurfaceReceipt {
                    path: surface.path,
                    required_marker_count: surface.markers.len(),
                    required_markers: surface.markers.to_vec(),
                });
            }
            if proof_planes.contains_key(plane.plane) {
                bail!(
                    "supported editor inline smoke route `{}` declares duplicate proof plane `{}`",
                    route.route,
                    plane.plane
                );
            }
            proof_planes.insert(
                plane.plane,
                ProofPlaneReceipt {
                    status,
                    owner: plane.owner,
                    proof_surface_count: proof_surfaces.len(),
                    proof_surfaces,
                },
            );
        }
        if supported_editor_routes.contains_key(route.route) {
            bail!(
                "supported editor inline smoke requirements declare duplicate route `{}`",
                route.route
            );
        }
        supported_editor_routes.insert(
            route.route,
            SupportedEditorRouteReceipt {
                status: "registered",
                claim: route.claim,
                proof_plane_count: proof_planes.len(),
                proof_planes,
            },
        );
    }

    let future_gated = BTreeMap::from([
        ("live_vscode_ui_automation", "future_gated"),
        ("live_lsp4ij_ui_automation", "future_gated"),
        ("runtime_next_edit_provider", "future_gated"),
        ("editor_visible_next_edit_suggestions", "future_gated"),
        ("runtime_multiline_inline_completion", "future_gated"),
        ("optional_ai_candidate_source", "future_gated"),
    ]);
    let next_edit_boundary = summarize_next_edit_supported_editor_boundary()?;

    Ok(SupportedEditorInlineSmokeReceipt {
        schema_version: "supported-editor-inline-smoke.v1",
        provider: "inline_completion",
        provider_action: "supported_editor_inline_smoke_bundle",
        claim_boundary: "machine-readable supported-editor inline smoke bundle only; verifies repository proof surfaces, command contracts, and default-off next-edit boundary state, not live editor UI automation, source mirror, release, AI behavior, editor-visible next-edit suggestions, or runtime multiline behavior",
        route_count: requirements.len(),
        all_supported_routes_registered: true,
        supported_editor_routes,
        next_edit_boundary,
        future_gated,
    })
}

fn validate_declared_status(route: &str, plane: &str, status: &str) -> Result<()> {
    if matches!(status, "not_proven" | "limited" | "client_not_exposed") {
        return Ok(());
    }

    bail!(
        "supported editor inline smoke route `{route}` proof plane `{plane}` has unsupported declared status `{status}`"
    );
}

fn summarize_next_edit_supported_editor_boundary() -> Result<NextEditSupportedEditorBoundaryReceipt>
{
    let provider = NextEditProvider;
    let context = PreparedInlineCompletionContext {
        prefix: "use My::".to_string(),
        current_line: "use My::".to_string(),
        previous_non_empty_line: Some("use strict;".to_string()),
        current_function: None,
        current_package: Some("Demo".to_string()),
        variables: vec!["$got".to_string()],
        imports: vec!["strict".to_string(), "warnings".to_string()],
        ..PreparedInlineCompletionContext::default()
    };
    let mut request = NextEditRequest::receipt_only(context);

    request.gate = NextEditFeatureGate::default();
    let default_response = provider.suggest(&request);
    if default_response.status != NextEditStatus::Disabled
        || !default_response.suggestions.is_empty()
    {
        bail!("supported-editor next-edit boundary must default to disabled with no suggestions");
    }

    request.gate = NextEditFeatureGate::explicit_enabled();
    let explicit_gate_response = provider.suggest(&request);
    if explicit_gate_response.status != NextEditStatus::RuntimeProviderNotRegistered
        || !explicit_gate_response.suggestions.is_empty()
    {
        bail!(
            "supported-editor next-edit explicit gate must remain provider-not-registered with no suggestions"
        );
    }
    if request.safety_policy.ai_source_enabled {
        bail!("supported-editor next-edit boundary must not enable AI candidate sources");
    }

    Ok(NextEditSupportedEditorBoundaryReceipt {
        claim_boundary: "supported-editor next-edit boundary proof only; default config emits no suggestions and explicit dev config still has no registered runtime provider",
        enabled_by_default: false,
        explicit_dev_gate_enabled: true,
        runtime_provider_registered: false,
        editor_visible_suggestions: false,
        ai_candidate_source_enabled: request.safety_policy.ai_source_enabled,
        default_response,
        explicit_gate_response,
    })
}

fn validate_proof_surface(
    root: &Path,
    route: &str,
    plane: &str,
    surface: &ProofSurfaceRequirement,
) -> Result<()> {
    let path = root.join(surface.path);
    let content =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let missing = surface
        .markers
        .iter()
        .copied()
        .filter(|marker| !content.contains(marker))
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        bail!(
            "supported editor inline smoke route `{route}` proof plane `{plane}` surface `{}` is missing markers: {}",
            surface.path,
            missing.join(", ")
        );
    }

    Ok(())
}

fn write_receipt(path: &Path, receipt: &SupportedEditorInlineSmokeReceipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(receipt)?;
    fs::write(path, format!("{json}\n"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::TempDir;

    const TEST_PROOFS: &[ProofSurfaceRequirement] = &[ProofSurfaceRequirement {
        path: "proof.md",
        markers: &["present marker", "missing marker"],
    }];
    const TEST_ROUTES: &[SupportedEditorRouteRequirement] = &[SupportedEditorRouteRequirement {
        route: "test_route",
        claim: "test route claim",
        proof_planes: &[proof_surface_plane(TEST_PROOFS)],
    }];

    const DUPLICATE_ROUTES: &[SupportedEditorRouteRequirement] = &[
        SupportedEditorRouteRequirement {
            route: "duplicate_route",
            claim: "first",
            proof_planes: &[proof_surface_plane(TEST_PROOFS)],
        },
        SupportedEditorRouteRequirement {
            route: "duplicate_route",
            claim: "second",
            proof_planes: &[proof_surface_plane(TEST_PROOFS)],
        },
    ];

    #[test]
    fn semantic_inline_receipts_record_supported_editor_routes() -> Result<()> {
        let temp = TempDir::new()?;
        write_fixture_files(temp.path(), ROUTES)?;

        let receipt = summarize_supported_editor_routes(temp.path(), ROUTES)?;

        assert!(receipt.all_supported_routes_registered);
        assert_eq!(receipt.route_count, ROUTES.len());
        for route in ROUTES {
            let entry = receipt
                .supported_editor_routes
                .get(route.route)
                .ok_or_else(|| color_eyre::eyre::eyre!("missing route {}", route.route))?;
            assert_eq!(entry.status, "registered");
            assert_eq!(entry.claim, route.claim);
            assert_eq!(entry.proof_plane_count, route.proof_planes.len());
            assert_eq!(entry.proof_planes.len(), route.proof_planes.len());
            for (name, plane) in entry.proof_planes.iter() {
                let expected = route
                    .proof_planes
                    .iter()
                    .find(|expected| expected.plane == *name)
                    .ok_or_else(|| color_eyre::eyre::eyre!("missing plane {name}"))?;
                assert_eq!(plane.proof_surface_count, expected.proof_surfaces.len());
                assert_eq!(plane.proof_surfaces.len(), expected.proof_surfaces.len());
                for (actual, expected) in
                    plane.proof_surfaces.iter().zip(expected.proof_surfaces.iter())
                {
                    assert_eq!(actual.path, expected.path);
                    assert_eq!(actual.required_marker_count, expected.markers.len());
                    assert_eq!(actual.required_markers, expected.markers.to_vec());
                }
            }
        }

        let lsp4ij = receipt
            .supported_editor_routes
            .get("lsp4ij_upstream_integration")
            .ok_or_else(|| color_eyre::eyre::eyre!("missing lsp4ij route"))?;
        let launch = lsp4ij
            .proof_planes
            .get("launch_identity")
            .ok_or_else(|| color_eyre::eyre::eyre!("missing launch_identity plane"))?;
        assert_eq!(launch.status, "registered");
        let actual_host = lsp4ij
            .proof_planes
            .get("actual_host_evidence")
            .ok_or_else(|| color_eyre::eyre::eyre!("missing actual_host_evidence plane"))?;
        assert_eq!(actual_host.status, "not_proven");
        assert_eq!(actual_host.owner, Some("#7719/#7977/#8179"));
        assert_eq!(actual_host.proof_surface_count, 0);

        assert_eq!(receipt.future_gated.get("live_vscode_ui_automation"), Some(&"future_gated"));
        assert_eq!(receipt.future_gated.get("live_lsp4ij_ui_automation"), Some(&"future_gated"));
        assert_eq!(receipt.future_gated.get("runtime_next_edit_provider"), Some(&"future_gated"));
        assert_eq!(
            receipt.future_gated.get("editor_visible_next_edit_suggestions"),
            Some(&"future_gated")
        );
        assert_eq!(
            receipt.future_gated.get("runtime_multiline_inline_completion"),
            Some(&"future_gated")
        );
        assert!(!receipt.next_edit_boundary.enabled_by_default);
        assert!(receipt.next_edit_boundary.explicit_dev_gate_enabled);
        assert!(!receipt.next_edit_boundary.runtime_provider_registered);
        assert!(!receipt.next_edit_boundary.editor_visible_suggestions);
        assert!(!receipt.next_edit_boundary.ai_candidate_source_enabled);
        assert_eq!(receipt.next_edit_boundary.default_response.status, NextEditStatus::Disabled);
        assert_eq!(
            receipt.next_edit_boundary.explicit_gate_response.status,
            NextEditStatus::RuntimeProviderNotRegistered
        );

        Ok(())
    }

    #[test]
    fn receipt_rejects_missing_proof_markers() -> Result<()> {
        let temp = TempDir::new()?;
        fs::write(temp.path().join("proof.md"), "present marker\n")?;

        let Err(error) = summarize_supported_editor_routes(temp.path(), TEST_ROUTES) else {
            bail!("missing proof marker must fail receipt generation");
        };
        assert!(
            error.to_string().contains("missing marker"),
            "error should identify missing marker, got {error}"
        );

        Ok(())
    }

    #[test]
    fn missing_plane_marker_names_route_and_plane() -> Result<()> {
        let temp = TempDir::new()?;
        write_fixture_files(temp.path(), ROUTES)?;

        let doc = temp.path().join("docs/EDITORS/INTELLIJ_IDEA_SETUP.md");
        let content = fs::read_to_string(&doc)?;
        let content = content.replace("perllsp --stdio", "perllsp --pipe");
        fs::write(&doc, content)?;

        let Err(error) = summarize_supported_editor_routes(temp.path(), ROUTES) else {
            bail!("removing the launch identity must fail receipt generation");
        };
        let message = error.to_string();
        assert!(
            message.contains("`lsp4ij_upstream_integration` proof plane `launch_identity`"),
            "error should name the route and plane, got {message}"
        );

        Ok(())
    }

    #[test]
    fn lsp4ij_host_support_boundary_removal_fails_receipt() -> Result<()> {
        let temp = TempDir::new()?;
        write_fixture_files(temp.path(), ROUTES)?;

        let doc = temp.path().join("docs/EDITORS/INTELLIJ_IDEA_SETUP.md");
        let content = fs::read_to_string(&doc)?;
        let content =
            content.replace("not the same thing as IntelliJ/LSP4IJ host support", "host support");
        fs::write(&doc, content)?;

        let Err(error) = summarize_supported_editor_routes(temp.path(), ROUTES) else {
            bail!("removing the host-support boundary must fail receipt generation");
        };
        let message = error.to_string();
        assert!(
            message.contains("proof plane `host_support_boundary`"),
            "error should name the boundary plane, got {message}"
        );

        Ok(())
    }

    #[test]
    fn declared_status_plane_requires_no_proof_surfaces() -> Result<()> {
        let temp = TempDir::new()?;
        write_fixture_files(temp.path(), TEST_ROUTES)?;

        let malformed = &[SupportedEditorRouteRequirement {
            route: "malformed_route",
            claim: "malformed route claim",
            proof_planes: &[ProofPlaneRequirement {
                plane: "bad_plane",
                owner: Some("owner"),
                declared_status: Some("not_proven"),
                proof_surfaces: TEST_PROOFS,
            }],
        }];

        let Err(error) = summarize_supported_editor_routes(temp.path(), malformed) else {
            bail!("a declared-status plane with proof surfaces must be rejected");
        };
        let message = error.to_string();
        assert!(
            message.contains("`bad_plane`"),
            "error should name the malformed plane, got {message}"
        );

        Ok(())
    }

    #[test]
    fn unknown_declared_status_is_rejected() -> Result<()> {
        let temp = TempDir::new()?;
        write_fixture_files(temp.path(), TEST_ROUTES)?;

        let malformed = &[SupportedEditorRouteRequirement {
            route: "malformed_status",
            claim: "malformed status claim",
            proof_planes: &[ProofPlaneRequirement {
                plane: "actual_host_evidence",
                owner: Some("owner"),
                declared_status: Some("maybe"),
                proof_surfaces: &[],
            }],
        }];

        let Err(error) = summarize_supported_editor_routes(temp.path(), malformed) else {
            bail!("unknown declared status must be rejected");
        };
        let message = error.to_string();
        assert!(message.contains("unsupported declared status `maybe`"), "{message}");
        Ok(())
    }

    #[test]
    fn duplicate_route_is_rejected() -> Result<()> {
        let temp = TempDir::new()?;
        write_fixture_files(temp.path(), DUPLICATE_ROUTES)?;

        let Err(error) = summarize_supported_editor_routes(temp.path(), DUPLICATE_ROUTES) else {
            bail!("duplicate route must be rejected");
        };
        let message = error.to_string();
        assert!(message.contains("duplicate route `duplicate_route`"), "{message}");
        Ok(())
    }

    #[test]
    fn semantic_inline_receipts_json_keeps_claim_boundary() -> Result<()> {
        let temp = TempDir::new()?;
        write_fixture_files(temp.path(), ROUTES)?;
        let receipt = summarize_supported_editor_routes(temp.path(), ROUTES)?;
        let value: Value = serde_json::to_value(&receipt)?;

        assert_eq!(
            value.get("schema_version").and_then(Value::as_str),
            Some("supported-editor-inline-smoke.v1")
        );
        assert_eq!(
            value.get("provider_action").and_then(Value::as_str),
            Some("supported_editor_inline_smoke_bundle")
        );
        let boundary = value
            .get("claim_boundary")
            .and_then(Value::as_str)
            .ok_or_else(|| color_eyre::eyre::eyre!("missing claim_boundary"))?;
        assert!(boundary.contains("not live editor UI automation"));
        assert!(boundary.contains("editor-visible next-edit suggestions"));
        assert!(boundary.contains("runtime multiline behavior"));
        let next_edit_boundary = value
            .get("next_edit_boundary")
            .and_then(Value::as_object)
            .ok_or_else(|| color_eyre::eyre::eyre!("missing next_edit_boundary"))?;
        assert_eq!(
            next_edit_boundary.get("enabled_by_default").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            next_edit_boundary.get("explicit_dev_gate_enabled").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            next_edit_boundary.get("runtime_provider_registered").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            next_edit_boundary.get("editor_visible_suggestions").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            next_edit_boundary.get("ai_candidate_source_enabled").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            next_edit_boundary
                .get("default_response")
                .and_then(|response| response.get("status"))
                .and_then(Value::as_str),
            Some("disabled")
        );
        assert_eq!(
            next_edit_boundary
                .get("explicit_gate_response")
                .and_then(|response| response.get("status"))
                .and_then(Value::as_str),
            Some("runtime_provider_not_registered")
        );

        Ok(())
    }

    #[test]
    fn write_receipt_creates_parent_dirs_and_serializes_routes() -> Result<()> {
        let temp = TempDir::new()?;
        write_fixture_files(temp.path(), ROUTES)?;
        let receipt = summarize_supported_editor_routes(temp.path(), ROUTES)?;
        let receipt_path = temp.path().join("nested").join("supported-editor.json");

        write_receipt(&receipt_path, &receipt)?;

        let value: Value = serde_json::from_str(&fs::read_to_string(receipt_path)?)?;
        assert_eq!(value.get("route_count").and_then(Value::as_u64), Some(ROUTES.len() as u64));
        assert_eq!(
            value
                .pointer("/supported_editor_routes/release_built_binary_smoke/status")
                .and_then(Value::as_str),
            Some("registered")
        );

        Ok(())
    }

    fn write_fixture_files(
        root: &Path,
        requirements: &[SupportedEditorRouteRequirement],
    ) -> Result<()> {
        let mut content_by_path = BTreeMap::<&str, Vec<&str>>::new();
        for route in requirements {
            for plane in route.proof_planes {
                for surface in plane.proof_surfaces {
                    content_by_path.entry(surface.path).or_default().extend(surface.markers);
                }
            }
        }

        for (path, markers) in content_by_path {
            let full_path = root.join(path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(full_path, markers.join("\n"))?;
        }

        Ok(())
    }
}
