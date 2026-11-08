import * as fs from 'fs';
import * as path from 'path';
import { SUPPORTED_PERL_LANGUAGE_IDS } from '../languageIdentity';
import {
  checkManifestReachability,
  stripComments,
  type ReachabilityAllowance,
  type ReachabilityInput,
} from './manifestReachability';

const EXT_ROOT = path.resolve(__dirname, '..', '..');
const SRC_ROOT = path.join(EXT_ROOT, 'src');

/**
 * Reviewed contributions with no runtime owner today. Each entry needs a
 * reason and the issue that owns wiring or removing it; an entry that stops
 * matching a violation fails as stale.
 */
const REVIEWED_ALLOWLIST: readonly ReachabilityAllowance[] = [];

/** Files that describe settings (ownership/migration tables) rather than read them. */
const DESCRIPTIVE_SETTING_SOURCES = [
  'src/configurationOwnership.ts',
  'src/configurationManifestConformance.ts',
  'src/configurationMigrationRegistry.ts',
];

function runtimeSources(): Map<string, string> {
  const sources = new Map<string, string>();
  const walk = (dir: string) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (full !== path.join(SRC_ROOT, 'test')) walk(full);
      } else if (entry.name.endsWith('.ts') && !entry.name.endsWith('.d.ts')) {
        const rel = path.relative(EXT_ROOT, full).split(path.sep).join('/');
        sources.set(rel, fs.readFileSync(full, 'utf8'));
      }
    }
  };
  walk(SRC_ROOT);
  return [...sources.keys()].sort().reduce((sorted, key) => {
    sorted.set(key, sources.get(key) ?? '');
    return sorted;
  }, new Map<string, string>());
}

function realInput(): ReachabilityInput {
  return {
    manifest: JSON.parse(fs.readFileSync(path.join(EXT_ROOT, 'package.json'), 'utf8')) as unknown,
    sources: runtimeSources(),
    clientLanguageIds: SUPPORTED_PERL_LANGUAGE_IDS,
    descriptiveSettingSources: DESCRIPTIVE_SETTING_SOURCES,
    allowlist: REVIEWED_ALLOWLIST,
  };
}

describe('manifest contribution reachability (#7765)', () => {
  test('every load-bearing contribution in package.json has a runtime owner', () => {
    expect(checkManifestReachability(realInput())).toEqual([]);
  });

  test('a second run over the same tree is identical', () => {
    expect(checkManifestReachability(realInput())).toEqual(checkManifestReachability(realInput()));
  });

  test('allowlist entries carry a reason and an owning issue', () => {
    for (const entry of REVIEWED_ALLOWLIST) {
      expect(entry.reason.trim().length).toBeGreaterThan(0);
      expect(entry.issue).toBeGreaterThan(0);
    }
  });
});

// ---------------------------------------------------------------------------
// Recurrence controls: structurally different fixtures, not a denylist of the
// defects already found.
// ---------------------------------------------------------------------------

const OWNED_SOURCE = `
import * as vscode from 'vscode';
const RESTART = 'perl-lsp.restart';
export function activate() {
  vscode.commands.registerCommand(RESTART, () => undefined);
  vscode.commands.registerCommand(
    'perl-lsp.showOutput',
    () => undefined,
  );
  vscode.debug.registerDebugConfigurationProvider('perl', {});
  vscode.languages.registerHoverProvider([{ language: 'gherkin' }], {});
  vscode.workspace.getConfiguration('perl-lsp').get('serverPath');
}
`;

type Entry = Record<string, unknown>;
interface FixtureManifest {
  activationEvents: string[];
  contributes: {
    commands: Entry[];
    menus: { commandPalette: Entry[]; [menu: string]: Entry[] };
    keybindings: Entry[];
    configuration: { properties: Record<string, Entry> };
    languages: Entry[];
    debuggers: Entry[];
    walkthroughs: Array<{ id: string; steps: Entry[] }>;
  };
}

function fixtureManifest(): FixtureManifest {
  return {
    activationEvents: ['onLanguage:perl', 'onDebugResolve:perl', 'onWalkthrough:start'],
    contributes: {
      commands: [{ command: 'perl-lsp.restart' }, { command: 'perl-lsp.showOutput' }],
      menus: { commandPalette: [{ command: 'perl-lsp.restart' }] },
      keybindings: [
        { command: 'perl-lsp.showOutput', key: 'f9' },
        { command: 'editor.action.formatDocument', key: 'f8' },
      ],
      configuration: { properties: { 'perl-lsp.serverPath': { type: 'string' } } },
      languages: [{ id: 'perl' }, { id: 'gherkin' }],
      debuggers: [{ type: 'perl' }],
      walkthroughs: [
        {
          id: 'start',
          steps: [{ id: 'one', description: '[Restart](command:perl-lsp.restart)' }],
        },
      ],
    },
  };
}

function fixture(
  mutate: (manifest: FixtureManifest) => void = () => undefined,
  extraSource = '',
  allowlist: readonly ReachabilityAllowance[] = [],
): ReachabilityInput {
  const manifest = fixtureManifest();
  mutate(manifest);
  return {
    manifest,
    sources: new Map([
      ['src/extension.ts', OWNED_SOURCE + extraSource],
      ['src/configurationOwnership.ts', `const ROWS = ['perl-lsp.orphanSetting'];`],
    ]),
    clientLanguageIds: ['perl'],
    descriptiveSettingSources: ['src/configurationOwnership.ts'],
    allowlist,
  };
}

function ids(input: ReachabilityInput): string[] {
  return checkManifestReachability(input).map((v) => `${v.kind}:${v.id}`);
}

describe('manifest reachability recurrence controls (#7765)', () => {
  test('the owned fixture is clean', () => {
    expect(checkManifestReachability(fixture())).toEqual([]);
  });

  test('adding an unowned command fails', () => {
    expect(
      ids(fixture((m) => m.contributes.commands.push({ command: 'perl-lsp.newUnowned' }))),
    ).toEqual(['command:perl-lsp.newUnowned']);
  });

  test('a commented-out registration does not count as an owner', () => {
    const input = fixture(
      (m) => m.contributes.commands.push({ command: 'perl-lsp.ghost' }),
      `// vscode.commands.registerCommand('perl-lsp.ghost', () => undefined);\n/* registerCommand('perl-lsp.ghost') */`,
    );
    expect(ids(input)).toEqual(['command:perl-lsp.ghost']);
  });

  test('a menu, keybinding or walkthrough targeting a removed command fails', () => {
    const input = fixture((m) => {
      m.contributes.menus['editor/context'] = [{ command: 'perl-lsp.removedMenu' }];
      m.contributes.keybindings.push({ command: 'perl-lsp.removedKey', key: 'f7' });
      m.contributes.walkthroughs[0]?.steps.push({
        id: 'two',
        description: 'Run [it](command:perl-lsp.removedLink)',
        completionEvents: ['onCommand:perl-lsp.removedEvent'],
      });
    });
    expect(ids(input)).toEqual([
      'commandReference:perl-lsp.removedMenu',
      'commandReference:perl-lsp.removedKey',
      'commandReference:perl-lsp.removedLink',
      'commandReference:perl-lsp.removedEvent',
    ]);
  });

  test('a menu entry for a registered but uncontributed command fails', () => {
    const input = fixture(
      (m) => m.contributes.menus.commandPalette.push({ command: 'perl-lsp.internal' }),
      `vscode.commands.registerCommand('perl-lsp.internal', () => undefined);`,
    );
    expect(ids(input)).toEqual(['commandReference:perl-lsp.internal']);
  });

  test('runtime code invoking an unregistered perl-lsp command fails', () => {
    const input = fixture(undefined, `vscode.commands.executeCommand('perl-lsp.vanished');`);
    expect(ids(input)).toEqual(['commandReference:perl-lsp.vanished']);
  });

  test('a setting only named in a descriptive table has no reader', () => {
    const input = fixture((m) => {
      m.contributes.configuration.properties['perl-lsp.orphanSetting'] = { type: 'boolean' };
    });
    expect(ids(input)).toEqual(['setting:perl-lsp.orphanSetting']);
  });

  test('activation for a language no selector attaches to fails', () => {
    expect(ids(fixture((m) => m.activationEvents.push('onLanguage:perl6')))).toEqual([
      'activationEvent:onLanguage:perl6',
    ]);
  });

  test('a contributed language without a selector fails', () => {
    expect(ids(fixture((m) => m.contributes.languages.push({ id: 'pod' })))).toEqual([
      'language:pod',
    ]);
  });

  test('a debugger type with no provider, or resolve activation for it, fails', () => {
    const input = fixture((m) => {
      m.contributes.debuggers.push({ type: 'perl-remote' });
      m.activationEvents.push('onDebugResolve:perl-remote');
    });
    expect(ids(input)).toEqual([
      'debugger:perl-remote',
      'activationEvent:onDebugResolve:perl-remote',
    ]);
  });

  test('an unclassified activation event kind is reported, not assumed owned', () => {
    expect(ids(fixture((m) => m.activationEvents.push('*')))).toEqual(['activationEvent:*']);
    expect(ids(fixture((m) => m.activationEvents.push('onWalkthrough:missing')))).toEqual([
      'activationEvent:onWalkthrough:missing',
    ]);
  });

  test('a registration through an unresolvable identifier is reported', () => {
    const input = fixture(
      undefined,
      `vscode.commands.registerCommand(dynamicId, () => undefined);`,
    );
    expect(ids(input)).toEqual(['command:src/extension.ts: registerCommand(dynamicId)']);
  });

  test('a reviewed allowance suppresses exactly its violation, and goes stale once owned', () => {
    const allowance: ReachabilityAllowance = {
      kind: 'command',
      id: 'perl-lsp.planned',
      reason: 'fixture',
      issue: 1,
    };
    expect(
      ids(
        fixture((m) => m.contributes.commands.push({ command: 'perl-lsp.planned' }), '', [
          allowance,
        ]),
      ),
    ).toEqual([]);
    expect(ids(fixture(undefined, '', [allowance]))).toEqual(['command:perl-lsp.planned']);
  });
});

describe('stripComments', () => {
  test('keeps string contents that look like comments', () => {
    const text = `const url = 'https://example.test'; // gone\nconst x = "/* kept */";`;
    const stripped = stripComments(text);
    expect(stripped).toContain(`'https://example.test'`);
    expect(stripped).toContain(`"/* kept */"`);
    expect(stripped).not.toContain('gone');
  });
});
