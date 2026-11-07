//! The bounded read-only Open VSX probe plan.
//!
//! Every request this tool sanctions is derived here, from the subject identity
//! alone. The plan is `GET`-only, single-origin, and carries no credential,
//! header, or body material of any kind, so a registry mutation is not
//! expressible through it. Classification refuses any observation whose cell
//! does not address the exact planned URL, which is what makes the probe and
//! the receipt describe the same request set rather than two related ones.

use color_eyre::eyre::{Result, WrapErr};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Public origin every planned request addresses.
pub(super) const REGISTRY_ORIGIN: &str = "https://open-vsx.org";

/// Redirect budget. Exceeding it is an instrument outcome, never an extension fact.
pub(super) const MAX_REDIRECTS: u32 = 3;

/// Per-request wall-clock budget in milliseconds.
pub(super) const TIMEOUT_MS: u64 = 20_000;

/// Result-page size of the single planned search request.
///
/// The plan reads one page and does not paginate, so this surface can only ever
/// say whether the subject appeared within this many results for its query. The
/// receipt states that bound as a limitation, sourced from here so the claimed
/// scope and the request that produced it cannot drift apart.
pub(super) const SEARCH_PAGE_SIZE: u32 = 50;

/// Byte budget for the JSON and HTML surfaces.
const METADATA_BYTE_BUDGET: u64 = 4 * 1024 * 1024;

/// Byte budget for the versioned package. A larger public file is reported as
/// truncated rather than digested, because a partial read cannot prove identity.
const PACKAGE_BYTE_BUDGET: u64 = 256 * 1024 * 1024;

/// The six registry surfaces, observed independently and never collapsed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Cell {
    Listing,
    Search,
    NamespaceMetadata,
    ExtensionMetadata,
    VersionRows,
    VersionedFile,
}

impl Cell {
    /// Fixed emission order. Receipt cell order never depends on map iteration.
    pub(super) const ALL: [Self; 6] = [
        Self::Listing,
        Self::Search,
        Self::NamespaceMetadata,
        Self::ExtensionMetadata,
        Self::VersionRows,
        Self::VersionedFile,
    ];

    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Listing => "listing",
            Self::Search => "search",
            Self::NamespaceMetadata => "namespace_metadata",
            Self::ExtensionMetadata => "extension_metadata",
            Self::VersionRows => "version_rows",
            Self::VersionedFile => "versioned_file",
        }
    }
}

/// One sanctioned read-only request and the bounds it must be executed under.
#[derive(Clone, Debug, Serialize)]
pub(super) struct PlannedRequest {
    pub(super) cell: Cell,
    pub(super) method: &'static str,
    pub(super) url: String,
    pub(super) max_response_bytes: u64,
    pub(super) max_redirects: u32,
    pub(super) timeout_ms: u64,
}

/// The complete request set for one subject identity and version.
#[derive(Clone, Debug, Serialize)]
pub(super) struct ProbePlan {
    pub(super) registry: &'static str,
    pub(super) origin: &'static str,
    pub(super) requests: Vec<PlannedRequest>,
}

impl ProbePlan {
    pub(super) fn request(&self, cell: Cell) -> Option<&PlannedRequest> {
        self.requests.iter().find(|request| request.cell == cell)
    }

    /// SHA-256 over the canonical plan encoding. Field order is the declaration
    /// order of these types, so the digest is stable for a fixed subject.
    ///
    /// Encoding failure is surfaced rather than absorbed: a default digest would
    /// bind every receipt to the same plan identity, which is exactly the
    /// confusion this field exists to prevent.
    pub(super) fn digest(&self) -> Result<String> {
        let canonical = serde_json::to_vec(self).wrap_err("encoding the Open VSX probe plan")?;
        let digest = Sha256::digest(&canonical);
        Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

/// Whether a namespace or extension segment is safe to place in a URL path.
///
/// The observation schema constrains these too; re-checking here keeps the plan
/// honest when it is built from any other caller.
pub(super) fn valid_registry_segment(raw: &str) -> bool {
    let mut characters = raw.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && characters
            .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character))
}

/// Whether a version is safe to place in a URL path segment.
pub(super) fn valid_version(raw: &str) -> bool {
    let mut characters = raw.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && characters
            .all(|character| character.is_ascii_alphanumeric() || ".+-".contains(character))
}

/// Build the plan for one identity and subject version.
///
/// Returns `None` when a segment could change the request's meaning once placed
/// in a URL; the caller reports that as an invalid observation rather than
/// probing something it cannot name exactly.
pub(super) fn probe_plan(namespace: &str, extension: &str, version: &str) -> Option<ProbePlan> {
    if !valid_registry_segment(namespace)
        || !valid_registry_segment(extension)
        || !valid_version(version)
    {
        return None;
    }

    let file_name = format!("{namespace}.{extension}-{version}.vsix");
    let requests = vec![
        planned(
            Cell::Listing,
            format!("{REGISTRY_ORIGIN}/extension/{namespace}/{extension}"),
            METADATA_BYTE_BUDGET,
        ),
        planned(
            Cell::Search,
            format!("{REGISTRY_ORIGIN}/api/-/search?query={extension}&size={SEARCH_PAGE_SIZE}"),
            METADATA_BYTE_BUDGET,
        ),
        planned(
            Cell::NamespaceMetadata,
            format!("{REGISTRY_ORIGIN}/api/{namespace}"),
            METADATA_BYTE_BUDGET,
        ),
        planned(
            Cell::ExtensionMetadata,
            format!("{REGISTRY_ORIGIN}/api/{namespace}/{extension}"),
            METADATA_BYTE_BUDGET,
        ),
        planned(
            Cell::VersionRows,
            format!("{REGISTRY_ORIGIN}/api/{namespace}/{extension}/versions"),
            METADATA_BYTE_BUDGET,
        ),
        planned(
            Cell::VersionedFile,
            format!("{REGISTRY_ORIGIN}/api/{namespace}/{extension}/{version}/file/{file_name}"),
            PACKAGE_BYTE_BUDGET,
        ),
    ];

    Some(ProbePlan { registry: "open_vsx", origin: REGISTRY_ORIGIN, requests })
}

fn planned(cell: Cell, url: String, max_response_bytes: u64) -> PlannedRequest {
    PlannedRequest {
        cell,
        method: "GET",
        url,
        max_response_bytes,
        max_redirects: MAX_REDIRECTS,
        timeout_ms: TIMEOUT_MS,
    }
}

#[cfg(test)]
mod tests {
    use super::{Cell, REGISTRY_ORIGIN, probe_plan, valid_registry_segment, valid_version};
    use color_eyre::eyre::{Result, bail};

    fn incident_plan() -> Result<super::ProbePlan> {
        probe_plan("EffortlessMetrics", "perl-lsp-rs", "0.17.0")
            .ok_or_else(|| color_eyre::eyre::eyre!("incident identity must produce a plan"))
    }

    #[test]
    fn every_cell_has_exactly_one_distinct_read_only_request() -> Result<()> {
        let plan = incident_plan()?;
        if plan.requests.len() != Cell::ALL.len() {
            bail!("plan must cover every cell exactly once");
        }
        let mut urls: Vec<&str> =
            plan.requests.iter().map(|request| request.url.as_str()).collect();
        urls.sort_unstable();
        let distinct = urls.len();
        urls.dedup();
        if urls.len() != distinct {
            bail!("two cells share a URL; surfaces would stop being independent observations");
        }
        for cell in Cell::ALL {
            let Some(request) = plan.request(cell) else {
                bail!("plan is missing the {} cell", cell.key());
            };
            if request.method != "GET" {
                bail!("{} is not a read-only request", cell.key());
            }
        }
        Ok(())
    }

    #[test]
    fn no_planned_request_can_carry_a_credential_or_leave_the_public_origin() -> Result<()> {
        let plan = incident_plan()?;
        for request in &plan.requests {
            if !request.url.starts_with(&format!("{REGISTRY_ORIGIN}/")) {
                bail!("{} leaves the public registry origin: {}", request.cell.key(), request.url);
            }
            // Userinfo is the only URL position that can carry a credential; a
            // planned URL must never contain one.
            let authority_and_rest = request.url.trim_start_matches("https://");
            let authority = authority_and_rest.split('/').next().unwrap_or_default();
            if authority.contains('@') {
                bail!("{} carries URL userinfo: {}", request.cell.key(), request.url);
            }
        }
        Ok(())
    }

    #[test]
    fn the_versioned_file_request_addresses_the_exact_published_package() -> Result<()> {
        let plan = incident_plan()?;
        let Some(request) = plan.request(Cell::VersionedFile) else {
            bail!("versioned file cell must be planned");
        };
        let expected = format!(
            "{REGISTRY_ORIGIN}/api/EffortlessMetrics/perl-lsp-rs/0.17.0/file/\
             EffortlessMetrics.perl-lsp-rs-0.17.0.vsix"
        );
        if request.url != expected {
            bail!("versioned file URL drifted: {} != {expected}", request.url);
        }
        Ok(())
    }

    #[test]
    fn the_digest_is_stable_for_one_subject_and_moves_with_it() -> Result<()> {
        let first = incident_plan()?.digest()?;
        let second = incident_plan()?.digest()?;
        if first != second {
            bail!("plan digest is not deterministic");
        }
        let Some(other_version) = probe_plan("EffortlessMetrics", "perl-lsp-rs", "0.18.0") else {
            bail!("alternate version must produce a plan");
        };
        if other_version.digest()? == first {
            bail!("plan digest ignores the subject version");
        }
        let Some(other_identity) = probe_plan("SomeoneElse", "perl-lsp-rs", "0.17.0") else {
            bail!("alternate identity must produce a plan");
        };
        if other_identity.digest()? == first {
            bail!("plan digest ignores the subject namespace");
        }
        Ok(())
    }

    #[test]
    fn segments_that_could_redirect_a_request_are_refused() -> Result<()> {
        for invalid in
            ["", ".", "..", "a/b", "a?b", "a#b", "a@b", "a b", "-leading", "a%2fb", "a\\b"]
        {
            if valid_registry_segment(invalid) {
                bail!("registry segment was accepted: {invalid:?}");
            }
            if probe_plan(invalid, "perl-lsp-rs", "0.17.0").is_some() {
                bail!("plan built from an unsafe namespace: {invalid:?}");
            }
            if probe_plan("EffortlessMetrics", invalid, "0.17.0").is_some() {
                bail!("plan built from an unsafe extension: {invalid:?}");
            }
        }
        for invalid in ["", "..", "0.17.0/../..", "0.17.0?x=1", "0.17.0 ", "/0.17.0"] {
            if valid_version(invalid) {
                bail!("version was accepted: {invalid:?}");
            }
            if probe_plan("EffortlessMetrics", "perl-lsp-rs", invalid).is_some() {
                bail!("plan built from an unsafe version: {invalid:?}");
            }
        }
        Ok(())
    }
}
