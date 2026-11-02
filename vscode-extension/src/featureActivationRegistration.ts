import type { FeatureActivationDomain } from './featureActivationMetrics';
import { FeatureActivationMetrics } from './featureActivationMetrics';
import { isActivationDomainDeferred } from './featureActivationDeferral';

export function measureOrDeferFeature<T>(
  metrics: FeatureActivationMetrics,
  domain: FeatureActivationDomain,
  activationCritical: boolean,
  register: () => T,
): T | undefined {
  if (!isActivationDomainDeferred(domain)) {
    return metrics.measure(domain, activationCritical, register);
  }
  if (activationCritical) {
    return metrics.measure(domain, activationCritical, register);
  }
  metrics.recordDeferred(domain, activationCritical);
  return undefined;
}
