//! Exact source bytes and validated edit plans for the native formatter.
use super::{FormatExecutionStrategy, FormatRequestTarget};
use perl_source_identity::{ContentDigest, ContentRevision, LogicalSourceId, SourceGeneration};
use serde::{Deserialize, Serialize};

/// Current byte edit-plan schema. Unknown versions never authorize application.
pub const FORMAT_EDIT_PLAN_SCHEMA: u32 = 1;

/// Exact content and optional caller-owned logical subject/currentness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatSourceIdentity {
    /// Digest produced by the shared source-identity owner.
    pub content: ContentDigest,
    /// Canonical logical identity, when the caller actually has one.
    pub logical_source: Option<LogicalSourceId>,
    /// Unknown is explicit and does not establish currentness.
    pub generation: SourceGeneration,
}

impl FormatSourceIdentity {
    /// Identify exact content without inventing a file or generation identity.
    #[must_use]
    pub fn content_only(source: &str) -> Self {
        Self {
            content: ContentDigest::of_bytes(source.as_bytes()),
            logical_source: None,
            generation: SourceGeneration::Unknown,
        }
    }

    /// Return a canonical content revision only when a logical source is known.
    #[must_use]
    pub fn revision(&self) -> Option<ContentRevision> {
        self.logical_source
            .clone()
            .map(|logical| ContentRevision::new(logical, self.content.clone()))
    }
}

/// Immutable exact text whose identity has been checked against its bytes.
#[derive(Debug, Clone)]
pub struct ExactSource<'a> {
    text: &'a str,
    identity: FormatSourceIdentity,
}

impl<'a> ExactSource<'a> {
    /// Admit source text with an explicitly unknown logical subject/generation.
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self { text, identity: FormatSourceIdentity::content_only(text) }
    }

    /// Admit caller-owned identity only when its digest names these exact bytes.
    pub fn with_identity(
        text: &'a str,
        identity: FormatSourceIdentity,
    ) -> Result<Self, EditPlanError> {
        if ContentDigest::of_bytes(text.as_bytes()) != identity.content {
            return Err(EditPlanError::StaleSource);
        }
        Ok(Self { text, identity })
    }

    /// Exact immutable source bytes, represented as valid UTF-8 text.
    #[must_use]
    pub fn text(&self) -> &'a str {
        self.text
    }

    /// Identity bound to the immutable text.
    #[must_use]
    pub fn identity(&self) -> &FormatSourceIdentity {
        &self.identity
    }

    /// The document target is exactly `0..source.len()`.
    #[must_use]
    pub fn whole_document(&self) -> SourceByteRange {
        SourceByteRange { source: self.identity.clone(), start: 0, end: self.text.len() }
    }
}

/// A half-open byte interval in one exact source.
///
/// Deserialization is a transport operation, not admission: `validate` is mandatory
/// at the native entry and plan applicator before the range can authorize slicing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceByteRange {
    source: FormatSourceIdentity,
    start: usize,
    end: usize,
}

impl SourceByteRange {
    /// Admit exact source bounds and UTF-8 boundaries without clamping.
    pub fn new(source: &ExactSource<'_>, start: usize, end: usize) -> Result<Self, EditPlanError> {
        let range = Self { source: source.identity.clone(), start, end };
        range.validate(source)?;
        Ok(range)
    }

    /// Exact source identity to which this interval belongs.
    #[must_use]
    pub fn source(&self) -> &FormatSourceIdentity {
        &self.source
    }
    /// Inclusive UTF-8 byte boundary.
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }
    /// Exclusive UTF-8 byte boundary.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }
    /// Whether the admitted interval contains no bytes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Validate identity, order, bounds and scalar boundaries against exact source.
    pub fn validate(&self, source: &ExactSource<'_>) -> Result<(), EditPlanError> {
        if self.source != source.identity {
            return Err(EditPlanError::MixedSource);
        }
        if self.start > self.end {
            return Err(EditPlanError::ReversedRange);
        }
        if self.end > source.text.len() {
            return Err(EditPlanError::OutOfBounds);
        }
        if !source.text.is_char_boundary(self.start) || !source.text.is_char_boundary(self.end) {
            return Err(EditPlanError::NotCharacterBoundary);
        }
        Ok(())
    }

    /// Whether an independently validated interval lies entirely inside this one.
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        self.source == other.source && self.start <= other.start && other.end <= self.end
    }
}

/// Explicit policy provenance for widening a requested interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetWidening {
    /// Named application policy that admitted additional source bytes.
    pub owner: String,
    /// Bounded explanation retained separately from the original request.
    pub reason: String,
}

/// Caller request and the exact byte interval admitted by its policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatRequest {
    /// Original document/range request, never inferred from returned edits.
    pub requested: FormatRequestTarget,
    /// Exact interval inside which edits may occur.
    pub admitted: SourceByteRange,
    /// Explicit provenance when admission expands the original request.
    pub widening: Option<TargetWidening>,
}

impl FormatRequest {
    /// Admit the exact complete document without a coordinate encoding.
    #[must_use]
    pub fn document(source: &ExactSource<'_>) -> Self {
        Self {
            requested: FormatRequestTarget::Document,
            admitted: source.whole_document(),
            widening: None,
        }
    }

    /// Admit exactly the caller's byte range, with no widening.
    pub fn range(source: &ExactSource<'_>, range: SourceByteRange) -> Result<Self, EditPlanError> {
        range.validate(source)?;
        Ok(Self {
            requested: FormatRequestTarget::Range { range: range.clone() },
            admitted: range,
            widening: None,
        })
    }

    /// Revalidate a request received from a transport or another component.
    pub fn validate(&self, source: &ExactSource<'_>) -> Result<(), EditPlanError> {
        self.admitted.validate(source)?;
        let requested = match &self.requested {
            FormatRequestTarget::Document => source.whole_document(),
            FormatRequestTarget::UnadmittedRange => return Err(EditPlanError::InvalidOutcome),
            FormatRequestTarget::Range { range } => {
                range.validate(source)?;
                range.clone()
            }
        };
        if !self.admitted.contains(&requested) {
            return Err(EditPlanError::OutsideTarget);
        }
        let expanded = requested != self.admitted;
        if expanded != self.widening.is_some() {
            return Err(EditPlanError::InvalidWidening);
        }
        if self
            .widening
            .as_ref()
            .is_some_and(|value| value.owner.trim().is_empty() || value.reason.trim().is_empty())
        {
            return Err(EditPlanError::InvalidWidening);
        }
        if matches!(self.requested, FormatRequestTarget::Document)
            && self.admitted != source.whole_document()
        {
            return Err(EditPlanError::OutsideTarget);
        }
        Ok(())
    }
}

/// Replacement over a validated interval in predecessor byte space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatEdit {
    /// Exact predecessor interval.
    pub range: SourceByteRange,
    /// UTF-8 replacement text.
    pub new_text: String,
}

impl FormatEdit {
    /// Construct an edit; full ordering and containment are admitted by the plan.
    #[must_use]
    pub fn new(range: SourceByteRange, new_text: impl Into<String>) -> Self {
        Self { range, new_text: new_text.into() }
    }
}

/// An independently applicable, source-bound native edit plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatEditPlan {
    schema: u32,
    predecessor: FormatSourceIdentity,
    request: FormatRequest,
    execution: FormatExecutionStrategy,
    edits: Vec<FormatEdit>,
    coverage: Vec<SourceByteRange>,
    rendered: ContentDigest,
}

impl FormatEditPlan {
    /// Validate the entire plan and prove its edits reproduce the renderer's bytes.
    pub fn new(
        source: &ExactSource<'_>,
        request: FormatRequest,
        execution: FormatExecutionStrategy,
        edits: Vec<FormatEdit>,
        rendered: &str,
    ) -> Result<Self, EditPlanError> {
        let plan = Self {
            schema: FORMAT_EDIT_PLAN_SCHEMA,
            predecessor: source.identity.clone(),
            request,
            execution,
            coverage: edits.iter().map(|edit| edit.range.clone()).collect(),
            edits,
            rendered: ContentDigest::of_bytes(rendered.as_bytes()),
        };
        let applied = plan.apply(source)?;
        if applied != rendered {
            return Err(EditPlanError::RenderedMismatch);
        }
        Ok(plan)
    }

    /// Exact predecessor identity.
    #[must_use]
    pub fn predecessor(&self) -> &FormatSourceIdentity {
        &self.predecessor
    }
    /// Requested/admitted/widened target identities.
    #[must_use]
    pub fn request(&self) -> &FormatRequest {
        &self.request
    }
    /// Actual execution strategy, independent from request and edit shape.
    #[must_use]
    pub const fn execution(&self) -> FormatExecutionStrategy {
        self.execution
    }
    /// Ordered source-bound edits.
    #[must_use]
    pub fn edits(&self) -> &[FormatEdit] {
        &self.edits
    }
    /// Actual coverage, independent from the admitted target.
    #[must_use]
    pub fn coverage(&self) -> &[SourceByteRange] {
        &self.coverage
    }
    /// Digest the complete plan must produce.
    #[must_use]
    pub fn rendered_digest(&self) -> &ContentDigest {
        &self.rendered
    }

    /// Validate every member and atomically compute the expected rendered text.
    ///
    /// Nothing is applied if any member is stale, malformed, duplicated,
    /// overlapping, outside admission, or disagrees with the expected digest.
    pub fn apply(&self, source: &ExactSource<'_>) -> Result<String, EditPlanError> {
        if self.schema != FORMAT_EDIT_PLAN_SCHEMA {
            return Err(EditPlanError::UnsupportedSchema);
        }
        if self.predecessor != source.identity {
            return Err(EditPlanError::StaleSource);
        }
        self.request.validate(source)?;
        if !self.edits.is_empty()
            && matches!(
                self.execution,
                FormatExecutionStrategy::Disabled
                    | FormatExecutionStrategy::NotStarted
                    | FormatExecutionStrategy::ExternalPartialUnsupported
            )
        {
            return Err(EditPlanError::InvalidOutcome);
        }
        if self.coverage != self.edits.iter().map(|edit| edit.range.clone()).collect::<Vec<_>>() {
            return Err(EditPlanError::InvalidCoverage);
        }
        let mut cursor = 0;
        let mut previous: Option<&SourceByteRange> = None;
        for edit in &self.edits {
            edit.range.validate(source)?;
            if !self.request.admitted.contains(&edit.range) {
                return Err(EditPlanError::OutsideTarget);
            }
            if previous == Some(&edit.range)
                || previous.is_some_and(|prior| {
                    prior.start == edit.range.start && (prior.is_empty() || edit.range.is_empty())
                })
            {
                return Err(EditPlanError::DuplicateEdit);
            }
            if previous
                .is_some_and(|prior| edit.range.start < prior.end || edit.range.start < prior.start)
            {
                return Err(EditPlanError::OverlappingEdits);
            }
            previous = Some(&edit.range);
        }
        let mut output = String::with_capacity(source.text.len());
        for edit in &self.edits {
            output.push_str(&source.text[cursor..edit.range.start]);
            output.push_str(&edit.new_text);
            cursor = edit.range.end;
        }
        output.push_str(&source.text[cursor..]);
        if ContentDigest::of_bytes(output.as_bytes()) != self.rendered {
            return Err(EditPlanError::RenderedMismatch);
        }
        Ok(output)
    }
}

/// A failed source/target/plan admission. No failure permits partial application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditPlanError {
    /// The transport names an unsupported contract version.
    UnsupportedSchema,
    /// Exact predecessor content or currentness does not match.
    StaleSource,
    /// An interval belongs to a different logical source/content/generation.
    MixedSource,
    /// An interval's end precedes its start.
    ReversedRange,
    /// An endpoint is beyond source EOF.
    OutOfBounds,
    /// An endpoint splits a UTF-8 scalar.
    NotCharacterBoundary,
    /// An edit or request escapes its admitted interval.
    OutsideTarget,
    /// Two edits duplicate one predecessor interval, including empty insertions.
    DuplicateEdit,
    /// Edits are unordered or overlap in predecessor space.
    OverlappingEdits,
    /// Widening provenance is absent, empty, or attached to an exact target.
    InvalidWidening,
    /// Coverage disagrees with the complete edit set.
    InvalidCoverage,
    /// Applying edits does not reproduce the declared render.
    RenderedMismatch,
    /// Applied outcome omitted its load-bearing plan.
    MissingPlan,
    /// A withheld/no-change result includes edits or changed bytes.
    InvalidOutcome,
}

impl std::fmt::Display for EditPlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for EditPlanError {}
