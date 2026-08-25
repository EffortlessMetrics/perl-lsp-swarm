# #4998 - External include-root authority acceptance

Each row names the invariant, the proof location, and the falsifier it
discriminates against.

| Row | Invariant | Proof | Falsifier |
| --- | --- | --- | --- |
| INC-AUTH-001 | Resource `includePaths` accept contained relative roots only | `update_from_value_rejects_absolute_include_paths`, `update_from_value_rejects_traversal_include_paths` (core) | absolute or traversal entry admitted |
| INC-AUTH-002 | Trusted user/operator adapter may propose validated external roots | `update_from_value_accepts_external_include_paths_from_trusted_operator` (core) | rule degenerates to "absolute paths impossible" |
| INC-AUTH-003 | Generic unscoped `workspace/configuration` result has no external-root authority | `unscoped_global_slot_cannot_authorize_external_roots` (runtime) | slot[0] absolute root applied to folders |
| INC-AUTH-004 | Per-folder/project/initialization/didChange input has no external-root authority | `hostile_client_duplicating_external_root_in_every_slot_stays_unauthorized`; `update_from_value_rejects_external_include_paths_from_folder_channel`; didChange sites pass `Untrusted(DidChangeConfiguration)` | duplicated malicious root admitted from any slot |
| INC-AUTH-005 | Client-supplied scope/trust labels confer no authority | authority is constructed by server call-site classification, never parsed from payloads | payload field promotes authority |
| INC-AUTH-006 | First result-array position confers no authority | same as INC-AUTH-003; `apply_workspace_configuration_results` classifies both slots untrusted | positional `true` restored |
| INC-AUTH-007 | Invalid/unauthorized candidates preserve prior complete state | `unauthorized_external_candidate_preserves_accepted_trusted_values` (core) | rejected candidate clears accepted set |
| INC-AUTH-008 | Runtime-derived roots are explicit and separately governed | `detected_roots_stay_relative_and_workspace_contained` recurrence gate | metadata-derived absolute root inherits resource validation by convention |
| INC-AUTH-009 | Zero outside-workspace read/index/provider visibility for unauthorized roots | `scenario_14_external_include_paths_unauthorized_zero_visibility` (ux end-to-end: PL701 fires, goto-def empty, completion absent, hover unresolved) | string-level rejection only while resolution still reads the file |
| INC-AUTH-010 | Contained relative roots keep resolving | ux positives: `scenario_14_relative_include_path`, `scenario_14_nested_module_relative_include_path`, PERL5LIB matrix | fail-closed overreach breaks legitimate users |
| INC-AUTH-011 | Product claims match server behavior | package.json setting text marks server application pending trusted transport | docs advertise inert capability as working |

## Close condition

The controlling issue closes when no resource/folder/generic/unscoped/unknown
observation can authorize an external root, contained relative roots remain
useful, declared catalog/runtime/schema authority agrees, and the outside
module is semantically invisible with zero outside-workspace reads. A future
trusted adapter (#10817) reopens capability, not the defect.
