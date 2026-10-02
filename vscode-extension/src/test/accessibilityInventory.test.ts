import * as fs from 'fs';
import * as path from 'path';
import {
  type AccessibilityInventory,
  type AccessibilityReceipt,
  CURRENT_ACCESSIBILITY_INVENTORY,
  accessibilityEvidenceCounts,
  accessibilityInventoryDigest,
  buildInstalledAccessibilityReceipt,
  composeAccessibilityVerdict,
  normalizedAccessibilityInventory,
  validateAccessibilityInventory,
  validateAccessibilityReceipt,
} from '../accessibilityInventory';

const extensionRoot = path.resolve(__dirname, '..', '..');

function readExtensionSource(fileName: string): string {
  return fs.readFileSync(path.join(extensionRoot, 'src', fileName), 'utf8');
}

function cloneInventory(): AccessibilityInventory {
  return JSON.parse(JSON.stringify(CURRENT_ACCESSIBILITY_INVENTORY)) as AccessibilityInventory;
}

describe('VS Code accessibility inventory', () => {
  test('current custom-surface inventory is structurally valid', () => {
    expect(validateAccessibilityInventory(CURRENT_ACCESSIBILITY_INVENTORY)).toEqual([]);

    const counts = accessibilityEvidenceCounts(CURRENT_ACCESSIBILITY_INVENTORY);
    expect(counts.native_inherited).toBeGreaterThan(0);
    expect(counts.semantic_automated_proven).toBeGreaterThan(0);
    // Every surface carries some evidence class after #7865: rows still owed
    // manual screen-reader observation say so through manual_* classes rather
    // than a stale not_proven.
    expect(counts.not_proven).toBe(0);
    expect(counts.manual_screen_reader_required).toBeGreaterThan(0);
  });

  test('normalizes surface ordering deterministically', () => {
    const inventory = cloneInventory();
    inventory.surfaces.reverse();

    const normalized = normalizedAccessibilityInventory(inventory);
    expect(normalized.surfaces.map((surface) => surface.surface_id)).toEqual(
      [...normalized.surfaces.map((surface) => surface.surface_id)].sort(),
    );
  });

  test('rejects required mouse-only or hover-only paths', () => {
    const inventory = cloneInventory();
    const status = inventory.surfaces.find((surface) => surface.surface_id === 'workspace_status')!;
    status.keyboard_route = null;

    expect(validateAccessibilityInventory(inventory)).toContain(
      'required surface has no keyboard route: workspace_status',
    );
  });

  test('rejects color or icon only state', () => {
    const inventory = cloneInventory();
    const status = inventory.surfaces.find((surface) => surface.surface_id === 'workspace_status')!;
    status.color_or_icon_only = true;

    expect(validateAccessibilityInventory(inventory)).toContain(
      'surface state cannot be color/icon only: workspace_status',
    );
  });

  test('requires custom webviews to own theme and reflow obligations', () => {
    const inventory = cloneInventory();
    const pod = inventory.surfaces.find((surface) => surface.surface_id === 'pod_preview')!;
    pod.theme_policy = 'native';
    pod.zoom_policy = 'native';
    pod.evidence = 'native_inherited';

    expect(validateAccessibilityInventory(inventory)).toEqual(
      expect.arrayContaining([
        'custom webview must use VS Code theme variables: pod_preview',
        'custom webview must declare zoom/reflow responsibility: pod_preview',
        'custom webview cannot inherit all accessibility evidence from VS Code: pod_preview',
      ]),
    );
  });

  test('does not let a custom surface omit its semantic source', () => {
    const inventory = cloneInventory();
    const status = inventory.surfaces.find((surface) => surface.surface_id === 'workspace_status')!;
    status.accessible_name_source = null;

    expect(validateAccessibilityInventory(inventory)).toEqual(
      expect.arrayContaining([
        'required surface has no accessible name source: workspace_status',
        'custom surface must name its accessibility semantic source: workspace_status',
      ]),
    );
  });
});

/**
 * `validateAccessibilityInventory` only checks the inventory against itself, so
 * on its own it cannot notice that a row has stopped describing the shipped
 * extension. A row could keep claiming a keyboard route after the command was
 * deleted, or claim `accessibilityInformation` after the assignment was
 * removed, and every structural test above would still pass.
 *
 * These bind each claim to the production artifact that has to back it.
 */
describe('accessibility inventory is bound to the shipped extension', () => {
  const contributedCommands = new Set(
    (
      JSON.parse(fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8')) as {
        contributes?: { commands?: Array<{ command?: string }> };
      }
    ).contributes?.commands?.flatMap((command) => (command.command ? [command.command] : [])) ?? [],
  );

  test('every command-shaped keyboard route is a really contributed command', () => {
    const commandRoutes = CURRENT_ACCESSIBILITY_INVENTORY.surfaces.flatMap((surface) =>
      surface.keyboard_route !== null && surface.keyboard_route.startsWith('perl-lsp.')
        ? [surface.keyboard_route]
        : [],
    );

    // Guard the guard: if nothing is command-shaped the assertion below is vacuous.
    expect(commandRoutes.length).toBeGreaterThan(0);
    for (const route of commandRoutes) {
      expect(contributedCommands).toContain(route);
    }
  });

  test('a surface naming a VS Code accessibility API has that API assigned in source', () => {
    const surfaces = CURRENT_ACCESSIBILITY_INVENTORY.surfaces.filter(
      (surface) => surface.accessible_name_source === 'StatusBarItem.accessibilityInformation',
    );

    expect(surfaces.length).toBeGreaterThan(0);
    expect(readExtensionSource('extension.ts')).toMatch(
      /statusBarItem\.accessibilityInformation\s*=/,
    );
  });

  test('a surface claiming VS Code theme variables really uses them', () => {
    const themedTsOwners = CURRENT_ACCESSIBILITY_INVENTORY.surfaces.flatMap((surface) =>
      surface.theme_policy === 'vscode_theme_variables' && surface.owner.endsWith('.ts')
        ? [surface.owner]
        : [],
    );

    expect(themedTsOwners.length).toBeGreaterThan(0);
    for (const owner of themedTsOwners) {
      expect(readExtensionSource(owner)).toContain('var(--vscode-');
    }
  });

  test('a source-level proven webview row has real shipped proof', () => {
    const pod = CURRENT_ACCESSIBILITY_INVENTORY.surfaces.find(
      (surface) => surface.surface_id === 'pod_preview',
    );
    // The row may only claim manual-obligation evidence while the source-level
    // contract tests backing it exist; dropping those tests invalidates the row.
    // Pin a distinctive contract assertion, not the describe header: deleting
    // the tests while keeping the name must fail this binding.
    expect(pod?.evidence).toBe('manual_screen_reader_required');
    expect(pod?.keyboard_route).toBe('perl-lsp.previewPod');
    expect(readExtensionSource('podPreview.ts')).toContain('export function buildWebviewHtml');
    const podPreviewTests = readExtensionSource(path.join('test', 'podPreview.test.ts'));
    expect(podPreviewTests).toContain('keeps the #6030 security boundary intact');
    expect(podPreviewTests).toContain("not.toContain('<script')");
    expect(podPreviewTests).toContain('prose reflows under zoom instead of clipping');
  });
});

/**
 * The installed receipt (#7865) joins live extension-host observations onto
 * the inventory. These tests pin the receipt math: verdict composition is
 * fail-closed, automation can never manufacture screen-reader proof, and the
 * receipt is bound to the inventory digest it exercised.
 */
describe('installed accessibility receipt (#7865)', () => {
  const candidate = {
    vsix_version: '0.18.0',
    vsix_sha256: 'b'.repeat(64),
    vscode_version: '1.130.2',
    platform: 'win32',
    inventory_digest: accessibilityInventoryDigest(CURRENT_ACCESSIBILITY_INVENTORY),
  };

  /** Observations in which every contributed inventory route is registered, titled, and visible. */
  function provenObservations() {
    const routes = CURRENT_ACCESSIBILITY_INVENTORY.surfaces.flatMap((surface) =>
      surface.keyboard_route !== null && surface.keyboard_route.startsWith('perl-lsp.')
        ? [surface.keyboard_route]
        : [],
    );
    return {
      registeredCommands: new Set<string>(routes),
      commandTitles: new Map(routes.map((route) => [route, `Human title for ${route}`])),
      paletteHiddenCommands: new Set<string>(),
    };
  }

  test('inventory digest is stable and sensitive to row changes', () => {
    const baseline = accessibilityInventoryDigest(CURRENT_ACCESSIBILITY_INVENTORY);
    expect(baseline).toBe(accessibilityInventoryDigest(cloneInventory()));
    expect(baseline).toMatch(/^[0-9a-f]{64}$/);

    const changed = cloneInventory();
    const status = changed.surfaces.find((surface) => surface.surface_id === 'workspace_status')!;
    status.keyboard_route = 'perl-lsp.runHealthCheck';
    expect(accessibilityInventoryDigest(changed)).not.toBe(baseline);
  });

  test('a fully exercised custom command set proves keyboard and stays bounded', () => {
    const receipt = buildInstalledAccessibilityReceipt(
      candidate,
      CURRENT_ACCESSIBILITY_INVENTORY,
      provenObservations(),
    );

    expect(validateAccessibilityReceipt(receipt, CURRENT_ACCESSIBILITY_INVENTORY)).toEqual([]);
    expect(receipt.verdict).toBe('bounded');
    for (const row of receipt.surfaces) {
      expect(row.screen_reader).toBe('not_proven');
      const surface = CURRENT_ACCESSIBILITY_INVENTORY.surfaces.find(
        (candidateSurface) => candidateSurface.surface_id === row.surface_id,
      )!;
      if (surface.keyboard_route !== null && surface.keyboard_route.startsWith('perl-lsp.')) {
        expect(row.keyboard).toBe('pass');
        expect(row.semantic_labels).toBe('pass');
      } else {
        expect(row.keyboard).toBe('not_proven');
      }
    }
  });

  test('a missing or palette-hidden required command fails its row and the verdict', () => {
    const missing = buildInstalledAccessibilityReceipt(candidate, CURRENT_ACCESSIBILITY_INVENTORY, {
      ...provenObservations(),
      registeredCommands: new Set<string>(),
    });
    const hidden = buildInstalledAccessibilityReceipt(candidate, CURRENT_ACCESSIBILITY_INVENTORY, {
      ...provenObservations(),
      paletteHiddenCommands: new Set(['perl-lsp.showWorkspaceStatus']),
    });
    const untitled = buildInstalledAccessibilityReceipt(
      candidate,
      CURRENT_ACCESSIBILITY_INVENTORY,
      {
        ...provenObservations(),
        commandTitles: new Map([['perl-lsp.showWorkspaceStatus', '12345']]),
      },
    );

    const statusOf = (receipt: AccessibilityReceipt) =>
      receipt.surfaces.find((row) => row.surface_id === 'workspace_status');
    // Per-dimension contract: a missing command fails BOTH dimensions (it was
    // never registered); a hidden command fails keyboard only; an inhuman
    // title (internal code rather than text) fails semantic_labels only. A
    // regression failing both in every scenario must not satisfy this.
    expect(statusOf(missing)?.keyboard).toBe('failed');
    expect(statusOf(missing)?.semantic_labels).toBe('failed');
    expect(statusOf(hidden)?.keyboard).toBe('failed');
    expect(statusOf(hidden)?.semantic_labels).toBe('pass');
    expect(statusOf(untitled)?.keyboard).toBe('pass');
    expect(statusOf(untitled)?.semantic_labels).toBe('failed');
    for (const receipt of [missing, hidden, untitled]) {
      expect(receipt.verdict).toBe('failed');
    }
  });

  test('a required surface without keyboard proof is not_proven, never bounded', () => {
    const inventory = cloneInventory();
    const repair = inventory.surfaces.find(
      (surface) => surface.surface_id === 'managed_binary_repair',
    )!;
    repair.keyboard_route = null;

    const receipt = buildInstalledAccessibilityReceipt(candidate, inventory, provenObservations());
    expect(composeAccessibilityVerdict(inventory, receipt.surfaces)).toBe('not_proven');
  });

  test('the validator rejects invented, missing, or dishonest rows', () => {
    const receipt = buildInstalledAccessibilityReceipt(
      candidate,
      CURRENT_ACCESSIBILITY_INVENTORY,
      provenObservations(),
    );

    const missingRow: AccessibilityReceipt = {
      ...receipt,
      surfaces: receipt.surfaces.filter((row) => row.surface_id !== 'report_issue'),
    };
    expect(validateAccessibilityReceipt(missingRow, CURRENT_ACCESSIBILITY_INVENTORY)).toContain(
      'receipt is missing a row for inventory surface: report_issue',
    );

    const inventedRow: AccessibilityReceipt = {
      ...receipt,
      surfaces: [
        ...receipt.surfaces,
        {
          surface_id: 'made_up_surface',
          keyboard: 'pass',
          semantic_labels: 'pass',
          screen_reader: 'not_proven',
          high_contrast: 'pass',
          zoom_reflow: 'pass',
        },
      ],
    };
    expect(validateAccessibilityReceipt(inventedRow, CURRENT_ACCESSIBILITY_INVENTORY)).toContain(
      'receipt names a surface outside the inventory: made_up_surface',
    );

    const screenReaderPass: AccessibilityReceipt = {
      ...receipt,
      surfaces: receipt.surfaces.map((row) =>
        row.surface_id === 'workspace_status' ? { ...row, screen_reader: 'pass' } : row,
      ),
    };
    expect(
      validateAccessibilityReceipt(screenReaderPass, CURRENT_ACCESSIBILITY_INVENTORY),
    ).toContain('automated receipt cannot claim screen-reader proof: workspace_status');

    // Automation never performs a screen-reader observation, so a claimed
    // `failed` is as manufactured as a claimed `pass`.
    const screenReaderFailed: AccessibilityReceipt = {
      ...receipt,
      surfaces: receipt.surfaces.map((row) =>
        row.surface_id === 'workspace_status' ? { ...row, screen_reader: 'failed' } : row,
      ),
    };
    expect(
      validateAccessibilityReceipt(screenReaderFailed, CURRENT_ACCESSIBILITY_INVENTORY),
    ).toContain('automated receipt cannot claim screen-reader proof: workspace_status');

    // The harness never exercises high-contrast or zoom/reflow surfaces, so
    // observation-class verdicts there are manufactured too.
    const highContrastPass: AccessibilityReceipt = {
      ...receipt,
      surfaces: receipt.surfaces.map((row) =>
        row.surface_id === 'workspace_status' ? { ...row, high_contrast: 'pass' } : row,
      ),
    };
    expect(
      validateAccessibilityReceipt(highContrastPass, CURRENT_ACCESSIBILITY_INVENTORY),
    ).toContain(
      'automated receipt cannot claim an observed high_contrast verdict: workspace_status',
    );

    // `manual_bounded` is reserved for bounded human screen-reader evidence.
    const manualBoundedKeyboard: AccessibilityReceipt = {
      ...receipt,
      surfaces: receipt.surfaces.map((row) =>
        row.surface_id === 'workspace_status' ? { ...row, keyboard: 'manual_bounded' } : row,
      ),
    };
    expect(
      validateAccessibilityReceipt(manualBoundedKeyboard, CURRENT_ACCESSIBILITY_INVENTORY),
    ).toContain('manual_bounded evidence is reserved for screen_reader: workspace_status');

    // A partial verdict must carry its limitations.
    const unboundedClaim: AccessibilityReceipt = { ...receipt, limitations: [] };
    expect(validateAccessibilityReceipt(unboundedClaim, CURRENT_ACCESSIBILITY_INVENTORY)).toContain(
      'receipt verdict bounded requires non-empty limitations',
    );

    const digestMismatch: AccessibilityReceipt = {
      ...receipt,
      candidate: { ...receipt.candidate, inventory_digest: 'd'.repeat(64) },
    };
    expect(validateAccessibilityReceipt(digestMismatch, CURRENT_ACCESSIBILITY_INVENTORY)).toContain(
      'receipt inventory digest does not match the current inventory',
    );

    const dishonestVerdict: AccessibilityReceipt = { ...receipt, verdict: 'pass' };
    expect(validateAccessibilityReceipt(dishonestVerdict, CURRENT_ACCESSIBILITY_INVENTORY)).toEqual(
      expect.arrayContaining([
        expect.stringMatching(/^receipt verdict pass disagrees with its own rows/),
      ]),
    );
  });

  test('receipt builder rebinds the candidate digest to the exercised inventory', () => {
    // The supplied digest deliberately disagrees with the exercised inventory:
    // the builder must override it (rebind), not pass the fixture through, so
    // this test can tell rebinding from blind copying.
    const mismatchedCandidate: typeof candidate = {
      ...candidate,
      inventory_digest: 'e'.repeat(64),
    };
    const receipt = buildInstalledAccessibilityReceipt(
      mismatchedCandidate,
      CURRENT_ACCESSIBILITY_INVENTORY,
      provenObservations(),
    );
    expect(receipt.candidate.inventory_digest).toBe(
      accessibilityInventoryDigest(CURRENT_ACCESSIBILITY_INVENTORY),
    );
    expect(receipt.candidate.inventory_digest).not.toBe(mismatchedCandidate.inventory_digest);
    // Non-digest candidate identity passes through unchanged, and the rebound
    // receipt validates clean against the inventory it exercised.
    expect(receipt.candidate.vsix_sha256).toBe(candidate.vsix_sha256);
    expect(validateAccessibilityReceipt(receipt, CURRENT_ACCESSIBILITY_INVENTORY)).toEqual([]);
    expect(receipt.limitations.length).toBeGreaterThan(0);
  });
});
