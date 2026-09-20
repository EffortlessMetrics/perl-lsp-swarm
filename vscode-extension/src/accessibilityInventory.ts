export type AccessibilityControlKind =
  | 'status_bar'
  | 'command'
  | 'notification'
  | 'quick_pick'
  | 'webview'
  | 'test_explorer'
  | 'debugger'
  | 'walkthrough';

export type AccessibilityEvidenceClass =
  | 'native_inherited'
  | 'semantic_automated_proven'
  | 'keyboard_automated_proven'
  | 'manual_screen_reader_required'
  | 'manual_theme_review_required'
  | 'not_proven';

export type AccessibilityThemePolicy = 'native' | 'vscode_theme_variables' | 'not_applicable';
export type AccessibilityZoomPolicy = 'native' | 'reflow_required' | 'not_applicable';

export interface AccessibilitySurface {
  surface_id: string;
  owner: string;
  control_kind: AccessibilityControlKind;
  native_accessibility_inherited: boolean;
  required_product_path: boolean;
  keyboard_route: string | null;
  accessible_name_source: string | null;
  textual_state: boolean;
  color_or_icon_only: boolean;
  theme_policy: AccessibilityThemePolicy;
  zoom_policy: AccessibilityZoomPolicy;
  evidence: AccessibilityEvidenceClass;
}

export interface AccessibilityInventory {
  schema_version: 'vscode_accessibility_inventory.v1';
  surfaces: AccessibilitySurface[];
}

export const CURRENT_ACCESSIBILITY_INVENTORY: AccessibilityInventory = {
  schema_version: 'vscode_accessibility_inventory.v1',
  surfaces: [
    {
      surface_id: 'workspace_status',
      owner: 'HealthWidget/statusBarItem',
      control_kind: 'status_bar',
      native_accessibility_inherited: false,
      required_product_path: true,
      keyboard_route: 'perl-lsp.showWorkspaceStatus',
      accessible_name_source: 'StatusBarItem.accessibilityInformation',
      textual_state: true,
      color_or_icon_only: false,
      theme_policy: 'native',
      zoom_policy: 'native',
      evidence: 'semantic_automated_proven',
    },
    {
      surface_id: 'startup_health_repair',
      owner: 'server command group / startup diagnosis',
      control_kind: 'notification',
      native_accessibility_inherited: true,
      required_product_path: true,
      keyboard_route: 'perl-lsp.runHealthCheck',
      accessible_name_source: 'native notification action labels',
      textual_state: true,
      color_or_icon_only: false,
      theme_policy: 'native',
      zoom_policy: 'native',
      evidence: 'native_inherited',
    },
    {
      surface_id: 'managed_binary_repair',
      owner: 'server command group / BinaryDownloader',
      control_kind: 'command',
      native_accessibility_inherited: true,
      required_product_path: true,
      keyboard_route: 'perl-lsp.reinstall',
      accessible_name_source: 'command title and native action label',
      textual_state: true,
      color_or_icon_only: false,
      theme_policy: 'native',
      zoom_policy: 'native',
      evidence: 'native_inherited',
    },
    {
      surface_id: 'report_issue',
      owner: 'supportCommands.ts',
      control_kind: 'command',
      native_accessibility_inherited: true,
      required_product_path: false,
      keyboard_route: 'perl-lsp.reportIssue',
      accessible_name_source: 'command title and native message actions',
      textual_state: true,
      color_or_icon_only: false,
      theme_policy: 'native',
      zoom_policy: 'native',
      evidence: 'native_inherited',
    },
    {
      surface_id: 'pod_preview',
      owner: 'podPreview.ts',
      control_kind: 'webview',
      native_accessibility_inherited: false,
      required_product_path: false,
      keyboard_route: null,
      accessible_name_source: 'generated semantic HTML',
      textual_state: true,
      color_or_icon_only: false,
      theme_policy: 'vscode_theme_variables',
      zoom_policy: 'reflow_required',
      evidence: 'not_proven',
    },
    {
      surface_id: 'test_explorer_actions',
      owner: 'PerlTestAdapter / VS Code Test Explorer',
      control_kind: 'test_explorer',
      native_accessibility_inherited: true,
      required_product_path: false,
      keyboard_route: 'native Testing view and contributed test commands',
      accessible_name_source: 'native Test Explorer plus extension labels',
      textual_state: true,
      color_or_icon_only: false,
      theme_policy: 'native',
      zoom_policy: 'native',
      evidence: 'native_inherited',
    },
    {
      surface_id: 'dap_preview_actions',
      owner: 'debugAdapter.ts / VS Code debugger',
      control_kind: 'debugger',
      native_accessibility_inherited: true,
      required_product_path: false,
      keyboard_route: 'native Run and Debug view and contributed debug commands',
      accessible_name_source: 'native debugger plus extension configuration labels',
      textual_state: true,
      color_or_icon_only: false,
      theme_policy: 'native',
      zoom_policy: 'native',
      evidence: 'native_inherited',
    },
    {
      surface_id: 'getting_started_walkthrough',
      owner: 'package.json walkthrough / onboarding',
      control_kind: 'walkthrough',
      native_accessibility_inherited: true,
      required_product_path: false,
      keyboard_route: 'Welcome: Open Walkthrough',
      accessible_name_source: 'native walkthrough step titles/descriptions',
      textual_state: true,
      color_or_icon_only: false,
      theme_policy: 'native',
      zoom_policy: 'native',
      evidence: 'native_inherited',
    },
  ],
};

export function normalizedAccessibilityInventory(
  inventory: AccessibilityInventory,
): AccessibilityInventory {
  return {
    ...inventory,
    surfaces: [...inventory.surfaces].sort((left, right) =>
      left.surface_id.localeCompare(right.surface_id),
    ),
  };
}

export function validateAccessibilityInventory(inventory: AccessibilityInventory): string[] {
  const errors: string[] = [];
  const ids = new Set<string>();

  for (const surface of inventory.surfaces) {
    if (ids.has(surface.surface_id)) {
      errors.push(`duplicate accessibility surface: ${surface.surface_id}`);
    }
    ids.add(surface.surface_id);

    if (surface.required_product_path && surface.keyboard_route === null) {
      errors.push(`required surface has no keyboard route: ${surface.surface_id}`);
    }
    if (surface.required_product_path && surface.accessible_name_source === null) {
      errors.push(`required surface has no accessible name source: ${surface.surface_id}`);
    }
    if (surface.color_or_icon_only) {
      errors.push(`surface state cannot be color/icon only: ${surface.surface_id}`);
    }
    if (!surface.native_accessibility_inherited && surface.accessible_name_source === null) {
      errors.push(
        `custom surface must name its accessibility semantic source: ${surface.surface_id}`,
      );
    }
    if (surface.control_kind === 'webview') {
      if (surface.theme_policy !== 'vscode_theme_variables') {
        errors.push(`custom webview must use VS Code theme variables: ${surface.surface_id}`);
      }
      if (surface.zoom_policy !== 'reflow_required') {
        errors.push(
          `custom webview must declare zoom/reflow responsibility: ${surface.surface_id}`,
        );
      }
      if (surface.evidence === 'native_inherited') {
        errors.push(
          `custom webview cannot inherit all accessibility evidence from VS Code: ${surface.surface_id}`,
        );
      }
    }
  }

  return errors;
}

export function accessibilityEvidenceCounts(
  inventory: AccessibilityInventory,
): Record<AccessibilityEvidenceClass, number> {
  const counts: Record<AccessibilityEvidenceClass, number> = {
    native_inherited: 0,
    semantic_automated_proven: 0,
    keyboard_automated_proven: 0,
    manual_screen_reader_required: 0,
    manual_theme_review_required: 0,
    not_proven: 0,
  };
  for (const surface of inventory.surfaces) {
    counts[surface.evidence] += 1;
  }
  return counts;
}
