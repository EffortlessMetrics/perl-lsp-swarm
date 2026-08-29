//! Content-addressed lsp-mode client patch packet (#13614).
//!
//! This module prepares one exact upstream-source change across the client
//! module, automatic package loading, generated client catalog, and docs
//! navigation. It performs no upstream write and proves no Emacs host behavior.

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: &str = "emacs_lsp_mode_upstream_patch.v1";
pub const CLAIM_CEILING: &str = "upstream_patch_prepared";
pub const BASE_REPOSITORY: &str = "https://github.com/emacs-lsp/lsp-mode";
pub const BASE_COMMIT: &str = "e15b8205cbd0369df40b412909eb3ed3264e96a2";
pub const BASE_TREE_SHA1: &str = "51274cfa292c0b5ae0a70edb9c0c61b153b5f916";

pub const NEW_CLIENT_PATH: &str = "clients/lsp-perllsp.el";
pub const LSP_MODE_PATH: &str = "lsp-mode.el";
pub const CLIENT_CATALOG_PATH: &str = "docs/lsp-clients.json";
pub const MKDOCS_PATH: &str = "mkdocs.yml";

const LSP_MODE_BLOB_SHA1: &str = "9575875cd4c7ef49ab0bd8e5473e44f73c4a0c7d";
const CLIENT_CATALOG_BLOB_SHA1: &str = "913c2724516bc92b807c0d1d79a8c9147c855d10";
const MKDOCS_BLOB_SHA1: &str = "b4bcf68c800f13de02b98f414c21e2eb1b729edc";
const PERL_LANGUAGE_SERVER_BLOB_SHA1: &str = "28569f7ecf22a5b02762976ef338693c655679ad";
const PERL_NAVIGATOR_BLOB_SHA1: &str = "51cbc768c960c40433d61299cc837b94f7830423";
const PLS_BLOB_SHA1: &str = "f5437fbf739dd661d0850ede25ad7ef73dbf81d4";

pub const NEW_CLIENT_CONTENT: &str = r#";;; lsp-perllsp.el --- perllsp support for lsp-mode -*- lexical-binding: t; -*-

;; Copyright (C) 2026 Steven Zimmerman
;; Copyright (C) 2026 emacs-lsp maintainers

;; Author: Steven Zimmerman <git@effortlesssteven.com>
;; Keywords: perl, lsp

;; This file is not part of GNU Emacs.

;; This program is free software: you can redistribute it and/or modify
;; it under the terms of the GNU General Public License as published by
;; the Free Software Foundation, either version 3 of the License, or
;; (at your option) any later version.

;; This program is distributed in the hope that it will be useful,
;; but WITHOUT ANY WARRANTY; without even the implied warranty of
;; MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
;; GNU General Public License for more details.

;; You should have received a copy of the GNU General Public License
;; along with this program.  If not, see <https://www.gnu.org/licenses/>.

;;; Commentary:

;; lsp-mode client for perllsp.
;; https://github.com/EffortlessMetrics/perl-lsp

;;; Code:

(require 'lsp-mode)

(defgroup lsp-perllsp nil
  "LSP support for Perl, using perllsp."
  :group 'lsp-mode
  :link '(url-link "https://github.com/EffortlessMetrics/perl-lsp")
  :package-version '(lsp-mode . "10.0.1"))

(defcustom lsp-perllsp-executable "perllsp"
  "Command used to start perllsp."
  :type 'file
  :risky t
  :group 'lsp-perllsp
  :package-version '(lsp-mode . "10.0.1"))

(lsp-register-client
 (make-lsp-client
  :new-connection
  (lsp-stdio-connection
   (lambda () (list lsp-perllsp-executable "--stdio")))
  :activation-fn (lsp-activate-on "perl")
  :major-modes '(perl-mode cperl-mode)
  :priority 1
  :server-id 'perllsp))

(lsp-consistency-check lsp-perllsp)

(provide 'lsp-perllsp)
;;; lsp-perllsp.el ends here
"#;

const PACKAGE_BEFORE: &str = concat!(
    "     lsp-ocaml lsp-odin lsp-openscad lsp-pascal lsp-perl lsp-perlnavigator\n",
    "      lsp-php lsp-pls lsp-postgres lsp-prisma",
);
const PACKAGE_AFTER: &str = concat!(
    "     lsp-ocaml lsp-odin lsp-openscad lsp-pascal lsp-perl lsp-perllsp lsp-perlnavigator\n",
    "      lsp-php lsp-pls lsp-postgres lsp-prisma",
);

const CATALOG_BEFORE: &str = r#"  {
    "name": "perl",
    "full-name": "Perl",
    "server-name": "Perl::LanguageServer",
    "server-url": "https://github.com/richterger/Perl-LanguageServer",
    "installation": "cpan Perl::LanguageServer",
    "debugger": "Not available"
  },
  {
    "name": "perlnavigator",
"#;
const CATALOG_AFTER: &str = r#"  {
    "name": "perl",
    "full-name": "Perl",
    "server-name": "Perl::LanguageServer",
    "server-url": "https://github.com/richterger/Perl-LanguageServer",
    "installation": "cpan Perl::LanguageServer",
    "debugger": "Not available"
  },
  {
    "name": "perllsp",
    "full-name": "Perl LSP",
    "server-name": "perllsp",
    "server-url": "https://github.com/EffortlessMetrics/perl-lsp",
    "installation-url": "https://github.com/EffortlessMetrics/perl-lsp#installation",
    "installation": "cargo install perllsp",
    "debugger": "Not available"
  },
  {
    "name": "perlnavigator",
"#;

const MKDOCS_BEFORE: &str = concat!(
    "    - Perl (PLS): page/lsp-pls.md\n",
    "    - Perl (Perl::LanguageServer): page/lsp-perl.md\n",
    "    - Perl (Navigator): page/lsp-perlnavigator.md\n",
);
const MKDOCS_AFTER: &str = concat!(
    "    - Perl (PLS): page/lsp-pls.md\n",
    "    - Perl (Perl::LanguageServer): page/lsp-perl.md\n",
    "    - Perl (perllsp): page/lsp-perllsp.md\n",
    "    - Perl (Navigator): page/lsp-perlnavigator.md\n",
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamFileIdentity {
    pub path: String,
    pub blob_sha1: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExistingPerlClient {
    pub server_id: String,
    pub priority: i32,
    pub source: UpstreamFileIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddedFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactReplacement {
    pub source: UpstreamFileIdentity,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionCase {
    pub case_id: String,
    pub expected_disposition: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LspModePatchPacket {
    pub schema_version: String,
    pub packet_id: String,
    pub patch_sha256: String,
    pub claim_ceiling: String,
    pub external_action_authorized: bool,
    pub base_repository: String,
    pub base_commit: String,
    pub base_tree_sha1: String,
    pub existing_perl_clients: Vec<ExistingPerlClient>,
    pub added_files: Vec<AddedFile>,
    pub replacements: Vec<ExactReplacement>,
    pub selection_cases: Vec<SelectionCase>,
    pub selection_rationale: String,
    pub unified_diff: String,
    pub upstream_checks: Vec<String>,
    pub actual_host_prerequisites: Vec<u32>,
    pub proposed_commit_title: String,
    pub proposed_pr_body: String,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestedUpstreamFile {
    pub blob_sha1: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestedUpstreamTree {
    pub repository: String,
    pub commit: String,
    pub tree_sha1: String,
    pub files: BTreeMap<String, AttestedUpstreamFile>,
}

impl LspModePatchPacket {
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
            self.base_repository == BASE_REPOSITORY
                && self.base_commit == BASE_COMMIT
                && self.base_tree_sha1 == BASE_TREE_SHA1,
            "patch must bind the exact audited lsp-mode source tree"
        );
        ensure!(
            self.existing_perl_clients == expected_existing_clients(),
            "existing Perl client identities and priorities must remain exact"
        );
        ensure!(
            self.added_files == expected_added_files(),
            "the patch must add exactly the reviewed lsp-perllsp client"
        );
        ensure!(
            self.replacements == expected_replacements(),
            "package, client-catalog, and navigation replacements must remain exact"
        );
        validate_new_client(&self.added_files[0].content)?;
        validate_selection_cases(&self.selection_cases)?;
        ensure!(
            self.selection_rationale
                == "priority 1 is the smallest integer above current Perl Navigator priority 0; major-mode restriction keeps perl-ts-mode outside the base client",
            "selection rationale must retain both priority and mode-boundary decisions"
        );
        ensure!(
            self.unified_diff == expected_unified_diff(),
            "unified patch bytes must match the reviewed structured changes"
        );
        ensure!(
            self.patch_sha256 == sha256_token(self.unified_diff.as_bytes()),
            "patch_sha256 must bind the exact unified diff"
        );
        ensure!(
            strings_equal(
                &self.upstream_checks,
                &[
                    "emacs --batch -Q -L . -L clients -l lsp-perllsp --eval '(lsp-consistency-check lsp-perllsp)'",
                    "make test",
                    "python scripts/generate-docs.py --check",
                ]
            ),
            "upstream proof commands must remain explicit and bounded"
        );
        ensure!(
            self.actual_host_prerequisites
                .iter()
                .copied()
                .eq([7708, 7727]),
            "packet must retain the exact host/protocol prerequisites"
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
                    "no_actual_lsp_mode_host_execution",
                    "no_upstream_submission_or_acceptance",
                    "no_released_client_discovery",
                    "perl_ts_mode_not_in_base_client",
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

    pub fn apply_to_attested_tree(
        &self,
        input: &AttestedUpstreamTree,
    ) -> Result<BTreeMap<String, String>> {
        self.validate()?;
        ensure!(
            input.repository == self.base_repository
                && input.commit == self.base_commit
                && input.tree_sha1 == self.base_tree_sha1,
            "attested tree does not match the packet's exact base"
        );

        let mut output = input
            .files
            .iter()
            .map(|(path, file)| (path.clone(), file.content.clone()))
            .collect::<BTreeMap<_, _>>();

        for replacement in &self.replacements {
            let input_file = input
                .files
                .get(&replacement.source.path)
                .ok_or_else(|| anyhow::anyhow!("missing attested file {}", replacement.source.path))?;
            ensure!(
                input_file.blob_sha1 == replacement.source.blob_sha1,
                "wrong blob for {}",
                replacement.source.path
            );
            ensure!(
                input_file.content.matches(&replacement.before).count() == 1,
                "exact replacement anchor must appear once in {}",
                replacement.source.path
            );
            ensure!(
                !input_file.content.contains(&replacement.after),
                "replacement already present in {}",
                replacement.source.path
            );
            output.insert(
                replacement.source.path.clone(),
                input_file
                    .content
                    .replacen(&replacement.before, &replacement.after, 1),
            );
        }

        for added in &self.added_files {
            ensure!(
                !input.files.contains_key(&added.path),
                "added client already exists at {}",
                added.path
            );
            output.insert(added.path.clone(), added.content.clone());
        }

        for client in &self.existing_perl_clients {
            let input_file = input
                .files
                .get(&client.source.path)
                .ok_or_else(|| anyhow::anyhow!("missing fallback client {}", client.server_id))?;
            ensure!(
                input_file.blob_sha1 == client.source.blob_sha1,
                "wrong fallback-client blob for {}",
                client.server_id
            );
            ensure!(
                output.get(&client.source.path) == Some(&input_file.content),
                "patch must not modify fallback client {}",
                client.server_id
            );
        }
        Ok(output)
    }

    fn expected_packet_id(&self) -> Result<String> {
        let mut canonical = self.clone();
        canonical.packet_id.clear();
        let digest = sha256_token(&serde_json::to_vec(&canonical)?);
        let suffix = digest
            .strip_prefix("sha256:")
            .and_then(|value| value.get(..16))
            .ok_or_else(|| anyhow::anyhow!("sha256 token is malformed"))?;
        Ok(format!("lsp_mode_patch_{suffix}"))
    }
}

pub fn checked_packet() -> Result<LspModePatchPacket> {
    let unified_diff = expected_unified_diff();
    let mut packet = LspModePatchPacket {
        schema_version: SCHEMA_VERSION.to_string(),
        packet_id: String::new(),
        patch_sha256: sha256_token(unified_diff.as_bytes()),
        claim_ceiling: CLAIM_CEILING.to_string(),
        external_action_authorized: false,
        base_repository: BASE_REPOSITORY.to_string(),
        base_commit: BASE_COMMIT.to_string(),
        base_tree_sha1: BASE_TREE_SHA1.to_string(),
        existing_perl_clients: expected_existing_clients(),
        added_files: expected_added_files(),
        replacements: expected_replacements(),
        selection_cases: expected_selection_cases(),
        selection_rationale: "priority 1 is the smallest integer above current Perl Navigator priority 0; major-mode restriction keeps perl-ts-mode outside the base client".to_string(),
        unified_diff,
        upstream_checks: vec![
            "emacs --batch -Q -L . -L clients -l lsp-perllsp --eval '(lsp-consistency-check lsp-perllsp)'".to_string(),
            "make test".to_string(),
            "python scripts/generate-docs.py --check".to_string(),
        ],
        actual_host_prerequisites: vec![7708, 7727],
        proposed_commit_title: "feat: add perllsp client".to_string(),
        proposed_pr_body: concat!(
            "Add a built-in lsp-mode client for perllsp using the explicit command ",
            "perllsp --stdio. The client activates for the canonical Perl language ID ",
            "but is restricted to perl-mode and cperl-mode, preserving perl-ts-mode as ",
            "a separate integration. Priority 1 makes an installed perllsp discoverable ",
            "without removing Perl Navigator, PLS, or Perl::LanguageServer fallbacks."
        )
        .to_string(),
        limitations: vec![
            "no_actual_lsp_mode_host_execution".to_string(),
            "no_upstream_submission_or_acceptance".to_string(),
            "no_released_client_discovery".to_string(),
            "perl_ts_mode_not_in_base_client".to_string(),
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

fn validate_new_client(content: &str) -> Result<()> {
    ensure!(
        content == NEW_CLIENT_CONTENT,
        "new lsp-perllsp client bytes must remain exact"
    );
    for required in [
        "(lambda () (list lsp-perllsp-executable \"--stdio\"))",
        ":activation-fn (lsp-activate-on \"perl\")",
        ":major-modes '(perl-mode cperl-mode)",
        ":priority 1",
        ":server-id 'perllsp",
        "(lsp-consistency-check lsp-perllsp)",
        "(provide 'lsp-perllsp)",
    ] {
        ensure!(content.contains(required), "new client is missing {required}");
    }
    for forbidden in [
        "perl-ts-mode",
        "lsp-dependency",
        "download-server-fn",
        "lsp-register-custom-settings",
        "initialization-options",
    ] {
        ensure!(
            !content.contains(forbidden),
            "new client contains forbidden first-patch surface {forbidden}"
        );
    }
    ensure!(
        !content.contains("perllsp --stdio\"")
            && !content.contains("\"perllsp --stdio\""),
        "program and --stdio must remain separate argv entries"
    );
    Ok(())
}

fn validate_selection_cases(cases: &[SelectionCase]) -> Result<()> {
    ensure!(
        cases == expected_selection_cases(),
        "selection matrix must retain all bounded coexistence cases"
    );
    let perllsp_priority = 1;
    let max_existing = expected_existing_clients()
        .iter()
        .map(|client| client.priority)
        .max()
        .ok_or_else(|| anyhow::anyhow!("existing Perl client set is empty"))?;
    ensure!(
        perllsp_priority > max_existing,
        "candidate priority must make installed perllsp discoverable"
    );
    Ok(())
}

fn expected_existing_clients() -> Vec<ExistingPerlClient> {
    vec![
        ExistingPerlClient {
            server_id: "perlnavigator".to_string(),
            priority: 0,
            source: file_identity("clients/lsp-perlnavigator.el", PERL_NAVIGATOR_BLOB_SHA1),
        },
        ExistingPerlClient {
            server_id: "pls".to_string(),
            priority: -1,
            source: file_identity("clients/lsp-pls.el", PLS_BLOB_SHA1),
        },
        ExistingPerlClient {
            server_id: "perl-language-server".to_string(),
            priority: -2,
            source: file_identity("clients/lsp-perl.el", PERL_LANGUAGE_SERVER_BLOB_SHA1),
        },
    ]
}

fn expected_added_files() -> Vec<AddedFile> {
    vec![AddedFile {
        path: NEW_CLIENT_PATH.to_string(),
        content: NEW_CLIENT_CONTENT.to_string(),
    }]
}

fn expected_replacements() -> Vec<ExactReplacement> {
    vec![
        ExactReplacement {
            source: file_identity(CLIENT_CATALOG_PATH, CLIENT_CATALOG_BLOB_SHA1),
            before: CATALOG_BEFORE.to_string(),
            after: CATALOG_AFTER.to_string(),
        },
        ExactReplacement {
            source: file_identity(LSP_MODE_PATH, LSP_MODE_BLOB_SHA1),
            before: PACKAGE_BEFORE.to_string(),
            after: PACKAGE_AFTER.to_string(),
        },
        ExactReplacement {
            source: file_identity(MKDOCS_PATH, MKDOCS_BLOB_SHA1),
            before: MKDOCS_BEFORE.to_string(),
            after: MKDOCS_AFTER.to_string(),
        },
    ]
}

fn expected_selection_cases() -> Vec<SelectionCase> {
    [
        ("perllsp_only", "perllsp_selected"),
        ("perllsp_with_perlnavigator", "perllsp_selected_priority_1_over_0"),
        ("perllsp_with_pls", "perllsp_selected_priority_1_over_minus_1"),
        (
            "perllsp_with_perl_language_server",
            "perllsp_selected_priority_1_over_minus_2",
        ),
        ("perllsp_absent", "existing_installed_client_remains_eligible"),
        (
            "perllsp_explicitly_disabled",
            "existing_enabled_client_remains_eligible",
        ),
        ("perllsp_explicitly_enabled", "perllsp_selected"),
        ("perl_ts_mode", "base_perllsp_client_not_eligible"),
        ("unrelated_language", "perllsp_not_eligible"),
    ]
    .into_iter()
    .map(|(case_id, expected_disposition)| SelectionCase {
        case_id: case_id.to_string(),
        expected_disposition: expected_disposition.to_string(),
    })
    .collect()
}

fn expected_unified_diff() -> String {
    let mut output = String::new();
    output.push_str(&added_file_diff(NEW_CLIENT_PATH, NEW_CLIENT_CONTENT));
    for replacement in expected_replacements() {
        output.push_str(&replacement_diff(&replacement));
    }
    output
}

fn added_file_diff(path: &str, content: &str) -> String {
    let line_count = content.lines().count();
    let mut output = format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{line_count} @@\n"
    );
    output.push_str(&prefixed_lines('+', content));
    output
}

fn replacement_diff(replacement: &ExactReplacement) -> String {
    let before_lines = replacement.before.lines().count();
    let after_lines = replacement.after.lines().count();
    let path = &replacement.source.path;
    let mut output = format!(
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,{before_lines} +1,{after_lines} @@\n"
    );
    output.push_str(&prefixed_lines('-', &replacement.before));
    output.push_str(&prefixed_lines('+', &replacement.after));
    output
}

fn prefixed_lines(prefix: char, content: &str) -> String {
    content
        .lines()
        .map(|line| format!("{prefix}{line}\n"))
        .collect()
}

fn file_identity(path: &str, blob_sha1: &str) -> UpstreamFileIdentity {
    UpstreamFileIdentity {
        path: path.to_string(),
        blob_sha1: blob_sha1.to_string(),
    }
}

fn sha256_token(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn strings_equal(actual: &[String], expected: &[&str]) -> bool {
    actual.iter().map(String::as_str).eq(expected.iter().copied())
}

pub fn require_expected_replacement_order(packet: &LspModePatchPacket) -> Result<()> {
    let paths = packet
        .replacements
        .iter()
        .map(|replacement| replacement.source.path.as_str());
    if paths.eq([CLIENT_CATALOG_PATH, LSP_MODE_PATH, MKDOCS_PATH]) {
        Ok(())
    } else {
        bail!("replacement order must remain deterministic")
    }
}