import { spawn } from 'child_process';

export interface UntrustedHostTestOptions {
  vscodeExecutablePath: string;
  /** One or more development extension roots; each becomes its own CLI flag. */
  extensionDevelopmentPath: string | string[];
  extensionTestsPath: string;
  /** Values override inherited env; an own undefined value explicitly unsets a key. */
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
  const env = { ...process.env, ...options.extensionTestsEnv };
  for (const [key, value] of Object.entries(options.extensionTestsEnv)) {
    if (value === undefined) {
      delete env[key];
    }
  }
  // Published Marketplace installs have no observed archive digest. The caller
  // deletes it from its overlay; merging ambient env must not resurrect it.
  // An explicit caller value, including an empty string, still takes precedence.
  if (
    env.PERL_LSP_PUBLISHED_EXTENSION_SOURCE?.trim() === 'marketplace' &&
    !Object.hasOwn(options.extensionTestsEnv, 'PERL_LSP_VSIX_SHA256')
  ) {
    delete env.PERL_LSP_VSIX_SHA256;
  }
  return new Promise((resolve, reject) => {
    const child = spawn(options.vscodeExecutablePath, args, {
      env,
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
