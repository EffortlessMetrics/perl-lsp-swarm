import type { FeatureActivationDomain } from './featureActivationMetrics';

const FEATURE_DOMAINS: readonly FeatureActivationDomain[] = [
  'mcp',
  'whats_new',
  'providers',
  'configuration',
  'debugger',
  'onboarding',
];

function isFeatureActivationDomain(value: string): value is FeatureActivationDomain {
  return (FEATURE_DOMAINS as readonly string[]).includes(value);
}

/**
 * Smoke-only controlled deferral for matched same-host activation experiments (#4262).
 * Production activation ignores these variables.
 */
export function parseDeferredActivationDomains(
  env: NodeJS.ProcessEnv = process.env,
): Set<FeatureActivationDomain> {
  if (env.PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT !== '1') {
    return new Set();
  }
  const raw = env.PERL_LSP_ACTIVATION_DEFER_DOMAINS?.trim();
  if (!raw) {
    return new Set();
  }
  const domains = new Set<FeatureActivationDomain>();
  for (const token of raw.split(/[,;\s]+/)) {
    const normalized = token.trim();
    if (!normalized) {
      continue;
    }
    if (!isFeatureActivationDomain(normalized)) {
      throw new Error(
        `PERL_LSP_ACTIVATION_DEFER_DOMAINS contains unknown domain ${JSON.stringify(normalized)}`,
      );
    }
    domains.add(normalized);
  }
  return domains;
}

export function isActivationDomainDeferred(
  domain: FeatureActivationDomain,
  env: NodeJS.ProcessEnv = process.env,
): boolean {
  return parseDeferredActivationDomains(env).has(domain);
}
