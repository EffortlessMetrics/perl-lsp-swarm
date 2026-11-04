import {
  isActivationDomainDeferred,
  parseDeferredActivationDomains,
} from '../featureActivationDeferral';

describe('feature activation deferral experiment env', () => {
  const originalExperiment = process.env.PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT;
  const originalDomains = process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS;

  beforeEach(() => {
    delete process.env.PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT;
    delete process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS;
  });

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

  test('returns empty set unless the experiment flag is armed', () => {
    process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS = 'providers';
    expect(parseDeferredActivationDomains()).toEqual(new Set());
  });

  test('parses deferred domains when the experiment flag is armed', () => {
    process.env.PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT = '1';
    process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS = 'providers, onboarding';
    expect(parseDeferredActivationDomains()).toEqual(new Set(['providers', 'onboarding']));
    expect(isActivationDomainDeferred('providers')).toBe(true);
    expect(isActivationDomainDeferred('providers')).toBe(false);
  });

  test('rejects unknown domains', () => {
    process.env.PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT = '1';
    process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS = 'not-a-domain';
    expect(() => parseDeferredActivationDomains()).toThrow(/unknown domain.*not-a-domain/);
  });
});
