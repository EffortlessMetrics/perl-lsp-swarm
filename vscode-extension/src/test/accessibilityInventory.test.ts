import {
  type AccessibilityInventory,
  CURRENT_ACCESSIBILITY_INVENTORY,
  accessibilityEvidenceCounts,
  normalizedAccessibilityInventory,
  validateAccessibilityInventory,
} from '../accessibilityInventory';

function cloneInventory(): AccessibilityInventory {
  return JSON.parse(JSON.stringify(CURRENT_ACCESSIBILITY_INVENTORY)) as AccessibilityInventory;
}

describe('VS Code accessibility inventory', () => {
  test('current custom-surface inventory is structurally valid', () => {
    expect(validateAccessibilityInventory(CURRENT_ACCESSIBILITY_INVENTORY)).toEqual([]);

    const counts = accessibilityEvidenceCounts(CURRENT_ACCESSIBILITY_INVENTORY);
    expect(counts.native_inherited).toBeGreaterThan(0);
    expect(counts.semantic_automated_proven).toBeGreaterThan(0);
    expect(counts.not_proven).toBeGreaterThan(0);
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
