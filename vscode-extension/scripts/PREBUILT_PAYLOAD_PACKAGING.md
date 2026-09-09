# Exact prebuilt payload packaging

The manifest-enabled package route consumes an externally generated
`vsix_candidate_payload.v1` manifest from #9932 and an explicit projection input.
It is selected with these environment variables:

- `PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST`: canonical candidate payload manifest;
- `PERL_LSP_VSIX_PROJECTION_INPUT`: release-topology projection input consumed by
  `deriveVsixTargetProjection`;
- `PERL_LSP_PREBUILT_SERVER_PATH` and, when required by the projection,
  `PERL_LSP_PREBUILT_DAP_PATH`: exact payload files;
- `PERL_LSP_CURRENT_SOURCE_SHA` and `PERL_LSP_RUST_TARGET`: admitted source and
  native-target identities;
- `PERL_LSP_VSCODE_TARGET`: optional target override; the validated projection
  target is authoritative for `vsce` and archive-member checks;

The route stages only the validated target members, verifies their archive bytes
and semantic inventory digest, rejects duplicate or non-regular ZIP payload
members while streaming their bytes, then restores the worktree. Ordinary binaryless
packaging remains available without a manifest. Any ambient native server or DAP
member requires the manifest route.

This route does not approve release baselines or publish artifacts. The existing
inventory checker remains host-platform based, so cross-host and universal-managed
acceptance remain outside this candidate's claim and require their owning package
policy work.
