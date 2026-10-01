//! The `public_release_claims.v2` contract (#10333): the sole current
//! route-aware public release claim model, schema projection, validator, and
//! explicit historical-v1 disposition.
//!
//! Claim ceiling (R03A): model, schema, validator, compatibility helpers, and
//! fixtures only. This module owns no route population, no evidence adapters
//! (#11432+), no ranking or preference (#11445+), no rendering, no installer,
//! package, release, or public behavior, and no CLI surface — downstream
//! leaves consume the typed model directly.
//!
//! Fail-closed law: missing, stale, cross-SHA, cross-target, cross-product,
//! cross-stage, or private evidence can never read as a pass. Product units
//! (`core_server`, `debug_preview`, `archive_pair`, `editor_package`,
//! `advanced_server_only`) and evidence stages (`declared` …
//! `publicly_verified`) are independent typed dimensions that cannot collapse
//! into version equality. v1 (`#6355`) stays historical/compatibility input
//! only: [`classify_v1_catalog`] can accept historical non-install claims or
//! demand the v2 cutover, but it can never make absent v1 evidence current,
//! and the Python v1 validator remains that historical input's checker — no
//! parallel active Python/Rust v2 validators exist to drift.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

use crate::schema_apply::validate_payload_against_schema;

/// Schema identity validated by this model.
pub const SCHEMA_VERSION: &str = "public_release_claims.v2";

/// Repository-relative path of the schema this model projects.
pub const SCHEMA_RELATIVE_PATH: &str = "schemas/public_release_claims.v2.schema.json";

/// The historical schema this contract supersedes for current install claims.
pub const HISTORICAL_V1_SCHEMA: &str = "public_release_claims.v1";

/// Track every current release claim binds.
pub const CURRENT_TRACK: &str = "public-beta";

/// The claim namespace that binds an exact route ID under v2.
const INSTALL_NAMESPACE: &str = "install.";

/// Executable members a first-party delivery can name.
const SERVER_EXECUTABLE: &str = "perllsp";
const DAP_EXECUTABLE: &str = "perllsp-dap";

// ── Typed dimensions ────────────────────────────────────────────────────────

/// Product units stay separate (#10333): a route names exactly what it
/// delivers, and pair members cannot be smuggled into server-only routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductUnit {
    CoreServer,
    DebugPreview,
    ArchivePair,
    EditorPackage,
    AdvancedServerOnly,
}

/// Evidence stages stay separate (#10333); ordering is load-bearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStage {
    Declared,
    CandidateBuilt,
    CandidateInstalled,
    PubliclyPublished,
    PubliclyVerified,
}

impl EvidenceStage {
    fn rank(self) -> u8 {
        match self {
            Self::Declared => 0,
            Self::CandidateBuilt => 1,
            Self::CandidateInstalled => 2,
            Self::PubliclyPublished => 3,
            Self::PubliclyVerified => 4,
        }
    }
}

/// Platform / execution environment, kept independent of target triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    NativeWindows,
    WslLinux,
    Macos,
    LinuxGnu,
    LinuxMusl,
    AdmittedRemoteHost,
}

impl Platform {
    /// The target-triple family this platform can truthfully name.
    fn triple_family(self) -> Option<&'static str> {
        match self {
            Self::NativeWindows => Some("windows"),
            Self::WslLinux | Self::LinuxGnu | Self::LinuxMusl => Some("linux"),
            Self::Macos => Some("darwin"),
            Self::AdmittedRemoteHost => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionClass {
    Native,
    Emulated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    GithubRelease,
    ManualArchive,
    SourceBuild,
    HomebrewTap,
    Scoop,
    Chocolatey,
    Winget,
    Marketplace,
    OpenVsx,
    CargoBinstall,
    CargoInstall,
    GithubSetupAction,
    DockerImage,
}

impl SourceKind {
    /// Kinds that deliver a binary artifact an installer or archive carries.
    fn delivers_binary(self) -> bool {
        matches!(
            self,
            Self::GithubRelease | Self::ManualArchive | Self::CargoBinstall | Self::DockerImage
        )
    }

    /// The user-facing install command kinds consistent with this source.
    fn compatible_commands(self) -> &'static [InstallCommandKind] {
        match self {
            Self::GithubRelease | Self::ManualArchive => &[
                InstallCommandKind::ManualDownload,
                InstallCommandKind::ShellCurlPiped,
                InstallCommandKind::PowershellIexPiped,
            ],
            Self::HomebrewTap => &[InstallCommandKind::BrewInstall],
            Self::Scoop => &[InstallCommandKind::ScoopInstall],
            Self::Chocolatey => &[InstallCommandKind::ChocoInstall],
            Self::Winget => &[InstallCommandKind::WingetInstall],
            Self::Marketplace => &[InstallCommandKind::MarketplaceInstall],
            Self::OpenVsx => &[InstallCommandKind::OpenVsxInstall],
            Self::CargoBinstall => &[InstallCommandKind::CargoBinstall],
            Self::CargoInstall | Self::SourceBuild | Self::GithubSetupAction => {
                &[InstallCommandKind::CargoInstall]
            }
            Self::DockerImage => &[InstallCommandKind::DockerPull],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallCommandKind {
    ManualDownload,
    ShellCurlPiped,
    PowershellIexPiped,
    MarketplaceInstall,
    OpenVsxInstall,
    BrewInstall,
    ScoopInstall,
    ChocoInstall,
    WingetInstall,
    CargoBinstall,
    CargoInstall,
    DockerPull,
}

impl InstallCommandKind {
    /// The platform families this command can truthfully run on. `None`
    /// means any admitted platform.
    fn platform_families(self) -> Option<&'static [&'static str]> {
        match self {
            Self::PowershellIexPiped
            | Self::ScoopInstall
            | Self::ChocoInstall
            | Self::WingetInstall => Some(&["windows"]),
            Self::ShellCurlPiped | Self::BrewInstall => Some(&["linux", "darwin"]),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteStatus {
    Proven,
    Bounded,
    Blocked,
    NotProven,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Proven,
    Bounded,
    Blocked,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionCell {
    Proven,
    NotProven,
    Failed,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathState {
    VerifiedFreshProcess,
    CurrentProcessOnly,
    ManualPathStep,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelState {
    Current,
    Stale,
    Pending,
    NotApplicable,
}

// ── Catalog model ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: String,
    pub release: String,
    pub track: String,
    pub subject_sha: String,
    pub topology_digest: String,
    #[serde(default)]
    pub install_routes: Vec<InstallRoute>,
    pub claims: Vec<Claim>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRoute {
    pub id: String,
    pub platform: Platform,
    pub target_triple: String,
    pub execution_class: ExecutionClass,
    pub product_units: Vec<ProductUnit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub executables: Vec<String>,
    pub source: Source,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication: Option<Publication>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<Integrity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_currentness: Option<ChannelCurrentness>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fresh_process_path: Option<FreshProcessPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manual_actions: Vec<ManualAction>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    pub evidence_stage: EvidenceStage,
    pub status: RouteStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub kind: SourceKind,
    pub identity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub subject_sha: String,
    pub artifact_digest: String,
    pub publication_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Integrity {
    pub checksum_verified: bool,
    pub checksum_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub independent: bool,
    pub provenance_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelCurrentness {
    pub state: ChannelState,
    #[serde(rename = "ref")]
    pub ref_: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreshProcessPath {
    pub state: PathState,
    #[serde(rename = "ref")]
    pub ref_: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub clean_install: TransitionCell,
    pub upgrade: TransitionCell,
    pub interruption_rollback: TransitionCell,
    pub removal: TransitionCell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualAction {
    pub step: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub subject_sha: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_triple: Option<String>,
    pub stage: EvidenceStage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub id: String,
    pub surfaces: Vec<String>,
    pub audience: String,
    pub text_or_command: String,
    pub authority: String,
    pub evidence_refs: Vec<String>,
    pub status: ClaimStatus,
    pub public_context: String,
    pub limitation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_route_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_command: Option<InstallCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallCommand {
    pub kind: InstallCommandKind,
    pub raw: String,
}

/// Explicit historical disposition of a v1 catalog (#10333 migration law).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoricalV1Disposition {
    /// A well-formed v1 catalog whose claims carry no current install
    /// namespace: preserved as historical compatibility input only.
    HistoricalV1Accepted,
    /// The v1 catalog names install claims: it cannot satisfy a current
    /// v0.18 install-route claim and requires the v2 cutover.
    CurrentInstallClaimRequiresV2,
    /// Not a v1 catalog at all (unknown schema_version or malformed shape).
    Invalid,
}

/// Classify a candidate v1 catalog. The result is never a current/passing
/// verdict: historical acceptance preserves non-install claims only, install
/// claims demand v2, and absent evidence can never become current.
#[must_use]
pub fn classify_v1_catalog(catalog: &Value) -> HistoricalV1Disposition {
    let Some(map) = catalog.as_object() else {
        return HistoricalV1Disposition::Invalid;
    };
    if map.get("schema_version").and_then(Value::as_str) != Some(HISTORICAL_V1_SCHEMA) {
        return HistoricalV1Disposition::Invalid;
    }
    let Some(claims) = map.get("claims").and_then(Value::as_array) else {
        return HistoricalV1Disposition::Invalid;
    };
    if claims.is_empty() {
        return HistoricalV1Disposition::Invalid;
    }
    let mut install_claim = false;
    for claim in claims {
        let Some(id) = claim.get("id").and_then(Value::as_str) else {
            return HistoricalV1Disposition::Invalid;
        };
        if id.starts_with(INSTALL_NAMESPACE) {
            install_claim = true;
        }
    }
    if install_claim {
        HistoricalV1Disposition::CurrentInstallClaimRequiresV2
    } else {
        HistoricalV1Disposition::HistoricalV1Accepted
    }
}

// ── Validation ──────────────────────────────────────────────────────────────

/// Validate a candidate v2 catalog against the schema projection and the
/// typed fail-closed laws, returning the parsed catalog or every violation.
///
/// # Errors
///
/// Returns every violation found: schema-level (unknown fields, wrong shapes)
/// first, then the typed law set, so one report names all defects.
pub fn validate(
    schema: &Value,
    schema_label: &str,
    catalog: &Value,
    catalog_label: &str,
) -> Result<Catalog, Vec<String>> {
    let mut violations =
        validate_payload_against_schema(schema, schema_label, catalog, catalog_label)
            .map_err(|error| vec![format!("{schema_label}: {error}")])?;
    let catalog: Catalog = match serde_json::from_value(catalog.clone()) {
        Ok(parsed) => parsed,
        Err(error) => {
            violations.push(format!("{catalog_label}: model rejected: {error}"));
            return Err(violations);
        }
    };
    violations.extend(catalog.laws(catalog_label));
    if violations.is_empty() { Ok(catalog) } else { Err(violations) }
}

impl Catalog {
    fn route(&self, id: &str) -> Option<&InstallRoute> {
        self.install_routes.iter().find(|route| route.id == id)
    }

    /// The typed fail-closed laws. Each violation names its law and the exact
    /// collapse it rejects.
    fn laws(&self, label: &str) -> Vec<String> {
        fn at(label: &str, law: &str, detail: impl std::fmt::Display) -> String {
            format!("{label}: law {law}: {detail}")
        }
        let mut violations = Vec::new();

        // Header identity.
        if self.schema_version != SCHEMA_VERSION {
            violations.push(at(label, "V00", format!("schema_version must be {SCHEMA_VERSION}")));
        }
        if self.track != CURRENT_TRACK {
            violations.push(at(label, "V00", format!("track must be {CURRENT_TRACK}")));
        }

        // V01 stable unique IDs in deterministic (strictly ascending) order.
        for (kind, ids) in [
            ("install_routes", self.install_routes.iter().map(|r| &r.id).collect::<Vec<_>>()),
            ("claims", self.claims.iter().map(|c| &c.id).collect::<Vec<_>>()),
        ] {
            let mut sorted = ids.clone();
            sorted.sort();
            sorted.dedup();
            if sorted.len() != ids.len() {
                violations.push(at(label, "V01", format!("{kind} contains duplicate IDs")));
            }
            let mut ascending = ids.clone();
            ascending.sort();
            if ascending != ids {
                violations.push(at(
                    label,
                    "V01",
                    format!("{kind} is not in strictly ascending ID order; catalog order must be deterministic"),
                ));
            }
        }

        for route in &self.install_routes {
            let route_laws = self.route_laws(route);
            violations.extend(route_laws.into_iter().map(|v| at_route(&route.id, v)));
        }

        for claim in &self.claims {
            violations.extend(
                self.claim_laws(claim).into_iter().map(|v| format!("{label} [{}]: {v}", claim.id)),
            );
        }

        // V13 privacy: no private path, credential marker, or raw environment
        // value may enter the durable catalog through any string field.
        for (path, text) in collect_strings(&serde_json::to_value(self).unwrap_or(Value::Null)) {
            for marker in privacy_markers() {
                if text.to_ascii_lowercase().contains(marker) {
                    violations.push(at(
                        label,
                        "V13",
                        format!("private/unsafe string at {path}: contains {marker:?}"),
                    ));
                }
            }
        }

        violations
    }

    fn route_laws(&self, route: &InstallRoute) -> Vec<String> {
        let mut violations = Vec::new();
        fn law(id: &str, detail: impl std::fmt::Display) -> String {
            format!("law {id}: {detail}")
        }

        // V03 product units stay separate; pair membership is exact.
        let has = |unit: ProductUnit| route.product_units.contains(&unit);
        if has(ProductUnit::ArchivePair) {
            for member in [SERVER_EXECUTABLE, DAP_EXECUTABLE] {
                if !route.executables.iter().any(|e| e == member) {
                    violations.push(law(
                        "V03",
                        format!("archive_pair route must name both members; missing {member}"),
                    ));
                }
            }
        }
        if has(ProductUnit::AdvancedServerOnly) {
            if route.executables.iter().any(|e| e == DAP_EXECUTABLE) {
                violations.push(law(
                    "V03",
                    "advanced_server_only route cannot name the DAP member (server-only-as-pair)",
                ));
            }
            if !route.executables.iter().any(|e| e == SERVER_EXECUTABLE) {
                violations
                    .push(law("V03", "advanced_server_only route must name the server member"));
            }
        }
        if has(ProductUnit::DebugPreview) && !has(ProductUnit::ArchivePair) {
            violations.push(law("V03", "debug_preview ships only inside the archive pair"));
        }

        // V04 platform/environment vs target family (native-Windows-as-WSL).
        if let Some(family) = route.platform.triple_family()
            && !route.target_triple.contains(family)
        {
            violations.push(law(
                "V04",
                format!(
                    "platform {:?} cannot truthfully name target triple {}",
                    route.platform, route.target_triple
                ),
            ));
        }

        // V05 exact evidence joins: cross-target evidence can never be
        // current (emulated-x64-as-native-ARM64).
        for evidence in &route.evidence_refs {
            if let Some(triple) = &evidence.target_triple
                && triple != &route.target_triple
            {
                violations.push(law(
                    "V05",
                    format!(
                        "evidence ref {} names target {triple}, not the route target {}",
                        evidence.ref_, route.target_triple
                    ),
                ));
            }
            if evidence.stage.rank() > route.evidence_stage.rank() {
                violations.push(law(
                    "V06",
                    format!(
                        "evidence ref {} claims stage {:?} beyond the route stage {:?}",
                        evidence.ref_, evidence.stage, route.evidence_stage
                    ),
                ));
            }
        }

        // V06 stage/status coherence; missing evidence never passes.
        if route.status == RouteStatus::Proven
            && route.evidence_stage != EvidenceStage::PubliclyVerified
        {
            violations.push(law(
                "V06",
                format!(
                    "proven route must stand at publicly_verified, not {:?}",
                    route.evidence_stage
                ),
            ));
        }
        if route.evidence_stage.rank() >= EvidenceStage::CandidateBuilt.rank() {
            let current = route
                .evidence_refs
                .iter()
                .find(|e| e.subject_sha == self.subject_sha && e.stage == route.evidence_stage);
            if current.is_none() {
                violations.push(law(
                    "V06",
                    format!(
                        "route stage {:?} requires one evidence ref bound to this catalog's subject SHA at the same stage; missing evidence is not a pass",
                        route.evidence_stage
                    ),
                ));
            }
        }

        // V07 publication identity binds the exact current subject.
        if route.evidence_stage.rank() >= EvidenceStage::PubliclyPublished.rank()
            && route.publication.is_none()
        {
            violations
                .push(law("V07", "publicly published routes must bind a publication identity"));
        }
        if let Some(publication) = &route.publication
            && publication.subject_sha != self.subject_sha
        {
            violations.push(law(
                "V07",
                "publication subject SHA differs from the catalog subject (cross-SHA-as-current)",
            ));
        }

        // V08 binary sources carry verified checksums once installed.
        if route.source.kind.delivers_binary()
            && route.evidence_stage.rank() >= EvidenceStage::CandidateInstalled.rank()
        {
            match &route.integrity {
                Some(integrity) if integrity.checksum_verified => {}
                Some(_) => {
                    violations.push(law("V08", "integrity present but checksum not verified"))
                }
                None => violations.push(law(
                    "V08",
                    "binary route at installed stage requires verified checksum integrity",
                )),
            }
        }

        // V09 checksum evidence can never double as independent provenance.
        if let Some(provenance) = &route.provenance
            && provenance.independent
            && let Some(integrity) = &route.integrity
            && provenance.provenance_ref == integrity.checksum_ref
        {
            violations.push(law("V09", "independent provenance reuses the checksum reference"));
        }

        // V10 channel currentness: stale/pending channels cannot be proven.
        if route.status == RouteStatus::Proven {
            match &route.channel_currentness {
                Some(channel) if channel.state == ChannelState::Current => {}
                Some(channel) => violations.push(law(
                    "V10",
                    format!("proven route requires a current channel, found {:?}", channel.state),
                )),
                None => violations
                    .push(law("V10", "proven route requires channel currentness evidence")),
            }

            // V11 fresh-process PATH: current-process-only and manual PATH
            // cannot read as proven resolution.
            match &route.fresh_process_path {
                Some(path)
                    if matches!(
                        path.state,
                        PathState::VerifiedFreshProcess | PathState::NotApplicable
                    ) => {}
                Some(path) => violations.push(law(
                    "V11",
                    format!(
                        "proven route requires fresh-process resolution, found {:?}",
                        path.state
                    ),
                )),
                None => {
                    violations.push(law("V11", "proven route requires fresh-process PATH evidence"))
                }
            }
        }

        // V14 npm stays absent under #8301.
        if route.source.identity.to_ascii_lowercase().contains("npm") {
            violations.push(law("V14", "npm remains an absent channel (#8301)"));
        }

        violations
    }

    fn claim_laws(&self, claim: &Claim) -> Vec<String> {
        let mut violations = Vec::new();
        fn law(id: &str, detail: impl std::fmt::Display) -> String {
            format!("law {id}: {detail}")
        }

        // V02 exact route binding: install claims bind one exact route ID and
        // no other claim invents a route.
        let install_namespaced = claim.id.starts_with(INSTALL_NAMESPACE);
        match (&claim.install_route_id, install_namespaced) {
            (None, true) => violations
                .push(law("V02", "install-namespace claim must bind one exact install_route_id")),
            (Some(route_id), false) => violations
                .push(law("V02", format!("non-install claim cannot bind route {route_id}"))),
            (Some(route_id), true) => {
                if self.route(route_id).is_none() {
                    violations.push(law(
                        "V02",
                        format!("bound route {route_id} does not exist in install_routes"),
                    ));
                }
            }
            (None, false) => {}
        }

        // V15 claim status cannot exceed the bound route; limitation law.
        if claim.status == ClaimStatus::Proven {
            if claim.limitation.is_some() {
                violations.push(law("V15", "proven claim carries a limitation"));
            }
            if let Some(route_id) = &claim.install_route_id
                && let Some(route) = self.route(route_id)
                && route.status != RouteStatus::Proven
            {
                violations.push(law(
                    "V15",
                    format!("claim is proven but route {route_id} is {:?}", route.status),
                ));
            }
        } else if claim.limitation.is_none() {
            violations.push(law(
                "V15",
                format!("{:?} claim requires an explicit limitation", claim.status),
            ));
        }

        // V17 install-claim evidence must come from the bound route's exact
        // evidence set.
        if let Some(route_id) = &claim.install_route_id
            && let Some(route) = self.route(route_id)
        {
            let route_refs: BTreeSet<&str> =
                route.evidence_refs.iter().map(|e| e.ref_.as_str()).collect();
            for evidence in &claim.evidence_refs {
                if !route_refs.contains(evidence.as_str()) {
                    violations.push(law(
                        "V17",
                        format!(
                            "claim evidence {evidence} is not part of route {route_id}'s evidence"
                        ),
                    ));
                }
            }
        }

        // V12 command/route consistency: a command kind contradicting the
        // route's platform or source is unrepresentable.
        if let Some(route_id) = &claim.install_route_id
            && let (Some(route), Some(command)) = (self.route(route_id), &claim.install_command)
        {
            if !route.source.kind.compatible_commands().contains(&command.kind) {
                violations.push(law(
                    "V12",
                    format!(
                        "command kind {:?} contradicts route source kind {:?}",
                        command.kind, route.source.kind
                    ),
                ));
            }
            if let Some(families) = command.kind.platform_families()
                && let Some(route_family) = route.platform.triple_family()
                && !families.contains(&route_family)
            {
                violations.push(law(
                    "V12",
                    format!(
                        "command kind {:?} cannot run on platform family {route_family}",
                        command.kind
                    ),
                ));
            }
        }

        // V14 npm command surface stays absent.
        if let Some(command) = &claim.install_command
            && command.raw.to_ascii_lowercase().contains("npm")
        {
            violations.push(law("V14", "npm remains an absent command surface (#8301)"));
        }

        violations
    }
}

fn at_route(route_id: &str, violation: String) -> String {
    format!("{route_id}: {violation}")
}

/// Markers that must never appear in a durable catalog string.
fn privacy_markers() -> &'static [&'static str] {
    &[
        "c:\\users\\",
        "\\users\\",
        "/home/",
        "/users/",
        "password",
        "api_key",
        "apikey",
        "aws_secret",
        "token=",
    ]
}

fn collect_strings(value: &Value) -> Vec<(String, String)> {
    let mut found = Vec::new();
    collect_strings_walk(value, String::new(), &mut found);
    found
}

fn collect_strings_walk(value: &Value, path: String, found: &mut Vec<(String, String)>) {
    match value {
        Value::String(text) => found.push((path, text.clone())),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_strings_walk(item, format!("{path}[{index}]"), found);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                collect_strings_walk(item, format!("{path}/{key}"), found);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "test-only fixtures and static patterns; production paths stay panic-free"
    )]
    use super::*;
    use serde_json::json;

    const SUBJECT_A: &str = "1111111111111111111111111111111111111111";
    const SUBJECT_B: &str = "2222222222222222222222222222222222222222";
    const TOPOLOGY: &str =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const ARTIFACT: &str =
        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn schema() -> Value {
        let root = crate::utils::project_root().expect("project root");
        let bytes = std::fs::read_to_string(root.join(SCHEMA_RELATIVE_PATH)).expect("schema file");
        serde_json::from_str(&bytes).expect("schema compiles")
    }

    fn positive_catalog() -> Value {
        json!({
            "schema_version": "public_release_claims.v2",
            "release": "0.18.0",
            "track": "public-beta",
            "subject_sha": SUBJECT_A,
            "topology_digest": TOPOLOGY,
            "install_routes": [{
                "id": "route.archive-pair.windows-x64",
                "platform": "native_windows",
                "target_triple": "x86_64-pc-windows-msvc",
                "execution_class": "native",
                "product_units": ["archive_pair"],
                "executables": ["perllsp", "perllsp-dap"],
                "source": {
                    "kind": "github_release",
                    "identity": "release 0.18.0 asset perllsp-0.18.0-x86_64-pc-windows-msvc.zip"
                },
                "publication": {
                    "subject_sha": SUBJECT_A,
                    "artifact_digest": ARTIFACT,
                    "publication_ref": "release 0.18.0 publication index"
                },
                "integrity": {
                    "checksum_verified": true,
                    "checksum_ref": "published sha256 manifest for the 0.18.0 windows asset"
                },
                "provenance": {
                    "independent": true,
                    "provenance_ref": "independent reproducible-build digest for 0.18.0"
                },
                "channel_currentness": {"state": "current", "ref": "release 0.18.0 channel index"},
                "fresh_process_path": {"state": "verified_fresh_process", "ref": "fresh shell resolution probe"},
                "transition": {
                    "clean_install": "proven",
                    "upgrade": "proven",
                    "interruption_rollback": "proven",
                    "removal": "proven"
                },
                "manual_actions": [],
                "evidence_refs": [{
                    "ref": "publicly-verified windows x64 install run",
                    "subject_sha": SUBJECT_A,
                    "target_triple": "x86_64-pc-windows-msvc",
                    "stage": "publicly_verified"
                }],
                "evidence_stage": "publicly_verified",
                "status": "proven",
                "limitations": []
            }],
            "claims": [{
                "id": "install.windows-x64.archive-pair",
                "surfaces": ["README"],
                "audience": "user",
                "text_or_command": "install the published 0.18.0 archive pair for native windows x64",
                "authority": "release_topology",
                "evidence_refs": ["publicly-verified windows x64 install run"],
                "status": "proven",
                "public_context": "both",
                "limitation": null,
                "install_route_id": "route.archive-pair.windows-x64",
                "install_command": {
                    "kind": "manual_download",
                    "raw": "download the published 0.18.0 archive pair and run its installer"
                }
            }]
        })
    }

    fn expect_rejected(catalog: Value, expected_law: &str) -> Vec<String> {
        let schema = schema();
        let violations =
            validate(&schema, SCHEMA_RELATIVE_PATH, &catalog, "fixture").expect_err("must reject");
        assert!(
            violations.iter().any(|v| v.contains(expected_law)),
            "expected {expected_law} in:\n{}",
            violations.join("\n")
        );
        violations
    }

    fn mutate_route(catalog: &mut Value, mutate: impl FnOnce(&mut Value)) {
        mutate(
            catalog
                .get_mut("install_routes")
                .and_then(Value::as_array_mut)
                .expect("routes")
                .get_mut(0)
                .expect("route"),
        );
    }

    #[test]
    fn positive_catalog_passes_schema_model_and_laws() {
        let schema = schema();
        let catalog = positive_catalog();
        let parsed = validate(&schema, SCHEMA_RELATIVE_PATH, &catalog, "positive")
            .expect("positive fixture must validate");
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert_eq!(parsed.install_routes.len(), 1);
        assert_eq!(
            parsed.claims[0].install_route_id.as_deref(),
            Some("route.archive-pair.windows-x64")
        );
    }

    #[test]
    fn validation_is_second_run_clean() {
        let schema = schema();
        let catalog = positive_catalog();
        let first = validate(&schema, SCHEMA_RELATIVE_PATH, &catalog, "run").expect("valid");
        let second = validate(&schema, SCHEMA_RELATIVE_PATH, &catalog, "run").expect("valid");
        assert_eq!(
            serde_json::to_string(&first).expect("serialize"),
            serde_json::to_string(&second).expect("serialize")
        );
    }

    #[test]
    fn server_only_cannot_pass_as_archive_pair() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["executables"] = json!(["perllsp"]);
        });
        expect_rejected(catalog, "V03");
    }

    #[test]
    fn native_windows_cannot_pass_as_wsl() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["platform"] = json!("wsl_linux");
        });
        expect_rejected(catalog, "V04");
    }

    #[test]
    fn emulated_x64_evidence_cannot_pass_as_native_arm64() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["target_triple"] = json!("aarch64-pc-windows-msvc");
        });
        expect_rejected(catalog, "V05");
    }

    #[test]
    fn declared_evidence_cannot_read_as_public() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["evidence_stage"] = json!("declared");
            route["status"] = json!("proven");
        });
        expect_rejected(catalog, "V06");
    }

    #[test]
    fn cross_sha_evidence_cannot_read_as_current() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["evidence_refs"][0]["subject_sha"] = json!(SUBJECT_B);
        });
        expect_rejected(catalog, "V06");
    }

    #[test]
    fn missing_evidence_cannot_read_as_pass() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["evidence_refs"] = json!([]);
        });
        expect_rejected(catalog, "V06");
    }

    #[test]
    fn cross_sha_publication_is_rejected() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["publication"]["subject_sha"] = json!(SUBJECT_B);
        });
        expect_rejected(catalog, "V07");
    }

    #[test]
    fn checksum_cannot_double_as_independent_provenance() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["provenance"]["provenance_ref"] = route["integrity"]["checksum_ref"].clone();
        });
        expect_rejected(catalog, "V09");
    }

    #[test]
    fn stale_channel_and_current_process_path_cannot_be_proven() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["channel_currentness"]["state"] = json!("stale");
        });
        expect_rejected(catalog, "V10");

        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["fresh_process_path"]["state"] = json!("current_process_only");
        });
        expect_rejected(catalog, "V11");
    }

    #[test]
    fn duplicate_and_unsorted_route_ids_fail_closed() {
        let mut catalog = positive_catalog();
        let route = catalog["install_routes"][0].clone();
        catalog["install_routes"] = json!([route.clone(), route]);
        expect_rejected(catalog, "V01");

        let mut catalog = positive_catalog();
        let second = json!({
            "id": "route.a.before",
            "platform": "native_windows",
            "target_triple": "x86_64-pc-windows-msvc",
            "execution_class": "native",
            "product_units": ["advanced_server_only"],
            "executables": ["perllsp"],
            "source": {"kind": "source_build", "identity": "workspace build"},
            "evidence_refs": [],
            "evidence_stage": "declared",
            "status": "not_proven"
        });
        catalog["install_routes"] = json!([catalog["install_routes"][0], second]);
        expect_rejected(catalog, "V01");
    }

    #[test]
    fn npm_stays_absent() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["source"]["identity"] = json!("npm registry mirror of the server binary");
        });
        expect_rejected(catalog, "V14");

        // The npm source kind is not even representable.
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["source"]["kind"] = json!("npm");
        });
        let violations = expect_rejected(catalog, "schema violation");
        assert!(
            violations.iter().any(|v| v.contains("source/kind")),
            "npm kind must fail at the schema layer: {violations:?}"
        );
    }

    #[test]
    fn private_paths_and_secrets_fail_closed() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["evidence_refs"][0]["ref"] =
                json!(format!("run log at C:\\Users\\dev\\AppData\\Local\\Temp\\run.json"));
        });
        expect_rejected(catalog, "V13");
    }

    #[test]
    fn unknown_fields_fail_at_the_schema_layer() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["ambient_environment"] = json!({"PATH": "/usr/local/bin"});
        });
        expect_rejected(catalog, "schema violation");
    }

    #[test]
    fn claim_cannot_exceed_its_route() {
        let mut catalog = positive_catalog();
        mutate_route(&mut catalog, |route| {
            route["status"] = json!("not_proven");
        });
        expect_rejected(catalog, "V15");

        let mut catalog = positive_catalog();
        catalog["claims"][0]["evidence_refs"] = json!(["unrelated repository check"]);
        expect_rejected(catalog, "V17");
    }

    #[test]
    fn command_kind_cannot_contradict_the_route() {
        let mut catalog = positive_catalog();
        catalog["claims"][0]["install_command"] =
            json!({"kind": "brew_install", "raw": "brew install perllsp"});
        expect_rejected(catalog, "V12");

        let mut catalog = positive_catalog();
        catalog["claims"][0]["install_command"] = json!({
            "kind": "shell_curl_piped",
            "raw": "curl -fsSL example/install.sh | sh"
        });
        // A POSIX piped-shell command contradicts the native-Windows route
        // platform even though the source kind would allow a download:
        expect_rejected(catalog, "V12");
    }

    #[test]
    fn non_install_claim_cannot_bind_a_route() {
        let mut catalog = positive_catalog();
        catalog["claims"][0]["id"] = json!("readiness.windows-x64");
        expect_rejected(catalog, "V02");
    }

    #[test]
    fn v1_catalogs_classify_but_never_validate_as_current() {
        let v1_install = json!({
            "schema_version": "public_release_claims.v1",
            "release": "0.17.0",
            "track": "public-beta",
            "subject_sha": SUBJECT_A,
            "topology_digest": TOPOLOGY,
            "claims": [{
                "id": "install.upgrade.path",
                "surfaces": ["README"],
                "audience": "user",
                "text_or_command": "upgrade",
                "authority": "installed_transition",
                "evidence_refs": ["old receipt"],
                "status": "proven",
                "public_context": "swarm",
                "limitation": null
            }]
        });
        assert_eq!(
            classify_v1_catalog(&v1_install),
            HistoricalV1Disposition::CurrentInstallClaimRequiresV2
        );
        // The v1 document can never satisfy the v2 schema.
        let violations = validate(&schema(), SCHEMA_RELATIVE_PATH, &v1_install, "v1-as-current")
            .expect_err("v1 must not validate as v2");
        assert!(
            violations.iter().any(|v| v.contains("schema violation")),
            "v1-as-current must fail at the schema layer: {violations:?}"
        );

        let v1_historical = json!({
            "schema_version": "public_release_claims.v1",
            "release": "0.17.0",
            "track": "public-beta",
            "subject_sha": SUBJECT_A,
            "topology_digest": TOPOLOGY,
            "claims": [{
                "id": "readiness.beta.exit",
                "surfaces": ["docs"],
                "audience": "user",
                "text_or_command": "readiness",
                "authority": "experience_contract",
                "evidence_refs": ["historical receipt"],
                "status": "bounded",
                "public_context": "swarm",
                "limitation": "historical claim"
            }]
        });
        assert_eq!(
            classify_v1_catalog(&v1_historical),
            HistoricalV1Disposition::HistoricalV1Accepted
        );

        assert_eq!(
            classify_v1_catalog(&json!({"schema_version": "something.else"})),
            HistoricalV1Disposition::Invalid
        );
        assert_eq!(
            classify_v1_catalog(&json!({"schema_version": HISTORICAL_V1_SCHEMA})),
            HistoricalV1Disposition::Invalid
        );
    }

    #[test]
    fn no_cli_command_surface_is_added() {
        // #10333 lands a library contract; a validator command belongs to a
        // later leaf that consumes it (#11432+). This guard mirrors the
        // import-cleanup precedent.
        let root = crate::utils::project_root().expect("project root");
        let main_rs = std::fs::read_to_string(root.join("xtask/src/main.rs")).expect("main.rs");
        assert!(
            !main_rs.contains("public_release_claims::run")
                && !main_rs.contains("Commands::PublicReleaseClaims"),
            "public_release_claims must not grow a CLI command surface in #10333"
        );
    }
}
