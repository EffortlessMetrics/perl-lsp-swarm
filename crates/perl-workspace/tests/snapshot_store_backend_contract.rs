//! Reuses the backend-neutral SnapshotStore differential harness (#4911).

use perl_snapshot_store::backend_differential::{open_directory_store, run_backend_differential};
use std::cell::RefCell;
use tempfile::TempDir;

#[test]
#[allow(clippy::expect_used)]
fn snapshot_store_directory_backend_contract_on_workspace_crate() {
    let keep = RefCell::new(Vec::<TempDir>::new());
    run_backend_differential(|| {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().to_path_buf();
        keep.borrow_mut().push(temp);
        open_directory_store(&path)
    })
    .expect("shared snapshot store contract");
}
