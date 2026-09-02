//! Versioned document model for `agent_candidate_handoff.v1` (D1, issue #13379).
//!
//! The manifest is the semantic authority for one exact local Git candidate.
//! Transport bytes carry the objects; the manifest carries what those objects
//! are claimed to be, so an independent reader can recompute every claim.
//!
//! Two identities are deliberately separate:
//!
//! - [`Manifest::candidate_identity_digest`] covers the *semantic* projection
//!   ([`SemanticIdentity`]) and is stable across worktrees, hosts, and object
//!   storage layouts.
//! - [`TransportFile::sha256`] covers the exact envelope bytes and is only an
//!   integrity claim about *this* envelope. Pack bytes are reproducible for a
//!   given Git version and packing configuration but are not a cross-version
//!   identity, which [`LimitationCode::TransportBytesNotVersionStable`] states
//!   in every manifest rather than leaving implied.

use serde::{Deserialize, Serialize};

/// Schema identity of the handoff manifest.
pub const HANDOFF_MANIFEST_SCHEMA_V1: &str = "agent_candidate_handoff.v1";

/// Schema identity of the producer's self-validation receipt.
pub const HANDOFF_RECEIPT_SCHEMA_V1: &str = "agent_candidate_handoff_receipt.v1";

/// Canonical file name of the manifest inside an envelope.
pub const MANIFEST_FILE_NAME: &str = "manifest.json";

/// Canonical file name of the producer receipt inside an envelope.
pub const RECEIPT_FILE_NAME: &str = "receipt.json";

/// Canonical file name of the object transport inside an envelope.
pub const PACK_FILE_NAME: &str = "candidate.pack";

/// Canonical directory holding declared proof artifacts inside an envelope.
pub const PROOF_DIR_NAME: &str = "proof";

/// How the producer established the repository the candidate belongs to.
///
/// Repository identity is never inferred from a directory name: an
/// unidentifiable workspace stays [`RepositoryIdentityStatus::NotProven`]
/// rather than acquiring a guess.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryIdentityStatus {
    /// Read from a configured Git remote the producer could parse.
    Observed,
    /// Supplied explicitly by the caller.
    Declared,
    /// No trustworthy source was available.
    NotProven,
}

/// Where an observed or declared repository identity came from.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryIdentitySource {
    /// Parsed from the `origin` remote URL.
    GitRemoteOrigin,
    /// Provided by the caller on the command line.
    CallerDeclared,
    /// No source produced an identity.
    Unavailable,
}

/// Repository the candidate claims to belong to.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryIdentity {
    /// Strength of the claim.
    pub status: RepositoryIdentityStatus,
    /// Lowercase `owner/name`, absent when the identity is not proven.
    pub value: Option<String>,
    /// Lowercase hosting authority the identity was observed on.
    ///
    /// `owner/name` alone is not a repository: `acme/app` on two different
    /// forges is two different repositories, and a publisher handed the bare
    /// pair could target the wrong one. An observed identity therefore always
    /// carries the host it was read from. A caller-declared identity carries
    /// none, because the caller named no host, and an unproven identity carries
    /// none because there is nothing to name.
    pub host: Option<String>,
    /// Origin of the claim.
    pub source: RepositoryIdentitySource,
}

/// Author or committer identity exactly as recorded in the commit object.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommitPerson {
    /// Display name.
    pub name: String,
    /// Email address.
    pub email: String,
    /// Raw Git date (`<unix seconds> <tz offset>`), preserved verbatim.
    pub date: String,
}

/// The exact commit under transport and the identities it depends on.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateIdentity {
    /// Full 40-hex candidate commit object ID.
    pub commit: String,
    /// Full 40-hex tree object ID of the candidate commit.
    pub tree: String,
    /// Ordered parent commit IDs. Order is load-bearing for merges.
    pub parents: Vec<String>,
    /// Tree IDs of `parents`, positionally aligned.
    pub parent_trees: Vec<String>,
    /// Full commit message, preserved verbatim.
    pub message: String,
    /// Commit author.
    pub author: CommitPerson,
    /// Commit committer.
    pub committer: CommitPerson,
    /// Whether the candidate has no parents.
    pub is_root_commit: bool,
    /// Whether the candidate has more than one parent.
    pub is_merge_commit: bool,
}

/// Change class of one inventory row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    /// Path exists only in the candidate tree.
    Added,
    /// Path exists in both trees with different content or mode.
    Modified,
    /// Path exists only in the base tree.
    Deleted,
    /// Path moved from `old_path`.
    Renamed,
    /// Path copied from `old_path`.
    Copied,
    /// Entry class changed (for example regular file to symlink).
    TypeChanged,
}

/// Git entry class of the candidate-side object.
///
/// Mode transitions are inventory facts, so an executable-bit flip with
/// identical bytes is a recomputable change rather than an invisible one.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryClass {
    /// Mode `100644`.
    RegularFile,
    /// Mode `100755`.
    ExecutableFile,
    /// Mode `120000`.
    Symlink,
    /// Mode `160000` — a submodule reference, not transported.
    Gitlink,
    /// The entry was deleted, so the candidate side has no class.
    Absent,
}

/// One recomputable change between the base tree and the candidate tree.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeRecord {
    /// Change class.
    pub status: ChangeStatus,
    /// Candidate-side path, or the deleted path for [`ChangeStatus::Deleted`].
    pub path: String,
    /// Source path for renames and copies.
    pub old_path: Option<String>,
    /// Base-side mode, absent for additions.
    pub old_mode: Option<String>,
    /// Candidate-side mode, absent for deletions.
    pub new_mode: Option<String>,
    /// Base-side object ID, absent for additions.
    pub old_object: Option<String>,
    /// Candidate-side object ID, absent for deletions.
    pub new_object: Option<String>,
    /// Rename or copy similarity score, when Git reported one.
    pub similarity: Option<u32>,
    /// Candidate-side entry class.
    pub entry_class: EntryClass,
}

/// How a submodule reference is handled by this envelope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitlinkDisposition {
    /// The gitlink is recorded, but the referenced commit lives in another
    /// repository and is deliberately not transported by this envelope.
    ReferencedNotTransported,
}

/// A submodule reference present in the candidate tree.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitlinkRecord {
    /// Path of the gitlink entry in the candidate tree.
    pub path: String,
    /// Commit ID the gitlink points at, in the submodule's own repository.
    pub commit: String,
    /// Bounded handling applied to this reference.
    pub disposition: GitlinkDisposition,
}

/// The complete changed-path inventory the receiver can recompute.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeInventory {
    /// Parent the inventory is computed against; absent for a root commit,
    /// where the comparison is the empty tree.
    pub base_parent: Option<String>,
    /// Ordered change rows, sorted by candidate-side path.
    pub changes: Vec<ChangeRecord>,
    /// Submodule references present in the candidate tree.
    pub gitlinks: Vec<GitlinkRecord>,
}

/// Object transport representation.
///
/// Only a Git-native complete object set is admitted. A textual patch cannot
/// carry object, mode, binary, rename, or parent identity, so it is not a
/// representable transport format in this schema.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportFormat {
    /// A single self-contained Git packfile.
    GitPackV2,
}

impl TransportFormat {
    /// Stable machine spelling.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::GitPackV2 => "git_pack_v2",
        }
    }
}

/// One declared file of the transport envelope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransportFile {
    /// Envelope-relative file name. Never an absolute or traversing path.
    pub name: String,
    /// Exact byte length.
    pub bytes: u64,
    /// SHA-256 of the exact bytes, lowercase hex.
    pub sha256: String,
}

/// The transported object set and the files carrying it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Transport {
    /// Representation of the object set.
    pub format: TransportFormat,
    /// Whether the envelope admits no undeclared bytes.
    pub closed_envelope: bool,
    /// Declared transport files.
    pub files: Vec<TransportFile>,
    /// Sorted full object IDs the transport is claimed to contain.
    pub object_ids: Vec<String>,
}

/// A content-addressed proof artifact carried alongside the candidate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProofReference {
    /// Stable identifier, also the file name under `proof/`.
    pub id: String,
    /// Envelope-relative path.
    pub path: String,
    /// Exact byte length.
    pub bytes: u64,
    /// SHA-256 of the exact bytes, lowercase hex.
    pub sha256: String,
    /// Candidate commit this proof is bound to.
    pub candidate_subject: String,
}

/// Bounded, stable statements about what this envelope does not establish.
///
/// Codes rather than prose so the semantic identity stays host-independent.
///
/// **Declaration order is part of the format contract.** `Ord` is derived, so
/// the variant order here is the order limitations are sorted into the manifest,
/// and the manifest's limitation list feeds the candidate identity digest.
/// Reordering or inserting a variant would therefore change the digest computed
/// for an unchanged candidate, and every envelope produced before the change
/// would stop matching one produced after it. New codes are appended at the end;
/// existing ones are never moved or renumbered.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitationCode {
    /// Any referenced proof is local; it is not a hosted GitHub check.
    LocalProofOnly,
    /// Retained manifest and receipt strings are scanned for credentials, but
    /// the transported Git objects are not. The envelope carries the committed
    /// blobs of the candidate as they already exist in the repository; it is a
    /// transport, not a content audit, and a secret already committed into the
    /// candidate's tree travels with it.
    TransportedObjectsNotSecretScanned,
    /// Rename rows in the inventory come from Git's rename *detection*, run at
    /// a pinned configuration, not from information the commit records.
    ///
    /// Git stores trees, not renames: a rename is inferred by comparing
    /// content. The comparison is pinned so producer and validator ask the same
    /// question, but it stays a heuristic, and a different Git version may
    /// classify the same trees as a rename where this one saw an add and a
    /// delete. The paths, modes, and object ids in each row are exact either
    /// way; it is the *rename* label that is inferred.
    InventoryRenamesAreDetected,
    /// The repository identity is the producer's word, and no receiver can
    /// check it against anything the envelope carries.
    ///
    /// Every other claim in this format is recomputable from the transported
    /// objects. Repository identity is not: the remote it was read from is
    /// deliberately never retained, so `observed` and `declared` are
    /// indistinguishable to a validator, and a resealed envelope can present
    /// either. The strength ladder is real information about how the *producer*
    /// obtained the value, and nothing more — which matters because the
    /// consumer of this field resolves it into a publication target.
    RepositoryIdentityNotReceiverVerifiable,
    /// Exact pack bytes are reproducible for a given Git version and packing
    /// configuration — including across the ordinary cross-host difference of
    /// loose objects versus a pack, which is proven — but they are not claimed
    /// stable across Git versions. Semantic identity is, and it is what the
    /// validator enforces.
    TransportBytesNotVersionStable,
    /// No trustworthy repository identity was available.
    RepositoryIdentityNotProven,
    /// A configured remote URL carried credentials and was refused as an
    /// identity source; no URL bytes were retained.
    RemoteUrlContainedCredentials,
    /// Submodule commits referenced by gitlinks are not transported.
    SubmoduleGitlinkNotTransported,
    /// The candidate is a root commit, compared against the empty tree.
    RootCommitDiffAgainstEmptyTree,
    /// The candidate is a merge, whose inventory is taken against the first
    /// parent; other parents remain transported and identified.
    MergeCommitDiffAgainstFirstParent,
}

/// Non-semantic producer observations, excluded from candidate identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerObservation {
    /// Producing tool name.
    pub producer_tool: String,
    /// Producing tool version.
    pub producer_version: String,
    /// Git version string observed at creation.
    pub git_version: String,
}

/// The `agent_candidate_handoff.v1` document.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Schema identity; always [`HANDOFF_MANIFEST_SCHEMA_V1`].
    pub schema_version: String,
    /// SHA-256 over the canonical [`SemanticIdentity`] projection.
    pub candidate_identity_digest: String,
    /// Repository the candidate claims to belong to.
    pub repository_identity: RepositoryIdentity,
    /// The exact candidate commit.
    pub candidate: CandidateIdentity,
    /// Recomputable changed-path inventory.
    pub inventory: ChangeInventory,
    /// Object transport.
    pub transport: Transport,
    /// Declared proof artifacts.
    pub proof_references: Vec<ProofReference>,
    /// Stable limitation codes, sorted and deduplicated.
    pub limitations: Vec<LimitationCode>,
    /// Non-semantic producer facts.
    pub observation: ProducerObservation,
}

/// The projection covered by [`Manifest::candidate_identity_digest`].
///
/// Excludes transport file names, byte counts, and digests, and excludes
/// [`ProducerObservation`] entirely, so two exports of the same objects from
/// different worktrees, hosts, or object storage layouts agree.
#[derive(Debug, Serialize)]
pub struct SemanticIdentity<'manifest> {
    /// Schema identity.
    pub schema_version: &'manifest str,
    /// Repository claim.
    pub repository_identity: &'manifest RepositoryIdentity,
    /// Candidate commit identity.
    pub candidate: &'manifest CandidateIdentity,
    /// Changed-path inventory.
    pub inventory: &'manifest ChangeInventory,
    /// Transport representation, without envelope byte facts.
    pub transport_format: TransportFormat,
    /// Sorted transported object IDs.
    pub transport_object_ids: &'manifest [String],
    /// Content identity of declared proofs, without envelope paths or sizes.
    pub proof_identities: Vec<ProofIdentity<'manifest>>,
    /// Limitation codes.
    pub limitations: &'manifest [LimitationCode],
}

/// Path- and size-free identity of one declared proof artifact.
#[derive(Debug, Serialize)]
pub struct ProofIdentity<'manifest> {
    /// Stable proof identifier.
    pub id: &'manifest str,
    /// Content digest.
    pub sha256: &'manifest str,
    /// Bound candidate commit.
    pub candidate_subject: &'manifest str,
}

impl Manifest {
    /// Borrow the projection that candidate identity is computed over.
    #[must_use]
    pub fn semantic_identity(&self) -> SemanticIdentity<'_> {
        SemanticIdentity {
            schema_version: &self.schema_version,
            repository_identity: &self.repository_identity,
            candidate: &self.candidate,
            inventory: &self.inventory,
            transport_format: self.transport.format,
            transport_object_ids: &self.transport.object_ids,
            proof_identities: self
                .proof_references
                .iter()
                .map(|proof| ProofIdentity {
                    id: &proof.id,
                    sha256: &proof.sha256,
                    candidate_subject: &proof.candidate_subject,
                })
                .collect(),
            limitations: &self.limitations,
        }
    }
}

/// The producer's own validation result, stored beside the manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerReceipt {
    /// Schema identity; always [`HANDOFF_RECEIPT_SCHEMA_V1`].
    pub schema_version: String,
    /// Candidate identity the producer emitted.
    pub candidate_identity_digest: String,
    /// Candidate commit the producer emitted.
    pub candidate_commit: String,
    /// Outcome of the producer's own post-write validation pass.
    pub producer_self_check: String,
    /// Limitation codes carried by the manifest.
    pub limitations: Vec<LimitationCode>,
}
