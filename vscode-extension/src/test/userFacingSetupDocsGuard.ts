const WORKSPACE_PLACEMENT =
  /\.vscode\/settings\.json|\.code-workspace|workspace `.vscode\/settings\.json|in your test workspace/i;

const USER_SETTINGS_DIRECTIVE =
  /Preferences:\s*Open User Settings \(JSON\)|Open User Settings \(JSON\)/i;

const NEGATION = /\b(?:do not|don't|never|ignored|ignores|must not|cannot)\b/i;

function workspacePlacementIsNegative(lines: string[], index: number): boolean {
  const line = lines[index];
  if (line === undefined) {
    return false;
  }
  if (NEGATION.test(line)) {
    return true;
  }
  const previous = lines[index - 1];
  if (previous !== undefined && NEGATION.test(previous) && WORKSPACE_PLACEMENT.test(line)) {
    return true;
  }
  return false;
}

function proseDirectsToUserSettingsOnly(lines: string[], beforeLineExclusive: number): boolean {
  const start = Math.max(0, beforeLineExclusive - 12);
  const slice = lines.slice(start, beforeLineExclusive).join('\n');
  return USER_SETTINGS_DIRECTIVE.test(slice);
}

function contextHasActionableWorkspacePlacement(
  lines: string[],
  beforeLineExclusive: number,
): boolean {
  const start = Math.max(0, beforeLineExclusive - 12);
  for (let i = start; i < beforeLineExclusive; i++) {
    const line = lines[i];
    if (line === undefined || !WORKSPACE_PLACEMENT.test(line)) {
      continue;
    }
    if (workspacePlacementIsNegative(lines, i)) {
      continue;
    }
    return true;
  }
  return false;
}

export const USER_FACING_SETUP_GUIDES = [
  'docs/EDITORS/VS_CODE_SETUP.md',
  'docs/EDITORS/CURSOR_SETUP.md',
  'docs/how-to/FEATURE_SELECTION.md',
  'vscode-extension/DEVELOPMENT.md',
] as const;

export type SetupDocViolation = {
  key: string;
  line: number;
  reason: string;
};

function lineNumberAtOffset(text: string, offset: number): number {
  return text.slice(0, offset).split(/\r?\n/).length;
}

function keysInJsonSnippet(snippet: string, machineKeys: ReadonlySet<string>): string[] {
  const found: string[] = [];
  for (const key of machineKeys) {
    if (snippet.includes(`"${key}"`)) {
      found.push(key);
    }
  }
  return found;
}

const AFFIRMATIVE_WORKSPACE_INSTRUCTION =
  /\b(?:add to|create or edit|create|edit)\b[^\n]*\.vscode\/settings\.json/i;

function keysInInstructionProse(snippet: string, machineKeys: ReadonlySet<string>): string[] {
  const found: string[] = [];
  for (const key of machineKeys) {
    if (snippet.includes(`\`${key}\``)) {
      found.push(key);
    }
  }
  return found;
}

/**
 * Finds machine-scoped setting keys documented as if they take effect from a
 * workspace `.vscode/settings.json` (or equivalent committed placement).
 */
export function machineScopedKeysInWorkspaceGuidance(
  markdown: string,
  machineKeys: readonly string[],
): SetupDocViolation[] {
  const machineSet = new Set(machineKeys);
  const lines = markdown.split(/\r?\n/);
  const violations: SetupDocViolation[] = [];
  const seen = new Set<string>();

  const record = (key: string, line: number, reason: string) => {
    const id = `${key}:${line}:${reason}`;
    if (seen.has(id)) {
      return;
    }
    seen.add(id);
    violations.push({ key, line, reason });
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (line === undefined || !AFFIRMATIVE_WORKSPACE_INSTRUCTION.test(line)) {
      continue;
    }
    if (workspacePlacementIsNegative(lines, i)) {
      continue;
    }
    const windowEnd = Math.min(lines.length, i + 13);
    for (let j = i; j < windowEnd; j++) {
      const windowLine = lines[j];
      if (windowLine === undefined) {
        continue;
      }
      if (j > i) {
        if (USER_SETTINGS_DIRECTIVE.test(windowLine) || /^###\s/.test(windowLine)) {
          break;
        }
      }
      for (const key of [
        ...keysInJsonSnippet(windowLine, machineSet),
        ...keysInInstructionProse(windowLine, machineSet),
      ]) {
        record(key, j + 1, `within 12 lines of workspace placement at line ${i + 1}`);
      }
    }
  }

  const fencedJson = /```json\s*\n([\s\S]*?)```/g;
  for (const match of markdown.matchAll(fencedJson)) {
    const body = match[1] ?? '';
    const blockStart = lineNumberAtOffset(markdown, match.index ?? 0);
    const beforeLine = blockStart - 1;

    const workspaceCommentInside = WORKSPACE_PLACEMENT.test(body);
    const workspaceProseBefore = contextHasActionableWorkspacePlacement(lines, beforeLine);
    const userOnlyBefore =
      proseDirectsToUserSettingsOnly(lines, beforeLine) && !workspaceProseBefore;

    if (userOnlyBefore && !workspaceCommentInside) {
      continue;
    }
    if (
      proseDirectsToUserSettingsOnly(lines, beforeLine) &&
      !workspaceProseBefore &&
      !workspaceCommentInside
    ) {
      continue;
    }
    if (!workspaceCommentInside && !workspaceProseBefore) {
      continue;
    }

    for (const key of keysInJsonSnippet(body, machineSet)) {
      record(
        key,
        blockStart,
        workspaceCommentInside
          ? 'JSON example comments reference workspace settings.json'
          : `JSON block follows workspace placement prose near line ${blockStart}`,
      );
    }
  }

  return violations.sort((a, b) => a.line - b.line || a.key.localeCompare(b.key));
}
