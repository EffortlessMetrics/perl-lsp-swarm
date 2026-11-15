//! Registry-activated Mojo::Base object-fact minting (#9682).
//!
//! This module turns source-extracted `has` declarations plus the #9681
//! checked activation/profile into the canonical object facts of a
//! `Mojo::Base` class: generated accessor members, the distinct reader and
//! fluent-setter callable-result relations, and the literal parent
//! relationship. It is the facts side of the #9682 producer leaf and builds
//! directly on the #9681 activation seam:
//!
//! - object facts mint **only through the registry-activated adapter**: a
//!   detected framework and an exact activation are both required. A
//!   same-named `has` call in a package without an exact activation is a hard
//!   negative and mints nothing — this module never treats `has` itself as
//!   activation evidence;
//! - every generated member points at the **real `has` declaration anchor**
//!   and never receives a fabricated method body. Members carry
//!   `Provenance::FrameworkSynthesis` / `Confidence::Medium` and the
//!   `SemanticReasonCode::GeneratedFromSource` reason, matching the accepted
//!   source-backed-generated provenance rules (PLSP-SPEC-0017 / -0024): a
//!   framework-generated accessor is source-backed by its generator, but it is
//!   never relabelled as explicit source;
//! - the fluent setter uses the neutral
//!   [`CallableResultRelation::ReceiverSelf`] vocabulary (#10904) rather than
//!   a Mojo-specific return-self shape, and carries no fabricated exit
//!   contributor: a generated accessor has a generator anchor, not a real
//!   return statement, so its callable-result facts stay
//!   [`CallableResultLimitation::GeneratedNoSource`];
//! - the reader result and the setter return-self relation are **two distinct
//!   facts about one member entity**. Default-value uncertainty limits the
//!   reader without erasing the independently determinate `ReceiverSelf`
//!   relation;
//! - the literal parent proposition reuses the canonical [`PackageEdge`] /
//!   [`PackageEdgeKind::Inherits`] vocabulary. This leaf produces the
//!   proposition a canonical relationship representation consumes; it does not
//!   claim `PackageEdge` is the final ProjectModel authority (that is the
//!   typed-relationship issue's to own) and it builds no Mojo-only hierarchy;
//! - facts are generation-owned and shadow receipts: every envelope carries
//!   the activation's source generation plus invalidation dependencies over
//!   the owning file and the activating `Mojo::Base` module, and the adapter
//!   disposition remains `Shadow`, so no provider surface can publish them
//!   (canonical publication is owned separately).
//!
//! Accessor semantics follow the reviewed `Mojo::Base` profile. `attr` binds
//! `($self, $attrs, $value, %kv)`: a name (or an array reference of names),
//! one optional default, and optional trailing key/value options — the corpus
//! spells `has app => undef, weak => 1;`. Every generated accessor is
//! read-write and a write returns the invocant, so an option this profile does
//! not model limits the *read* result without disturbing the accessor identity
//! or the write contract. A `sub { ... }` default is a lazy builder rather
//! than a code-reference value.

use perl_semantic_facts::framework::AdapterDetectionResult;
use perl_semantic_facts::framework_adapters::mojo_base::{
    MOJO_BASE_ADAPTER_ID, MOJO_BASE_FRAMEWORK_NAME, MojoBaseActivationFacts,
    MojoBaseActivationOutcome,
};
use perl_semantic_facts::framework_adapters::mojo_base_facts::*;
use perl_semantic_facts::{
    AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, EntityId, FactId,
    FileId, GeneratedMember, GeneratedMemberKind, InvalidationDependency, LifecyclePhase,
    PackageEdge, PackageEdgeKind, Provenance, SemanticConfidence, SemanticFactKind,
    SemanticFreshness, SemanticProducer, SemanticProvenance, SemanticReasonCode, SourceAnchor,
    SourceGeneration, ValueShape,
};
use perl_semantic_facts::{
    CallableResultCompleteness, CallableResultFact, CallableResultLimitation,
    CallableResultRelation, SemanticFactEnvelope,
};

/// Mint the canonical `Mojo::Base` object facts for one activating package.
///
/// Returns no facts unless `detection` established the framework and
/// `activation` is exact — the registry-activated adapter contract. A
/// declaration of another package is skipped, and a declaration whose name is
/// computed or malformed mints no member (the typed boundary stays on the
/// declaration; this leaf never guesses an accessor name).
///
/// `file_id` owns the activating import — the activation profile carries the
/// import's source interval but not its file, and the parent relationship is
/// anchored in that file.
///
/// `package` must be the package that made the activation. It is checked
/// against `activation.package` and the whole call fails closed on a
/// mismatch, so a caller cannot borrow one package's activation to establish
/// another package's accessors or inheritance.
///
/// A declaration must then belong to that activation on **four** counts,
/// because the carriers arrive as a plain slice a caller could mismatch
/// against the activation:
///
/// - the same owning package;
/// - the same file as the activating import, so a same-named package in
///   another file cannot contribute accessors to this activation;
/// - the same source generation, so a carrier extracted from an older parse
///   cannot be restamped with the current generation and resurrect an accessor
///   that has since been renamed or removed;
/// - a position **after** the activating import, because `Mojo::Base` installs
///   `has` at import time. A `has` call earlier in the package is a different
///   function — the reviewed profile has no accessor to generate from it.
///
/// Every minted fact carries the activation's source generation plus
/// invalidation dependencies over the owning source file and the `Mojo::Base`
/// module, so an accessor, import, or module edit invalidates the dependent
/// projections in the current generation.
#[must_use]
pub fn mojo_base_object_facts(
    detection: &AdapterDetectionResult,
    activation: &MojoBaseActivationFacts,
    file_id: FileId,
    package: Option<&str>,
    declarations: &[MojoBaseAttributeDeclaration],
) -> MojoBaseObjectFacts {
    let mut facts = MojoBaseObjectFacts::default();
    if !detection.is_detected() || !activation.is_exact() {
        return facts;
    }
    // `is_detected()` only says *some* framework was detected. The two inputs
    // arrive separately, so nothing else stops a caller pairing another
    // adapter's detection — or a newer project generation — with a cached
    // exact `Mojo::Base` activation. Bind them explicitly instead of trusting
    // the pairing: the detection must be this adapter's, and it must describe
    // the same generation the activation was observed in.
    if detection.descriptor.adapter_id != MOJO_BASE_ADAPTER_ID
        || detection.project_generation != activation.source_generation
    {
        return facts;
    }
    // An unknown generation is missing evidence, not a generation that happens
    // to match. `SourceGeneration::Unknown` compares equal to itself, so every
    // generation gate here would pass on a pair of unknowns and the facts
    // would still be stamped `SemanticFreshness::Fresh` — claiming a currency
    // nothing established, and carrying invalidation dependencies that could
    // never be invalidated because they name no snapshot.
    if !activation.source_generation.is_known() {
        return facts;
    }
    // The activation already knows which package imported `Mojo::Base`. Asking
    // it for a different package's members must fail closed rather than trust
    // the caller: an activation `App` made establishes neither `Other`'s
    // accessors nor an `Other inherits Mojo::Base` edge.
    if activation.package.as_deref() != package {
        return facts;
    }
    let generation = &activation.source_generation;
    let (_, import_end_byte) = activation.source_interval;

    let mut admitted: Vec<(&MojoBaseAttributeDeclaration, &str)> = Vec::new();
    // Execution-order key of the earliest declaration `attr` refuses, if any.
    // See the `Unsupported` arm below and the cut that follows the loop.
    let mut refused_at: Option<(u8, u32)> = None;
    for declaration in declarations {
        if declaration.package.as_deref() != package {
            continue;
        }
        // A same-named package in another file is treated as a different
        // class: its `has` calls are not this activation's accessors.
        //
        // **Documented limitation.** A Perl package can legitimately span
        // files, and `Mojo::Base` installs `has` into the package symbol
        // table, so a second file reopening the package *after* the first has
        // loaded really can declare accessors. Admitting those needs proof of
        // load order between the two files, which is a workspace-level fact
        // this per-file producer does not have — and without it, admitting
        // them would also re-admit the genuinely unrelated same-named package
        // this guard exists to reject.
        //
        // So the guard stays closed while the ordering is unknown: it omits
        // accessors of a split package rather than inventing accessors for an
        // unrelated one. #9682's own fixture list asks for multi-root
        // same-name isolation, which is the same posture.
        if declaration.file_id != file_id {
            continue;
        }
        // A carrier from an older parse cannot be relabelled current.
        if declaration.source_generation != *generation {
            continue;
        }
        // Future phase/default variants need their own interpretation before
        // this producer can establish execution order or an accessor.
        if !matches!(
            declaration.execution_phase,
            MojoBaseExecutionPhase::CompileImmediate
                | MojoBaseExecutionPhase::PostCompile
                | MojoBaseExecutionPhase::Run
        ) || !matches!(
            declaration.default,
            MojoBaseAttributeDefault::Absent
                | MojoBaseAttributeDefault::Constant
                | MojoBaseAttributeDefault::LazyBuilder
                | MojoBaseAttributeDefault::Dynamic { .. }
                | MojoBaseAttributeDefault::Unsupported { .. }
        ) {
            continue;
        }
        // Source order against the import only decides anything for a
        // declaration that runs during compilation.
        //
        // Perl runs `use` inside an implicit `BEGIN`, so the import completes
        // while the file is still being compiled — before any ordinary
        // statement executes. An ordinary `has` written above the import
        // therefore still calls the imported `has` when it finally runs, and
        // rejecting it on byte order would drop a real accessor. (In the
        // reviewed paren-less spelling that arrangement does not compile at
        // all — `has 'x';` above the import is a syntax error, since `has` is
        // not predeclared there — so this admits the parenthesized spelling
        // and costs nothing on the paren-less one.)
        //
        // A declaration inside an early phaser is the case where order does
        // decide: a phaser above the activating import runs before `has`
        // exists.
        if declaration.execution_phase.is_order_sensitive()
            && declaration.declaration_anchor.start_byte < import_end_byte
        {
            continue;
        }
        // Only a literal spelling names a method. A computed or malformed
        // name keeps its typed boundary on the declaration and mints nothing:
        // a guessed accessor name would be a fabricated member.
        let Some(name) = declaration.name.literal() else {
            continue;
        };
        if package.is_none() {
            // An activation without an owning package cannot own a member:
            // the generated-member payload requires a real package identity.
            continue;
        }
        // `Mojo::Base::attr` croaks at import when a default is a non-code
        // reference: the accessor is never installed at all, not installed
        // with an uncertain value. Admitting the declaration anyway would
        // mint an accessor member and a determinate `ReceiverSelf` setter for
        // a method that does not exist at runtime — the opposite of the fail
        // closed posture this producer otherwise keeps. So an `Unsupported`
        // default mints nothing for its own declaration, and it also never
        // competes for a same-named slot below: `attr` never runs far enough
        // to call `monkey_patch` for it, so it cannot be "the declaration
        // that leaves the live accessor" for `live_declaration_index` either
        // — a same-named sibling that a determinate default reaches is the
        // only one that can still be live.
        if matches!(declaration.default, MojoBaseAttributeDefault::Unsupported { .. }) {
            // The croak is not local to this statement either. It propagates
            // out of the `has` call and aborts the enclosing phase, so every
            // declaration that would have executed *after* it never runs and
            // the package fails to load. Confirmed against `perl` with the
            // released guard transcribed verbatim: given
            //
            //     has 'before_it' => 1;   # installed
            //     has 'list'      => [];  # croaks
            //     has 'after_it'  => 2;   # never runs
            //
            // only `before_it` reaches the symbol table. Minting `after_it`
            // would publish a method that provably does not exist, which is
            // the same over-report this arm exists to prevent.
            //
            // Record the earliest refusal in *execution* order, not source
            // order: a refusal inside `BEGIN` croaks during compilation, so an
            // ordinary `has` written above it has not run yet and never will.
            let refusal =
                (execution_rank(declaration.execution_phase), declaration.declaration_index);
            refused_at = Some(refused_at.map_or(refusal, |earliest| earliest.min(refusal)));
            continue;
        }
        admitted.push((declaration, name));
    }
    // Drop everything the croak above prevents from executing. Names sharing
    // one `has [qw(a b)]` statement share `declaration_index`, and a statement
    // has one default, so a refused statement is already wholly excluded by the
    // arm above; the strict `<` here only removes genuinely later statements.
    if let Some(cut) = refused_at {
        admitted.retain(|(declaration, _)| {
            (execution_rank(declaration.execution_phase), declaration.declaration_index) < cut
        });
    }
    let Some(owning_package) = package else {
        return facts;
    };

    // One package slot holds one accessor. `attr` installs each with an
    // unconditional `monkey_patch`, so a repeated name leaves exactly one live
    // method and the earlier declarations are gone. Minting an independent
    // unbounded member for each would publish accessors that no longer exist
    // and let a consumer read the superseded default as current.
    for (index, (declaration, name)) in admitted.iter().enumerate() {
        let Some(live) = live_declaration_index(&admitted, name) else {
            continue;
        };
        if live != index {
            continue;
        }
        let contested = admitted.iter().filter(|(_, other)| other == name).count() > 1;
        // When a repeat is contested only between run-phase declarations the
        // later one determinately wins, so the surviving fact needs no caveat.
        // When any competitor runs in a phaser the relative order is not
        // modelled here — `CHECK` blocks run in reverse declaration order, for
        // one — so the surviving choice is best-effort and says so.
        let supersession = (contested
            && admitted.iter().any(|(other, other_name)| {
                other_name == name && other.execution_phase != MojoBaseExecutionPhase::Run
            }))
        .then(|| {
            BoundaryLink::new(
                None,
                BoundaryKind::CompileTimeExecution,
                BoundaryDisposition::Degrade,
                SemanticReasonCode::UnsupportedEffect,
            )
        });
        let slot_undetermined = supersession.is_some();
        facts.members.push(mint_member_fact(
            declaration,
            name,
            owning_package,
            generation,
            supersession,
        ));
        facts.reader_results.push(mint_reader_fact(
            declaration,
            owning_package,
            generation,
            slot_undetermined,
        ));
        facts.setter_results.push(mint_setter_fact(declaration, owning_package, generation));
    }

    if let Some(parent) = activation_parent(activation) {
        facts.parents.push(mint_parent_fact(activation, file_id, package, parent, generation));
    }
    facts
}

/// Index of the declaration that leaves the live accessor for `name`.
///
/// Execution order decides, not source order alone: `BEGIN` runs during
/// compilation, `UNITCHECK`/`CHECK`/`INIT` after it, and ordinary statements
/// last, so a run-phase `has` overwrites any phaser's accessor regardless of
/// where it is written. Within one phase the later declaration wins.
fn live_declaration_index(
    admitted: &[(&MojoBaseAttributeDeclaration, &str)],
    name: &str,
) -> Option<usize> {
    admitted
        .iter()
        .enumerate()
        .filter(|(_, (_, candidate))| *candidate == name)
        .max_by_key(|(index, (declaration, _))| {
            (execution_rank(declaration.execution_phase), *index)
        })
        .map(|(index, _)| index)
}

/// Relative order in which a phase's statements execute.
fn execution_rank(phase: MojoBaseExecutionPhase) -> u8 {
    match phase {
        MojoBaseExecutionPhase::CompileImmediate => 0,
        MojoBaseExecutionPhase::PostCompile => 1,
        MojoBaseExecutionPhase::Run => 2,

        _ => u8::MAX,
    }
}

/// Parent package established by an exact activation.
///
/// `use Mojo::Base -base;` makes the caller inherit from `Mojo::Base` itself;
/// `use Mojo::Base 'Parent';` inherits from the literal spelling. Every other
/// outcome — including a dynamic or unmodeled parent — establishes no
/// relationship.
fn activation_parent(activation: &MojoBaseActivationFacts) -> Option<&str> {
    match &activation.outcome {
        MojoBaseActivationOutcome::ExactBaseActivation => Some(MOJO_BASE_FRAMEWORK_NAME),
        MojoBaseActivationOutcome::ExactLiteralParentActivation { parent } => Some(parent.as_str()),
        _ => None,
    }
}

/// Invalidation dependencies shared by every fact of one activation: the
/// owning source file and the activating `Mojo::Base` module.
fn dependencies(file_id: FileId, generation: &SourceGeneration) -> Vec<InvalidationDependency> {
    vec![
        InvalidationDependency::new(format!("file:{}", file_id.0), generation.clone()),
        InvalidationDependency::new(
            format!("module:{MOJO_BASE_FRAMEWORK_NAME}"),
            generation.clone(),
        ),
    ]
}

/// Build the canonical envelope for one minted source-backed-generated fact.
///
/// Producer `FrameworkAdapter` with `FrameworkSynthesis` / `Medium` and the
/// `GeneratedFromSource` reason: a generated accessor is source-backed by its
/// `has` generator, but it is never explicit source. This deliberately does
/// not reuse the route family's exact-source envelope.
fn generated_envelope(
    kind: SemanticFactKind,
    fact_id: FactId,
    entity_id: EntityId,
    package: &str,
    anchor: SourceAnchor,
    generation: &SourceGeneration,
    file_id: FileId,
    boundary: Option<BoundaryLink>,
) -> SemanticFactEnvelope {
    SemanticFactEnvelope::new(
        fact_id,
        Some(entity_id),
        kind,
        anchor,
        generation.clone(),
        None,
        Some(package.to_string()),
        LifecyclePhase::Runtime,
        SemanticProducer::FrameworkAdapter,
        SemanticProvenance::Known(Provenance::FrameworkSynthesis),
        SemanticConfidence::Known(Confidence::Medium),
        SemanticFreshness::Fresh,
        boundary,
        dependencies(file_id, generation),
        SemanticReasonCode::GeneratedFromSource,
    )
}

fn mint_member_fact(
    declaration: &MojoBaseAttributeDeclaration,
    name: &str,
    package: &str,
    generation: &SourceGeneration,
    supersession: Option<BoundaryLink>,
) -> MojoBaseGeneratedMemberFact {
    // Which of a colliding pair is live after initialization depends on the
    // declaration's phase, so the two cases cannot share one boundary.
    //
    // A run-phase `has` executes after the whole file is compiled, so every
    // explicit `sub` in the file already exists and `monkey_patch` overwrites
    // it unconditionally: the accessor is determinately live. The boundary
    // records only that it shadows a declaration which keeps the stronger
    // source evidence — a conflict a consumer should see rather than one
    // silently resolved here.
    //
    // A compile-phase `has` interleaves with subroutine compilation instead,
    // so whichever comes later in source wins, and a phaser can be overwritten
    // by a `sub` below it. Resolving that would need the explicit method's own
    // position and phase, which this carrier does not hold, so the winner is
    // reported as undetermined rather than guessed. Claiming the accessor is
    // live here would be exactly the overclaim this producer refuses
    // elsewhere.
    // A contested slot is the stronger caveat: when the surviving declaration
    // itself is uncertain, saying which explicit method it shadows is beside
    // the point, so supersession takes the single boundary slot when present.
    if let Some(supersession) = supersession {
        return build_member_fact(declaration, name, package, generation, Some(supersession));
    }
    let boundary = match (declaration.explicit_method, declaration.execution_phase) {
        (MojoBaseExplicitMethodState::None, _) => None,
        (
            MojoBaseExplicitMethodState::Collides,
            MojoBaseExecutionPhase::Run | MojoBaseExecutionPhase::PostCompile,
        ) => Some(BoundaryLink::new(
            None,
            BoundaryKind::Compatibility,
            BoundaryDisposition::Degrade,
            SemanticReasonCode::CompatibilityBoundary,
        )),
        (MojoBaseExplicitMethodState::Collides, MojoBaseExecutionPhase::CompileImmediate) => {
            Some(BoundaryLink::new(
                None,
                BoundaryKind::CompileTimeExecution,
                BoundaryDisposition::Degrade,
                SemanticReasonCode::UnsupportedEffect,
            ))
        }

        _ => Some(BoundaryLink::new(
            None,
            BoundaryKind::Unsupported,
            BoundaryDisposition::Degrade,
            SemanticReasonCode::UnsupportedEffect,
        )),
    };
    build_member_fact(declaration, name, package, generation, boundary)
}

/// Assemble one generated-member fact with an already-decided boundary.
fn build_member_fact(
    declaration: &MojoBaseAttributeDeclaration,
    name: &str,
    package: &str,
    generation: &SourceGeneration,
    boundary: Option<BoundaryLink>,
) -> MojoBaseGeneratedMemberFact {
    let (fact_id, entity_id) = mojo_base_member_identity(
        declaration.file_id,
        declaration.declaration_index,
        declaration.name_index,
        generation,
    );
    // The generator anchor is the real `has` statement: a generated member has
    // no body of its own, so it never receives a fabricated source interval.
    let source_anchor_id = declaration
        .declaration_anchor
        .anchor_id
        .unwrap_or(AnchorId(u64::from(declaration.declaration_anchor.start_byte)));
    MojoBaseGeneratedMemberFact::new(
        generated_envelope(
            SemanticFactKind::Declaration,
            fact_id,
            entity_id,
            package,
            declaration.declaration_anchor,
            generation,
            declaration.file_id,
            boundary,
        ),
        GeneratedMember::new(
            entity_id,
            name.to_string(),
            // Every `Mojo::Base` accessor is read-write: one method both
            // reads and writes the attribute.
            GeneratedMemberKind::Accessor,
            source_anchor_id,
            package.to_string(),
            Provenance::FrameworkSynthesis,
            Confidence::Medium,
        ),
        declaration.explicit_method,
    )
}

/// Reader-result relation implied by the declaration's default evidence.
///
/// Only a source-literal constant default proves a value shape, and it proves
/// it for the *default* alone: a writable accessor can store any value, so the
/// relation is never a complete exit denominator. Every other default form
/// leaves the read result unknown.
fn reader_relation(
    default: &MojoBaseAttributeDefault,
) -> (CallableResultRelation, Vec<CallableResultLimitation>, BoundaryKind) {
    match default {
        MojoBaseAttributeDefault::Constant => (
            CallableResultRelation::Concrete(ValueShape::Scalar),
            vec![
                CallableResultLimitation::GeneratedNoSource,
                CallableResultLimitation::DynamicValue,
            ],
            // The default's shape is proven; what stays open is the stored
            // value a later write may put there — a dynamic value, not an
            // unsupported form.
            BoundaryKind::DynamicValue,
        ),
        // A lazy builder's value is whatever its body returns; this leaf does
        // not evaluate default expressions.
        MojoBaseAttributeDefault::LazyBuilder | MojoBaseAttributeDefault::Absent => (
            CallableResultRelation::Unknown,
            vec![
                CallableResultLimitation::GeneratedNoSource,
                CallableResultLimitation::DynamicValue,
            ],
            BoundaryKind::DynamicValue,
        ),
        MojoBaseAttributeDefault::Dynamic { .. } => (
            CallableResultRelation::Unknown,
            vec![
                CallableResultLimitation::GeneratedNoSource,
                CallableResultLimitation::DynamicValue,
            ],
            BoundaryKind::DynamicValue,
        ),
        // The admission loop above already excludes an `Unsupported`
        // declaration from `admitted`, so `mint_reader_fact` is never called
        // with this variant in production: `Mojo::Base::attr` croaks at
        // import for a non-code reference default, so no accessor — and no
        // reader — is ever installed to describe. This arm stays only for
        // exhaustive matching (`MojoBaseAttributeDefault` is matched
        // elsewhere with no wildcard) and as the honest value this function
        // would still owe if it were ever called on this variant directly.
        MojoBaseAttributeDefault::Unsupported { .. } => (
            CallableResultRelation::Unknown,
            vec![
                CallableResultLimitation::GeneratedNoSource,
                CallableResultLimitation::Unsupported,
            ],
            BoundaryKind::Unsupported,
        ),

        _ => (
            CallableResultRelation::Unknown,
            vec![
                CallableResultLimitation::GeneratedNoSource,
                CallableResultLimitation::Unsupported,
            ],
            BoundaryKind::Unsupported,
        ),
    }
}

fn mint_reader_fact(
    declaration: &MojoBaseAttributeDeclaration,
    package: &str,
    generation: &SourceGeneration,
    slot_undetermined: bool,
) -> CallableResultFact {
    let (fact_id, entity_id) = mojo_base_reader_identity(
        declaration.file_id,
        declaration.declaration_index,
        declaration.name_index,
        generation,
    );
    let (mut relation, mut limitations, mut boundary_kind) = reader_relation(&declaration.default);
    if slot_undetermined {
        // The surviving declaration for this slot is a best-effort choice, so
        // its default proves nothing about what a read yields: the live
        // accessor may be a different declaration with a different default.
        // The member already carries the ordering boundary; without this the
        // reader would still present that declaration's value shape as
        // established.
        //
        // The setter is deliberately *not* degraded alongside it. Every
        // `Mojo::Base` accessor returns the invocant on write, whichever
        // declaration won, so the `ReceiverSelf` relation stays exact — the
        // uncertainty is about the value, not the write contract.
        relation = CallableResultRelation::Unknown;
        limitations.push(CallableResultLimitation::Unsupported);
        boundary_kind = BoundaryKind::CompileTimeExecution;
    }
    if !declaration.unmodeled_options.is_empty() {
        // `Mojo::Base` accepts the options (`weak => 1` and friends), so the
        // accessor and its write contract are unaffected — but an option this
        // profile does not model can change what a read yields, so the reader
        // cannot keep a default-derived shape.
        relation = CallableResultRelation::Unknown;
        limitations.push(CallableResultLimitation::Unsupported);
        boundary_kind = BoundaryKind::Unsupported;
    }
    CallableResultFact::new(
        generated_envelope(
            SemanticFactKind::CallableResult,
            fact_id,
            entity_id,
            package,
            declaration.declaration_anchor,
            generation,
            declaration.file_id,
            // The reader's certainty really is limited: what the accessor
            // returns depends on values this leaf cannot enumerate. The kind
            // names *which* limit applies, so a consumer that filters on
            // `Unsupported` sees only genuinely unsupported declarations.
            Some(BoundaryLink::new(
                None,
                boundary_kind,
                BoundaryDisposition::Degrade,
                SemanticReasonCode::GeneratedFromSource,
            )),
        ),
        relation,
        // No exit contributor is fabricated: a generated accessor has a
        // generator declaration anchor, not a real return statement.
        Vec::new(),
        CallableResultCompleteness::Partial,
        limitations,
    )
}

fn mint_setter_fact(
    declaration: &MojoBaseAttributeDeclaration,
    package: &str,
    generation: &SourceGeneration,
) -> CallableResultFact {
    let (fact_id, entity_id) = mojo_base_setter_identity(
        declaration.file_id,
        declaration.declaration_index,
        declaration.name_index,
        generation,
    );
    CallableResultFact::new(
        generated_envelope(
            SemanticFactKind::CallableResult,
            fact_id,
            entity_id,
            package,
            declaration.declaration_anchor,
            generation,
            declaration.file_id,
            // No boundary: unlike the reader, nothing about this relation is
            // dynamic or unsupported — `Mojo::Base` always returns the
            // invocant from a write. The fact is still not exact source (the
            // `GeneratedFromSource` reason code degrades it, and the
            // `GeneratedNoSource` limitation records that there is no method
            // body), but claiming a *boundary* here would mislabel a
            // determinate framework contract as unmodeled.
            None,
        ),
        // The framework contract is determinate regardless of default-value
        // uncertainty: a write returns the invocant. The relation stays
        // symbolic so no concrete package is hard-coded here.
        CallableResultRelation::ReceiverSelf,
        // Same rule as the reader: no fabricated exit contributor.
        Vec::new(),
        // Mojo::Base generates exactly one write exit, so the alternative set
        // is complete even though it is not source-anchored.
        CallableResultCompleteness::Complete,
        vec![CallableResultLimitation::GeneratedNoSource],
    )
}

fn mint_parent_fact(
    activation: &MojoBaseActivationFacts,
    file_id: FileId,
    package: Option<&str>,
    parent: &str,
    generation: &SourceGeneration,
) -> MojoBaseParentFact {
    let (import_start, import_end) = activation.source_interval;
    // The literal parent's own range when the site located it, otherwise the
    // activating import statement. A range is never fabricated.
    let (anchor_start, anchor_end) = activation.parent_range.unwrap_or((import_start, import_end));
    let (fact_id, entity_id) = mojo_base_parent_identity(file_id, import_start, generation);
    let anchor = SourceAnchor::new(
        Some(AnchorId(u64::from(anchor_start))),
        file_id,
        anchor_start,
        anchor_end,
    );
    let from_package = package.unwrap_or("main").to_string();
    // The two activation forms do not have the same evidence, so they must not
    // claim the same provenance.
    //
    // `use Mojo::Base 'Parent'` spells the parent in source: the edge repeats a
    // literal the reader can point at, anchored on that spelling's own range,
    // so it is exact.
    //
    // `use Mojo::Base -base` spells no parent at all. That the resulting
    // superclass is `Mojo::Base` is knowledge about the framework, not a
    // reading of this file, and the anchor falls back to the import statement
    // because there is no parent range to point at. Claiming `ExactAst` there
    // would assert a source backing that does not exist — the same overclaim
    // this producer refuses for generated accessors.
    let literal_parent = matches!(
        activation.outcome,
        MojoBaseActivationOutcome::ExactLiteralParentActivation { .. }
    );
    let (provenance, confidence, reason_code) = if literal_parent {
        (Provenance::ExactAst, Confidence::High, SemanticReasonCode::ExactSource)
    } else {
        (
            Provenance::FrameworkSynthesis,
            Confidence::Medium,
            SemanticReasonCode::GeneratedFromSource,
        )
    };
    MojoBaseParentFact::new(
        SemanticFactEnvelope::new(
            fact_id,
            Some(entity_id),
            SemanticFactKind::Declaration,
            anchor,
            generation.clone(),
            None,
            Some(from_package.clone()),
            LifecyclePhase::Runtime,
            SemanticProducer::FrameworkAdapter,
            SemanticProvenance::Known(provenance),
            SemanticConfidence::Known(confidence),
            SemanticFreshness::Fresh,
            None,
            dependencies(file_id, generation),
            reason_code,
        ),
        PackageEdge::new(
            from_package,
            parent.to_string(),
            PackageEdgeKind::Inherits,
            Some(AnchorId(u64::from(anchor_start))),
            provenance,
            confidence,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_semantic_facts::SemanticFactStatus;
    use perl_test_must::must_some;

    fn anchor(start: u32, end: u32) -> SourceAnchor {
        SourceAnchor::new(Some(AnchorId(u64::from(start))), FileId(1), start, end)
    }

    /// Byte offset the fixture declarations start at.
    ///
    /// Past [`exact_activation`]'s import interval end, so a fixture
    /// declaration is positioned after the activating import the way real
    /// source is. Placing one before that interval is how the ordering
    /// negative control below is built.
    const AFTER_IMPORT: u32 = 40;

    fn declaration(
        index: u32,
        name: &str,
        default: MojoBaseAttributeDefault,
    ) -> MojoBaseAttributeDeclaration {
        declaration_at(AFTER_IMPORT + 10 * index, index, name, default)
    }

    fn declaration_at(
        start: u32,
        index: u32,
        name: &str,
        default: MojoBaseAttributeDefault,
    ) -> MojoBaseAttributeDeclaration {
        MojoBaseAttributeDeclaration {
            declaration_index: index,
            name_index: 0,
            file_id: FileId(1),
            package: Some("App".to_string()),
            declaration_anchor: anchor(start, start + 9),
            name_anchor: anchor(start + 4, start + 8),
            name: MojoBaseAttributeName::Literal(name.to_string()),
            default,
            explicit_method: MojoBaseExplicitMethodState::None,
            execution_phase: MojoBaseExecutionPhase::Run,
            unmodeled_options: Vec::new(),
            source_generation: SourceGeneration::known("gen-1"),
        }
    }

    fn exact_activation(outcome: MojoBaseActivationOutcome) -> MojoBaseActivationFacts {
        MojoBaseActivationFacts::new(
            outcome,
            perl_semantic_facts::framework_adapters::mojo_base::MOJO_BASE_PROFILE_VERSION,
            Some("App".to_string()),
            (13, 33),
            None,
            None,
            None,
            None,
            "9.34".to_string(),
            Confidence::High,
            SourceGeneration::known("gen-1"),
            false,
            Vec::new(),
            Vec::new(),
        )
    }

    #[test]
    fn reader_and_setter_relations_are_semantically_distinct() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let declarations = [declaration(1, "name", MojoBaseAttributeDefault::Constant)];
        let facts = mint_with_detection(&activation, &declarations);
        assert_eq!(facts.members.len(), 1);
        assert_eq!(facts.reader_results.len(), 1);
        assert_eq!(facts.setter_results.len(), 1);
        assert_eq!(facts.setter_results[0].relation, CallableResultRelation::ReceiverSelf);
        assert_eq!(
            facts.reader_results[0].relation,
            CallableResultRelation::Concrete(ValueShape::Scalar)
        );
        assert_ne!(
            facts.reader_results[0].envelope.fact_id, facts.setter_results[0].envelope.fact_id,
            "reader and setter must be two distinct facts"
        );
        assert_eq!(
            facts.reader_results[0].envelope.entity_id, facts.setter_results[0].envelope.entity_id,
            "both relations describe one accessor entity"
        );
    }

    #[test]
    fn generated_facts_never_claim_exact_source_or_fabricate_an_exit() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let declarations = [declaration(1, "name", MojoBaseAttributeDefault::Constant)];
        let facts = mint_with_detection(&activation, &declarations);
        let member = &facts.members[0];
        assert_eq!(member.member.provenance, Provenance::FrameworkSynthesis);
        assert_eq!(member.member.confidence, Confidence::Medium);
        assert_eq!(member.envelope.reason_code, SemanticReasonCode::GeneratedFromSource);
        assert_ne!(member.envelope.status(), SemanticFactStatus::Exact);
        for fact in facts.reader_results.iter().chain(facts.setter_results.iter()) {
            assert!(fact.exit_anchors().is_empty(), "no exit contributor may be fabricated");
            assert!(
                fact.limitations().contains(&CallableResultLimitation::GeneratedNoSource),
                "a generated accessor has no source body"
            );
            assert_ne!(fact.status(), SemanticFactStatus::Exact);
        }
    }

    #[test]
    fn member_anchors_the_real_has_declaration() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let declarations = [declaration(2, "host", MojoBaseAttributeDefault::Absent)];
        let facts = mint_with_detection(&activation, &declarations);
        let expected_start = AFTER_IMPORT + 20;
        assert_eq!(facts.members[0].member.source_anchor_id, AnchorId(u64::from(expected_start)));
        assert_eq!(facts.members[0].envelope.anchor.start_byte, expected_start);
    }

    #[test]
    fn base_activation_inherits_from_mojo_base_and_literal_parent_from_its_spelling() {
        let base = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let facts = mint_with_detection(&base, &[]);
        assert_eq!(facts.parents.len(), 1);
        assert_eq!(facts.parents[0].edge.to_package, "Mojo::Base");
        assert_eq!(facts.parents[0].edge.kind, PackageEdgeKind::Inherits);

        let literal = exact_activation(MojoBaseActivationOutcome::ExactLiteralParentActivation {
            parent: "Mojo::EventEmitter".to_string(),
        });
        let facts = mint_with_detection(&literal, &[]);
        assert_eq!(facts.parents[0].edge.to_package, "Mojo::EventEmitter");
        assert_eq!(facts.parents[0].edge.from_package, "App");
        assert_eq!(facts.parents[0].edge.provenance, Provenance::ExactAst);
    }

    #[test]
    fn a_computed_name_mints_no_member() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let mut dynamic = declaration(1, "ignored", MojoBaseAttributeDefault::Absent);
        dynamic.name =
            MojoBaseAttributeName::Dynamic { reason: "computed name expression".to_string() };
        let facts = mint_with_detection(&activation, &[dynamic]);
        assert!(facts.members.is_empty(), "a guessed accessor name is never minted");
        assert!(facts.reader_results.is_empty());
        assert!(facts.setter_results.is_empty());
    }

    #[test]
    fn declarations_of_another_package_are_skipped() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let mut foreign = declaration(1, "name", MojoBaseAttributeDefault::Absent);
        foreign.package = Some("Other".to_string());
        let facts = mint_with_detection(&activation, &[foreign]);
        assert!(facts.members.is_empty());
    }

    #[test]
    fn a_collision_is_recorded_without_dropping_the_live_accessor() {
        // `monkey_patch` overwrites unconditionally and runs after compile-time
        // sub installation, so the accessor is the live method and must still
        // be minted; the boundary records the shadowing for consumers.
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let mut colliding = declaration(1, "name", MojoBaseAttributeDefault::Absent);
        colliding.explicit_method = MojoBaseExplicitMethodState::Collides;
        let facts = mint_with_detection(&activation, &[colliding]);
        assert_eq!(facts.members.len(), 1, "the live accessor is still a fact");
        assert_eq!(facts.members[0].member.name, "name");
        assert_eq!(facts.setter_results.len(), 1, "its write contract is unaffected");
    }

    #[test]
    fn an_explicit_same_named_method_degrades_the_generated_member() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let mut colliding = declaration(1, "name", MojoBaseAttributeDefault::Absent);
        colliding.explicit_method = MojoBaseExplicitMethodState::Collides;
        let facts = mint_with_detection(&activation, &[colliding]);
        assert_eq!(facts.members[0].explicit_method, MojoBaseExplicitMethodState::Collides);
        let boundary = facts.members[0].envelope.boundary.as_ref();
        assert!(boundary.is_some(), "the collision must remain visible as conflict evidence");
    }

    #[test]
    fn a_declaration_must_match_the_activation_on_file_generation_and_order() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);

        // A `BEGIN` declaration above the activating import: it runs at its
        // own position during compilation, before the import installed `has`.
        let mut before = declaration_at(0, 0, "early", MojoBaseAttributeDefault::Absent);
        before.execution_phase = MojoBaseExecutionPhase::CompileImmediate;
        assert!(mint_with_detection(&activation, &[before]).members.is_empty());

        // The same position under `CHECK`/`INIT`/`UNITCHECK` does mint: those
        // are scheduled after compilation, so the import below has already
        // run. Only `BEGIN` is order-sensitive.
        let mut post_compile = declaration_at(0, 0, "early", MojoBaseAttributeDefault::Absent);
        post_compile.execution_phase = MojoBaseExecutionPhase::PostCompile;
        assert_eq!(mint_with_detection(&activation, &[post_compile]).members.len(), 1);

        // The same position at run phase does mint: `use` completes during
        // compilation, so an ordinary statement reaches the imported `has`
        // whether it is written above or below the import. This pairs with the
        // refusal above so the guard is pinned against widening as well as
        // narrowing.
        let before_at_run = declaration_at(0, 0, "early", MojoBaseAttributeDefault::Absent);
        assert_eq!(
            before_at_run.execution_phase,
            MojoBaseExecutionPhase::Run,
            "fixture default is run phase"
        );
        assert_eq!(mint_with_detection(&activation, &[before_at_run]).members.len(), 1);

        // Another file, same package name.
        let mut foreign_file = declaration(1, "elsewhere", MojoBaseAttributeDefault::Absent);
        foreign_file.file_id = FileId(2);
        assert!(mint_with_detection(&activation, &[foreign_file]).members.is_empty());

        // An older parse's carrier.
        let mut stale = declaration(1, "outdated", MojoBaseAttributeDefault::Absent);
        stale.source_generation = SourceGeneration::known("gen-0");
        assert!(mint_with_detection(&activation, &[stale]).members.is_empty());

        // A detection from another adapter cannot authorize Mojo facts, and
        // neither can one describing a different project generation.
        let ok = declaration(1, "kept", MojoBaseAttributeDefault::Absent);
        let mut foreign = detected_result("9.34", "gen-1");
        foreign.descriptor = crate::framework_adapters::dancer2::dancer2_descriptor();
        assert!(
            mojo_base_object_facts(
                &foreign,
                &activation,
                FileId(1),
                Some("App"),
                std::slice::from_ref(&ok)
            )
            .members
            .is_empty()
        );
        let mut newer = detected_result("9.34", "gen-1");
        newer.project_generation = SourceGeneration::known("gen-2");
        assert!(
            mojo_base_object_facts(
                &newer,
                &activation,
                FileId(1),
                Some("App"),
                std::slice::from_ref(&ok)
            )
            .members
            .is_empty()
        );

        // An unknown generation is missing evidence: two unknowns compare
        // equal, so every generation gate would pass and the facts would still
        // be stamped `Fresh`.
        // The declaration must carry the *same* unknown generation, or the
        // per-declaration generation guard rejects it first and this control
        // passes for the wrong reason.
        let mut unknown_declaration = declaration(1, "kept", MojoBaseAttributeDefault::Absent);
        unknown_declaration.source_generation = SourceGeneration::Unknown;
        let mut unknown_activation = activation.clone();
        unknown_activation.source_generation = SourceGeneration::Unknown;
        let mut unknown_detection = detected_result("9.34", "gen-1");
        unknown_detection.project_generation = SourceGeneration::Unknown;
        assert!(
            mojo_base_object_facts(
                &unknown_detection,
                &unknown_activation,
                FileId(1),
                Some("App"),
                std::slice::from_ref(&unknown_declaration)
            )
            .members
            .is_empty(),
            "matching unknown generations are not a match"
        );

        // Control: a declaration matching on all counts still mints, so the
        // refusals above isolate each guard rather than a broken fixture.
        let ok = declaration(1, "kept", MojoBaseAttributeDefault::Absent);
        assert_eq!(mint_with_detection(&activation, &[ok]).members.len(), 1);
    }

    #[test]
    fn an_admitted_determinate_default_limits_the_reader_dynamically_not_as_unsupported() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let determinate = [
            MojoBaseAttributeDefault::Constant,
            MojoBaseAttributeDefault::LazyBuilder,
            MojoBaseAttributeDefault::Absent,
        ];
        for default in determinate {
            let facts = mint_with_detection(&activation, &[declaration(1, "name", default)]);
            let boundary = must_some(facts.reader_results[0].envelope.boundary.as_ref());
            assert_eq!(
                boundary.kind,
                BoundaryKind::DynamicValue,
                "an admitted default limits the reader dynamically, not as unsupported"
            );
            assert!(
                facts.setter_results[0].envelope.boundary.is_none(),
                "the write contract is determinate and carries no boundary"
            );
        }
    }

    /// `Mojo::Base::attr` croaks at import for a non-code reference default,
    /// so the accessor it would have installed never exists at runtime: an
    /// `Unsupported` declaration must mint no member, reader, or setter at
    /// all — not an accessor whose reader merely carries an `Unsupported`
    /// boundary.
    #[test]
    fn an_unsupported_default_mints_no_member_reader_or_setter() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let unsupported = MojoBaseAttributeDefault::Unsupported { reason: "ref".to_string() };
        let facts = mint_with_detection(&activation, &[declaration(1, "broken", unsupported)]);
        assert!(
            facts.members.is_empty(),
            "no accessor member for a default Mojo::Base::attr rejects at import"
        );
        assert!(
            facts.reader_results.is_empty(),
            "no reader for an accessor that was never installed"
        );
        assert!(
            facts.setter_results.is_empty(),
            "no setter for an accessor that was never installed"
        );
    }

    /// The `Unsupported` exclusion is per-declaration, not per-activation: a
    /// sibling `has` with a determinate default in the same package still
    /// mints a member, reader, and setter. The activation stays good; only the
    /// one refused declaration is dropped.
    ///
    /// The determinate sibling is written *above* the refused one because that
    /// is the only position where it genuinely still installs. `attr` croaks,
    /// and the croak aborts the enclosing phase, so a sibling below the refused
    /// declaration never executes — see
    /// `a_refused_default_also_drops_every_later_declaration`.
    #[test]
    fn a_sibling_determinate_declaration_still_mints_beside_an_unsupported_one() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let unsupported = MojoBaseAttributeDefault::Unsupported { reason: "ref".to_string() };
        let facts = mint_with_detection(
            &activation,
            &[
                declaration(1, "ok", MojoBaseAttributeDefault::Constant),
                declaration(2, "broken", unsupported),
            ],
        );
        assert_eq!(facts.members.len(), 1, "only the determinate sibling mints a member");
        assert_eq!(facts.members[0].member.name, "ok");
        assert_eq!(facts.reader_results.len(), 1);
        assert_eq!(facts.setter_results.len(), 1);
    }

    /// The croak aborts the enclosing phase, so a refused default also drops
    /// every declaration that would have executed after it.
    ///
    /// Verified against `perl` with the released `attr` guard transcribed: with
    /// `before` installed, `bad => []` croaking and `after` written below it,
    /// the load fails and only `before` reaches the symbol table.
    #[test]
    fn a_refused_default_also_drops_every_later_declaration() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let facts = mint_with_detection(
            &activation,
            &[
                declaration(1, "before", MojoBaseAttributeDefault::Constant),
                declaration(
                    2,
                    "bad",
                    MojoBaseAttributeDefault::Unsupported { reason: "ref".to_string() },
                ),
                declaration(3, "after", MojoBaseAttributeDefault::Constant),
            ],
        );
        let names: Vec<&str> =
            facts.members.iter().map(|member| member.member.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["before"],
            "only the declaration executing before the croak installs an accessor"
        );
        assert_eq!(facts.reader_results.len(), 1, "one surviving accessor, one read result");
        assert_eq!(facts.setter_results.len(), 1, "one surviving accessor, one write contract");
    }

    /// Execution order decides what the croak kills, not source order.
    ///
    /// A refusal inside `BEGIN` croaks during compilation, so an ordinary `has`
    /// written *above* it has not run yet and never will. Verified against
    /// `perl`:
    ///
    /// ```perl
    /// has 'run_phase' => 1;         # written first, runs in the run phase
    /// BEGIN { has 'bad' => []; }    # written second, croaks at compile time
    /// ```
    ///
    /// reports `BEGIN failed--compilation aborted` and installs neither.
    /// Cutting on source order alone would wrongly keep `run_phase`.
    #[test]
    fn a_refusal_in_begin_kills_a_run_phase_declaration_written_above_it() {
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let run_phase = declaration(1, "run_phase", MojoBaseAttributeDefault::Constant);
        let mut refused_in_begin = declaration(
            2,
            "bad",
            MojoBaseAttributeDefault::Unsupported { reason: "ref".to_string() },
        );
        refused_in_begin.execution_phase = MojoBaseExecutionPhase::CompileImmediate;
        let facts = mint_with_detection(&activation, &[run_phase, refused_in_begin]);
        assert!(
            facts.members.is_empty(),
            "the compile-time croak aborts the file before any run-phase `has` executes"
        );
        assert!(facts.reader_results.is_empty());
        assert!(facts.setter_results.is_empty());
    }

    /// Run the real Mojo::Base detection path, then mint over its result, so
    /// the gating in these tests is the production gate rather than a stub.
    fn mint_with_detection(
        activation: &MojoBaseActivationFacts,
        declarations: &[MojoBaseAttributeDeclaration],
    ) -> MojoBaseObjectFacts {
        mojo_base_object_facts(
            &detected_result("9.34", "gen-1"),
            activation,
            FileId(1),
            activation.package.as_deref(),
            declarations,
        )
    }

    fn detected_result(version: &str, generation: &str) -> AdapterDetectionResult {
        use crate::framework_adapters::mojo_base::{detect_mojo_base, mojo_base_descriptor};
        use perl_semantic_facts::framework::{
            AdapterCancellation, AdapterDetectionInput, DetectionEvidenceClass,
            ModuleActivationIdentity, ModuleObservationReceipt, ModuleSelectorEvaluation,
            ModuleSelectorOutcome, ModuleVersionEvidence,
        };

        let activation = ModuleActivationIdentity::new(
            MOJO_BASE_FRAMEWORK_NAME,
            Some(FileId(7)),
            SourceGeneration::known(generation),
        )
        .with_observed_version(ModuleVersionEvidence::new(
            version,
            SourceGeneration::known(generation),
        ));
        let evaluation = ModuleSelectorEvaluation::new(
            MOJO_BASE_FRAMEWORK_NAME,
            ModuleSelectorOutcome::Matched {
                activation,
                evidence_class: DetectionEvidenceClass::ResolvedModule,
            },
        );
        detect_mojo_base(&AdapterDetectionInput::new(
            mojo_base_descriptor(),
            ModuleObservationReceipt::new(
                "module-resolver.v1",
                "root:fixture",
                "project-environment.v1",
                SourceGeneration::known(generation),
                "sha256:fixture-input",
                vec![evaluation],
            ),
            None,
            AdapterCancellation::active(),
        ))
    }

    #[test]
    fn an_undetected_framework_mints_nothing() {
        use crate::framework_adapters::mojo_base::{detect_mojo_base, mojo_base_descriptor};
        use perl_semantic_facts::framework::{
            AdapterCancellation, AdapterDetectionInput, ModuleObservationReceipt,
        };

        // No module evaluation at all: the framework was never established, so
        // an otherwise exact activation profile still mints no object fact.
        let undetected = detect_mojo_base(&AdapterDetectionInput::new(
            mojo_base_descriptor(),
            ModuleObservationReceipt::new(
                "module-resolver.v1",
                "root:fixture",
                "project-environment.v1",
                SourceGeneration::known("gen-1"),
                "sha256:fixture-input",
                Vec::new(),
            ),
            None,
            AdapterCancellation::active(),
        ));
        let activation = exact_activation(MojoBaseActivationOutcome::ExactBaseActivation);
        let declarations = [declaration(1, "name", MojoBaseAttributeDefault::Constant)];
        let facts =
            mojo_base_object_facts(&undetected, &activation, FileId(1), Some("App"), &declarations);
        assert!(facts.is_empty(), "detection must gate every object fact");
    }

    #[test]
    fn a_non_exact_activation_mints_nothing() {
        // The strongest negative control of #9682: `has` in a package whose
        // activation is not exact is never accessor evidence.
        let inexact = exact_activation(MojoBaseActivationOutcome::DynamicOrUnmodeledParent {
            reason: "computed parent expression".to_string(),
        });
        let declarations = [declaration(1, "name", MojoBaseAttributeDefault::Constant)];
        let facts = mint_with_detection(&inexact, &declarations);
        assert!(facts.is_empty(), "a same-named `has` without exact activation emits no facts");
    }
}
