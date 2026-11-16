import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';

// A separate Node process loads the actual tsc-emitted wrapper. Only spawn is
// replaced; no editor starts, and the fixture never serializes the user's env.
const wrapperProbe = `
const fs = require('node:fs');
const { EventEmitter } = require('node:events');
const childProcess = require('node:child_process');
const fixture = JSON.parse(fs.readFileSync(0, 'utf8'));
process.env = { ...fixture.ambient };
const overlay = { ...fixture.overlay };
for (const key of fixture.unset) overlay[key] = undefined;
if (fixture.deleteDigest) delete overlay.PERL_LSP_VSIX_SHA256;
if (fixture.nonEnumerableDigest) Object.defineProperty(overlay, 'PERL_LSP_VSIX_SHA256', {
  value: overlay.PERL_LSP_VSIX_SHA256, enumerable: false,
});
const calls = [];
childProcess.spawn = (command, args, options) => {
  calls.push({ command, args, options, envKeys: Object.keys(options.env) });
  const child = new EventEmitter();
  queueMicrotask(() => child.emit('exit', 0, null));
  return child;
};
const { runWithoutForcedWorkspaceTrust } = require(process.argv[1]);
runWithoutForcedWorkspaceTrust({
  vscodeExecutablePath: '/fixture/code',
  extensionDevelopmentPath: ['/fixture/harness', '/fixture/companion'],
  extensionTestsPath: '/fixture/suite',
  extensionTestsEnv: overlay,
  launchArgs: ['/fixture/workspace', '--user-data-dir=/fixture/profile'],
}).then(() => {
  process.stdout.write(JSON.stringify({ calls, ambient: process.env, overlayKeys: Object.keys(overlay) }));
}).catch(error => { console.error(error); process.exitCode = 1; });
`;

interface SpawnObservation {
  calls: [
    {
      command: string;
      args: string[];
      options: { env: NodeJS.ProcessEnv; stdio: string; windowsHide: boolean };
      envKeys: string[];
    },
  ];
  ambient: NodeJS.ProcessEnv;
  overlayKeys: string[];
}

function observeWrapper(
  ambient: NodeJS.ProcessEnv,
  overlay: NodeJS.ProcessEnv,
  unset: string[] = [],
  deleteDigest = false,
  nonEnumerableDigest = false,
): SpawnObservation {
  const result = spawnSync(
    process.execPath,
    ['-e', wrapperProbe, path.join(__dirname, 'runVsCodeTests.js')],
    {
      input: JSON.stringify({ ambient, overlay, unset, deleteDigest, nonEnumerableDigest }),
      encoding: 'utf8',
    },
  );
  expect(result.error).toBeUndefined();
  expect(result.status).toBe(0);
  expect(result.stderr).toBe('');
  const observation = JSON.parse(result.stdout) as SpawnObservation;
  expect(observation.calls).toHaveLength(1);
  expect(observation.calls[0].command).toBe('/fixture/code');
  expect(observation.calls[0].args).toEqual([
    '/fixture/workspace',
    '--user-data-dir=/fixture/profile',
    '--no-sandbox',
    '--disable-gpu-sandbox',
    '--disable-updates',
    '--skip-welcome',
    '--skip-release-notes',
    '--extensionTestsPath=/fixture/suite',
    '--extensionDevelopmentPath=/fixture/harness',
    '--extensionDevelopmentPath=/fixture/companion',
  ]);
  expect(observation.calls[0].options.stdio).toBe('inherit');
  expect(observation.calls[0].options.windowsHide).toBe(true);
  expect(observation.ambient).toEqual(ambient);
  return observation;
}

const ambient = { PATH: '/fixture/bin', KEEP: 'ambient', PERL_LSP_VSIX_SHA256: 'stale-archive' };

test('Marketplace caller deletion cannot restore the inherited archive digest', () => {
  // Reproduce private published main's copied environment and delete operation.
  const observation = observeWrapper(
    ambient,
    { ...ambient, PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'marketplace', KEEP: 'caller' },
    [],
    true,
  );
  expect(observation.overlayKeys).not.toContain('PERL_LSP_VSIX_SHA256');
  expect(observation.calls[0].options.env).toEqual({
    PATH: '/fixture/bin',
    KEEP: 'caller',
    PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'marketplace',
  });
  expect(observation.calls[0].envKeys).not.toContain('PERL_LSP_VSIX_SHA256');
});

test('ambient-only Marketplace attribution does not supply an archive digest', () => {
  const observation = observeWrapper(
    { ...ambient, PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'marketplace' },
    { EXTRA: 'caller' },
  );
  expect(observation.calls[0].options.env).toEqual({
    PATH: '/fixture/bin',
    KEEP: 'ambient',
    EXTRA: 'caller',
    PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'marketplace',
  });
  expect(observation.calls[0].envKeys).not.toContain('PERL_LSP_VSIX_SHA256');
});

test.each(['hidden-digest', undefined])(
  'non-enumerable digest %j does not restore ambient Marketplace attribution',
  (digest) => {
    const observation = observeWrapper(
      ambient,
      {
        PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'marketplace',
        PERL_LSP_VSIX_SHA256: digest,
      },
      [],
      false,
      true,
    );
    expect(observation.overlayKeys).not.toContain('PERL_LSP_VSIX_SHA256');
    expect(observation.calls[0].options.env).toEqual({
      PATH: '/fixture/bin',
      KEEP: 'ambient',
      PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'marketplace',
    });
    expect(observation.calls[0].envKeys).not.toContain('PERL_LSP_VSIX_SHA256');
  },
);

test.each(['vsix', 'open-vsx'])('%s forwards explicit bound archive digest and path', (source) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'wrapper-bound-vsix-'));
  const vsixPath = path.join(root, 'candidate.vsix');
  const bytes = Buffer.from('archive A');
  fs.writeFileSync(vsixPath, bytes);
  const digest = createHash('sha256').update(bytes).digest('hex');
  try {
    const observation = observeWrapper(
      {
        ...ambient,
        PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'marketplace',
        PERL_LSP_PUBLISHED_VSIX_PATH: '/fixture/old.vsix',
      },
      {
        PERL_LSP_PUBLISHED_EXTENSION_SOURCE: source,
        PERL_LSP_VSIX_SHA256: digest,
        PERL_LSP_PUBLISHED_VSIX_PATH: vsixPath,
      },
    );
    expect(observation.calls[0].options.env).toEqual({
      PATH: '/fixture/bin',
      KEEP: 'ambient',
      PERL_LSP_PUBLISHED_EXTENSION_SOURCE: source,
      PERL_LSP_VSIX_SHA256: digest,
      PERL_LSP_PUBLISHED_VSIX_PATH: vsixPath,
    });
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test.each(['explicit-digest', ''])('Marketplace preserves explicit caller digest %j', (digest) => {
  const observation = observeWrapper(ambient, {
    PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'marketplace',
    PERL_LSP_VSIX_SHA256: digest,
  });
  expect(observation.calls[0].options.env).toEqual({
    ...ambient,
    PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'marketplace',
    PERL_LSP_VSIX_SHA256: digest,
  });
});

test('undefined overlay values transport explicit unsets without dropping unrelated env', () => {
  const observation = observeWrapper(
    { ...ambient, REMOVE: 'ambient' },
    { KEEP: '', EXTRA: 'caller' },
    ['PERL_LSP_VSIX_SHA256', 'REMOVE'],
  );
  expect(observation.calls[0].options.env).toEqual({
    PATH: '/fixture/bin',
    KEEP: '',
    EXTRA: 'caller',
  });
  expect(observation.calls[0].envKeys).not.toContain('PERL_LSP_VSIX_SHA256');
  expect(observation.calls[0].envKeys).not.toContain('REMOVE');
});

test('ordinary overlays retain ambient values and explicit overrides', () => {
  const observation = observeWrapper(ambient, { KEEP: 'caller', EXTRA: '' });
  expect(observation.calls[0].options.env).toEqual({ ...ambient, KEEP: 'caller', EXTRA: '' });
});
