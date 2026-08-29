//! Content-addressed Eglot registration patch packet (#13613).
//!
//! The packet prepares one exact upstream source change. It performs no
//! upstream write and proves no Emacs host behavior or released discovery.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCHEMA_VERSION: &str = "emacs_eglot_upstream_patch.v1";
pub const CLAIM_CEILING: &str = "upstream_patch_prepared";
pub const BASE_REPOSITORY: &str = "https://github.com/emacs-mirror/emacs";
pub const BASE_COMMIT: &str = "f4f249a2249a7047ba41a659b8fcdcd7e1caf4e0";
pub const BASE_TREE_SHA1: &str = "ffd5ed14f7cc689e22163527e47d6ae0d0acbea0";
pub const BASE_PATH: &str = "lisp/progmodes/eglot.el";
pub const BASE_BLOB_SHA1: &str = "f2a9e36989cd90500e66900efe5138e6dee56668";

pub const BEFORE_ANCHOR: &str = concat!(
    "    ((perl-mode cperl-mode)\n",
    "     . (\"perl\" \"-MPerl::LanguageServer\" \"-e\" ",
    "\"Perl::LanguageServer::run\"))\n",
);

pub const AFTER_ANCHOR: &str = concat!(
    "    (((perl-mode :language-id \"perl\")\n",
    "      (cperl-mode :language-id \"perl\"))\n",
    "     . ,(eglot-alternatives\n",
    "         '((\"perllsp\" \"--stdio\")\n",
    "           (\"perl\" \"-MPerl::LanguageServer\" \"-e\"\n",
    "            \"Perl::LanguageServer::run\"))))\n",
);

pub const UNIFIED_DIFF: &str = concat!(
    "--- a/lisp/progmodes/eglot.el\n",
    "+++ b/lisp/progmodes/eglot.el\n",
    "@@ -349,2 +349,6 @@\n",
    "-    ((perl-mode cperl-mode)\n",
    "-     . (\"perl\" \"-MPerl::LanguageServer\" \"-e\" ",
    "\"Perl::LanguageServer::run\"))\n",
    "+    (((perl-mode :language-id \"perl\")\n",
    "+      (cperl-mode :language-id \"perl\"))\n",
    "+     . ,(eglot-alternatives\n",
    "+         '((\"perllsp\" \"--stdio\")\n",
    "+           (\"perl\" \"-MPerl::LanguageServer\" \"-e\"\n",
    "+            \"Perl::LanguageServer::run\"))))\n",
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamSourceIdentity {
    pub repository: String,
    pub commit: String,
    pub tree_sha1: String,
    pub path: String,
    pub blob_sha1: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EglotPatchPacket {
    pub schema_version: String,
    pub packet_id: String,
    pub claim_ceiling: String,
    pub external_action_authorized: bool,
    pub base: UpstreamSourceIdentity,
    pub before_anchor: String,
    pub after_anchor: String,
    pub unified_diff: String,
    pub upstream_checks: Vec<String>,
    pub actual_host_prerequisites: Vec<u32>,
    pub proposed_commit_title: String,
    pub proposed_pr_body: String,
    pub limitations: Vec<String>,
}

impl EglotPatchPacket {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == SCHEMA_VERSION,
            "schema_version must be {SCHEMA_VERSION}"
        );
        ensure!(
            self.claim_ceiling == CLAIM_CEILING,
            "claim_ceiling must be {CLAIM_CEILING}"
        );
        ensure!(
            !self.external_action_authorized,
            "patch preparation cannot authorize an upstream write"
        );
        ensure!(
            self.base == expected_base(),
            "patch must bind the exact audited Eglot source"
        );
        ensure!(
            self.before_anchor == BEFORE_ANCHOR,
            "patch must replace the exact current Perl contact"
        );
        ensure!(
            self.after_anchor == AFTER_ANCHOR,
            "patched contact must preserve the reviewed selector and alternative order"
        );
        ensure!(
            self.unified_diff == UNIFIED_DIFF,
            "unified diff must match the reviewed exact-anchor replacement"
        );
        validate_after_anchor(&self.after_anchor)?;
        ensure!(
            strings_equal(
                &self.upstream_checks,
                &[
                    "emacs --batch -Q -L lisp/progmodes -l eglot --eval '(message \\"eglot-load-ok\\")'",
                    "make -C test lisp/progmodes/eglot-tests",
                ]
            ),
            "upstream checks must remain explicit and bounded"
        );
        ensure!(
            self.actual_host_prerequisites
                .iter()
                .copied()
                .eq([7708, 7126, 7721]),
            "packet must retain the exact host/protocol blockers"
        );
        ensure!(
            !self.proposed_commit_title.trim().is_empty()
                && !self.proposed_pr_body.trim().is_empty(),
            "proposed upstream correspondence must be present"
        );
        ensure!(
            strings_equal(
                &self.limitations,
                &[
                    "no_actual_emacs_host_execution",
                    "no_upstream_submission_or_acceptance",
                    "no_released_client_discovery",
                ]
            ),
            "claim limitations must remain complete and deterministic"
        );
        ensure!(
            self.packet_id == self.expected_packet_id()?,
            "packet_id must content-address the complete packet"
        );
        Ok(())
    }

    pub fn apply_to_source(&self, source: &str) -> Result<String> {
        self.validate()?;
        ensure!(
            source.matches(self.before_anchor.as_str()).count() == 1,
            "exact Eglot Perl contact must appear once before patching"
        );
        ensure!(
            !source.contains(self.after_anchor.as_str()),
            "patched Eglot contact already exists in the source"
        );
        Ok(source.replacen(
            self.before_anchor.as_str(),
            self.after_anchor.as_str(),
            1,
        ))
    }

    fn expected_packet_id(&self) -> Result<String> {
        let mut canonical = self.clone();
        canonical.packet_id.clear();
        let bytes = serde_json::to_vec(&canonical)?;
        let digest = format!("{:x}", Sha256::digest(bytes));
        Ok(format!("eglot_patch_{}", &digest[..16]))
    }
}

pub fn checked_packet() -> Result<EglotPatchPacket> {
    let mut packet = EglotPatchPacket {
        schema_version: SCHEMA_VERSION.to_string(),
        packet_id: String::new(),
        claim_ceiling: CLAIM_CEILING.to_string(),
        external_action_authorized: false,
        base: expected_base(),
        before_anchor: BEFORE_ANCHOR.to_string(),
        after_anchor: AFTER_ANCHOR.to_string(),
        unified_diff: UNIFIED_DIFF.to_string(),
        upstream_checks: vec![
            "emacs --batch -Q -L lisp/progmodes -l eglot --eval '(message \\"eglot-load-ok\\")'"
                .to_string(),
            "make -C test lisp/progmodes/eglot-tests".to_string(),
        ],
        actual_host_prerequisites: vec![7708, 7126, 7721],
        proposed_commit_title: "Eglot: prefer perllsp for Perl buffers".to_string(),
        proposed_pr_body: concat!(
            "Prefer perllsp --stdio for perl-mode and cperl-mode when it is installed, ",
            "while retaining Perl::LanguageServer as the fallback. Both modes explicitly ",
            "use the LSP language ID perl; this patch adds no download or client-specific ",
            "protocol behavior."
        )
        .to_string(),
        limitations: vec![
            "no_actual_emacs_host_execution".to_string(),
            "no_upstream_submission_or_acceptance".to_string(),
            "no_released_client_discovery".to_string(),
        ],
    };
    packet.packet_id = packet.expected_packet_id()?;
    packet.validate()?;
    Ok(packet)
}

pub fn render_checked_json() -> Result<String> {
    let packet = checked_packet()?;
    let mut rendered = serde_json::to_string_pretty(&packet)?;
    rendered.push('\n');
    Ok(rendered)
}

fn expected_base() -> UpstreamSourceIdentity {
    UpstreamSourceIdentity {
        repository: BASE_REPOSITORY.to_string(),
        commit: BASE_COMMIT.to_string(),
        tree_sha1: BASE_TREE_SHA1.to_string(),
        path: BASE_PATH.to_string(),
        blob_sha1: BASE_BLOB_SHA1.to_string(),
    }
}

fn validate_after_anchor(anchor: &str) -> Result<()> {
    ensure!(
        anchor.contains("((perl-mode :language-id \"perl\")"),
        "perl-mode must explicitly negotiate language ID perl"
    );
    ensure!(
        anchor.contains("(cperl-mode :language-id \"perl\")"),
        "cperl-mode must explicitly negotiate language ID perl"
    );
    let perllsp = anchor
        .find("(\"perllsp\" \"--stdio\")")
        .ok_or_else(|| anyhow::anyhow!("perllsp --stdio alternative is missing"))?;
    let legacy = anchor
        .find("Perl::LanguageServer::run")
        .ok_or_else(|| anyhow::anyhow!("legacy Perl::LanguageServer fallback is missing"))?;
    ensure!(
        perllsp < legacy,
        "perllsp must precede the ubiquitous perl fallback"
    );
    ensure!(
        !anchor.contains("perl-ts-mode"),
        "third-party perl-ts-mode is outside the core upstream patch"
    );
    Ok(())
}

fn strings_equal(actual: &[String], expected: &[&str]) -> bool {
    actual.iter().map(String::as_str).eq(expected.iter().copied())
}