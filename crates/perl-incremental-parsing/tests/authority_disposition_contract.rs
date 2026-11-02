#[path = "disposition_contract/validate.rs"]
mod validate;

use std::error::Error;
use std::path::Path;
use validate::{
    cargo_snapshot_from_metadata, fixture_root, load_inventory_from_path, source_files_with_tests,
    validate_inventory,
};

#[test]
fn compatibility_package_disposition_ledger_matches_cargo_and_schema() -> Result<(), Box<dyn Error>>
{
    let root = fixture_root();
    let inventory = load_inventory_from_path(&root.join("behavior_disposition.json"))?;
    let cargo = cargo_snapshot_from_metadata(&root)?;
    let sources = source_files_with_tests(&root.join("src"), Path::new("src"))?;
    validate_inventory(&inventory, &root, env!("CARGO_PKG_NAME"), &cargo, &sources)?;
    Ok(())
}
