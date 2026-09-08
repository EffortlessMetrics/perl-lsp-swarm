import * as path from 'path';
import Mocha from 'mocha';

export function assertSmokeSelector(environment: NodeJS.ProcessEnv = process.env): void {
  if (
    environment.PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE === '1' &&
    environment.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE !== '1'
  ) {
    throw new Error(
      'PERL_LSP_HEALTH_CHECK_RECOVERY_SMOKE requires PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE=1',
    );
  }
}

export async function run(): Promise<void> {
  assertSmokeSelector();
  const mocha = new Mocha({
    ui: 'tdd',
    color: true,
    timeout: 180_000,
  });

  const currentSourceSmoke = process.env.PERL_LSP_CURRENT_SOURCE_SMOKE === '1';
  const packagedBundleSmoke = process.env.PERL_LSP_PACKAGED_BUNDLE_SMOKE === '1';
  const healthCheckFailureSmoke = process.env.PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE === '1';
  const activationFailureSmoke = process.env.PERL_LSP_ACTIVATION_FAILURE_SMOKE === '1';
  const conflictingHealthMode = [
    ['PERL_LSP_ACTIVATION_FAILURE_SMOKE', activationFailureSmoke],
    ['PERL_LSP_CRASH_RECOVERY_SMOKE', process.env.PERL_LSP_CRASH_RECOVERY_SMOKE === '1'],
    ['PERL_LSP_PACKAGED_BUNDLE_SMOKE', packagedBundleSmoke],
    ['PERL_LSP_CURRENT_SOURCE_SMOKE', currentSourceSmoke],
  ]
    .filter(([, enabled]) => enabled)
    .map(([name]) => name);
  if (healthCheckFailureSmoke && conflictingHealthMode.length > 0) {
    throw new Error(
      `PERL_LSP_HEALTH_CHECK_FAILURE_SMOKE cannot be combined with ${conflictingHealthMode.join(', ')}`,
    );
  }
  const activationFailureLeg = process.env.PERL_LSP_ACTIVATION_FAILURE_LEG ?? '';
  if (
    activationFailureSmoke &&
    activationFailureLeg !== 'failure' &&
    activationFailureLeg !== 'retry'
  ) {
    throw new Error(
      `PERL_LSP_ACTIVATION_FAILURE_LEG must be 'failure' or 'retry' when PERL_LSP_ACTIVATION_FAILURE_SMOKE=1, got ${JSON.stringify(activationFailureLeg)}`,
    );
  }
  const crashRecoverySmoke = process.env.PERL_LSP_CRASH_RECOVERY_SMOKE === '1';
  const crashRecoveryLeg = process.env.PERL_LSP_CRASH_RECOVERY_LEG ?? '';
  if (crashRecoverySmoke && crashRecoveryLeg !== 'transient' && crashRecoveryLeg !== 'breaker') {
    throw new Error(
      `PERL_LSP_CRASH_RECOVERY_LEG must be 'transient' or 'breaker' when PERL_LSP_CRASH_RECOVERY_SMOKE=1, got ${JSON.stringify(crashRecoveryLeg)}`,
    );
  }
  const smokeTestPaths = crashRecoverySmoke
    ? [path.resolve(__dirname, '../crashRecoveryJourney.test.js')]
    : activationFailureSmoke
      ? [path.resolve(__dirname, '../activationFailureJourney.test.js')]
      : packagedBundleSmoke
        ? [path.resolve(__dirname, '../packagedBundleJourney.test.js')]
        : healthCheckFailureSmoke
          ? [path.resolve(__dirname, '../healthCheckFailureJourney.test.js')]
          : currentSourceSmoke
            ? [path.resolve(__dirname, '../../integration/firstHourReceipt.test.js')]
            : [path.resolve(__dirname, '../managedBinaryPublishedSmoke.test.js')];
  for (const smokeTestPath of smokeTestPaths) {
    mocha.addFile(smokeTestPath);
  }
  await mocha.loadFilesAsync();

  if (mocha.suite.total() === 0) {
    throw new Error(`No published-extension smoke tests loaded from ${smokeTestPaths.join(', ')}`);
  }

  return new Promise((resolve, reject) => {
    const runner = mocha.run((failures) => {
      if (runner.total === 0) {
        reject(new Error('No published-extension smoke tests matched.'));
        return;
      }
      if (failures > 0) {
        reject(new Error(`${failures} published-extension smoke test(s) failed.`));
        return;
      }
      resolve();
    });
  });
}
