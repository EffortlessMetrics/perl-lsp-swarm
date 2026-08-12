import type {
  BinaryCompatibilityReason,
  BinaryIdentityResponseV1,
} from "./binaryIdentityProtocol.generated";

export type BinaryIdentityUiState =
  | "ready_exact"
  | "ready_partial"
  | "update_or_repair_required"
  | "configured_binary_incompatible"
  | "managed_binary_missing"
  | "unsupported"
  | "not_proven";

export type BinaryIdentityAction =
  | "none"
  | "refresh_identity"
  | "repair_managed_pair"
  | "inspect_configured_binary"
  | "copy_support_packet";

export type SelectedBinaryRole = "managed" | "user_supplied";

export interface BinaryIdentityPresentation {
  state: BinaryIdentityUiState;
  action: BinaryIdentityAction;
  quiet: boolean;
  label: string;
  detail: string;
  supportPacket: string;
  noticeKey?: string;
}

const mismatchReasons = new Set<BinaryCompatibilityReason>([
  "server_product_mismatch",
  "extension_identity_mismatch",
  "version_mismatch",
  "target_mismatch",
  "source_revision_mismatch",
  "candidate_mismatch",
  "dap_role_mismatch",
]);

function hasDefiniteMismatch(reasons: readonly BinaryCompatibilityReason[]): boolean {
  return reasons.some((reason) => mismatchReasons.has(reason));
}

function supportPacket(response: BinaryIdentityResponseV1): string {
  const packet = {
    feature_version: response.feature_version,
    compatibility: response.compatibility,
    reasons: [...response.reasons].sort(),
    server: response.server,
    dap: response.dap,
    expected_extension: response.expected_extension,
    server_instance_id: response.server_instance_id,
    environment_snapshot_id: response.environment_snapshot_id,
    limitations: response.limitations ?? [],
    redacted: true,
  };
  return JSON.stringify(packet, null, 2);
}

function noticeKey(response: BinaryIdentityResponseV1, state: BinaryIdentityUiState): string {
  return JSON.stringify({
    state,
    reasons: [...response.reasons].sort(),
    server_instance_id: response.server_instance_id,
    environment_snapshot_id: response.environment_snapshot_id,
  });
}

export function projectBinaryIdentityStatus(
  response: BinaryIdentityResponseV1,
  selectedRole: SelectedBinaryRole,
): BinaryIdentityPresentation {
  const copied = supportPacket(response);

  switch (response.compatibility) {
    case "exact_match":
      return {
        state: "ready_exact",
        action: "none",
        quiet: true,
        label: "Perl LSP server identity verified",
        detail: `${response.server.binary.executable} ${response.server.binary.version} matches the extension candidate.`,
        supportPacket: copied,
      };

    case "compatible_partial":
      return {
        state: "ready_partial",
        action: "copy_support_packet",
        quiet: true,
        label: "Perl LSP server identity is compatible but partial",
        detail: "The server is usable, but source, target, candidate, or DAP identity is not fully proven.",
        supportPacket: copied,
      };

    case "stale":
      return {
        state: "not_proven",
        action: "refresh_identity",
        quiet: true,
        label: "Perl LSP identity status is stale",
        detail: "The server process or workspace snapshot changed. Refresh identity before drawing a compatibility conclusion.",
        supportPacket: copied,
      };

    case "unsupported":
      return {
        state: "unsupported",
        action: "copy_support_packet",
        quiet: true,
        label: "This server does not support binary identity reporting",
        detail: "Ordinary LSP remains available; exact extension/server parity is not proven.",
        supportPacket: copied,
      };

    case "not_proven":
      return {
        state: "not_proven",
        action: "copy_support_packet",
        quiet: true,
        label: "Perl LSP binary identity is not proven",
        detail: "Identity evidence was unavailable or contradictory. This is not a product-failure verdict.",
        supportPacket: copied,
      };

    case "mismatch": {
      const definite = hasDefiniteMismatch(response.reasons);
      if (selectedRole === "managed") {
        const state: BinaryIdentityUiState = definite
          ? "update_or_repair_required"
          : "managed_binary_missing";
        return {
          state,
          action: "repair_managed_pair",
          quiet: false,
          label: "Perl LSP managed binary repair required",
          detail: "The selected server, DAP, target, source, or candidate does not match the extension expectation.",
          supportPacket: copied,
          noticeKey: noticeKey(response, state),
        };
      }
      const state: BinaryIdentityUiState = "configured_binary_incompatible";
      return {
        state,
        action: "inspect_configured_binary",
        quiet: false,
        label: "Configured Perl LSP binary is incompatible",
        detail: "Review the configured user-supplied binary. It will not be replaced automatically.",
        supportPacket: copied,
        noticeKey: noticeKey(response, state),
      };
    }
  }
}

export class BinaryIdentityNoticeTracker {
  private lastNoticeKey: string | undefined;

  shouldNotify(presentation: BinaryIdentityPresentation): boolean {
    if (presentation.quiet || presentation.noticeKey === undefined) {
      if (presentation.state === "ready_exact") {
        this.lastNoticeKey = undefined;
      }
      return false;
    }
    if (presentation.noticeKey === this.lastNoticeKey) {
      return false;
    }
    this.lastNoticeKey = presentation.noticeKey;
    return true;
  }
}
