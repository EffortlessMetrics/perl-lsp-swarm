import { FeatureActivationMetrics } from '../featureActivationMetrics';
import { measureOrDeferFeature } from '../featureActivationRegistration';

describe('measureOrDeferFeature', () => {
  const originalExperiment = process.env.PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT;
  const originalDomains = process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS;

  afterEach(() => {
    if (originalExperiment === undefined) {
      delete process.env.PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT;
    } else {
      process.env.PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT = originalExperiment;
    }
    if (originalDomains === undefined) {
      delete process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS;
    } else {
      process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS = originalDomains;
    }
  });

  test('registers normally when the experiment is not armed', () => {
    const metrics = new FeatureActivationMetrics();
    metrics.beginActivation();
    const value = measureOrDeferFeature(metrics, 'mcp', false, () => 'registered');
    expect(value).toBe('registered');
    expect(metrics.snapshot().measurements[0]?.registration_status).toBe('registered');
  });

  test('skips non-critical registration when deferral is armed', () => {
    process.env.PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT = '1';
    process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS = 'mcp';
    const metrics = new FeatureActivationMetrics();
    metrics.beginActivation();
    const value = measureOrDeferFeature(metrics, 'mcp', false, () => 'registered');
    expect(value).toBeUndefined();
    expect(metrics.snapshot().measurements[0]).toMatchObject({
      domain: 'mcp',
      registration_status: 'deferred',
    });
  });

  test('still registers activation-critical domains when deferral is armed', () => {
    process.env.PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT = '1';
    process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS = 'providers';
    const metrics = new FeatureActivationMetrics();
    metrics.beginActivation();
    measureOrDeferFeature(metrics, 'providers', true, () => undefined);
    expect(metrics.snapshot().measurements[0]?.registration_status).toBe('registered');
  });
});
