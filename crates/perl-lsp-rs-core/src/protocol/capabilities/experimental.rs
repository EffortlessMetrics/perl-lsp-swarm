use super::BuildFlags;
use gen_lsp_types::ServerCapabilities;

pub(super) fn apply_experimental_features(_caps: &mut ServerCapabilities, _build: &BuildFlags) {
    // SEAM-EXPERIMENTAL-TYPEHIERARCHY exited with the substrate migration (#11803):
    // the selected substrate carries `type_hierarchy_provider` as a typed
    // `ServerCapabilities` field (PATCH-TYPEHIERARCHY), so the capability is no
    // longer injected under `experimental`. The negative gate for
    // `experimental.inlineCompletionProvider` remains authoritative.
}
