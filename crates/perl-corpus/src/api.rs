pub(crate) mod root;
mod topology;

pub use crate::cases::{
    complex_data_structure_cases, edge_cases, find_complex_case, get_complex_data_structure_tests,
    sample_complex_case, ComplexDataStructureCase, EdgeCase, EdgeCaseGenerator,
};
pub use crate::codegen::{
    generate_perl_code, generate_perl_code_with_options, generate_perl_code_with_seed,
    generate_perl_code_with_statements, CodegenOptions, StatementKind,
};
pub use crate::concepts::{load_concept_registry, ConceptRow, LoadedConcept};
pub use crate::continue_redo::{
    cases_by_tag as continue_redo_cases_by_tag, continue_redo_cases,
    find_case as find_continue_redo_case, invalid_cases as invalid_continue_redo_cases,
    valid_cases as valid_continue_redo_cases, ContinueRedoCase,
};
pub use crate::files::{
    get_all_test_files, get_corpus_files, get_corpus_files_from, get_fuzz_files, get_test_files,
    CorpusFile, CorpusLayer, CorpusPaths, ResolvedCorpusPaths,
};
pub use crate::format_statements::{
    find_format_case, format_statement_cases, FormatStatementCase, FormatStatementGenerator,
};
pub use crate::glob_expressions::{
    find_glob_case, glob_expression_cases, GlobExpressionCase, GlobExpressionGenerator,
};
pub use crate::gold::*;
pub use crate::inventory::*;
pub use crate::loading::{
    load_plain_perl_source, load_sectioned_corpus_document, parse_dir, parse_file, CorpusLoadError,
    NewlineStyle, PlainPerlSource, SectionCaseId, SectionedCase, SectionedCorpusDocument,
};
pub use crate::metadata::{find_by_flag, find_by_tag, Section};
pub use crate::sidecar::*;
pub use crate::tie_interface::{
    find_tie_case, tie_cases_by_tag, tie_cases_by_tags_all, tie_cases_by_tags_any,
    tie_interface_cases, TieInterfaceCase,
};
pub use root::{CorpusRoot, CorpusRootError, CorpusRootSource, CORPUS_ROOT_ENV};
pub use topology::{
    AssetRequirement, CorpusAsset, CorpusAssetKind, CorpusAssetLayer, CorpusTopology,
    CorpusTopologyError, CORPUS_TOPOLOGY_SCHEMA_VERSION,
};
