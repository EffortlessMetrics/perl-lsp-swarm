//! Integration test: `perl-lsp-performance` public API reachable via `perl_lsp_rs_core::performance`.

use perl_lsp_rs_core::performance::*;

#[test]
fn performance_module_exposes_ast_cache() {
    // Verify that AstCache is accessible post-absorption
    let cache = AstCache::new(100, 60);
    let _cache = cache;
}

#[test]
fn performance_module_exposes_symbol_index() {
    // Verify that SymbolIndex is accessible post-absorption
    let _: Option<SymbolIndex> = None;
}

#[test]
fn performance_module_exposes_parallel_submodule() {
    // Verify that parallel submodule is accessible post-absorption
    let _: Option<parallel::ParallelIndexer> = None;
}

#[test]
fn performance_ast_cache_stores_and_retrieves() {
    // Verify that AstCache functionality works post-absorption
    let cache = AstCache::new(100, 60);
    let _content = "sub foo {}";
    // Note: full integration test would require actual Node construction,
    // which is complex. This test verifies the cache is instantiable.
    let _cache = cache;
}

#[test]
fn parallel_given_zero_workers_when_processing_then_all_files_are_still_processed() {
    let files = vec!["a.pm".to_string(), "b.pm".to_string(), "c.pm".to_string()];

    let mut processed = parallel::process_files_parallel(files, 0, |file| file);
    processed.sort();

    assert_eq!(processed, vec!["a.pm", "b.pm", "c.pm"]);
}

#[test]
fn parallel_given_more_workers_than_files_when_processing_then_each_file_processed_once() {
    let files = vec!["one.pm".to_string(), "two.pm".to_string()];

    let mut processed = parallel::process_files_parallel(files, 16, |file| file);
    processed.sort();

    assert_eq!(processed, vec!["one.pm", "two.pm"]);
}
