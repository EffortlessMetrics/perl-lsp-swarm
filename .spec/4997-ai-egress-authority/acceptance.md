# #4997 - AI egress activation authority acceptance

Each row names the invariant, the proof location, and the falsifier it
discriminates against.

| Row | Invariant | Proof | Falsifier |
| --- | --- | --- | --- |
| AI-AUTH-001 | Project `enabled=true` cannot arm backend (landed pre-slice; preserved) | `workspace_ai_completion_ignores_untrusted_endpoint_and_credential_settings`, `hostile_project_config_cannot_install_ai_backend_without_user_enable` | project arm restored |
| AI-AUTH-002/003/004/005 | Generic didChangeConfiguration, initializationOptions, unscoped and folder-scoped workspace/configuration shapes cannot arm or select | `generic_channel_ai_activation_shapes_fail_closed_across_clients` (core parser); `generic_client_channels_cannot_arm_or_select_ai_backend` (runtime construction); `hostile_generic_enable_cannot_enter_streaming_route` (transport) | workspace-derived payload arms user_enabled/provider/model/streaming |
| AI-AUTH-006 | Client-supplied scope/trust/client labels confer no authority | authority is set only by server-side constructor; parser never reads trust labels from payloads | payload field promotes authority |
| AI-AUTH-007 | Provider/model cannot exceed activation authority | `provider_and_model_selection_cannot_exceed_activation_authority`; runtime selection assertions in `generic_client_channels_cannot_arm_or_select_ai_backend` | selection accepted while activation unavailable |
| AI-AUTH-008 | Endpoint/credential/project authority remains absent (#4955/#5684 rows unchanged) | `client_configuration_ignores_ai_endpoint_and_credential_fields_from_didChange` | credential routing reopened |
| AI-AUTH-010 | Project opt-out reduces a trusted capability; monotonic | `project_config_can_opt_out_of_user_enabled_ai_completions`, `project_opt_out_clears_when_ai_completion_section_removed` | opt-out lost or inverted to enable |
| AI-AUTH-011 | Missing/mixed/unknown provenance is unauthorized, not default-user | default `AiActivationAuthority::Unavailable`; malformed-value rows in `hostile_and_malformed_traffic_preserves_accepted_trusted_ai_state` | unknown provenance defaults to trusted |
| AI-AUTH-012 | Unauthorized input creates zero egress-eligible backend | construction oracle configures endpoint + resolvable credential so only authority can prevent it (`generic_client_channels_cannot_arm_or_select_ai_backend`) | None passes vacuously via missing endpoint/key |
| AI-AUTH-013 | Accepted trusted state is not cleared by unauthorized traffic | trusted-preservation half of `hostile_and_malformed_traffic_preserves_accepted_trusted_ai_state`; runtime trusted-state block | hostile disable/malformed clears activation |
| AI-AUTH-014 | Secret values absent from warnings/receipts | rejection warnings name keys and channels only; no payload values logged | payload echo into logs |
| AI-AUTH-015 | Schema/catalog/docs match landed behavior | `generic_schema_excludes_security_sensitive_lsp_settings` transport assertions; catalog gates `ai_arm_and_select_rows_admit_only_trusted_operator_sources` + extended `client_channels_cannot_override_trusted_command_or_ai_transport_fields`; docs/reference/AI_COMPLETION.md honesty rewrite | inert activation advertised as client-settable |
| — (positive) | Legitimate trusted operator enable still constructs; not "AI impossible" | `refresh_ai_backend_installs_connector_auth_backend`, `trusted_operator_admission_arms_eligibility_generic_traffic_cannot`, trusted tail of `generic_client_channels_cannot_arm_or_select_ai_backend` | fail-closed overreach breaks future adapter |
| — (streaming) | Streaming activation follows the same authority law | `hostile_generic_disable_streaming_is_equally_unauthorized`; gated armed progress contract `streaming_completion_progress_schema_validation_armed`, `streaming_completion_cancel_rotates_session_identity` | streaming toggle implies/revokes authorization |

## Close condition

The controlling issue closes when no project/resource/generic/mixed/unknown
observation can construct or trigger a remote backend or select its identity,
project opt-out still reduces, public/catalog/schema contracts agree, and the
construction oracle is discriminating (usable transport configured). The
complete accepted-generation/session/cache cutover remains #10909; typed
observation transport remains #10817. This slice consumes the #4998
disposition pattern so those trains inherit one vocabulary.
