/**
 * Manifest contribution reachability (#7765).
 *
 * `package.json` is a product contract: every command, setting, activation
 * event, debugger and language it contributes is something a user can
 * discover. This checker proves each load-bearing contribution maps to a
 * runtime owner in the extension's own sources, or sits in an explicit,
 * reviewed allowlist with a reason and an owning issue.
 *
 * It is static and deliberately bounded:
 *
 * - a command is owned when `vscode.commands.registerCommand` (or
 *   `registerTextEditorCommand`) registers its exact ID, given as a string
 *   literal or as a `const` bound to one;
 * - a setting is owned when its key, or the key without the `perl-lsp.`
 *   section, is a string literal in runtime source outside the descriptive
 *   ownership tables (`configurationOwnership.ts` records *how* each setting
 *   is consumed; this check proves a reader still exists);
 * - an activation event is owned when its trigger has a matching runtime
 *   owner (a selector language, a registered command, a debug provider, a
 *   contributed walkthrough); an event kind the checker does not know is
 *   reported as unclassified rather than assumed fine;
 * - a debugger type is owned when a debug configuration provider or adapter
 *   descriptor factory is registered for it;
 * - a language is owned when a document selector attaches to it.
 *
 * Every `perl-lsp.*` command referenced from menus, keybindings, walkthrough
 * links/completion events, or runtime `executeCommand`/`command:` targets
 * must resolve to a registered command, and menu targets must also be
 * contributed. References to commands outside the `perl-lsp.` namespace
 * belong to VS Code or other extensions and are out of scope.
 *
 * It does not prove semantic correctness, installed reachability (#4346 /
 * #6056), debugger mode support (#7278) or workspace topology (#4261).
 */

export type ContributionKind =
  | 'command'
  | 'commandReference'
  | 'setting'
  | 'activationEvent'
  | 'debugger'
  | 'language';

export interface ReachabilityViolation {
  readonly kind: ContributionKind;
  readonly id: string;
  readonly reason: string;
}

/** A reviewed, explicitly bounded contribution with no runtime owner today. */
export interface ReachabilityAllowance {
  readonly kind: ContributionKind;
  readonly id: string;
  readonly reason: string;
  /** Issue that owns wiring or removing the contribution. */
  readonly issue: number;
}

export interface ReachabilityInput {
  readonly manifest: unknown;
  /** Runtime (non-test) TypeScript sources keyed by repository-relative path. */
  readonly sources: ReadonlyMap<string, string>;
  /** Language IDs the language client's document selector attaches to. */
  readonly clientLanguageIds: readonly string[];
  /** Source paths that describe settings rather than read them. */
  readonly descriptiveSettingSources: readonly string[];
  readonly allowlist: readonly ReachabilityAllowance[];
}

const OWN_COMMAND_PREFIX = 'perl-lsp.';

type JsonObject = Record<string, unknown>;

function isObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function objects(value: unknown): JsonObject[] {
  return Array.isArray(value) ? value.filter(isObject) : [];
}

function str(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined;
}

/**
 * Replace comments with spaces so commented-out registrations cannot count as
 * owners. String and template literal contents are kept verbatim.
 */
export function stripComments(text: string): string {
  let out = '';
  let i = 0;
  while (i < text.length) {
    const ch = text[i];
    const next = text[i + 1];
    if (ch === '/' && next === '/') {
      while (i < text.length && text[i] !== '\n') {
        out += ' ';
        i += 1;
      }
      continue;
    }
    if (ch === '/' && next === '*') {
      const end = text.indexOf('*/', i + 2);
      const stop = end === -1 ? text.length : end + 2;
      out += text.slice(i, stop).replace(/[^\n]/g, ' ');
      i = stop;
      continue;
    }
    if (ch === "'" || ch === '"' || ch === '`') {
      let j = i + 1;
      while (j < text.length && text[j] !== ch) {
        if (text[j] === '\\') j += 1;
        else if (ch !== '`' && text[j] === '\n') break;
        j += 1;
      }
      out += text.slice(i, j + 1);
      i = j + 1;
      continue;
    }
    out += ch;
    i += 1;
  }
  return out;
}

// Quote group kept for stable group numbering; no backreference, because this
// fragment is embedded after other groups in larger patterns.
const LITERAL = String.raw`(['"\x60])([^'"\x60\n]+)['"\x60]`;
const IDENT = String.raw`([A-Za-z_$][\w$]*)`;
const ARG = String.raw`\(\s*(?:${LITERAL}|${IDENT})\s*[,)]`;

/** Resolve string `const` bindings across all sources; ambiguous names map to undefined. */
function stringConstants(sources: Iterable<string>): Map<string, string | undefined> {
  const constants = new Map<string, string | undefined>();
  const re = new RegExp(
    String.raw`\bconst\s+${IDENT}\s*(?::\s*[^=;]+)?=\s*${LITERAL}\s*(?:as\s+const\s*)?;`,
    'g',
  );
  for (const text of sources) {
    for (const match of text.matchAll(re)) {
      const [, name, , value] = match;
      if (name === undefined || value === undefined) continue;
      if (constants.has(name) && constants.get(name) !== value) constants.set(name, undefined);
      else constants.set(name, value);
    }
  }
  return constants;
}

interface CallArguments {
  readonly resolved: Set<string>;
  readonly unresolved: string[];
}

function firstArguments(
  label: string,
  callee: string,
  sources: ReadonlyMap<string, string>,
  constants: Map<string, string | undefined>,
): CallArguments {
  const resolved = new Set<string>();
  const unresolved: string[] = [];
  const re = new RegExp(String.raw`\b${callee}${ARG}`, 'g');
  for (const [file, text] of sources) {
    for (const match of text.matchAll(re)) {
      const literal = match[2];
      const ident = match[3];
      if (literal !== undefined) {
        resolved.add(literal);
      } else if (ident !== undefined) {
        const value = constants.get(ident);
        if (value === undefined) unresolved.push(`${file}: ${label}(${ident})`);
        else resolved.add(value);
      }
    }
  }
  return { resolved, unresolved };
}

function literalsIn(sources: Iterable<string>): Set<string> {
  const literals = new Set<string>();
  const re = new RegExp(LITERAL, 'g');
  for (const text of sources) {
    for (const match of text.matchAll(re)) {
      if (match[2] !== undefined) literals.add(match[2]);
    }
  }
  return literals;
}

function commandReferencesInSource(
  sources: ReadonlyMap<string, string>,
  constants: Map<string, string | undefined>,
): Array<{ id: string; where: string }> {
  const refs: Array<{ id: string; where: string }> = [];
  const patterns = [
    new RegExp(String.raw`\bexecuteCommand${ARG}`, 'g'),
    new RegExp(String.raw`\bcommand:\s*(?:${LITERAL}|${IDENT})`, 'g'),
  ];
  for (const [file, text] of sources) {
    for (const re of patterns) {
      for (const match of text.matchAll(re)) {
        const id = match[2] ?? (match[3] !== undefined ? constants.get(match[3]) : undefined);
        if (id?.startsWith(OWN_COMMAND_PREFIX)) refs.push({ id, where: file });
      }
    }
  }
  return refs;
}

function contributedSettingKeys(contributes: JsonObject): string[] {
  const configuration = contributes.configuration;
  const groups = Array.isArray(configuration) ? objects(configuration) : objects([configuration]);
  return groups.flatMap((group) =>
    isObject(group.properties) ? Object.keys(group.properties) : [],
  );
}

/** Every `perl-lsp.*` command a manifest surface points at, with where it was found. */
function manifestCommandReferences(
  contributes: JsonObject,
): Array<{ id: string; where: string; menu: boolean }> {
  const refs: Array<{ id: string; where: string; menu: boolean }> = [];
  const menus = isObject(contributes.menus) ? contributes.menus : {};
  for (const [menu, entries] of Object.entries(menus)) {
    for (const entry of objects(entries)) {
      const id = str(entry.command);
      if (id !== undefined) refs.push({ id, where: `menus.${menu}`, menu: true });
    }
  }
  for (const binding of objects(contributes.keybindings)) {
    const id = str(binding.command);
    if (id !== undefined) refs.push({ id, where: 'keybindings', menu: false });
  }
  for (const walkthrough of objects(contributes.walkthroughs)) {
    for (const step of objects(walkthrough.steps)) {
      const where = `walkthroughs.${str(walkthrough.id) ?? '?'}.${str(step.id) ?? '?'}`;
      const text = [str(step.description) ?? '', JSON.stringify(step.media ?? null)].join('\n');
      for (const match of text.matchAll(/command:([A-Za-z0-9_.-]+)/g)) {
        if (match[1] !== undefined) refs.push({ id: match[1], where, menu: false });
      }
      for (const event of Array.isArray(step.completionEvents) ? step.completionEvents : []) {
        const id = str(event)?.match(/^onCommand:(.+)$/)?.[1];
        if (id !== undefined) refs.push({ id, where: `${where}.completionEvents`, menu: false });
      }
    }
  }
  return refs.filter((ref) => ref.id.startsWith(OWN_COMMAND_PREFIX));
}

/** Check every load-bearing manifest contribution against its runtime owner. */
export function checkManifestReachability(input: ReachabilityInput): ReachabilityViolation[] {
  const manifest = isObject(input.manifest) ? input.manifest : {};
  const contributes = isObject(manifest.contributes) ? manifest.contributes : {};

  const runtime = new Map<string, string>();
  for (const [file, text] of input.sources) runtime.set(file, stripComments(text));
  const constants = stringConstants(runtime.values());

  const commands = firstArguments(
    'registerCommand',
    String.raw`register(?:TextEditor)?Command`,
    runtime,
    constants,
  );
  const debugProviders = firstArguments(
    'registerDebugConfigurationProvider',
    String.raw`registerDebugConfigurationProvider`,
    runtime,
    constants,
  );
  const debugFactories = firstArguments(
    'registerDebugAdapterDescriptorFactory',
    String.raw`registerDebugAdapterDescriptorFactory`,
    runtime,
    constants,
  );
  const selectorLanguages = new Set<string>(input.clientLanguageIds);
  for (const text of runtime.values()) {
    const re = new RegExp(String.raw`\blanguage:\s*(?:${LITERAL}|${IDENT})`, 'g');
    for (const match of text.matchAll(re)) {
      const id = match[2] ?? (match[3] !== undefined ? constants.get(match[3]) : undefined);
      if (id !== undefined) selectorLanguages.add(id);
    }
  }

  const found: ReachabilityViolation[] = [];
  const report = (kind: ContributionKind, id: string, reason: string) =>
    found.push({ kind, id, reason });

  for (const site of [
    ...commands.unresolved,
    ...debugProviders.unresolved,
    ...debugFactories.unresolved,
  ]) {
    report('command', site, 'registration ID is not a string literal or string const');
  }

  // Commands.
  const contributedCommands = new Set<string>();
  for (const entry of objects(contributes.commands)) {
    const id = str(entry.command);
    if (id === undefined) continue;
    contributedCommands.add(id);
    if (!commands.resolved.has(id)) report('command', id, 'contributed but never registered');
  }

  // Command references from manifest surfaces and runtime source.
  for (const ref of manifestCommandReferences(contributes)) {
    if (!commands.resolved.has(ref.id)) {
      report('commandReference', ref.id, `${ref.where} targets a command that is never registered`);
    } else if (ref.menu && !contributedCommands.has(ref.id)) {
      report('commandReference', ref.id, `${ref.where} targets a command that is not contributed`);
    }
  }
  for (const ref of commandReferencesInSource(runtime, constants)) {
    if (!commands.resolved.has(ref.id)) {
      report('commandReference', ref.id, `${ref.where} invokes a command that is never registered`);
    }
  }

  // Settings.
  const descriptive = new Set(input.descriptiveSettingSources);
  const readerLiterals = literalsIn(
    [...runtime].filter(([file]) => !descriptive.has(file)).map(([, text]) => text),
  );
  for (const key of contributedSettingKeys(contributes)) {
    const section = key.startsWith(OWN_COMMAND_PREFIX) ? key.slice(OWN_COMMAND_PREFIX.length) : key;
    if (!readerLiterals.has(key) && !readerLiterals.has(section)) {
      report('setting', key, 'contributed but no runtime source reads it');
    }
  }

  // Languages and debuggers.
  const contributedLanguages = new Set<string>();
  for (const language of objects(contributes.languages)) {
    const id = str(language.id);
    if (id === undefined) continue;
    contributedLanguages.add(id);
    if (!selectorLanguages.has(id))
      report('language', id, 'contributed but no document selector attaches to it');
  }
  const contributedDebuggers = new Set<string>();
  for (const debuggerEntry of objects(contributes.debuggers)) {
    const type = str(debuggerEntry.type);
    if (type === undefined) continue;
    contributedDebuggers.add(type);
    if (!debugProviders.resolved.has(type) && !debugFactories.resolved.has(type)) {
      report(
        'debugger',
        type,
        'contributed but no debug provider or adapter factory is registered',
      );
    }
  }
  const walkthroughs = new Set(objects(contributes.walkthroughs).map((w) => str(w.id)));
  const views = new Set(
    Object.values(isObject(contributes.views) ? contributes.views : {}).flatMap((entries) =>
      objects(entries).map((view) => str(view.id)),
    ),
  );

  // Activation events.
  const events = Array.isArray(manifest.activationEvents) ? manifest.activationEvents : [];
  for (const raw of events) {
    const event = str(raw);
    if (event === undefined) continue;
    const [kind, ...rest] = event.split(':');
    const target = rest.join(':');
    let owned: boolean;
    switch (kind) {
      case 'onLanguage':
        owned = selectorLanguages.has(target);
        break;
      case 'onCommand':
        owned = commands.resolved.has(target);
        break;
      case 'onDebugResolve':
        owned = debugProviders.resolved.has(target);
        break;
      case 'onDebugInitialConfigurations':
      case 'onDebugDynamicConfigurations':
        owned = target === '' && debugProviders.resolved.size > 0;
        break;
      case 'onDebugAdapterProtocolTracker':
      case 'onDebug':
        owned = target === '' ? contributedDebuggers.size > 0 : contributedDebuggers.has(target);
        break;
      case 'onWalkthrough':
        owned = walkthroughs.has(target);
        break;
      case 'onView':
        owned = views.has(target);
        break;
      default:
        report('activationEvent', event, 'event kind is not classified by the reachability check');
        continue;
    }
    if (!owned) report('activationEvent', event, 'activation trigger has no runtime owner');
  }

  // Apply the reviewed allowlist; stale entries fail too.
  const remaining: ReachabilityViolation[] = [];
  const used = new Set<ReachabilityAllowance>();
  for (const violation of found) {
    const allowance = input.allowlist.find(
      (entry) => entry.kind === violation.kind && entry.id === violation.id,
    );
    if (allowance === undefined) remaining.push(violation);
    else used.add(allowance);
  }
  for (const entry of input.allowlist) {
    if (!used.has(entry)) {
      remaining.push({
        kind: entry.kind,
        id: entry.id,
        reason: `stale allowlist entry (#${entry.issue}): contribution is owned or no longer declared`,
      });
    }
  }
  return remaining;
}
