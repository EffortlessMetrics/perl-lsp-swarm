#!/usr/bin/env python3
"""Consolidate the exact textDocumentSync wire contract into its existing test."""

from pathlib import Path


path = Path("crates/perl-lsp-rs/tests/lsp_api_contracts.rs")
text = path.read_text(encoding="utf-8")

old_import = "use std::collections::HashSet;"
new_import = "use std::collections::{BTreeSet, HashSet};"
if text.count(old_import) != 1:
    raise SystemExit("expected one HashSet import to extend")
text = text.replace(old_import, new_import, 1)

old_contract = '''    assert_eq!(sync.get("openClose"), Some(&Value::Bool(true)));
    assert_eq!(sync.get("change").and_then(Value::as_u64), Some(1));
    assert_eq!(sync.get("willSave"), Some(&Value::Bool(true)));
    assert_eq!(sync.get("willSaveWaitUntil"), Some(&Value::Bool(true)));

    let save = sync
        .get("save")
        .and_then(Value::as_object)
        .ok_or("textDocumentSync.save must be an object")?;
    assert_eq!(save.get("includeText"), Some(&Value::Bool(true)));

    for snake_case_key in ["open_close", "will_save", "will_save_wait_until"] {
        assert!(!sync.contains_key(snake_case_key));
    }
    assert!(!save.contains_key("include_text"));
'''
new_contract = '''    let observed_keys: BTreeSet<_> = sync.keys().map(String::as_str).collect();
    let expected_keys =
        BTreeSet::from(["change", "openClose", "save", "willSave", "willSaveWaitUntil"]);
    assert_eq!(
        observed_keys, expected_keys,
        "textDocumentSync must change only through an intentional wire-contract update"
    );

    assert_eq!(sync.get("openClose"), Some(&Value::Bool(true)));
    assert_eq!(sync.get("change").and_then(Value::as_u64), Some(1));
    assert_eq!(sync.get("willSave"), Some(&Value::Bool(true)));
    assert_eq!(sync.get("willSaveWaitUntil"), Some(&Value::Bool(true)));

    let save = sync
        .get("save")
        .and_then(Value::as_object)
        .ok_or("textDocumentSync.save must be an object")?;
    let observed_save_keys: BTreeSet<_> = save.keys().map(String::as_str).collect();
    assert_eq!(
        observed_save_keys,
        BTreeSet::from(["includeText"]),
        "textDocumentSync.save must preserve its exact LSP wire shape"
    );
    assert_eq!(save.get("includeText"), Some(&Value::Bool(true)));
'''
if text.count(old_contract) != 1:
    raise SystemExit("expected one existing textDocumentSync contract block")

path.write_text(text.replace(old_contract, new_contract, 1), encoding="utf-8")
