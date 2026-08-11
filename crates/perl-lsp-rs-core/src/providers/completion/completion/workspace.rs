//! Workspace symbol completion for Perl
//!
//! Provides completion for symbols from other files in the workspace using the workspace index.
//! Includes module name completion for `use`/`require` statements, workspace-aware method
//! completion for `->` expressions, and general cross-file symbol completion.

use super::{
    auto_import,
    context::CompletionContext,
    items::{CompletionItem, CompletionItemKind, InsertTextFormat},
};
use crate::providers::completion::module_scan_cache::{ModuleCompletionScanCache, ScanCacheKey};
use perl_lexer::{PerlLexer, TokenType};
use perl_module::path::module_name_to_path;
use perl_parser_core::SourceLocation;
use perl_semantic_analyzer::{
    Node, NodeKind, Parser,
    receiver_facts::{
        ReceiverFact, ReceiverFactContext, ReceiverFactFreshness, ReceiverFallbackState,
        ReceiverKind, receiver_fact_for_method_call,
    },
    semantic::SemanticModel,
    symbol::{ScopeKind, SymbolKind},
    type_facts::TypeEvidence,
    type_inference::{PerlType, TypeInferenceEngine},
};
use perl_semantic_facts::{
    Confidence, DefinitionCandidate, EntityKind, FileId, PackageEdge, PackageEdgeKind, Provenance,
