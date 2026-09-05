//! Deterministic probe table binding the landed #11369 public-surface
//! inventory (and subject-manifest facts) to bounded textual needles in the
//! upstream tree.
//!
//! Every needle was grounded in the pinned bytes `e10d1864` before landing
//! and is re-checked against the landed inventory by the #11411 contract
//! tests: a probe-row surface absent from the inventory, or an inventory
//! surface without probes, fails closed offline.
//!
//! Probes answer one question only — "does this signature still exist in the
//! observed upstream bytes?" — and are never a behavior claim.

use anyhow::{Result, ensure};

use crate::vim_lsp_subject_refresh::model::DriftClass;

/// Upstream file paths probed by the table. Bounded, literal, relative.
pub const FILE_PLUGIN: &str = "plugin/lsp.vim";
pub const FILE_AUTOLOAD: &str = "autoload/lsp.vim";
pub const FILE_UTILS: &str = "autoload/lsp/utils.vim";
pub const FILE_OMNI: &str = "autoload/lsp/omni.vim";
pub const FILE_CAPABILITIES: &str = "autoload/lsp/capabilities.vim";
pub const FILE_WORKSPACE_CONFIG: &str = "autoload/lsp/utils/workspace_config.vim";
pub const FILE_WORKSPACE_EDIT: &str = "autoload/lsp/utils/workspace_edit.vim";
pub const FILE_TEXT_EDIT: &str = "autoload/lsp/utils/text_edit.vim";
pub const FILE_README: &str = "README.md";
pub const FILE_DOC: &str = "doc/vim-lsp.txt";

/// One bounded needle probe and the drift class its absence classifies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceProbe {
    pub surface: &'static str,
    pub file: &'static str,
    pub needle: &'static str,
    pub class_if_absent: DriftClass,
}

/// The complete public-surface probe table. Surfaces are the exact
/// `surfaces[].surface` strings of `.ci/editor-clients/vim-vim-lsp-public-surface.v1.json`.
pub const SURFACE_PROBES: &[SurfaceProbe] = &[
    // server registration and root callback
    SurfaceProbe {
        surface: "server registration and root callback",
        file: FILE_AUTOLOAD,
        needle: "function! lsp#register_server(",
        class_if_absent: DriftClass::RegistrationRootOrConfigApiChanged,
    },
    SurfaceProbe {
        surface: "server registration and root callback",
        file: FILE_AUTOLOAD,
        needle: "'root_uri'",
        class_if_absent: DriftClass::RegistrationRootOrConfigApiChanged,
    },
    SurfaceProbe {
        surface: "server registration and root callback",
        file: FILE_UTILS,
        needle: "function! lsp#utils#find_nearest_parent_file_directory(",
        class_if_absent: DriftClass::RegistrationRootOrConfigApiChanged,
    },
    SurfaceProbe {
        surface: "server registration and root callback",
        file: FILE_UTILS,
        needle: "function! lsp#utils#path_to_uri(",
        class_if_absent: DriftClass::RegistrationRootOrConfigApiChanged,
    },
    // server initialized / buffer enabled events
    SurfaceProbe {
        surface: "server initialized / buffer enabled events",
        file: FILE_AUTOLOAD,
        needle: "User lsp_server_init",
        class_if_absent: DriftClass::ReadinessDiagnosticsOrLoggingSurfaceChanged,
    },
    SurfaceProbe {
        surface: "server initialized / buffer enabled events",
        file: FILE_AUTOLOAD,
        needle: "User lsp_buffer_enabled",
        class_if_absent: DriftClass::ReadinessDiagnosticsOrLoggingSurfaceChanged,
    },
    // client diagnostics state or event
    SurfaceProbe {
        surface: "client diagnostics state or event",
        file: FILE_AUTOLOAD,
        needle: "User lsp_diagnostics_updated",
        class_if_absent: DriftClass::ReadinessDiagnosticsOrLoggingSurfaceChanged,
    },
    SurfaceProbe {
        surface: "client diagnostics state or event",
        file: FILE_AUTOLOAD,
        needle: "function! lsp#get_buffer_diagnostics_counts(",
        class_if_absent: DriftClass::ReadinessDiagnosticsOrLoggingSurfaceChanged,
    },
    // generic request channel (hover/definition/references/rename/formatting/completion)
    SurfaceProbe {
        surface: "generic request channel (hover/definition/references/rename/formatting/completion results)",
        file: FILE_AUTOLOAD,
        needle: "function! lsp#send_request(",
        class_if_absent: DriftClass::NavigationOrWorkspaceEditActionChanged,
    },
    SurfaceProbe {
        surface: "generic request channel (hover/definition/references/rename/formatting/completion results)",
        file: FILE_AUTOLOAD,
        needle: "function! lsp#get_text_document_identifier(",
        class_if_absent: DriftClass::NavigationOrWorkspaceEditActionChanged,
    },
    SurfaceProbe {
        surface: "generic request channel (hover/definition/references/rename/formatting/completion results)",
        file: FILE_AUTOLOAD,
        needle: "function! lsp#get_position(",
        class_if_absent: DriftClass::NavigationOrWorkspaceEditActionChanged,
    },
    // completion conversion/application
    SurfaceProbe {
        surface: "completion conversion/application",
        file: FILE_OMNI,
        needle: "function! lsp#omni#get_vim_completion_items(",
        class_if_absent: DriftClass::CompletionOrSnippetApplicationModelChanged,
    },
    // rename/workspace-edit application
    SurfaceProbe {
        surface: "rename/workspace-edit application",
        file: FILE_WORKSPACE_EDIT,
        needle: "function! lsp#utils#workspace_edit#apply_workspace_edit(",
        class_if_absent: DriftClass::NavigationOrWorkspaceEditActionChanged,
    },
    // formatting application
    SurfaceProbe {
        surface: "formatting application",
        file: FILE_TEXT_EDIT,
        needle: "function! lsp#utils#text_edit#apply_text_edits(",
        class_if_absent: DriftClass::NavigationOrWorkspaceEditActionChanged,
    },
    // workspace configuration refresh
    SurfaceProbe {
        surface: "workspace configuration refresh",
        file: FILE_AUTOLOAD,
        needle: "function! lsp#update_workspace_config(",
        class_if_absent: DriftClass::WorkspaceConfigurationBehaviorChanged,
    },
    SurfaceProbe {
        surface: "workspace configuration refresh",
        file: FILE_WORKSPACE_CONFIG,
        needle: "function! lsp#utils#workspace_config#get(",
        class_if_absent: DriftClass::WorkspaceConfigurationBehaviorChanged,
    },
    // buffer edit/open/close/reopen lifecycle
    SurfaceProbe {
        surface: "buffer edit/open/close/reopen lifecycle",
        file: FILE_AUTOLOAD,
        needle: "function! lsp#enable(",
        class_if_absent: DriftClass::ServerRestartOrBufferLifecycleSurfaceChanged,
    },
    // server stop/restart and log/status inspection
    SurfaceProbe {
        surface: "server stop/restart and log/status inspection",
        file: FILE_AUTOLOAD,
        needle: "function! lsp#stop_server(",
        class_if_absent: DriftClass::ServerRestartOrBufferLifecycleSurfaceChanged,
    },
    SurfaceProbe {
        surface: "server stop/restart and log/status inspection",
        file: FILE_AUTOLOAD,
        needle: "function! lsp#get_server_status(",
        class_if_absent: DriftClass::ServerRestartOrBufferLifecycleSurfaceChanged,
    },
    SurfaceProbe {
        surface: "server stop/restart and log/status inspection",
        file: FILE_AUTOLOAD,
        needle: "function! lsp#is_server_running(",
        class_if_absent: DriftClass::ServerRestartOrBufferLifecycleSurfaceChanged,
    },
    SurfaceProbe {
        surface: "server stop/restart and log/status inspection",
        file: FILE_AUTOLOAD,
        needle: "function! lsp#print_server_status(",
        class_if_absent: DriftClass::ServerRestartOrBufferLifecycleSurfaceChanged,
    },
    SurfaceProbe {
        surface: "server stop/restart and log/status inspection",
        file: FILE_PLUGIN,
        needle: "let g:lsp_log_file =",
        class_if_absent: DriftClass::ReadinessDiagnosticsOrLoggingSurfaceChanged,
    },
    SurfaceProbe {
        surface: "server stop/restart and log/status inspection",
        file: FILE_PLUGIN,
        needle: "let g:lsp_log_verbose =",
        class_if_absent: DriftClass::ReadinessDiagnosticsOrLoggingSurfaceChanged,
    },
    // didChange observation/instrumentation seam
    SurfaceProbe {
        surface: "didChange observation/instrumentation seam",
        file: FILE_AUTOLOAD,
        needle: "on_text_document_did_change",
        class_if_absent: DriftClass::TextSyncOrDidChangeObservationSurfaceChanged,
    },
    SurfaceProbe {
        surface: "didChange observation/instrumentation seam",
        file: FILE_PLUGIN,
        needle: "let g:lsp_log_verbose =",
        class_if_absent: DriftClass::TextSyncOrDidChangeObservationSurfaceChanged,
    },
    SurfaceProbe {
        surface: "didChange observation/instrumentation seam",
        file: FILE_PLUGIN,
        needle: "let g:lsp_log_file =",
        class_if_absent: DriftClass::TextSyncOrDidChangeObservationSurfaceChanged,
    },
    // experimental workspace folders
    SurfaceProbe {
        surface: "experimental workspace folders",
        file: FILE_PLUGIN,
        needle: "let g:lsp_experimental_workspace_folders =",
        class_if_absent: DriftClass::WorkspaceFolderOrChangeNotificationsBehaviorChanged,
    },
    SurfaceProbe {
        surface: "experimental workspace folders",
        file: FILE_CAPABILITIES,
        needle: "function! lsp#capabilities#has_workspace_folders_change_notifications(",
        class_if_absent: DriftClass::WorkspaceFolderOrChangeNotificationsBehaviorChanged,
    },
];

/// Plugin-load shape needles: the plugin entry file, its once-guard, and the
/// manifest's entry-file set. Absence classifies
/// `plugin_load_or_install_shape_changed`.
pub const LOAD_GUARD_NEEDLE: &str = "g:lsp_loaded";

/// Plugin global defaults recorded by #11369 as theoretical feature gates.
/// The default expressions are compared verbatim; the contract test asserts
/// each expression appears in the landed manifest's prose so the table
/// cannot silently diverge from the pin.
pub const EXPECTED_PLUGIN_DEFAULTS: &[(&str, &str)] = &[
    ("g:lsp_use_lua", "has('nvim-0.4.0') || (has('lua') && has('patch-8.2.0775'))"),
    ("g:lsp_use_event_queue", "has('nvim') || has('patch-8.1.0889')"),
    ("g:lsp_text_edit_enabled", "has('patch-8.0.1493')"),
];

/// Closed maintenance-marker vocabulary. A marker present in the observed
/// README classifies `maintenance_state_changed`; the vocabulary is closed
/// so arbitrary upstream prose cannot flow into the artifact.
pub const MAINTENANCE_MARKERS: &[&str] = &[
    "maintenance mode",
    "no longer maintained",
    "not maintained",
    "unmaintained",
    "this project is archived",
    "this plugin is deprecated",
    "vim-lsp is deprecated",
];

/// The recorded capability note the observer re-checks: vim-lsp's README
/// statement that snippets are not supported by default.
pub const SNIPPET_NOTE_NEEDLE: &str = "does not support snippets by default";

/// The floor sentence needle in `doc/vim-lsp.txt`. The pinned bytes state
/// the theoretical floor as
/// `Requires NeoVim with version 0.3 or Vim 8.1.1035 or newer.`
/// (possibly line-wrapped); the parser accepts bounded whitespace.
pub const FLOOR_SENTENCE_ANCHOR: &str = "Requires NeoVim with version";

/// Validate the compiled table against the landed public-surface inventory:
/// every inventory surface must be fully covered by probe rows and every
/// probe row must cite a landed surface. Offline, deterministic.
pub fn validate_table_against_inventory(inventory: &serde_json::Value) -> Result<()> {
    let mut landed: Vec<&str> = Vec::new();
    if let Some(surfaces) = inventory.get("surfaces").and_then(|value| value.as_array()) {
        for surface in surfaces {
            let name = surface.get("surface").and_then(|value| value.as_str()).unwrap_or_default();
            ensure!(!name.is_empty(), "public-surface inventory carried an unnamed surface");
            landed.push(name);
        }
    }
    ensure!(
        !landed.is_empty(),
        "public-surface inventory carried no surfaces; probe table cannot bind"
    );

    let mut probed: Vec<&str> = SURFACE_PROBES.iter().map(|probe| probe.surface).collect();
    probed.sort_unstable();
    probed.dedup();
    for surface in &probed {
        ensure!(
            landed.contains(surface),
            "probe table cites surface {surface} absent from the landed #11369 inventory"
        );
    }
    for surface in &landed {
        ensure!(
            probed.contains(surface),
            "landed inventory surface {surface} has no #11411 probe rows; add probes or reclassify"
        );
    }
    Ok(())
}
