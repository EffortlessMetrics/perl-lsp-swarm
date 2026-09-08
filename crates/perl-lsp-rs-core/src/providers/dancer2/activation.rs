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
use perl_semantic_analyzer::analysis::dancer2_two_x_activation::extract_dancer2_two_x_activation_sites;
use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDetectionInput, AdapterDetectionResult, DetectionEvidenceClass,
    ModuleActivationIdentity, ModuleObservationReceipt, ModuleSelectorEvaluation,
    ModuleSelectorOutcome, ModuleVersionEvidence,
};
use perl_semantic_facts::framework_adapters::dancer2::{
    Dancer2ActivationFacts, Dancer2ActivationState, Dancer2ImportEvidence,
    dancer2_activation_facts, dancer2_descriptor, detect_dancer2,
};
use perl_semantic_facts::framework_adapters::dancer2_two_x::{
    Dancer2TwoXActivationFacts, Dancer2TwoXActivationState, dancer2_two_x_activation_facts,
    dancer2_two_x_descriptor, detect_dancer2_two_x,
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
    /// 2.x activation facts per activating package (#14989): populated only
    /// when the resolved Dancer2 version satisfies the pinned 2.x contract
    /// and the 2.x detector accepts the observation. Comparison-only output
    /// (the 2.x adapter stays `Shadow` until the disposition decision) —
    /// never a publication authority.
    pub two_x_packages: Vec<Dancer2TwoXPackageActivation>,
    /// The 2.x adapter's own detection result for this document (#14989).
    /// Distinct from `detection` (the 1.x verdict): a 2.x-resolved module is
    /// detected by the 2.x contract and refused by the 1.x constraint.
    pub two_x_detection: Option<AdapterDetectionResult>,
}

/// One 2.x activating package with its typed facts (#14989).
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Dancer2TwoXPackageActivation {
    /// Activating package (application identity scope).
    pub package: String,
    /// Typed 2.x activation facts (state, DSL, keyword states with
    /// route-handler scope).
    pub facts: Dancer2TwoXActivationFacts,
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
    source: &str,
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

    // Dual-adapter arbitration (#14989): the resolved version decides which
    // contract owns the document. A 2.x-resolved module fails the 1.x
    // adapter's constraint above (its facts stay NotActivated), so the 2.x
    // path below is the only contract that can speak for such a document.
    let (two_x_packages, two_x_detection) =
        two_x_activations(ast, source, file_id, module, generation);
    activations.two_x_packages = two_x_packages;
    activations.two_x_detection = two_x_detection;
    activations
}

/// Build the 2.x activation facts for one document: only a resolved module
/// whose declared version satisfies the pinned 2.x constraint admits the 2.x
/// detector, and only a generation-reconciled observation reaches it. No
/// runtime observation or an out-of-range version leaves the carrier empty —
/// the 1.x facts above stay the honest record for those documents.
fn two_x_activations(
    ast: &Node,
    source: &str,
    file_id: FileId,
    module: Option<&RuntimeDancer2Module>,
    generation: &SourceGeneration,
) -> (Vec<Dancer2TwoXPackageActivation>, Option<AdapterDetectionResult>) {
    let Some(module) = module else {
        return (Vec::new(), None);
    };
    // Version arbitration runs before any extraction work: the pinned 2.x
    // constraint decides ownership of the document.
    if perl_semantic_facts::framework::version_constraint_matches(
        perl_semantic_facts::framework_adapters::dancer2_two_x::DANCER2_TWO_X_VERSION_CONSTRAINT,
        &module.declared_version,
    ) != Some(true)
    {
        return (Vec::new(), None);
    }
    let sites = extract_dancer2_two_x_activation_sites(ast, source, file_id, generation.clone());
    if sites.is_empty() {
        // No 2.x activation sites: no facts to mint and no 2.x detection
        // result worth carrying.
        return (Vec::new(), None);
    }
    let activation = ModuleActivationIdentity::new("Dancer2", Some(file_id), generation.clone())
        .with_observed_version(ModuleVersionEvidence::new(
            module.declared_version.clone(),
            generation.clone(),
        ));
    let observation = ModuleObservationReceipt::new(
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
        dancer2_two_x_descriptor(),
        observation,
        None,
        AdapterCancellation::active(),
    );
    let detection = detect_dancer2_two_x(&input);
    let detection_clone = detection.clone();
    // Multiple imports in one package fold by pinned import semantics
    // (#15006 review): an odd-arity site dies compilation and dominates; a
    // suppressed site contributes nothing; keyword states fold per keyword
    // (exclusion cannot uninstall a prior install, and a package-sub shadow
    // beats both); the first exact app identity stands.
    let mut packages: Vec<Dancer2TwoXPackageActivation> = Vec::new();
    for site in &sites {
        let package = site.package.clone().unwrap_or_else(|| "main".to_string());
        let facts = dancer2_two_x_activation_facts(
            &detection,
            site.package.as_deref(),
            &site.evidence,
            &site.shadowed_keywords,
        );
        match packages.iter_mut().find(|existing| existing.package == package) {
            Some(existing) => fold_package_facts(&mut existing.facts, facts),
            None => packages.push(Dancer2TwoXPackageActivation { package, facts }),
        }
    }
    (packages, Some(detection_clone))
}

/// Fold one more import's facts into a package's record under the pinned
/// import semantics.
fn fold_package_facts(
    existing: &mut Dancer2TwoXActivationFacts,
    incoming: Dancer2TwoXActivationFacts,
) {
    use perl_semantic_facts::framework_adapters::dancer2_two_x::Dancer2TwoXKeywordState;

    // A compile-time die anywhere in the package dominates the record:
    // an odd-arity import aborts compilation, so an incoming die replaces
    // whatever ran before it, and an earlier die means later imports never
    // ran at all (#15006 review).
    if matches!(incoming.state, Dancer2TwoXActivationState::ImportDied { .. }) {
        *existing = incoming;
        return;
    }
    if matches!(existing.state, Dancer2TwoXActivationState::ImportDied { .. }) {
        return;
    }
    // An exact incoming import folds into an exact record; a non-exact
    // incoming import never downgrades an exact one; two inactive records
    // keep the later reason.
    if !incoming.is_exact() {
        if !existing.is_exact() {
            *existing = incoming;
        }
        return;
    }
    if !existing.is_exact() {
        *existing = incoming;
        return;
    }
    let mut keywords = std::mem::take(&mut existing.keywords);
    for keyword in &mut keywords {
        let incoming_state = incoming
            .keywords
            .iter()
            .find(|candidate| candidate.keyword == keyword.keyword)
            .map(|candidate| candidate.state);
        keyword.state = match (keyword.state, incoming_state) {
            // Exclusion cannot uninstall a prior install.
            (Dancer2TwoXKeywordState::Imported, Some(Dancer2TwoXKeywordState::Excluded))
            | (Dancer2TwoXKeywordState::Excluded, Some(Dancer2TwoXKeywordState::Imported)) => {
                Dancer2TwoXKeywordState::Imported
            }
            (Dancer2TwoXKeywordState::Excluded, Some(Dancer2TwoXKeywordState::Excluded)) => {
                Dancer2TwoXKeywordState::Excluded
            }
            (Dancer2TwoXKeywordState::Shadowed, _)
            | (_, Some(Dancer2TwoXKeywordState::Shadowed)) => Dancer2TwoXKeywordState::Shadowed,
            (current, _) => current,
        };
    }
    existing.keywords = keywords;
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
            file_activations(&ast, source, FileId(1), None, &SourceGeneration::known("gen-test"));
        assert!(!activations.has_exact(), "no resolved module: no exact activation");
        assert!(
            activations.packages.iter().all(|p| !p.facts.is_exact()),
            "zero framework output without #8914 activation evidence"
        );
        // No runtime observation: the 2.x carrier stays empty as well — the
        // 2.x contract never speaks without resolved version evidence
        // (#14989).
        assert!(activations.two_x_packages.is_empty());
    }

    /// Dual-adapter arbitration (#14989): a 2.x-resolved module routes the
    /// document to the 2.x contract — the 1.x facts refuse on the version
    /// constraint while the 2.x facts activate with typed keyword scope.
    #[test]
    fn two_x_resolved_module_arbitrates_to_the_two_x_contract() {
        let source = "package App;\nuse Dancer2;\n";
        let ast = parse(source);
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "2.0.1");
        let activations = file_activations(
            &ast,
            source,
            FileId(1),
            Some(&module),
            &SourceGeneration::known("gen-test"),
        );
        // The 1.x contract refuses the 2.x module explicitly.
        assert!(
            activations.packages.iter().all(|p| !p.facts.is_exact()),
            "a 2.x module can never satisfy the 1.x contract"
        );
        // The 2.x contract activates with route-handler scope on get.
        assert_eq!(activations.two_x_packages.len(), 1, "the 2.x path owns the document");
        let two_x = &activations.two_x_packages[0];
        assert_eq!(two_x.package, "App");
        assert!(two_x.facts.is_exact(), "2.0.1 satisfies the pinned 2.x constraint");
        let get = must_some_with(
            two_x.facts.keywords.iter().find(|k| k.keyword == "get"),
            "get fact on the 2.x contract",
        );
        assert!(matches!(
            get.state,
            perl_semantic_facts::framework_adapters::dancer2_two_x::Dancer2TwoXKeywordState::Imported
        ));
    }

    /// A 1.x-resolved module never reaches the 2.x carrier: version
    /// arbitration gates the 2.x path before extraction.
    #[test]
    fn one_x_resolved_module_never_yields_two_x_facts() {
        let source = "package App;\nuse Dancer2;\n";
        let ast = parse(source);
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "1.1.1");
        let activations = file_activations(
            &ast,
            source,
            FileId(1),
            Some(&module),
            &SourceGeneration::known("gen-test"),
        );
        assert!(activations.has_exact(), "the 1.x contract owns a 1.x module");
        assert!(
            activations.two_x_packages.is_empty(),
            "a 1.x module can never reach the 2.x contract"
        );
    }

    /// A suppressed first import never hides a later activating import in
    /// the same package: the activating site owns the package's evidence
    /// (#15006 review).
    #[test]
    fn later_activating_import_overrides_an_earlier_suppressed_one() {
        let source = "package App;
use Dancer2 ();
use Dancer2;
";
        let ast = parse(source);
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "2.0.1");
        let activations = file_activations(
            &ast,
            source,
            FileId(1),
            Some(&module),
            &SourceGeneration::known("gen-test"),
        );
        assert_eq!(activations.two_x_packages.len(), 1, "one package, one record");
        assert!(
            activations.two_x_packages[0].facts.is_exact(),
            "the later bare import activates: got {:?}",
            activations.two_x_packages[0].facts.state
        );
    }

    /// The reverse order: a suppressed import after an activating one never
    /// erases the earlier activation (un-overwrite preserves the keywords).
    #[test]
    fn later_suppressed_import_never_erases_an_earlier_activation() {
        let source = "package App;
use Dancer2;
use Dancer2 ();
";
        let ast = parse(source);
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "2.0.1");
        let activations = file_activations(
            &ast,
            source,
            FileId(1),
            Some(&module),
            &SourceGeneration::known("gen-test"),
        );
        assert_eq!(activations.two_x_packages.len(), 1);
        assert!(
            activations.two_x_packages[0].facts.is_exact(),
            "the suppressed re-import cannot uninstall: got {:?}",
            activations.two_x_packages[0].facts.state
        );
    }

    /// Fold semantics: `use Dancer2 '!get';` followed by a bare re-import
    /// re-installs `get` (the second import's export map has no exclusion
    /// and the glob was empty).
    #[test]
    fn fold_excluded_then_bare_import_installs_the_keyword() {
        let source = "package App;
use Dancer2 '!get';
use Dancer2;
";
        let ast = parse(source);
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "2.0.1");
        let activations = file_activations(
            &ast,
            source,
            FileId(1),
            Some(&module),
            &SourceGeneration::known("gen-test"),
        );
        assert_eq!(activations.two_x_packages.len(), 1);
        let get = must_some_with(
            activations.two_x_packages[0].facts.keywords.iter().find(|k| k.keyword == "get"),
            "get fact",
        );
        assert!(
            matches!(
                get.state,
                perl_semantic_facts::framework_adapters::dancer2_two_x::Dancer2TwoXKeywordState::Imported
            ),
            "the later bare import installs get: {:?}",
            get.state
        );
    }

    /// Fold semantics: a bare import followed by an exclusion cannot
    /// uninstall the already-installed keyword.
    #[test]
    fn fold_bare_then_excluded_import_keeps_the_keyword() {
        let source = "package App;
use Dancer2;
use Dancer2 '!get';
";
        let ast = parse(source);
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "2.0.1");
        let activations = file_activations(
            &ast,
            source,
            FileId(1),
            Some(&module),
            &SourceGeneration::known("gen-test"),
        );
        assert_eq!(activations.two_x_packages.len(), 1);
        let get = must_some_with(
            activations.two_x_packages[0].facts.keywords.iter().find(|k| k.keyword == "get"),
            "get fact",
        );
        assert!(
            matches!(
                get.state,
                perl_semantic_facts::framework_adapters::dancer2_two_x::Dancer2TwoXKeywordState::Imported
            ),
            "exclusion cannot uninstall an installed keyword: {:?}",
            get.state
        );
    }

    /// A 2.x-resolved module with a same-package `sub get` records the
    /// shadow on the 2.x keyword facts (the un-overwrite rule, 2.x contract).
    #[test]
    fn two_x_facts_honor_the_un_overwrite_rule() {
        let source = "package App;\nsub get { 1 }\nuse Dancer2;\n";
        let ast = parse(source);
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "2.0.1");
        let activations = file_activations(
            &ast,
            source,
            FileId(1),
            Some(&module),
            &SourceGeneration::known("gen-test"),
        );
        assert_eq!(activations.two_x_packages.len(), 1);
        let two_x = &activations.two_x_packages[0];
        let get =
            must_some_with(two_x.facts.keywords.iter().find(|k| k.keyword == "get"), "get fact");
        assert!(
            matches!(
                get.state,
                perl_semantic_facts::framework_adapters::dancer2_two_x::Dancer2TwoXKeywordState::Shadowed
            ),
            "a pre-import same-package definition shadows the 2.x keyword"
        );
    }

    #[test]
    fn versioned_module_yields_exact_activation_with_keyword_facts() {
        let source = "use Dancer2;\n";
        let ast = parse(source);
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "1.1.1");
        let activations = file_activations(
            &ast,
            source,
            FileId(1),
            Some(&module),
            &SourceGeneration::known("gen-test"),
        );
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
        let activations = file_activations(
            &ast,
            source,
            FileId(1),
            Some(&module),
            &SourceGeneration::known("gen-test"),
        );
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
        let activations = file_activations(
            &ast,
            source,
            FileId(1),
            Some(&module),
            &SourceGeneration::known("gen-test"),
        );
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
        let activations = file_activations(
            &ast,
            source,
            FileId(1),
            Some(&module),
            &SourceGeneration::known("gen-test"),
        );
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
        let without_module =
            file_activations(&ast, source, FileId(1), None, &SourceGeneration::known("g"));
        let facts = &must_some_with(without_module.for_package("main"), "main").facts;
        let reason = activation_state_reason(facts, false);
        assert!(reason.contains("not resolved with version evidence"), "{reason}");
    }
}
