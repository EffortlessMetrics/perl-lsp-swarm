//! Focused behavioral proof for notebook preview admission boundaries.

use super::*;
use perl_lsp_rs_core::features::policy::FeatureProfile;
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn supported_profiles_reject_notebook_routes_without_mutation() -> TestResult {
    for profile in [FeatureProfile::Production, FeatureProfile::GaLock] {
        let server = LspServer::new_with_feature_profile(profile);
        let notebook_uri = "file:///disabled.ipynb";
        let cell_uri = "file:///disabled.ipynb#cell1";
        let open = Some(json!({
            "notebookDocument": {
                "uri": notebook_uri,
                "notebookType": "jupyter-notebook",
                "version": 1,
                "cells": [{"kind": 2, "document": cell_uri}]
            },
            "cellTextDocuments": [{
                "uri": cell_uri,
                "languageId": "perl",
                "version": 1,
                "text": "sub must_not_exist {}"
            }]
        }));

        let routes = [
            server.handle_notebook_did_open(open),
            server.handle_notebook_did_change(Some(json!({
                "notebookDocument": {"uri": notebook_uri, "version": 2},
                "change": {}
            }))),
            server.handle_notebook_did_save(Some(json!({
                "notebookDocument": {"uri": notebook_uri}
            }))),
            server.handle_notebook_did_close(Some(json!({
                "notebookDocument": {"uri": notebook_uri},
                "cellTextDocuments": [{"uri": cell_uri}]
            }))),
        ];

        for result in routes {
            let error = result.err().ok_or("disabled notebook route unexpectedly succeeded")?;
            if error.code != -32601 {
                return Err(format!("disabled notebook route returned {}", error.code).into());
            }
        }
        if server.notebook_store.get_notebook(notebook_uri).is_some()
            || server.notebook_store.get_notebook_for_cell(cell_uri).is_some()
            || server.documents_guard().contains_key(cell_uri)
        {
            return Err(
                format!("{} notebook route mutated retained state", profile.as_str()).into()
            );
        }
    }
    Ok(())
}

#[test]
fn unsupported_notebook_type_returns_invalid_params_without_mutation() -> TestResult {
    let server = LspServer::new_with_feature_profile(FeatureProfile::All);
    let notebook_uri = "file:///unsupported.ipynb";
    let cell_uri = "file:///unsupported.ipynb#cell1";

    let error = server
        .handle_notebook_did_open(Some(json!({
            "notebookDocument": {
                "uri": notebook_uri,
                "notebookType": "quarto-notebook",
                "version": 1,
                "cells": [{"kind": 2, "document": cell_uri}]
            },
            "cellTextDocuments": [{
                "uri": cell_uri,
                "languageId": "perl",
                "version": 1,
                "text": "sub must_not_exist {}"
            }]
        })))
        .err()
        .ok_or("unsupported notebook type unexpectedly succeeded")?;

    if error.code != -32602
        || error.message != "Unsupported notebookDocument.notebookType"
        || server.notebook_store.get_notebook(notebook_uri).is_some()
        || server.notebook_store.get_notebook_for_cell(cell_uri).is_some()
        || server.documents_guard().contains_key(cell_uri)
    {
        return Err(
            format!("unsupported notebook type escaped fail-closed boundary: {error:?}").into()
        );
    }
    Ok(())
}

#[test]
fn preview_does_not_grant_perl_authority_to_unselected_cells() -> TestResult {
    let server = LspServer::new_with_feature_profile(FeatureProfile::All);
    let notebook_uri = "file:///mixed.ipynb";
    let perl_uri = "file:///mixed.ipynb#perl";
    let python_uri = "file:///mixed.ipynb#python";
    let missing_uri = "file:///mixed.ipynb#missing";

    server.handle_notebook_did_open(Some(json!({
        "notebookDocument": {
            "uri": notebook_uri,
            "notebookType": "jupyter-notebook",
            "version": 1,
            "cells": [
                {"kind": 2, "document": perl_uri},
                {"kind": 2, "document": python_uri},
                {"kind": 2, "document": missing_uri}
            ]
        },
        "cellTextDocuments": [
            {"uri": perl_uri, "languageId": "perl", "version": 1, "text": "sub perl_cell {}"},
            {"uri": python_uri, "languageId": "python", "version": 1, "text": "def python_cell(): pass"},
            {"uri": missing_uri, "version": 1, "text": "sub missing_language {}"}
        ]
    })))?;

    let documents = server.documents_guard();
    if !documents.contains_key(perl_uri)
        || documents.contains_key(python_uri)
        || documents.contains_key(missing_uri)
    {
        return Err("preview document authority exceeded the exact Perl selector".into());
    }
    Ok(())
}

#[test]
fn non_perl_did_change_cell_retains_structure_without_gaining_authority() -> TestResult {
    let server = LspServer::new_with_feature_profile(FeatureProfile::All);
    let notebook_uri = "file:///change-selector.ipynb";
    let original_perl_uri = "file:///change-selector.ipynb#perl-original";
    let added_perl_uri = "file:///change-selector.ipynb#perl-added";
    let python_uri = "file:///change-selector.ipynb#python";

    server.handle_notebook_did_open(Some(json!({
        "notebookDocument": {
            "uri": notebook_uri,
            "notebookType": "jupyter-notebook",
            "version": 1,
            "cells": [{"kind": 2, "document": original_perl_uri}]
        },
        "cellTextDocuments": [{
            "uri": original_perl_uri,
            "languageId": "perl",
            "version": 1,
            "text": "sub retained_perl_cell {}"
        }]
    })))?;

    server.handle_notebook_did_change(Some(json!({
        "notebookDocument": {"uri": notebook_uri, "version": 2},
        "change": {
            "cells": {
                "structure": {
                    "array": {
                        "start": 1,
                        "deleteCount": 0,
                        "cells": [
                            {"kind": 2, "document": python_uri},
                            {"kind": 2, "document": added_perl_uri}
                        ]
                    },
                    "didOpen": [
                        {
                            "uri": python_uri,
                            "languageId": "python",
                            "version": 1,
                            "text": "def must_not_become_authoritative(): pass"
                        },
                        {
                            "uri": added_perl_uri,
                            "languageId": "perl",
                            "version": 1,
                            "text": "sub newly_authorized_perl_cell {}"
                        }
                    ]
                }
            }
        }
    })))?;

    let notebook = server
        .notebook_store
        .get_notebook(notebook_uri)
        .ok_or("notebook state disappeared after didChange")?;
    let documents = server.documents_guard();
    if notebook.version != 2
        || notebook.cells.len() != 3
        || server.notebook_store.get_notebook_for_cell(python_uri).as_deref() != Some(notebook_uri)
        || server.notebook_store.get_notebook_for_cell(added_perl_uri).as_deref()
            != Some(notebook_uri)
        || documents.contains_key(python_uri)
        || !documents.contains_key(added_perl_uri)
    {
        return Err("didChange selector confused notebook structure with Perl authority".into());
    }
    Ok(())
}
