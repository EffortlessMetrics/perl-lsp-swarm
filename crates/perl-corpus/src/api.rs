mod asset_path;
pub(crate) mod root;
mod topology;

pub use crate::cases::{
    ComplexDataStructureCase, EdgeCase, EdgeCaseGenerator, complex_data_structure_cases,
    edge_cases, find_complex_case, get_complex_data_structure_tests, sample_complex_case,
};
pub use crate::codegen::{
    CodegenOptions, StatementKind, generate_perl_code, generate_perl_code_with_options,
    generate_perl_code_with_seed, generate_perl_code_with_statements,
};
pub use crate::concepts::{ConceptRow, LoadedConcept, load_concept_registry};
pub use crate::continue_redo::{
    ContinueRedoCase, cases_by_tag as continue_redo_cases_by_tag, continue_redo_cases,
    find_case as find_continue_redo_case, invalid_cases as invalid_continue_redo_cases,
    valid_cases as valid_continue_redo_cases,
};
pub use crate::files::{
    CorpusFile, CorpusLayer, CorpusPaths, ResolvedCorpusPaths, get_all_test_files,
    get_corpus_files, get_corpus_files_from, get_fuzz_files, get_test_files,
};
pub use crate::format_statements::{
    FormatStatementCase, FormatStatementGenerator, find_format_case, format_statement_cases,
};
pub use crate::glob_expressions::{
    GlobExpressionCase, GlobExpressionGenerator, find_glob_case, glob_expression_cases,
};
pub use crate::gold::*;
pub use crate::inventory::*;
pub use crate::loading::{
    CorpusLoadError, NO_FOLLOW_REVIEWED, NewlineStyle, PlainPerlSource, SectionCaseId,
    SectionedCase, SectionedCorpusDocument, load_plain_perl_source, load_sectioned_corpus_document,
    parse_dir, parse_file,
};
pub use crate::metadata::{Section, find_by_flag, find_by_tag};
pub use crate::sidecar::*;
pub use crate::tie_interface::{
    TieInterfaceCase, find_tie_case, tie_cases_by_tag, tie_cases_by_tags_all,
    tie_cases_by_tags_any, tie_interface_cases,
};
pub use asset_path::{CorpusAssetPath, CorpusAssetPathError};
pub use root::{CORPUS_ROOT_ENV, CorpusRoot, CorpusRootError, CorpusRootSource};
pub use topology::{
    AssetRequirement, CORPUS_TOPOLOGY_SCHEMA_VERSION, CorpusAsset, CorpusAssetKind,
    CorpusAssetLayer, CorpusTopology, CorpusTopologyError,
};

/// Serializes every test that reads or mutates the process-wide current
/// directory.
///
/// `std::env::set_current_dir` is per-process, not per-thread, so a test that
/// enters a temporary directory moves relative-path resolution out from under
/// any concurrently running test that resolved a path against the previous
/// current directory. With `--test-threads` above one that is a real race, not
/// a hypothetical: it is why the relative runtime-root binding test failed on
/// `main` against a path that no longer existed. Every test in this module
/// tree that touches the current directory must hold this lock for the whole
/// window between reading it and resolving against it.
#[cfg(test)]
pub(crate) static CWD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
