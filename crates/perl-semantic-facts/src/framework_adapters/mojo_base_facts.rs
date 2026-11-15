//! Shared mojo base facts contracts and reference data.
//!
//! Concrete interpretation is owned by `perl_semantic_analyzer::framework_adapters::mojo_base_facts`.
//! This module preserves the fact vocabulary, stable identities, and reviewed tables.

use crate::envelope::{CallableResultFact, SemanticFactEnvelope};

use crate::{
    EntityId, FactId, FileId, GeneratedMember, PackageEdge, SourceAnchor, SourceGeneration,
};

/// Attribute name selection of one source-extracted `has` declaration.
///
/// The name decides whether a member can be generated at all: only a literal
/// spelling names a method. Computed and malformed spellings stay explicit
/// typed boundaries and never become a guessed accessor name.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MojoBaseAttributeName {
    /// A source-literal attribute name (quoted, bareword, or `qw` word).
    Literal(String),
    /// A computed name expression — an explicit dynamic boundary.
    Dynamic {
        /// Bounded dynamic explanation.
        reason: String,
    },
    /// A `has` form the reviewed profile cannot interpret as one name.
    Malformed {
        /// Bounded malformed explanation.
        reason: String,
    },
}

impl MojoBaseAttributeName {
    /// The literal spelling, when this selection names a method.
    #[must_use]
    pub fn literal(&self) -> Option<&str> {
        match self {
            Self::Literal(name) => Some(name.as_str()),
            Self::Dynamic { .. } | Self::Malformed { .. } => None,
        }
    }
}

/// Default-value evidence of one source-extracted `has` declaration.
///
/// `Mojo::Base` admits two default *values*: a constant, and a code reference
/// invoked lazily to build the value. Any other reference croaks at runtime,
/// so it is an explicit unsupported boundary rather than a guessed value.
///
/// This describes the default operand only. `Mojo::Base::attr` binds
/// `($self, $attrs, $value, %kv)`, so a declaration may also carry trailing
/// option pairs after the default; those are recorded separately on
/// [`MojoBaseAttributeDeclaration::unmodeled_options`] and do not make the
/// default unsupported.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MojoBaseAttributeDefault {
    /// No default operand: the attribute reads as undefined until written.
    Absent,
    /// A source-literal constant default (string or number).
    Constant,
    /// A `sub { ... }` lazy builder called to build the default value.
    LazyBuilder,
    /// A computed default expression — an explicit dynamic boundary.
    Dynamic {
        /// Bounded dynamic explanation.
        reason: String,
    },
    /// A default form the reviewed profile does not admit (`Mojo::Base`
    /// rejects a non-code reference default at runtime).
    Unsupported {
        /// Bounded unsupported explanation.
        reason: String,
    },
}

/// Whether an explicit source method of the same name exists in the owning
/// package.
///
/// The collision is preserved as conflict evidence rather than silently
/// deleting either side, and the runtime outcome is **determinate rather than
/// order-dependent**: `Mojo::Base::attr` installs the generated accessor with
/// an unconditional `monkey_patch($class, $attr, $sub)` — no check for an
/// existing method — and Perl installs `sub name { ... }` at compile time
/// while the top-level `has` runs afterwards. The generated accessor therefore
/// overwrites the explicit method whichever order they appear in.
///
/// So the two sides carry different kinds of authority, and neither subsumes
/// the other:
///
/// - the **explicit method** has stronger *source* evidence — a real body, a
///   real span, exact provenance;
/// - the **generated accessor** is the *live* method after class
///   initialization.
///
/// A consumer that reports "definition" should weigh the first; one that
/// answers "what does calling this do" should weigh the second. The shadowing
/// is itself worth surfacing — an explicit method silently replaced by an
/// accessor is usually a mistake in the source, not an intent.
///
/// Scope of that determinacy: a **run-phase** `has` in the same file as the
/// `sub`. It runs after the whole file is compiled, so the `sub` already
/// exists and `monkey_patch` overwrites it unconditionally — source order
/// between the two does not matter.
///
/// A compile-phase `has` (inside `BEGIN`, `UNITCHECK`, `CHECK` or `INIT`)
/// interleaves with subroutine compilation instead, so whichever comes later
/// in source wins and the accessor can itself be overwritten. That winner is
/// reported as undetermined rather than guessed; see
/// [`MojoBaseExecutionPhase`]. A method installed at runtime after the `has`
/// executes remains outside the reviewed profile.
/// When a declaration's `has` call executes, relative to compilation.
///
/// Perl runs `use` inside an implicit `BEGIN`, so execution phase — not source
/// position — decides whether a declaration saw the imported `has`, and
/// whether it survives a same-named explicit `sub`. The phase travels with the
/// carrier rather than being re-guessed at minting.
///
/// The split is between `BEGIN` and everything else, which is narrower than
/// "compile time". Verified against `perl`, with the import written *below*
/// each phaser and a same-named `sub` written below it:
///
/// | Phase | Sees that import | Collision winner |
/// | --- | --- | --- |
/// | ordinary statement | yes | accessor |
/// | `BEGIN` | **no** | **undetermined** |
/// | `UNITCHECK` / `CHECK` / `INIT` | yes | accessor |
///
/// `BEGIN` is alone in running *at its own position* during compilation. The
/// other three are scheduled to run once compilation has finished, so by then
/// every `use` has imported and every `sub` is installed — they behave exactly
/// like an ordinary statement for both questions, and are kept distinct here
/// only because they are not ordinary statements.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MojoBaseExecutionPhase {
    /// An ordinary statement, which runs after the whole file is compiled.
    Run,
    /// A statement inside a `BEGIN` block, which runs during compilation at
    /// the point the block appears.
    ///
    /// This is the only phase for which source order against the activating
    /// import matters, and the only one whose collision winner is
    /// undetermined.
    CompileImmediate,
    /// A statement inside `UNITCHECK`, `CHECK` or `INIT` — scheduled to run
    /// after compilation completes but before the run phase.
    PostCompile,
}

impl MojoBaseExecutionPhase {
    /// Whether source order against the activating import decides anything.
    ///
    /// Only a `BEGIN` block can run before an import written below it.
    #[must_use]
    pub fn is_order_sensitive(self) -> bool {
        matches!(self, Self::CompileImmediate)
    }
}

/// Whether a same-named explicit method exists in the owning package.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MojoBaseExplicitMethodState {
    /// No same-named explicit method was found in the owning package.
    None,
    /// A same-named explicit `sub` is declared in the owning package. The
    /// generated accessor overwrites it at class initialization; the explicit
    /// declaration keeps the stronger source evidence.
    Collides,
}

/// One source-extracted `Mojo::Base` `has` attribute declaration.
///
/// Extraction is pure source observation performed by the semantic analyzer:
/// it knows the reviewed `has` grammar, it does not decide activation. An
/// object fact exists only after this module minted it over an exact
/// activation.
///
/// `has [qw(host port)];` contributes one declaration per name, all sharing
/// the statement's `declaration_index` and separated by `name_index`.
///
/// Like the Dancer2 declaration carriers, this input struct stays
/// exhaustively constructible so the extracting analyzer crate can build it
/// directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojoBaseAttributeDeclaration {
    /// Source-order index of the `has` statement within its file.
    pub declaration_index: u32,
    /// Position of this name within the statement's name operand (`0` for a
    /// single name, the list position for an array-reference name list).
    pub name_index: u32,
    /// File the declaration appears in.
    pub file_id: FileId,
    /// Caller package at the declaration (activation scope).
    pub package: Option<String>,
    /// Anchor of the whole `has` statement — the generator anchor every
    /// generated member points at.
    pub declaration_anchor: SourceAnchor,
    /// Anchor of the name operand itself.
    pub name_anchor: SourceAnchor,
    /// Attribute name selection.
    pub name: MojoBaseAttributeName,
    /// Default-value evidence.
    pub default: MojoBaseAttributeDefault,
    /// Same-named explicit source method state in the owning package.
    pub explicit_method: MojoBaseExplicitMethodState,
    /// When this declaration's `has` call executes, relative to compilation.
    pub execution_phase: MojoBaseExecutionPhase,
    /// Option keys supplied after the default that the reviewed profile does
    /// not model.
    ///
    /// `Mojo::Base::attr` binds `($self, $attrs, $value, %kv)`, so a
    /// declaration may carry trailing key/value options — the corpus spells
    /// `has app => undef, weak => 1;`. The reviewed profile models the name
    /// and the default; every option key is recorded here rather than
    /// silently ignored or mistaken for extra operands, and it limits the
    /// reader result without disturbing the accessor identity or the write
    /// contract, both of which `Mojo::Base` still generates normally.
    pub unmodeled_options: Vec<String>,
    /// Source generation this declaration was extracted from.
    ///
    /// Load-bearing: minting refuses a declaration whose generation differs
    /// from the activation's, so a carrier extracted from an older parse
    /// cannot be restamped with a current generation and reappear as a fresh
    /// fact after the attribute was renamed or removed.
    pub source_generation: SourceGeneration,
}

/// One minted generated-accessor member fact.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojoBaseGeneratedMemberFact {
    /// Shared semantic identity, generation, proof, and invalidation data.
    pub envelope: SemanticFactEnvelope,
    /// Canonical generated-member payload anchored at the real `has`
    /// declaration.
    pub member: GeneratedMember,
    /// Same-named explicit source method state preserved as conflict
    /// evidence.
    pub explicit_method: MojoBaseExplicitMethodState,
}

impl MojoBaseGeneratedMemberFact {
    /// Assemble a record from the caller's established contract values.
    #[must_use]
    pub fn new(
        envelope: SemanticFactEnvelope,
        member: GeneratedMember,
        explicit_method: MojoBaseExplicitMethodState,
    ) -> Self {
        Self { envelope, member, explicit_method }
    }
}

/// One minted literal-parent relationship fact.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojoBaseParentFact {
    /// Shared semantic identity, generation, proof, and invalidation data.
    pub envelope: SemanticFactEnvelope,
    /// Canonical inheritance edge payload.
    pub edge: PackageEdge,
}

impl MojoBaseParentFact {
    /// Assemble a record from the caller's established contract values.
    #[must_use]
    pub fn new(envelope: SemanticFactEnvelope, edge: PackageEdge) -> Self {
        Self { envelope, edge }
    }
}

/// Canonical `Mojo::Base` object facts minted for one activating package.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MojoBaseObjectFacts {
    /// Generated accessor members, in source order.
    pub members: Vec<MojoBaseGeneratedMemberFact>,
    /// Reader-result relation, one per minted member, in the same order.
    pub reader_results: Vec<CallableResultFact>,
    /// Fluent setter return-self relation, one per minted member, in the same
    /// order.
    pub setter_results: Vec<CallableResultFact>,
    /// Literal parent relationship established by the activating import.
    pub parents: Vec<MojoBaseParentFact>,
}

impl MojoBaseObjectFacts {
    /// Whether no object fact of any kind was minted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
            && self.reader_results.is_empty()
            && self.setter_results.is_empty()
            && self.parents.is_empty()
    }
}

/// Domain separator keeping generated-member identities disjoint from every
/// other fact family minted over the same (file, declaration order,
/// generation).
const MOJO_MEMBER_IDENTITY_DOMAIN: u64 = 0x4D4F_4A4F_4D45_4D00;

/// Domain separator for the reader-result fact of one generated member.
const MOJO_READER_IDENTITY_DOMAIN: u64 = 0x4D4F_4A4F_5245_4144;

/// Domain separator for the setter return-self fact of one generated member.
const MOJO_SETTER_IDENTITY_DOMAIN: u64 = 0x4D4F_4A4F_5345_5401;

/// Domain separator for the literal-parent relationship fact.
const MOJO_PARENT_IDENTITY_DOMAIN: u64 = 0x4D4F_4A4F_5041_5201;

/// Deterministic generated-member identity for one
/// (file, `has` statement, name position, generation).
///
/// Identity derives from the owning file, the source declaration order, the
/// name position inside the statement, and the minting generation under a
/// Mojo-specific domain separator, so member facts never collide with the
/// route or hook families minted over the same file/order/generation.
#[must_use]
pub fn mojo_base_member_identity(
    file_id: FileId,
    declaration_index: u32,
    name_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let generation_digest = match generation {
        // FNV-1a accumulation: order- and repetition-sensitive, so distinct
        // generation identities never collide.
        SourceGeneration::Known(value) => {
            value.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x1000_0000_01b3)
            })
        }
        // An unknown generation is still a distinct minting context; the
        // envelope degrades it separately.
        SourceGeneration::Unknown => 0x1a2b_3c4d_5e6f_7081_u64,
    };
    let file = file_id.0.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let index = u64::from(declaration_index).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    let name = u64::from(name_index).wrapping_mul(0x87C3_7B29_11C5_21D3_u64);
    let fact = file ^ index ^ name ^ generation_digest ^ MOJO_MEMBER_IDENTITY_DOMAIN;
    (FactId(fact), EntityId(fact.wrapping_add(1)))
}

/// Deterministic reader-result identity for one generated member.
///
/// Shares the member's entity — the reader relation is a fact *about* the
/// generated accessor — while using its own fact-identity domain so it can
/// never collide with the member fact or the setter relation.
#[must_use]
pub fn mojo_base_reader_identity(
    file_id: FileId,
    declaration_index: u32,
    name_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let (fact_id, entity_id) =
        mojo_base_member_identity(file_id, declaration_index, name_index, generation);
    (FactId(fact_id.0 ^ MOJO_READER_IDENTITY_DOMAIN), entity_id)
}

/// Deterministic setter return-self identity for one generated member.
///
/// Same entity as the reader relation and its own fact-identity domain: one
/// accessor entity carries two semantically distinct callable-result facts.
#[must_use]
pub fn mojo_base_setter_identity(
    file_id: FileId,
    declaration_index: u32,
    name_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let (fact_id, entity_id) =
        mojo_base_member_identity(file_id, declaration_index, name_index, generation);
    (FactId(fact_id.0 ^ MOJO_SETTER_IDENTITY_DOMAIN), entity_id)
}

/// Deterministic literal-parent relationship identity for one activation
/// site.
///
/// A parent relationship belongs to the activating import, not to a `has`
/// statement, so there is no declaration order to key it by. The import's
/// start byte is passed through [`mojo_base_member_identity`]'s
/// `declaration_index` slot instead — deliberately, because that slot is just
/// a distinguishing integer inside the mix. Reusing it is safe only because
/// the parent domain separator below is XORed in afterwards: without it, a
/// `has` statement whose declaration index happened to equal the import's
/// start byte would collide with this fact. The separator, not the slot
/// value, is what keeps the families disjoint — see
/// `parent_identity_never_collides_with_a_member_family_identity`.
#[must_use]
pub fn mojo_base_parent_identity(
    file_id: FileId,
    import_start_byte: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let (fact_id, _) = mojo_base_member_identity(file_id, import_start_byte, 0, generation);
    let fact = FactId(fact_id.0 ^ MOJO_PARENT_IDENTITY_DOMAIN);
    (fact, EntityId(fact.0.wrapping_add(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_identity_never_collides_with_a_member_family_identity() {
        // The parent fact passes the import's start byte through the slot the
        // member families use for a declaration index, so only the parent
        // domain separator keeps the two apart. Sweep the byte offsets and
        // declaration indices of a realistic file against each other: without
        // the separator the diagonal (import_start_byte == declaration_index)
        // would collide on every row.
        let generation = SourceGeneration::known("gen-1");
        for import_start_byte in 0..64_u32 {
            let (parent_fact, parent_entity) =
                mojo_base_parent_identity(FileId(1), import_start_byte, &generation);
            for declaration_index in 0..64_u32 {
                for name_index in 0..4_u32 {
                    for (fact, entity) in [
                        mojo_base_member_identity(
                            FileId(1),
                            declaration_index,
                            name_index,
                            &generation,
                        ),
                        mojo_base_reader_identity(
                            FileId(1),
                            declaration_index,
                            name_index,
                            &generation,
                        ),
                        mojo_base_setter_identity(
                            FileId(1),
                            declaration_index,
                            name_index,
                            &generation,
                        ),
                    ] {
                        assert_ne!(
                            parent_fact, fact,
                            "parent fact id collided with a member-family fact id at \
                             import_start_byte={import_start_byte} \
                             declaration_index={declaration_index} name_index={name_index}"
                        );
                        assert_ne!(
                            parent_entity, entity,
                            "parent entity collided with a member-family entity at \
                             import_start_byte={import_start_byte} \
                             declaration_index={declaration_index} name_index={name_index}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn member_reader_and_setter_identities_are_mutually_disjoint() {
        let generation = SourceGeneration::known("gen-1");
        let member = mojo_base_member_identity(FileId(1), 3, 1, &generation);
        let reader = mojo_base_reader_identity(FileId(1), 3, 1, &generation);
        let setter = mojo_base_setter_identity(FileId(1), 3, 1, &generation);
        assert_ne!(member.0, reader.0);
        assert_ne!(member.0, setter.0);
        assert_ne!(reader.0, setter.0);
        assert_eq!(
            (member.1, reader.1),
            (setter.1, setter.1),
            "all three facts describe one accessor entity"
        );
    }
    #[test]
    fn identities_are_deterministic_and_generation_scoped() {
        let first = mojo_base_member_identity(FileId(1), 3, 0, &SourceGeneration::known("gen-1"));
        let repeat = mojo_base_member_identity(FileId(1), 3, 0, &SourceGeneration::known("gen-1"));
        let newer = mojo_base_member_identity(FileId(1), 3, 0, &SourceGeneration::known("gen-2"));
        let sibling = mojo_base_member_identity(FileId(1), 3, 1, &SourceGeneration::known("gen-1"));
        assert_eq!(first, repeat, "identity must be deterministic");
        assert_ne!(first, newer, "a new generation must mint a new identity");
        assert_ne!(first, sibling, "names of one statement must not collide");
    }
}
