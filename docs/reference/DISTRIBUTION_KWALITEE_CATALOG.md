# Native CPANTS-compatible catalog v1

> Generated from `crates/perl-release-readiness/distribution_kwalitee_catalog.v1.toml`.
> Do not edit this table independently.

## Contract

- kind: `distribution_kwalitee.catalog`
- catalog version: `v1`
- schema version: `1`
- status: `frozen`
- scoring: `compatible_core_score = passed applicable cpants_offline_core / applicable cpants_offline_core`
- extra, experimental, site analogue, native extension, and deferred rows never enter the compatible core score
- invalid input has no ordinary score
- authoring trees are not staged input and have no ordinary score
- unverified required core evidence stays in the denominator; strict staged evaluation is incomplete
- a NotApplicable observation cannot drop an applicable core row from the denominator
- production runtime: `native_rust_offline`
- oracle role: `test_only_pinned_cpants`
- Module::CPANTS::Analyse: `1.03`
- SiteKwalitee: `github.com/cpants/Module-CPANTS-SiteKwalitee@2025-01-18`
- metrics: 62 (31 compatible-core)

## Metrics

| ID | Alias | Class | Score | Relationship | Source | Owner | Fixtures |
|---|---|---|---|---|---|---:|---|
| `cpants.has_readme` | `has_readme` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_has_readme` |
| `cpants.has_manifest` | `has_manifest` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_has_manifest` |
| `cpants.has_meta_yml` | `has_meta_yml` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_has_meta_yml` |
| `cpants.has_meta_json` | `has_meta_json` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::Kwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_has_meta_json` |
| `cpants.has_buildtool` | `has_buildtool` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_has_buildtool` |
| `cpants.has_changelog` | `has_changelog` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_has_changelog` |
| `cpants.no_files_to_be_skipped` | `no_files_to_be_skipped` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_no_files_to_be_skipped` |
| `cpants.no_symlinks` | `no_symlinks` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_no_symlinks` |
| `cpants.has_tests` | `has_tests` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_has_tests` |
| `cpants.has_tests_in_t_dir` | `has_tests_in_t_dir` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::Kwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_has_tests_in_t_dir` |
| `cpants.no_stdin_for_prompting` | `no_stdin_for_prompting` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::Kwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_no_stdin_for_prompting` |
| `cpants.no_maniskip_error` | `no_maniskip_error` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::Kwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_no_maniskip_error` |
| `cpants.manifest_matches_dist` | `manifest_matches_dist` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::Manifest` | #7178 | `Acme-CatalogFreeze`, `defect_manifest_matches_dist` |
| `cpants.meta_yml_is_parsable` | `meta_yml_is_parsable` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::MetaYML` | #4806 | `Acme-CatalogFreeze`, `defect_meta_yml_is_parsable` |
| `cpants.meta_json_is_parsable` | `meta_json_is_parsable` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::MetaYML` | #4806 | `Acme-CatalogFreeze`, `defect_meta_json_is_parsable` |
| `cpants.meta_yml_has_provides` | `meta_yml_has_provides` | `cpants_offline_experimental` | no | `direct` | `Module::CPANTS::Kwalitee::MetaYML` | #4806 | `Acme-CatalogFreeze`, `defect_meta_yml_has_provides` |
| `cpants.meta_yml_conforms_to_known_spec` | `meta_yml_conforms_to_known_spec` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::MetaYML` | #4806 | `Acme-CatalogFreeze`, `defect_meta_yml_conforms_to_known_spec` |
| `cpants.meta_json_conforms_to_known_spec` | `meta_json_conforms_to_known_spec` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::MetaYML` | #4806 | `Acme-CatalogFreeze`, `defect_meta_json_conforms_to_known_spec` |
| `cpants.meta_yml_declares_perl_version` | `meta_yml_declares_perl_version` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::Kwalitee::MetaYML` | #4806 | `Acme-CatalogFreeze`, `defect_meta_yml_declares_perl_version` |
| `cpants.meta_yml_has_repository_resource` | `meta_yml_has_repository_resource` | `cpants_offline_experimental` | no | `direct` | `Module::CPANTS::Kwalitee::MetaYML` | #4806 | `Acme-CatalogFreeze`, `defect_meta_yml_has_repository_resource` |
| `cpants.proper_libs` | `proper_libs` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::Kwalitee::FindModules` | #4806 | `Acme-CatalogFreeze`, `defect_proper_libs` |
| `cpants.no_missing_files_in_provides` | `no_missing_files_in_provides` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::Kwalitee::FindModules` | #4806 | `Acme-CatalogFreeze`, `defect_no_missing_files_in_provides` |
| `cpants.meta_yml_has_license` | `meta_yml_has_license` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::Kwalitee::License` | #4806 | `Acme-CatalogFreeze`, `defect_meta_yml_has_license` |
| `cpants.has_human_readable_license` | `has_human_readable_license` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::License` | #4806 | `Acme-CatalogFreeze`, `defect_has_human_readable_license` |
| `cpants.has_separate_license_file` | `has_separate_license_file` | `cpants_offline_experimental` | no | `direct` | `Module::CPANTS::Kwalitee::License` | #4806 | `Acme-CatalogFreeze`, `defect_has_separate_license_file` |
| `cpants.has_license_in_source_file` | `has_license_in_source_file` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::License` | #4806 | `Acme-CatalogFreeze`, `defect_has_license_in_source_file` |
| `cpants.has_known_license_in_source_file` | `has_known_license_in_source_file` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::Kwalitee::License` | #4806 | `Acme-CatalogFreeze`, `defect_has_known_license_in_source_file` |
| `cpants.has_abstract_in_pod` | `has_abstract_in_pod` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::Pod` | #4806 | `Acme-CatalogFreeze`, `defect_has_abstract_in_pod` |
| `cpants.no_abstract_stub_in_pod` | `no_abstract_stub_in_pod` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::Kwalitee::Pod` | #4806 | `Acme-CatalogFreeze`, `defect_no_abstract_stub_in_pod` |
| `cpants.no_broken_module_install` | `no_broken_module_install` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::BrokenInstaller` | #4806 | `Acme-CatalogFreeze`, `defect_no_broken_module_install` |
| `cpants.no_broken_auto_install` | `no_broken_auto_install` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::BrokenInstaller` | #4806 | `Acme-CatalogFreeze`, `defect_no_broken_auto_install` |
| `cpants.use_strict` | `use_strict` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::Kwalitee::Uses` | #4806 | `Acme-CatalogFreeze`, `defect_use_strict` |
| `cpants.use_warnings` | `use_warnings` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::Kwalitee::Uses` | #4806 | `Acme-CatalogFreeze`, `defect_use_warnings` |
| `cpants.buildtool_not_executable` | `buildtool_not_executable` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::SiteKwalitee::Files` | #7178 | `Acme-CatalogFreeze`, `defect_buildtool_not_executable` |
| `cpants.no_generated_files` | `no_generated_files` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::SiteKwalitee::Files` | #4806 | `Acme-CatalogFreeze`, `defect_no_generated_files` |
| `cpants.portable_filenames` | `portable_filenames` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::SiteKwalitee::Files` | #4806 | `Acme-CatalogFreeze`, `defect_portable_filenames` |
| `cpants.no_dot_underscore_files` | `no_dot_underscore_files` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::SiteKwalitee::Files` | #4806 | `Acme-CatalogFreeze`, `defect_no_dot_underscore_files` |
| `cpants.no_dot_dirs` | `no_dot_dirs` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::SiteKwalitee::Files` | #4806 | `Acme-CatalogFreeze`, `defect_no_dot_dirs` |
| `cpants.no_local_dirs` | `no_local_dirs` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::SiteKwalitee::Files` | #4806 | `Acme-CatalogFreeze`, `defect_no_local_dirs` |
| `cpants.extractable` | `extractable` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::SiteKwalitee::Extraction` | #4805 | `Acme-CatalogFreeze`, `defect_extractable` |
| `cpants.extracts_nicely` | `extracts_nicely` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::SiteKwalitee::Extraction` | #4805 | `Acme-CatalogFreeze`, `defect_extracts_nicely` |
| `cpants.no_pax_headers` | `no_pax_headers` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::SiteKwalitee::Extraction` | #4805 | `Acme-CatalogFreeze`, `defect_no_pax_headers` |
| `cpants.has_version` | `has_version` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::SiteKwalitee::DistVersion` | #4806 | `Acme-CatalogFreeze`, `defect_has_version` |
| `cpants.has_proper_version` | `has_proper_version` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::SiteKwalitee::DistVersion` | #4806 | `Acme-CatalogFreeze`, `defect_has_proper_version` |
| `cpants.distname_matches_name_in_meta` | `distname_matches_name_in_meta` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::SiteKwalitee::Distname` | #4806 | `Acme-CatalogFreeze`, `defect_distname_matches_name_in_meta` |
| `cpants.no_invalid_versions` | `no_invalid_versions` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::SiteKwalitee::Version` | #4806 | `Acme-CatalogFreeze`, `defect_no_invalid_versions` |
| `cpants.consistent_version` | `consistent_version` | `cpants_offline_extra` | no | `direct` | `Module::CPANTS::SiteKwalitee::Version` | #4806 | `Acme-CatalogFreeze`, `defect_consistent_version` |
| `cpants.main_module_version_matches_dist_version` | `main_module_version_matches_dist_version` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::SiteKwalitee::Version` | #4806 | `Acme-CatalogFreeze`, `defect_main_module_version_matches_dist_version` |
| `cpants.no_pod_errors` | `no_pod_errors` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::SiteKwalitee::Pod` | #4806 | `Acme-CatalogFreeze`, `defect_no_pod_errors` |
| `cpants.no_mymeta_files` | `no_mymeta_files` | `cpants_offline_core` | core | `direct` | `Module::CPANTS::SiteKwalitee::MyMeta` | #4806 | `Acme-CatalogFreeze`, `defect_no_mymeta_files` |
| `cpants.valid_signature` | `valid_signature` | `cpants_offline_extra` | no | `adapted` | `Module::CPANTS::SiteKwalitee::Signature` | #4806 | `Acme-CatalogFreeze`, `defect_valid_signature` |
| `cpants.no_unauthorized_packages` | `no_unauthorized_packages` | `unsupported_or_deferred` | no | `deferred` | `Module::CPANTS::SiteKwalitee::Permissions` | #7170 | `Acme-CatalogFreeze`, `defect_no_unauthorized_packages` |
| `cpants.is_prereq` | `is_prereq` | `unsupported_or_deferred` | no | `deferred` | `Module::CPANTS::SiteKwalitee::Prereq` | #7170 | `Acme-CatalogFreeze`, `defect_is_prereq` |
| `cpants.prereq_matches_use` | `prereq_matches_use` | `cpants_site_analogue` | no | `site_analogue` | `Module::CPANTS::SiteKwalitee::Prereq` | #4806 | `Acme-CatalogFreeze`, `defect_prereq_matches_use` |
| `cpants.test_prereq_matches_use` | `test_prereq_matches_use` | `cpants_site_analogue` | no | `site_analogue` | `Module::CPANTS::SiteKwalitee::Prereq` | #4806 | `Acme-CatalogFreeze`, `defect_test_prereq_matches_use` |
| `cpants.configure_prereq_matches_use` | `configure_prereq_matches_use` | `cpants_site_analogue` | no | `site_analogue` | `Module::CPANTS::SiteKwalitee::Prereq` | #4806 | `Acme-CatalogFreeze`, `defect_configure_prereq_matches_use` |
| `cpants.has_security_doc` | `has_security_doc` | `cpants_offline_experimental` | no | `direct` | `Module::CPANTS::SiteKwalitee::Security` | #4806 | `Acme-CatalogFreeze`, `defect_has_security_doc` |
| `cpants.security_doc_contains_contact` | `security_doc_contains_contact` | `cpants_offline_experimental` | no | `direct` | `Module::CPANTS::SiteKwalitee::Security` | #4806 | `Acme-CatalogFreeze`, `defect_security_doc_contains_contact` |
| `cpants.has_contributing_doc` | `has_contributing_doc` | `cpants_offline_experimental` | no | `direct` | `Module::CPANTS::SiteKwalitee::Security` | #4806 | `Acme-CatalogFreeze`, `defect_has_contributing_doc` |
| `cpants.easily_repackageable_by_debian` | `easily_repackageable_by_debian` | `unsupported_or_deferred` | no | `deferred` | `Module::CPANTS::SiteKwalitee::Debian` | #7170 | `Acme-CatalogFreeze`, `defect_easily_repackageable_by_debian` |
| `cpants.easily_repackageable_by_fedora` | `easily_repackageable_by_fedora` | `unsupported_or_deferred` | no | `deferred` | `Module::CPANTS::SiteKwalitee::Fedora` | #7170 | `Acme-CatalogFreeze`, `defect_easily_repackageable_by_fedora` |
| `cpants.fits_fedora_license` | `fits_fedora_license` | `unsupported_or_deferred` | no | `deferred` | `Module::CPANTS::SiteKwalitee::Fedora` | #7170 | `Acme-CatalogFreeze`, `defect_fits_fedora_license` |

## Interpretation

- `cpants_offline_core` is the only class that participates in `compatible_core_score`.
- Site analogues keep a narrower local claim and cannot masquerade as compatible core.
- Fixture identities are frozen here; reserved trees are filled by #8433/#9220.
- This catalog does not implement indicators, load archives, or invoke CPANTS.
