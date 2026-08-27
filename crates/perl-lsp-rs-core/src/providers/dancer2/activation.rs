//! Runtime Dancer2 activation bridge (#8928).
//!
//! Builds the canonical registry-activated Dancer2 activation facts
//! (#8914) for one document from runtime evidence:
//!
//! 1. the canonical activation-site extractor
//!    (`extract_dancer2_activation_sites`) supplies the exact `use Dancer2`
//!    sites and their parsed import evidence — this module adds no grammar;
//! 2. the runtime supplies a [`RuntimeDancer2Module`]: the `Dancer2` module
//!    resolved through the request's effective `@INC` plus its declared
//!    `$VERSION`, read by the bounded generic module-metadata scanner
//!    [`read_declared_module_version`] (standard Perl module version
//!    declaration, not Dancer2 DSL grammar);
//! 3. [`detect_dancer2`] and [`dancer2_activation_facts`] — the canonical
//!    producers — decide detection and exactness. Without a resolved module
//!    with version evidence, detection stays `Unsupported` and no package
//!    activates: every provider cell then returns zero framework output.
//!
//! Receipt identities are honestly labeled with the runtime seam that
//! produced them (`lsp-inc-resolution.v1` / `lsp-workspace.v1`), and the
//! source generation is derived from the document content digest so every
//! edit moves the generation and stale facts cannot survive a re-query.

use perl_parser_core::Node;
use perl_semantic_analyzer::analysis::dancer2_activation::extract_dancer2_activation_sites;
use perl_semantic_facts::framework::{
    AdapterDetectionInput, AdapterDetectionResult, DetectionEvidenceClass,
    ModuleActivationIdentity, ModuleSelectorEvaluation, ModuleSelectorOutcome,
    ModuleVersionEvidence,
};
use perl_semantic_facts::framework_adapters::dancer2::{
    Dancer2ActivationFacts, Dancer2ActivationState, Dancer2ImportEvidence,
    dancer2_activation_facts, dancer2_descriptor, detect_dancer2,
};
use perl_semantic_facts::{FileId, SourceGeneration};
use std::collections::HashMap;

/// Resolver identity recorded on runtime observation receipts.
pub const RUNTIME_RESOLVER_IDENTITY: &str = "lsp-inc-resolution.v1";

/// Project environment identity recorded on runtime observation receipts.
pub const RUNTIME_ENVIRONMENT_IDENTITY: &str = "lsp-workspace.v1";

/// One resolved `Dancer2` module observation from the runtime.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDancer2Module {
    /// Filesystem path the request's effective `@INC` resolved `Dancer2` to.
    pub resolved_path: String,
    /// Declared `$VERSION` of the resolved module (observed version evidence).
    pub declared_version: String,
}

impl RuntimeDancer2Module {
    /// Construct a runtime module observation.
    #[must_use]
    pub fn new(resolved_path: impl Into<String>, declared_version: impl Into<String>) -> Self {
        Self { resolved_path: resolved_path.into(), declared_version: declared_version.into() }
    }
}

/// Read a standard declared module version from Perl source.
///
/// Recognizes the standard module version declaration forms
/// `our $VERSION = '1.234';`, `our $VERSION = "v1.2.3";`, and the unquoted
/// `our $VERSION = 1.234;`. This is generic Perl module metadata (the
/// CPAN convention), not framework grammar: it never interprets DSL
/// keywords. Returns `None` when no standard declaration is present.
#[must_use]
pub fn read_declared_module_version(pm_source: &str) -> Option<String> {
    let mut in_pod = false;
    for line in pm_source.lines() {
        let trimmed = line.trim_start();
        if in_pod {
            if trimmed.starts_with("=cut") {
                in_pod = false;
            }
            continue;
        }
        if trimmed.starts_with('=') && !trimmed.starts_with("==") && trimmed.len() > 1 {
            in_pod = true;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("our $VERSION") {
            let rest = rest.trim_start();
            let Some(rest) = rest.strip_prefix('=') else { continue };
            let rest = rest.trim_start();
            let value = rest.split(';').next().unwrap_or("").trim();
            let unquoted = value.trim_matches('\'').trim_matches('"').trim();
            if !unquoted.is_empty()
                && unquoted
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            {
                return Some(unquoted.to_string());
            }
            continue;
        }
    }
    None
}

/// Canonical activation facts for one activating package of one document.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2PackageActivation {
    /// Activating package (application identity scope).
    pub package: String,
    /// Canonical activation facts (exactness, DSL selection, keyword imports).
    pub facts: Dancer2ActivationFacts,
}

/// Per-document canonical activation state over all activation sites.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct Dancer2FileActivations {
    /// Activation facts per activating package, in source order.
    pub packages: Vec<Dancer2PackageActivation>,
    /// Canonical detection result behind every activation (one per document).
    pub detection: Option<AdapterDetectionResult>,
    /// Import evidence per activating package (for boundary reporting).
    pub evidence: HashMap<String, Dancer2ImportEvidence>,
    /// Runtime module observation used for detection, when one existed.
    pub module: Option<RuntimeDancer2Module>,
}

impl Dancer2FileActivations {
    /// Whether the document has at least one exact Dancer2 activation.
    #[must_use]
    pub fn has_exact(&self) -> bool {
        self.packages.iter().any(|activation| activation.facts.is_exact())
    }

    /// Activation facts for one package.
    #[must_use]
    pub fn for_package(&self, package: &str) -> Option<&Dancer2PackageActivation> {
        self.packages.iter().find(|activation| activation.package == package)
    }
}

/// Byte offset of the document's first exact `use Dancer2` activation site.
///
/// The anchoring point for position-aware effective-`@INC` evaluation
/// (#12776): only `use lib` / `no lib` operations active at or before this
/// offset may contribute to the request's include roots. Returns `None`
/// when the AST has no activation site, which keeps the cheap in-memory
/// gate (`has_activation_site`) the sole discriminator for skipping all
/// filesystem module resolution on Dancer2-free documents.
#[must_use]
pub fn first_activation_site_offset(ast: &Node) -> Option<usize> {
    let sites = extract_dancer2_activation_sites(ast, FileId(0));
    sites.into_iter().map(|site| site.span_start_byte as usize).min()
}

/// Whether the AST contains any exact `use Dancer2` activation site.
///
/// Cheap in-memory gate: documents without an activation site skip the
/// filesystem module resolution entirely on the provider paths.
#[must_use]
pub fn has_activation_site(ast: &Node) -> bool {
    first_activation_site_offset(ast).is_some()
}

/// Bounded human reason for the current activation state of a package.
#[must_use]
pub fn activation_state_reason(facts: &Dancer2ActivationFacts, has_module: bool) -> String {
    match &facts.state {
        Dancer2ActivationState::Exact { framework_version, .. } => {
            format!("exact Dancer2 activation (version {framework_version})")
        }
        Dancer2ActivationState::DynamicBoundary { reason } => {
            format!("dynamic boundary: {reason}")
        }
        Dancer2ActivationState::NotActivated { reason } => {
            if has_module {
                format!("not activated: {reason}")
            } else {
                "not activated: the Dancer2 module was not resolved with version evidence in \
                 this request's include roots"
                    .to_string()
            }
        }
        _ => "not activated: unknown state".to_string(),
    }
}

fn stable_digest(parts: &[&str]) -> String {
    // Deterministic non-cryptographic digest for receipt labeling. This is
    // evidence identity, not a security boundary.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x1000_0000_01b3);
        }
    }
    format!("fnv1a:{hash:016x}")
}

/// Build the per-document canonical activation state.
///
/// `module` is the runtime observation of the `Dancer2` module for this
/// request; `None` keeps detection `Unsupported` and every package
/// non-activated (zero framework output). `generation` should be derived
/// from the current document content digest by the caller.
#[must_use]
pub fn file_activations(
    ast: &Node,
    file_id: FileId,
    module: Option<&RuntimeDancer2Module>,
    generation: &SourceGeneration,
) -> Dancer2FileActivations {
    let sites = extract_dancer2_activation_sites(ast, file_id);
    let mut activations = Dancer2FileActivations::default();
    if sites.is_empty() {
        return activations;
    }

    let detection = match module {
        Some(module) => {
            let activation =
                ModuleActivationIdentity::new("Dancer2", Some(file_id), generation.clone())
                    .with_observed_version(ModuleVersionEvidence::new(
                        module.declared_version.clone(),
                        generation.clone(),
                    ));
            let observation = perl_semantic_facts::framework::ModuleObservationReceipt::new(
                RUNTIME_RESOLVER_IDENTITY,
                format!("module-file:{}", module.resolved_path),
                RUNTIME_ENVIRONMENT_IDENTITY,
                generation.clone(),
                stable_digest(&[&module.resolved_path, &module.declared_version]),
                vec![ModuleSelectorEvaluation::new(
                    "Dancer2",
                    ModuleSelectorOutcome::Matched {
                        activation,
                        evidence_class: DetectionEvidenceClass::ResolvedModule,
                    },
                )],
            );
            let input = AdapterDetectionInput::new(
                dancer2_descriptor(),
                observation,
                None,
                perl_semantic_facts::framework::AdapterCancellation::active(),
            );
            detect_dancer2(&input)
        }
        None => {
            // No runtime module observation: the required selector was never
            // evaluated, which the registry contract records as unavailable.
            let observation = perl_semantic_facts::framework::ModuleObservationReceipt::new(
                RUNTIME_RESOLVER_IDENTITY,
                "unresolved:Dancer2",
                RUNTIME_ENVIRONMENT_IDENTITY,
                generation.clone(),
                stable_digest(&["unresolved"]),
                vec![ModuleSelectorEvaluation::new(
                    "Dancer2",
                    ModuleSelectorOutcome::Unavailable {
                        reason: "no runtime module resolution".to_string(),
                    },
                )],
            );
            let input = AdapterDetectionInput::new(
                dancer2_descriptor(),
                observation,
                None,
                perl_semantic_facts::framework::AdapterCancellation::active(),
            );
            detect_dancer2(&input)
        }
    };

    for site in &sites {
        let package = site.package.clone().unwrap_or_else(|| "main".to_string());
        let facts = dancer2_activation_facts(&detection, site.package.as_deref(), &site.evidence);
        if activations.for_package(&package).is_none() {
            activations
                .packages
                .push(Dancer2PackageActivation { package: package.clone(), facts: facts.clone() });
            activations.evidence.insert(package, site.evidence.clone());
        }
    }
    activations.detection = Some(detection);
    activations.module = module.cloned();
    activations
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_semantic_analyzer::Parser;
    use perl_semantic_facts::framework_adapters::dancer2::{
        Dancer2KeywordState, DslSelection, parse_dancer2_import_args,
    };
    use perl_test_must::{must_some_with, must_with};

    fn parse(source: &str) -> Node {
        let mut parser = Parser::new(source);
        must_with(parser.parse(), "fixture must parse")
    }

    const VERSIONED_MODULE: &str = "package Dancer2;\nour $VERSION = '1.1.1';\n1;\n";

    #[test]
    fn read_declared_module_version_handles_standard_forms() {
        assert_eq!(read_declared_module_version(VERSIONED_MODULE).as_deref(), Some("1.1.1"));
        assert_eq!(
            read_declared_module_version("our $VERSION = \"1.234\";").as_deref(),
            Some("1.234")
        );
        assert_eq!(
            read_declared_module_version("our $VERSION = 1.001002;").as_deref(),
            Some("1.001002")
        );
        assert_eq!(read_declared_module_version("our $VERSION = version->new(1.2);"), None);
        assert_eq!(read_declared_module_version("package Dancer2;\n1;"), None);
        // POD is skipped, not treated as source.
        assert_eq!(
            read_declared_module_version(
                "=pod\nour $VERSION = '9.9';\n=cut\nour $VERSION = '1.1';"
            )
            .as_deref(),
            Some("1.1")
        );
    }

    #[test]
    fn no_module_observation_yields_no_exact_activation() {
        let source = "use Dancer2;\nget '/x' => sub { 1 };\n";
        let ast = parse(source);
        let activations =
            file_activations(&ast, FileId(1), None, &SourceGeneration::known("gen-test"));
        assert!(!activations.has_exact(), "no resolved module: no exact activation");
        assert!(
            activations.packages.iter().all(|p| !p.facts.is_exact()),
            "zero framework output without #8914 activation evidence"
        );
    }

    #[test]
    fn versioned_module_yields_exact_activation_with_keyword_facts() {
        let source = "use Dancer2;\n";
        let ast = parse(source);
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "1.1.1");
        let activations =
            file_activations(&ast, FileId(1), Some(&module), &SourceGeneration::known("gen-test"));
        assert!(activations.has_exact());
        let facts = &must_some_with(activations.for_package("main"), "main activation").facts;
        assert_eq!(facts.dsl, DslSelection::Default);
        assert!(
            facts
                .keywords
                .iter()
                .any(|k| k.keyword == "get" && k.state == Dancer2KeywordState::Imported)
        );
    }

    #[test]
    fn exclusion_is_honored_in_keyword_import_facts() {
        let source = "use Dancer2 '!get';\n";
        let ast = parse(source);
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "1.1.1");
        let activations =
            file_activations(&ast, FileId(1), Some(&module), &SourceGeneration::known("gen-test"));
        let facts = &must_some_with(activations.for_package("main"), "main activation").facts;
        let get = must_some_with(facts.keywords.iter().find(|k| k.keyword == "get"), "get fact");
        assert_eq!(get.state, Dancer2KeywordState::Excluded);
        let post = must_some_with(facts.keywords.iter().find(|k| k.keyword == "post"), "post fact");
        assert_eq!(post.state, Dancer2KeywordState::Imported);
    }

    #[test]
    fn dancer2_core_import_is_not_an_activation_site() {
        let source = "use Dancer2::Core;\n";
        let ast = parse(source);
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "1.1.1");
        let activations =
            file_activations(&ast, FileId(1), Some(&module), &SourceGeneration::known("gen-test"));
        assert!(activations.packages.is_empty(), "Dancer2::Core is not DSL activation");
    }

    #[test]
    fn custom_dsl_is_a_dynamic_boundary_not_exact() {
        let args: Vec<String> = ["dsl", "'My::DSL'"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_import_args(&args);
        assert!(matches!(evidence.dsl, Some(DslSelection::CustomLiteral(_))));
    }

    #[test]
    fn custom_dsl_with_versioned_module_is_not_exact() {
        // A custom DSL owns its keyword vocabulary: even with a resolved
        // versioned Dancer2 module the activation stays a dynamic boundary.
        let source = "use Dancer2 dsl => 'My::DSL';
";
        let mut parser = Parser::new(source);
        let ast = must_with(parser.parse(), "fixture must parse");
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "1.1.1");
        let activations =
            file_activations(&ast, FileId(1), Some(&module), &SourceGeneration::known("gen-test"));
        assert!(
            !activations.has_exact(),
            "custom DSL with version evidence must not become an exact activation"
        );
        let facts = &must_some_with(activations.for_package("main"), "main activation").facts;
        assert!(facts.keywords.is_empty(), "default keyword facts are not inherited");
        assert!(
            matches!(
                facts.state,
                perl_semantic_facts::framework_adapters::dancer2::Dancer2ActivationState::DynamicBoundary { .. }
            ),
            "custom DSL is a dynamic boundary"
        );
    }

    #[test]
    fn activation_state_reason_distinguishes_missing_evidence() {
        let source = "use Dancer2;\n";
        let ast = parse(source);
        let without_module = file_activations(&ast, FileId(1), None, &SourceGeneration::known("g"));
        let facts = &must_some_with(without_module.for_package("main"), "main").facts;
        let reason = activation_state_reason(facts, false);
        assert!(reason.contains("not resolved with version evidence"), "{reason}");
    }
}
