import { spawn } from 'child_process';

export interface UntrustedHostTestOptions {
  vscodeExecutablePath: string;
  /** One or more development extension roots; each becomes its own CLI flag. */
  extensionDevelopmentPath: string | string[];
  extensionTestsPath: string;
  extensionTestsEnv: NodeJS.ProcessEnv;
  launchArgs: string[];
}

function developmentPathArgs(extensionDevelopmentPath: string | string[]): string[] {
  const paths = Array.isArray(extensionDevelopmentPath)
    ? extensionDevelopmentPath
    : [extensionDevelopmentPath];
  return paths.map((extensionPath) => `--extensionDevelopmentPath=${extensionPath}`);
}

/**
 * Runs the extension host without the test-electron package's forced
 * `--disable-workspace-trust` argument. This is intentionally test-only: the
 * normal runner remains authoritative for every other smoke mode.
 */
export function runWithoutForcedWorkspaceTrust(options: UntrustedHostTestOptions): Promise<void> {
  const args = [
    ...options.launchArgs,
    '--no-sandbox',
    '--disable-gpu-sandbox',
    '--disable-updates',
    '--skip-welcome',
    '--skip-release-notes',
    `--extensionTestsPath=${options.extensionTestsPath}`,
    ...developmentPathArgs(options.extensionDevelopmentPath),
  ];
  return new Promise((resolve, reject) => {
    const child = spawn(options.vscodeExecutablePath, args, {
      env: { ...process.env, ...options.extensionTestsEnv },
      stdio: 'inherit',
      windowsHide: true,
    });
    child.on('error', reject);
    child.on('exit', (code, signal) => {
      if (code === 0) {
        resolve();
        return;
      }
      reject(new Error(`VS Code extension host exited with ${code ?? signal ?? 'unknown'}`));
    });
  });
}
