/**
 * Client-side proof for the perl-lsp/loadedModuleReload v1 custom DAP
 * family: consumes the canonical JSON vectors under
 * .spec/10138-loaded-module-reload-family/fixtures and asserts the
 * generated TypeScript projection classifies, negotiates, and bounds
 * exactly as the Rust adapter side does.
 */

import * as fs from 'fs';
import * as path from 'path';
import {
  DETAIL_REDACTED_MARKER,
  LOADED_MODULE_RELOAD_FAMILY,
  LOADED_MODULE_RELOAD_FAMILY_VERSION,
  LOADED_MODULE_RELOAD_REQUEST,
  REASONS_TRUNCATED_MARKER,
  RELOAD_FAMILY_BOUNDS,
  classifyReloadTerminal,
  negotiateLoadedModuleReloadFamily,
} from '../loadedModuleReloadFamily.generated';

const extensionRoot = path.resolve(__dirname, '..', '..');
const fixturesDir = path.join(
  extensionRoot,
  '..',
  '.spec',
  '10138-loaded-module-reload-family',
  'fixtures',
);

interface VectorFixture {
  schema: string;
  name: string;
  description?: string;
  negotiation?: {
    client: { family: string; versions: number[] } | null;
    adapter: { epoch: number; backed: boolean };
  };
  request?: Record<string, unknown>;
  outcome?: {
    kind: string;
    phase?: string;
    disposition?: string;
    cause?: string;
  };
  generation_before?: number;
  previously_admitted_operations?: number[];
  oversized_reasons_input?: string[];
  oversized_remediation_input?: string;
  response_probe?: { kind: string; possiblyApplied?: boolean };
  expect: {
    evaluation?: 'admitted' | 'rejected';
    code?: string;
    kind?: string;
    phase?: string;
    disposition?: string;
    cause?: string;
    success?: boolean;
    possibly_applied?: boolean;
    classification?: string;
    classify?: string;
    reasons_count?: number;
    reasons_truncated_marker?: string;
    remediation?: string;
  };
}

function loadVectors(): VectorFixture[] {
  const files = fs
    .readdirSync(fixturesDir)
    .filter((file) => file.endsWith('.json'))
    .sort();
  expect(files.length).toBeGreaterThan(0);
  return files.map((file) => {
    const parsed = JSON.parse(
      fs.readFileSync(path.join(fixturesDir, file), 'utf8'),
    ) as VectorFixture;
    expect(parsed.schema).toBe('perl_dap.loaded_module_reload_family.vector.v1');
    return parsed;
  });
}

const vectors = loadVectors();
const outcomeVectors = vectors.filter((vector) => vector.outcome !== undefined);

describe('loadedModuleReloadFamily generated projection', () => {
  test('registers the namespaced, versioned, unadvertised family identity', () => {
    expect(LOADED_MODULE_RELOAD_FAMILY).toBe('perl-lsp/loadedModuleReload');
    expect(LOADED_MODULE_RELOAD_REQUEST).toBe(LOADED_MODULE_RELOAD_FAMILY);
    expect(LOADED_MODULE_RELOAD_FAMILY).toContain('/');
    expect(LOADED_MODULE_RELOAD_FAMILY_VERSION).toBe(1);
    // The family is namespaced and mechanically distinguishable from every
    // standard DAP request name (which never contain '/').
    const parts = LOADED_MODULE_RELOAD_FAMILY.split('/');
    expect(parts[0]?.length ?? 0).toBeGreaterThan(0);
    expect(parts[1]?.length ?? 0).toBeGreaterThan(0);
  });

  test('classifies every terminal vector exactly as the adapter projects it', () => {
    expect(outcomeVectors.length).toBeGreaterThanOrEqual(6);
    const seenKinds = new Set<string>();
    for (const vector of outcomeVectors) {
      const kind = vector.outcome?.kind as string;
      seenKinds.add(kind);
      const classified = classifyReloadTerminal(
        vector.expect.possibly_applied === undefined
          ? { kind }
          : { kind, possiblyApplied: vector.expect.possibly_applied },
      );
      const expected =
        kind === 'reloaded'
          ? 'reloaded_clean'
          : kind === 'refused'
            ? 'refused_clean_failure'
            : kind === 'failed_before_mutation'
              ? 'failed_before_mutation_clean_failure'
              : 'possibly_applied';
      expect(classified).toBe(expected);
    }
    // Every frozen terminal kind is covered by the corpus.
    for (const kind of [
      'reloaded',
      'refused',
      'failed_before_mutation',
      'indeterminate_possibly_applied',
    ]) {
      expect(seenKinds.has(kind)).toBe(true);
    }
  });

  test('never flattens indeterminate_possibly_applied to a clean or ordinary failure', () => {
    const indeterminate = outcomeVectors.filter(
      (vector) => vector.outcome?.kind === 'indeterminate_possibly_applied',
    );
    expect(indeterminate.length).toBeGreaterThanOrEqual(2);
    for (const vector of indeterminate) {
      expect(vector.expect.success).toBe(false);
      expect(vector.expect.possibly_applied).toBe(true);
      expect(classifyReloadTerminal({ kind: 'indeterminate_possibly_applied' })).toBe(
        'possibly_applied',
      );
      // Even a lying possiblyApplied=false field cannot demote the kind.
      expect(
        classifyReloadTerminal({ kind: 'indeterminate_possibly_applied', possiblyApplied: false }),
      ).toBe('possibly_applied');
    }
  });

  test('fails closed on unknown mandatory variants and contradictory bodies', () => {
    const probe = vectors.find((vector) => vector.response_probe !== undefined) as VectorFixture;
    expect(probe).toBeDefined();
    expect(probe.response_probe?.kind).not.toBe('reloaded');
    expect(classifyReloadTerminal(probe.response_probe as { kind: string })).toBe(
      probe.expect.classify,
    );
    expect(classifyReloadTerminal({ kind: 'reloaded', possiblyApplied: true })).toBe(
      'unknown_fail_closed',
    );
    expect(classifyReloadTerminal({ kind: 'refused', possiblyApplied: true })).toBe(
      'unknown_fail_closed',
    );
    expect(classifyReloadTerminal({ kind: 'failed_before_mutation', possiblyApplied: true })).toBe(
      'unknown_fail_closed',
    );
    expect(classifyReloadTerminal({ kind: 'runtime_rejected' })).toBe('unknown_fail_closed');
  });

  test('negotiates exactly as the registry rules require', () => {
    expect(negotiateLoadedModuleReloadFamily(null)).toEqual({
      negotiated: false,
      reason: 'family_absent',
    });
    expect(negotiateLoadedModuleReloadFamily({ family: 'modules', versions: [1] })).toEqual({
      negotiated: false,
      reason: 'family_name_mismatch',
    });
    expect(
      negotiateLoadedModuleReloadFamily({
        family: LOADED_MODULE_RELOAD_FAMILY,
        versions: [2],
      }),
    ).toEqual({ negotiated: false, reason: 'no_overlapping_version' });
    expect(
      negotiateLoadedModuleReloadFamily({
        family: LOADED_MODULE_RELOAD_FAMILY,
        versions: [0, 1, 2],
      }),
    ).toEqual({ negotiated: true, version: 1 });

    // The vector corpus drives the same mirror: unnegotiated and
    // version-unsupported vectors must correspond to negotiation refusals.
    for (const vector of vectors) {
      if (!vector.negotiation || vector.expect.evaluation !== 'rejected') {
        continue;
      }
      const outcome = negotiateLoadedModuleReloadFamily(vector.negotiation.client);
      if (vector.expect.code === 'family_not_negotiated') {
        expect(outcome).toEqual({ negotiated: false, reason: 'family_absent' });
      } else if (vector.expect.code === 'family_version_unsupported') {
        expect(outcome).toEqual({ negotiated: false, reason: 'no_overlapping_version' });
      } else {
        expect(outcome.negotiated).toBe(true);
      }
    }
  });

  test('bounds are mirrored and enforced before publication', () => {
    expect(RELOAD_FAMILY_BOUNDS.maxIdentityChars).toBe(256);
    expect(RELOAD_FAMILY_BOUNDS.maxReasons).toBe(16);
    expect(REASONS_TRUNCATED_MARKER).toBe('reasons_truncated');
    expect(DETAIL_REDACTED_MARKER).toBe('detail_redacted');

    const oversized = vectors.find(
      (vector) => vector.expect.code === 'identity_too_large',
    ) as VectorFixture;
    const identity = oversized.request?.subject as { moduleIdentity: string };
    expect(identity.moduleIdentity.length).toBeGreaterThan(RELOAD_FAMILY_BOUNDS.maxIdentityChars);

    const clamped = vectors.find(
      (vector) => vector.expect.reasons_count !== undefined,
    ) as VectorFixture;
    expect(clamped.expect.reasons_count).toBe(RELOAD_FAMILY_BOUNDS.maxReasons);
    expect(clamped.expect.reasons_truncated_marker).toBe(REASONS_TRUNCATED_MARKER);
  });

  test('carries typed opaque subject identity only', () => {
    for (const vector of vectors) {
      if (
        !vector.request ||
        vector.expect.code === 'unknown_field_rejected' ||
        vector.expect.code === 'raw_client_input_refused'
      ) {
        // Vectors that smuggle unknown fields or raw client input
        // deliberately carry malformed subjects; they expect refusal.
        continue;
      }
      const request = vector.request as {
        family: string;
        familyVersion: number;
        subject: Record<string, unknown>;
      };
      if (vector.expect.code === 'family_name_mismatch') {
        continue;
      }
      expect(request.family).toBe(LOADED_MODULE_RELOAD_FAMILY);
      const subject = request.subject;
      for (const required of [
        'moduleIdentity',
        'savedSourceDigest',
        'logicalSourceUri',
        'observationGeneration',
      ]) {
        expect(subject).toHaveProperty(required);
      }
      // The raw input channels are absent from the admissible shape even
      // in the vectors that smuggle them (those expect refusal).
      if (vector.expect.code !== 'raw_client_input_refused') {
        for (const forbidden of ['path', 'command', 'expression', 'incKey', 'packageName']) {
          expect(subject).not.toHaveProperty(forbidden);
        }
      }
    }
  });
});
