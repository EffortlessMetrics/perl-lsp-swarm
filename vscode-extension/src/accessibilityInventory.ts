import { createHash } from 'crypto';

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
      keyboard_route: 'perl-lsp.previewPod',
      accessible_name_source: 'generated semantic HTML',
      textual_state: true,
      color_or_icon_only: false,
      theme_policy: 'vscode_theme_variables',
      zoom_policy: 'reflow_required',
      // Semantic structure, keyboard usability, and theme-variable styling of
      // the generated webview HTML are proven by automated contract tests
      // (#7865); screen-reader observation still requires a bounded manual
      // run, so the row can never claim full proven evidence.
      evidence: 'manual_screen_reader_required',
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

// ---------------------------------------------------------------------------
// Installed accessibility receipt (#7865)
// ---------------------------------------------------------------------------

/**
 * One per-dimension verdict of an installed accessibility exercise. `pass` and
 * `failed` require an observation from this run; anything unobserved is
 * `not_proven`. `manual_bounded` is reserved for a bounded human screen-reader
 * run and never produced by automation.
 */
export type AccessibilityDimensionVerdict = 'pass' | 'failed' | 'not_proven' | 'manual_bounded';

/** The dimensions of issue #7865's `vscode_accessibility.v1` surface row. */
export const ACCESSIBILITY_DIMENSIONS = [
  'keyboard',
  'semantic_labels',
  'screen_reader',
  'high_contrast',
  'zoom_reflow',
] as const;

export type AccessibilityDimension = (typeof ACCESSIBILITY_DIMENSIONS)[number];

export interface AccessibilitySurfaceReceipt {
  surface_id: string;
  keyboard: AccessibilityDimensionVerdict;
  semantic_labels: AccessibilityDimensionVerdict;
  screen_reader: AccessibilityDimensionVerdict;
  high_contrast: AccessibilityDimensionVerdict;
  zoom_reflow: AccessibilityDimensionVerdict;
}

/** Candidate identity the receipt is bound to (issue #7865 "Candidate identity"). */
export interface AccessibilityReceiptCandidate {
  vsix_version: string | null;
  vsix_sha256: string | null;
  vscode_version: string;
  platform: string;
  inventory_digest: string;
}

export interface AccessibilityReceipt {
  schema_version: 'vscode_accessibility.v1';
  candidate: AccessibilityReceiptCandidate;
  surfaces: AccessibilitySurfaceReceipt[];
  limitations: string[];
  verdict: 'pass' | 'bounded' | 'failed' | 'not_proven';
}

/**
 * What the installed harness could observe for one run. The child resolves the
 * installed manifest itself (including NLS titles); this module only consumes
 * the derived facts so the receipt math stays deterministic and unit-testable.
 */
export interface InstalledAccessibilityObservations {
  /** Command ids registered in the live extension host. */
  registeredCommands: ReadonlySet<string>;
  /** NLS-resolved human titles by command id, for commands that have one. */
  commandTitles: ReadonlyMap<string, string>;
  /** Command ids hidden from the command palette (a `when: 'false'` entry). */
  paletteHiddenCommands: ReadonlySet<string>;
}

/**
 * Stable digest of the current inventory contract.
 *
 * The installed receipt names this digest so consumers (#4346/#6056) can tell
 * which inventory version an installed exercise actually ran against, and so a
 * stale compiled inventory can never be mistaken for the candidate's.
 */
export function accessibilityInventoryDigest(inventory: AccessibilityInventory): string {
  return createHash('sha256')
    .update(JSON.stringify(normalizedAccessibilityInventory(inventory)))
    .digest('hex');
}

/** The keyboard route that names an extension-contributed command, if any. */
function contributedCommandRoute(surface: AccessibilitySurface): string | null {
  return surface.keyboard_route !== null && surface.keyboard_route.startsWith('perl-lsp.')
    ? surface.keyboard_route
    : null;
}

function isDimensionVerdict(value: unknown): value is AccessibilityDimensionVerdict {
  return (
    value === 'pass' || value === 'failed' || value === 'not_proven' || value === 'manual_bounded'
  );
}

/**
 * Compose the receipt verdict fail-closed from its rows:
 *
 * - any observed failure → `failed`;
 * - a required surface without proven keyboard reachability → `not_proven`
 *   (keyboard reachability of required actions is the one obligation this
 *   harness must prove or admit it did not);
 * - every row fully proven → `pass`;
 * - otherwise `bounded`: required keyboard obligations are proven and named
 *   dimensions remain honestly `not_proven`.
 */
export function composeAccessibilityVerdict(
  inventory: AccessibilityInventory,
  rows: readonly AccessibilitySurfaceReceipt[],
): AccessibilityReceipt['verdict'] {
  const failed = rows.some((row) =>
    ACCESSIBILITY_DIMENSIONS.some((dimension) => row[dimension] === 'failed'),
  );
  if (failed) {
    return 'failed';
  }
  const requiredIds = new Set(
    inventory.surfaces.filter((surface) => surface.required_product_path).map((s) => s.surface_id),
  );
  const requiredKeyboardProven = [...requiredIds].every((id) => {
    const row = rows.find((candidate) => candidate.surface_id === id);
    return row !== undefined && row.keyboard === 'pass';
  });
  if (!requiredKeyboardProven) {
    return 'not_proven';
  }
  const allProven = rows.every((row) =>
    ACCESSIBILITY_DIMENSIONS.every((dimension) => row[dimension] === 'pass'),
  );
  return allProven ? 'pass' : 'bounded';
}

/**
 * Build the installed accessibility receipt from live-host observations.
 *
 * Only extension-contributed commands (`perl-lsp.*` keyboard routes) are
 * asserted: native VS Code surfaces are inherited by design and are not
 * retested (#7807's native boundary). Screen-reader rows are always
 * `not_proven` here — semantic labels observed by automation are not
 * screen-reader proof.
 */
export function buildInstalledAccessibilityReceipt(
  candidate: AccessibilityReceiptCandidate,
  inventory: AccessibilityInventory,
  observations: InstalledAccessibilityObservations,
): AccessibilityReceipt {
  const surfaces = normalizedAccessibilityInventory(inventory).surfaces.map(
    (surface): AccessibilitySurfaceReceipt => {
      const routeId = contributedCommandRoute(surface);
      let keyboard: AccessibilityDimensionVerdict = 'not_proven';
      let semanticLabels: AccessibilityDimensionVerdict = 'not_proven';
      if (routeId !== null) {
        const registered = observations.registeredCommands.has(routeId);
        keyboard =
          registered && !observations.paletteHiddenCommands.has(routeId) ? 'pass' : 'failed';
        const title = observations.commandTitles.get(routeId);
        semanticLabels =
          registered && typeof title === 'string' && /\p{L}/u.test(title) ? 'pass' : 'failed';
      }
      return {
        surface_id: surface.surface_id,
        keyboard,
        semantic_labels: semanticLabels,
        screen_reader: 'not_proven',
        high_contrast: 'not_proven',
        zoom_reflow: 'not_proven',
      };
    },
  );

  const limitations = [
    'Screen-reader behavior is not instrumented by the automated installed harness; screen_reader rows are not_proven and can only move past that through a bounded manual run.',
    'High-contrast and zoom/reflow rows are not exercised by the automated installed harness; custom-surface styling obligations are proven by source-level contract tests, not here.',
    'Native VS Code surfaces are not retested: only extension-contributed commands (perl-lsp.* keyboard routes) are asserted, by design (#7807 native boundary).',
  ];

  return {
    schema_version: 'vscode_accessibility.v1',
    candidate: { ...candidate, inventory_digest: accessibilityInventoryDigest(inventory) },
    surfaces,
    limitations,
    verdict: composeAccessibilityVerdict(inventory, surfaces),
  };
}

/**
 * Structural validation of an installed accessibility receipt against the
 * inventory it claims to exercise. This is the automated negative control: a
 * receipt that invents surfaces, drops one, claims screen-reader proof from
 * automation, or disagrees with its own rows is rejected.
 */
export function validateAccessibilityReceipt(
  receipt: AccessibilityReceipt,
  inventory: AccessibilityInventory,
): string[] {
  const errors: string[] = [];
  if (receipt.schema_version !== 'vscode_accessibility.v1') {
    errors.push(`unexpected receipt schema: ${String(receipt.schema_version)}`);
  }

  const expectedIds = normalizedAccessibilityInventory(inventory).surfaces.map((s) => s.surface_id);
  const rowIds = receipt.surfaces.map((row) => row.surface_id);
  for (const id of expectedIds) {
    if (!rowIds.includes(id)) {
      errors.push(`receipt is missing a row for inventory surface: ${id}`);
    }
  }
  for (const id of rowIds) {
    if (!expectedIds.includes(id)) {
      errors.push(`receipt names a surface outside the inventory: ${id}`);
    }
  }
  if (new Set(rowIds).size !== rowIds.length) {
    errors.push('receipt repeats a surface row');
  }

  for (const row of receipt.surfaces) {
    for (const dimension of ACCESSIBILITY_DIMENSIONS) {
      if (!isDimensionVerdict(row[dimension])) {
        errors.push(`row ${row.surface_id} has an invalid ${dimension} verdict`);
      }
      // `manual_bounded` is reserved for bounded human screen-reader evidence
      // (see the verdict docs); an automated receipt claiming it on any other
      // dimension is presenting human-bounded evidence that never existed.
      if (row[dimension] === 'manual_bounded' && dimension !== 'screen_reader') {
        errors.push(`manual_bounded evidence is reserved for screen_reader: ${row.surface_id}`);
      }
    }
    // The negative control from #7807/#7865: automation can never observe a
    // screen reader, so neither a `pass` nor a `failed` here is observable —
    // both are manufactured results; only not_proven/manual_bounded exist.
    if (row.screen_reader === 'pass' || row.screen_reader === 'failed') {
      errors.push(`automated receipt cannot claim screen-reader proof: ${row.surface_id}`);
    }
    // The automated harness never exercises high-contrast or zoom/reflow
    // surfaces either (their rows are hard-coded `not_proven` by the
    // builder), so an observation-class verdict there is manufactured too.
    for (const dimension of ['high_contrast', 'zoom_reflow'] as const) {
      if (row[dimension] === 'pass' || row[dimension] === 'failed') {
        errors.push(
          `automated receipt cannot claim an observed ${dimension} verdict: ${row.surface_id}`,
        );
      }
    }
  }

  const expectedDigest = accessibilityInventoryDigest(inventory);
  if (receipt.candidate.inventory_digest !== expectedDigest) {
    errors.push('receipt inventory digest does not match the current inventory');
  }

  // A partial receipt must say why its evidence is incomplete: `bounded` and
  // `not_proven` verdicts without limitations would present an unbounded
  // claim as evidence.
  if (
    (receipt.verdict === 'bounded' || receipt.verdict === 'not_proven') &&
    (!Array.isArray(receipt.limitations) ||
      receipt.limitations.length === 0 ||
      receipt.limitations.every((limitation) => limitation.trim() === ''))
  ) {
    errors.push(`receipt verdict ${receipt.verdict} requires non-empty limitations`);
  }

  if (errors.length === 0) {
    const composed = composeAccessibilityVerdict(inventory, receipt.surfaces);
    if (receipt.verdict !== composed) {
      errors.push(
        `receipt verdict ${receipt.verdict} disagrees with its own rows (expected ${composed})`,
      );
    }
  }

  return errors;
}
