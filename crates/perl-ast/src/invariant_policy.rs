//! Exhaustive invariant policy metadata for every primary AST node kind.
//!
//! This registry is deliberately separate from child traversal. Structural
//! children continue to come from [`crate::Node::try_for_each_child_with_field`];
//! the registry states which range, source, payload, synthetic, and child
//! policies apply to each stable `NodeKind` identity.

/// Structural role assigned to one `NodeKind` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstNodeClassification {
    /// A source node with no structural AST children.
    Leaf,
    /// A node that may own one or more structural children.
    ChildBearing,
    /// A node whose primary role is wrapping one optional or required child.
    Wrapper,
    /// A synthetic or recovery node.
    Recovery,
    /// A node representing a specialized or opaque source region.
    SourceBoundary,
}

impl AstNodeClassification {
    /// Whether an observed instance may expose structural children.
    #[must_use]
    pub const fn permits_children(self) -> bool {
        matches!(self, Self::ChildBearing | Self::Wrapper | Self::Recovery)
    }
}

/// How a node range and payload relate to source bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstSourceBacking {
    /// Range and registered payload anchors are source-exact.
    Exact,
    /// Range is source-exact while payload may use a registered normalization.
    Normalized,
    /// Node is synthetic and does not claim an exact source payload.
    Synthetic,
    /// Node combines source-backed and synthetic or opaque regions.
    Mixed,
}

/// Policy for zero-width ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstEmptyRangePolicy {
    /// The selected validation profile decides whether an empty range is valid.
    ProfileControlled,
    /// Empty ranges are permitted only for registered synthetic/recovery nodes.
    SyntheticAllowed,
}

/// Parent/child containment policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstChildContainmentPolicy {
    /// Every structural child must remain inside its parent range.
    Required,
    /// The variant has no structural-child containment contract.
    NotApplicable,
}

/// Direct-child source ordering policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstChildOrderPolicy {
    /// Direct children must have nondecreasing source starts.
    Nondecreasing,
    /// The variant has no direct-child ordering contract.
    NotApplicable,
}

/// Direct-child overlap policy registered for later variant-specific proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstChildOverlapPolicy {
    /// Direct children are expected to be source-disjoint.
    Disjoint,
    /// Overlap may be valid and requires a stronger variant-specific rule.
    MayOverlap,
    /// The variant has no direct-child overlap contract.
    NotApplicable,
}

/// Stable source-derived payload policy hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstPayloadPolicy {
    /// Identifier spelling must agree with its source anchor.
    IdentifierExact,
    /// Variable sigil, package qualification, and name must agree with source.
    VariableSigilAndName,
    /// Operator text must be exact or use one registered canonicalization.
    OperatorExactOrCanonical,
    /// Raw and cooked literal fields follow a registered normalization contract.
    LiteralRawAndCooked,
    /// Quote/regex family and delimiter metadata must retain source identity.
    QuoteDelimiterMetadata,
    /// Heredoc label, indentation form, body, and terminator anchors are checked.
    HeredocLabelAndIndent,
    /// Declaration or call name fields require an exact source anchor.
    DeclarationNameAnchor,
    /// Recovery/synthetic payload must not claim ordinary source exactness.
    RecoverySynthetic,
    /// Payload is preserved as an explicitly opaque source region.
    OpaqueSourceRegion,
}

/// Complete invariant policy row for one stable `NodeKind` identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AstNodePolicy {
    /// Stable [`crate::NodeKind::kind_name`] token.
    pub kind_name: &'static str,
    /// Structural role.
    pub classification: AstNodeClassification,
    /// Source-backing contract.
    pub source_backing: AstSourceBacking,
    /// Empty-range contract.
    pub empty_range: AstEmptyRangePolicy,
    /// Parent/child containment contract.
    pub child_containment: AstChildContainmentPolicy,
    /// Direct-child ordering contract.
    pub child_order: AstChildOrderPolicy,
    /// Direct-child overlap contract.
    pub child_overlap: AstChildOverlapPolicy,
    /// Source-derived payload policies executed by the payload-validation layer.
    pub payload_policies: &'static [AstPayloadPolicy],
}

macro_rules! policy {
    (
        $name:literal,
        $classification:ident,
        $source:ident,
        $empty:ident,
        $containment:ident,
        $order:ident,
        $overlap:ident,
        $payloads:expr
    ) => {
        AstNodePolicy {
            kind_name: $name,
            classification: AstNodeClassification::$classification,
            source_backing: AstSourceBacking::$source,
            empty_range: AstEmptyRangePolicy::$empty,
            child_containment: AstChildContainmentPolicy::$containment,
            child_order: AstChildOrderPolicy::$order,
            child_overlap: AstChildOverlapPolicy::$overlap,
            payload_policies: $payloads,
        }
    };
}

/// Version of the invariant policy contract.
pub const AST_NODE_POLICY_SCHEMA_VERSION: u32 = 1;

/// One policy row for every member of [`crate::NodeKind::ALL_KIND_NAMES`].
///
/// The table is intentionally one row per line so a reviewer can diff a single
/// variant's policy against its neighbours; rustfmt would otherwise expand each
/// row across eight lines and hide that alignment.
#[rustfmt::skip]
pub const AST_NODE_POLICIES: &[AstNodePolicy] = &[
    policy!("Program", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, Disjoint, &[]),
    policy!("ExpressionStatement", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("VariableDeclaration", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("VariableListDeclaration", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, Disjoint, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("NestedVariableList", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, Disjoint, &[]),
    policy!("Variable", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::VariableSigilAndName]),
    policy!("VariableWithAttributes", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("Assignment", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::OperatorExactOrCanonical]),
    policy!("Binary", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::OperatorExactOrCanonical]),
    policy!("ArraySlice", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("HashSlice", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("KeyValueSlice", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("ChainedComparison", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, Disjoint, &[AstPayloadPolicy::OperatorExactOrCanonical]),
    policy!("Ternary", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("Unary", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::OperatorExactOrCanonical]),
    policy!("Diamond", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[]),
    policy!("Ellipsis", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[]),
    policy!("Undef", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[]),
    policy!("Readline", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::IdentifierExact]),
    policy!("Glob", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::OpaqueSourceRegion]),
    policy!("Typeglob", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::IdentifierExact]),
    policy!("Number", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::LiteralRawAndCooked]),
    policy!("String", Leaf, Normalized, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::LiteralRawAndCooked]),
    policy!("VString", Leaf, Normalized, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::LiteralRawAndCooked]),
    policy!("Heredoc", SourceBoundary, Mixed, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::HeredocLabelAndIndent, AstPayloadPolicy::OpaqueSourceRegion]),
    policy!("ArrayLiteral", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, Disjoint, &[]),
    policy!("HashLiteral", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("Block", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, Disjoint, &[]),
    policy!("Eval", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("Do", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("Defer", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("Try", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("If", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("LabeledStatement", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("While", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::OperatorExactOrCanonical]),
    policy!("Tie", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("Untie", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("For", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("Foreach", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("Given", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("When", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("Default", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("StatementModifier", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::OperatorExactOrCanonical]),
    policy!("Subroutine", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("Prototype", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::OpaqueSourceRegion]),
    policy!("Signature", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, Disjoint, &[]),
    policy!("MandatoryParameter", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("OptionalParameter", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("SlurpyParameter", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("NamedParameter", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("Method", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("Return", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[]),
    policy!("LoopControl", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::OperatorExactOrCanonical, AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("Goto", Wrapper, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::OperatorExactOrCanonical]),
    policy!("MethodCall", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, Disjoint, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("FunctionCall", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, Disjoint, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("AmperCall", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, Disjoint, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("IndirectCall", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("Regex", Leaf, Normalized, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::QuoteDelimiterMetadata, AstPayloadPolicy::OpaqueSourceRegion]),
    policy!("Match", ChildBearing, Normalized, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::QuoteDelimiterMetadata, AstPayloadPolicy::OpaqueSourceRegion]),
    policy!("Substitution", ChildBearing, Normalized, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::QuoteDelimiterMetadata, AstPayloadPolicy::OpaqueSourceRegion]),
    policy!("Transliteration", ChildBearing, Normalized, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::QuoteDelimiterMetadata, AstPayloadPolicy::OpaqueSourceRegion]),
    policy!("Package", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("Use", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("No", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("PhaseBlock", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("DataSection", SourceBoundary, Mixed, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::OpaqueSourceRegion]),
    policy!("Class", ChildBearing, Exact, ProfileControlled, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::DeclarationNameAnchor]),
    policy!("Format", SourceBoundary, Mixed, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::DeclarationNameAnchor, AstPayloadPolicy::OpaqueSourceRegion]),
    policy!("Identifier", Leaf, Exact, ProfileControlled, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::IdentifierExact]),
    policy!("Error", Recovery, Mixed, SyntheticAllowed, Required, Nondecreasing, MayOverlap, &[AstPayloadPolicy::RecoverySynthetic]),
    policy!("MissingExpression", Recovery, Synthetic, SyntheticAllowed, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::RecoverySynthetic]),
    policy!("MissingStatement", Recovery, Synthetic, SyntheticAllowed, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::RecoverySynthetic]),
    policy!("MissingIdentifier", Recovery, Synthetic, SyntheticAllowed, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::RecoverySynthetic]),
    policy!("MissingBlock", Recovery, Synthetic, SyntheticAllowed, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::RecoverySynthetic]),
    policy!("UnknownRest", Recovery, Synthetic, SyntheticAllowed, NotApplicable, NotApplicable, NotApplicable, &[AstPayloadPolicy::RecoverySynthetic]),
];

/// Return the registered policy for a stable `NodeKind` name.
#[must_use]
pub fn ast_node_policy(kind_name: &str) -> Option<&'static AstNodePolicy> {
    AST_NODE_POLICIES.iter().find(|policy| policy.kind_name == kind_name)
}

/// Return all registered policies in canonical declaration order.
#[must_use]
pub const fn all_ast_node_policies() -> &'static [AstNodePolicy] {
    AST_NODE_POLICIES
}

/// Return whether a policy is compatible with an observed child-bearing node.
///
/// This helper exists so negative tests can falsify a misclassified policy
/// without inventing a second child traversal.
#[must_use]
pub const fn policy_accepts_observed_children(
    policy: &AstNodePolicy,
    observed_children: bool,
) -> bool {
    !observed_children || policy.classification.permits_children()
}
