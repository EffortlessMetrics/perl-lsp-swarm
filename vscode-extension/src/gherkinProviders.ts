import * as vscode from 'vscode';
import { isPotentiallyExpensiveRegex } from './gherkinRedosGuard';
import {
  MAX_STEP_DEFINITION_FILE_BYTES,
  MAX_STEP_DEFINITION_TOTAL_BYTES,
  MAX_STEP_DEFINITION_TOTAL_READ_BYTES,
  readBoundedFile,
} from './gherkinStepDefinitions';

type OutlineKind = 'feature' | 'rule' | 'background' | 'scenario' | 'examples' | 'step';
type StepKeyword = 'Given' | 'When' | 'Then' | 'And' | 'But' | '*';
type StepDefinitionKeyword = Exclude<StepKeyword, '*'>;

interface OutlineNode {
  name: string;
  detail: string;
  kind: OutlineKind;
  level: number;
  line: number;
  startCharacter: number;
  endCharacter: number;
  endLine: number;
  children: OutlineNode[];
}

interface GherkinStepReference {
  keyword: StepKeyword;
  effectiveKeyword?: StepDefinitionKeyword | undefined;
  text: string;
  originSelectionRange: vscode.Range;
}

export interface StepDefinitionDocument {
  uri: vscode.Uri;
  text: string;
}

interface ParsedStepDefinition {
  keyword: StepDefinitionKeyword;
  matcher: StepMatcher;
  range: vscode.Range;
  uri: vscode.Uri;
  score: number;
}

type StepMatcher =
  | {
      kind: 'exact';
      text: string;
    }
  | {
      kind: 'regex';
      source: string;
      flags: string;
    };

const HEADER_RE = /^\s*(Feature|Rule|Background|Scenario(?: Outline| Template)?|Examples)\s*:(.*)$/;
const STEP_RE = /^\s*(Given|When|Then|And|But|\*)(?:\s+|$)(.*)$/;
const STEP_DEFINITION_RE = /^\s*(Given|When|Then|And|But)\b/gm;
const STEP_DEFINITION_FILE_GLOBS = [
  '**/features/step_definitions/**/*.pm',
  '**/step_definitions/**/*.pm',
  '**/*.pm',
  '**/*.pl',
  '**/*.t',
] as const;
const STEP_DEFINITION_EXCLUDE_GLOB = '{**/node_modules/**,**/blib/**,**/.git/**}';
const STEP_DEFINITION_FILE_LIMIT = 1000;
const MAX_MATCH_REGEX_LENGTH = 256;
const MAX_MATCH_STEP_TEXT_LENGTH = 512;
// Catastrophic backtracking (ReDoS) requires a *quantified group that itself
// contains a quantifier, a backreference, a lookaround, or alternation. A
// single character class followed by one quantifier
// (`[^"]+`, `[0-9]+`) is linear-time and safe — flagging it produced false
// negatives that suppressed step-definition links for ordinary `"([^"]+)"`
// patterns (see #859). The parser and denylist live in gherkinRedosGuard.ts.
const DELIMITER_PAIRS: Record<string, string> = {
  '{': '}',
  '[': ']',
  '(': ')',
  '<': '>',
};

export function registerGherkinProviders(): vscode.Disposable[] {
  const selector: vscode.DocumentSelector = [{ language: 'gherkin' }];

  const symbolProvider: vscode.DocumentSymbolProvider = {
    provideDocumentSymbols(document: vscode.TextDocument): vscode.DocumentSymbol[] {
      return provideGherkinDocumentSymbols(document.getText());
    },
  };

  const foldingProvider: vscode.FoldingRangeProvider = {
    provideFoldingRanges(document: vscode.TextDocument): vscode.FoldingRange[] {
      return provideGherkinFoldingRanges(document.getText());
    },
  };

  const definitionProvider: vscode.DefinitionProvider = {
    async provideDefinition(
      document: vscode.TextDocument,
      position: vscode.Position,
      token: vscode.CancellationToken,
    ): Promise<vscode.LocationLink[] | undefined> {
      const candidates = await loadStepDefinitionDocuments(token);
      if (token.isCancellationRequested) {
        return undefined;
      }

      const links = provideGherkinStepDefinitionLinks(document.getText(), position, candidates);
      return links.length > 0 ? links : undefined;
    },
  };

  return [
    vscode.languages.registerDocumentSymbolProvider(selector, symbolProvider),
    vscode.languages.registerFoldingRangeProvider(selector, foldingProvider),
    vscode.languages.registerDefinitionProvider(selector, definitionProvider),
  ];
}

export function provideGherkinDocumentSymbols(text: string): vscode.DocumentSymbol[] {
  return buildOutline(text).map(toDocumentSymbol);
}

export function provideGherkinFoldingRanges(text: string): vscode.FoldingRange[] {
  const ranges: vscode.FoldingRange[] = [];

  const visit = (node: OutlineNode): void => {
    if (node.kind !== 'step' && node.endLine > node.line) {
      ranges.push({ start: node.line, end: node.endLine } as vscode.FoldingRange);
    }
    for (const child of node.children) {
      visit(child);
    }
  };

  for (const root of buildOutline(text)) {
    visit(root);
  }

  return ranges;
}

export function provideGherkinStepDefinitionLinks(
  featureText: string,
  position: vscode.Position,
  documents: readonly StepDefinitionDocument[],
): vscode.LocationLink[] {
  const step = extractStepReference(featureText, position);
  if (!step) {
    return [];
  }

  const matches: ParsedStepDefinition[] = [];
  for (const document of documents) {
    matches.push(...findMatchingStepDefinitions(step, document));
  }

  matches.sort((left, right) => {
    if (right.score !== left.score) {
      return right.score - left.score;
    }

    const leftUri = left.uri.toString();
    const rightUri = right.uri.toString();
    if (leftUri !== rightUri) {
      return leftUri.localeCompare(rightUri);
    }

    return compareRanges(left.range, right.range);
  });

  const deduped = new Map<string, vscode.LocationLink>();
  for (const match of matches) {
    const key = `${match.uri.toString()}:${match.range.start.line}:${match.range.start.character}`;
    if (!deduped.has(key)) {
      deduped.set(key, {
        originSelectionRange: step.originSelectionRange,
        targetUri: match.uri,
        targetRange: match.range,
        targetSelectionRange: match.range,
      });
    }
  }

  return Array.from(deduped.values());
}

function buildOutline(text: string): OutlineNode[] {
  const lines = text.split(/\r?\n/);
  const roots: OutlineNode[] = [];
  const stack: OutlineNode[] = [];

  for (let lineNumber = 0; lineNumber < lines.length; lineNumber += 1) {
    const line = lines[lineNumber];
    if (line === undefined) {
      continue;
    }

    const headerMatch = line.match(HEADER_RE);
    const stepMatch = line.match(STEP_RE);

    let node: OutlineNode | null = null;
    if (headerMatch) {
      const keyword = headerMatch[1];
      const title = headerMatch[2];
      if (keyword !== undefined && title !== undefined) {
        node = createHeaderNode(line, lineNumber, keyword, title.trim());
      }
    } else if (stepMatch) {
      const keyword = stepMatch[1];
      const text = stepMatch[2];
      if (keyword !== undefined && text !== undefined) {
        node = createStepNode(line, lineNumber, keyword, text.trim());
      }
    }

    if (!node) {
      continue;
    }

    while (true) {
      const parent = stack.at(-1);
      if (!parent || parent.level < node.level) {
        break;
      }

      const completed = stack.pop();
      if (completed) {
        finalizeNode(completed, lineNumber - 1);
      }
    }

    const parent = stack.at(-1);
    if (parent) {
      parent.children.push(node);
    } else {
      roots.push(node);
    }

    stack.push(node);
  }

  const lastLine = Math.max(lines.length - 1, 0);
  while (stack.length > 0) {
    finalizeNode(stack.pop()!, lastLine);
  }

  return roots;
}

async function loadStepDefinitionDocuments(
  token: vscode.CancellationToken,
): Promise<StepDefinitionDocument[]> {
  const seen = new Map<string, vscode.Uri>();

  for (const pattern of STEP_DEFINITION_FILE_GLOBS) {
    if (token.isCancellationRequested) {
      return [];
    }

    const uris = await vscode.workspace.findFiles(
      pattern,
      STEP_DEFINITION_EXCLUDE_GLOB,
      STEP_DEFINITION_FILE_LIMIT,
    );

    for (const uri of uris) {
      seen.set(uri.toString(), uri);
    }
  }

  const scan = await collectStepDefinitionDocuments(Array.from(seen.values()), token);
  return scan.documents;
}

/** Typed refusal causes for the bounded step-definition workspace scan. */
export type StepDefinitionScanRefusal = 'read_budget_exhausted';

/** Outcome of the bounded step-definition workspace scan. */
export interface StepDefinitionScan {
  documents: StepDefinitionDocument[];
  /** Every attempted read, including candidates the scan went on to reject. */
  attemptedBytes: number;
  /** Set when the scan stopped rather than let an attempted read cross the budget. */
  refusal: StepDefinitionScanRefusal | null;
}

/**
 * Read candidate step-definition files sequentially under the same envelope as
 * the step-definition collector (#9773): a per-file byte cap, an aggregate cap
 * on both attempted and retained bytes, and regular local files only. The
 * previous implementation opened each candidate through `openTextDocument`
 * with no byte bound at all, so a workspace of 1000 large or special files
 * could occupy the extension host.
 *
 * The retained cap alone is not a read bound: a candidate rejected by the
 * per-file cap has already consumed up to `MAX_STEP_DEFINITION_FILE_BYTES + 1`
 * bytes of I/O, so every attempted read is charged against the read budget and
 * the scan stops with the typed `read_budget_exhausted` refusal instead of
 * letting the next attempted read cross it. Candidates whose URI scheme is not
 * `file` are skipped: their `fsPath` names no local file this scan may read.
 * A candidate open in the editor with unsaved edits resolves from its buffer
 * under the same envelope, so links never follow stale disk contents; because
 * an edit can land while the disk read is pending, the buffer is re-checked
 * after the await and preferred over the just-read disk text. A clean
 * document takes the disk path so the regular-file and symlink checks stay on
 * the read path.
 *
 * Exported for the containment proof in gherkinProviders.test.ts: the scan
 * bounds are a security claim and need a direct seam, not one observed only
 * through the definition provider.
 */
export async function collectStepDefinitionDocuments(
  candidates: readonly vscode.Uri[],
  token: vscode.CancellationToken,
): Promise<StepDefinitionScan> {
  const documents: StepDefinitionDocument[] = [];
  let acceptedBytes = 0;
  let attemptedBytes = 0;
  let refusal: StepDefinitionScanRefusal | null = null;

  const dirtyDocumentFor = (candidate: vscode.Uri): vscode.TextDocument | undefined =>
    vscode.workspace.textDocuments.find(
      (document) => document.uri.toString() === candidate.toString() && document.isDirty,
    );

  // Admit one candidate's buffer text under the same envelope as a disk read:
  // charged against the read budget, capped per file, and capped on the
  // retained total.
  const admitBufferText = (
    uri: vscode.Uri,
    text: string,
  ): 'admitted' | 'over-file-cap' | 'over-retained-cap' | 'read-budget-exhausted' => {
    const bytes = Buffer.byteLength(text, 'utf8');
    if (attemptedBytes + bytes > MAX_STEP_DEFINITION_TOTAL_READ_BYTES) {
      return 'read-budget-exhausted';
    }
    attemptedBytes += bytes;
    if (bytes > MAX_STEP_DEFINITION_FILE_BYTES) {
      return 'over-file-cap';
    }
    if (acceptedBytes + bytes > MAX_STEP_DEFINITION_TOTAL_BYTES) {
      return 'over-retained-cap';
    }
    acceptedBytes += bytes;
    documents.push({ uri, text });
    return 'admitted';
  };

  const admitDirtyBuffer = (uri: vscode.Uri): 'stop' | 'continue' => {
    const outcome = admitBufferText(uri, dirtyDocumentFor(uri)!.getText());
    if (outcome === 'read-budget-exhausted') {
      refusal = 'read_budget_exhausted';
      return 'stop';
    }
    if (outcome === 'over-retained-cap') {
      return 'stop';
    }
    return 'continue';
  };

  for (const uri of candidates) {
    if (token.isCancellationRequested) {
      break;
    }
    if (uri.scheme !== 'file') {
      continue;
    }
    if (acceptedBytes >= MAX_STEP_DEFINITION_TOTAL_BYTES) {
      break;
    }

    // The scan that replaced `openTextDocument` must not regress dirty-buffer
    // visibility: a document open with unsaved edits resolves from its buffer
    // (charged and capped like any read), while a clean document takes the
    // disk path so the regular-file and symlink checks stay on the read path.
    if (dirtyDocumentFor(uri)) {
      if (admitDirtyBuffer(uri) === 'stop') {
        break;
      }
      continue;
    }

    // Rejected candidates are charged at their worst case — the per-file cap
    // plus the one overflow byte readBoundedFile reads to distinguish "at the
    // limit" from "over it" — so no attempted read can cross the read budget.
    if (
      attemptedBytes + MAX_STEP_DEFINITION_FILE_BYTES + 1 >
      MAX_STEP_DEFINITION_TOTAL_READ_BYTES
    ) {
      refusal = 'read_budget_exhausted';
      break;
    }

    const read = await readBoundedFile(uri.fsPath, MAX_STEP_DEFINITION_FILE_BYTES);
    attemptedBytes += read ? read.byteLength : MAX_STEP_DEFINITION_FILE_BYTES + 1;
    if (!read) {
      continue;
    }

    // An edit can land while the read is pending, which turns the just-read
    // disk text stale. Reconcile after the await: a buffer that is dirty now
    // wins under the same envelope.
    if (dirtyDocumentFor(uri)) {
      if (admitDirtyBuffer(uri) === 'stop') {
        break;
      }
      continue;
    }

    if (acceptedBytes + read.byteLength > MAX_STEP_DEFINITION_TOTAL_BYTES) {
      break;
    }

    acceptedBytes += read.byteLength;
    documents.push({ uri, text: read.text });
  }

  return { documents, attemptedBytes, refusal };
}

function extractStepReference(
  text: string,
  position: vscode.Position,
): GherkinStepReference | null {
  const lines = text.split(/\r?\n/);
  const line = lines[position.line];
  if (line === undefined) {
    return null;
  }

  const match = line.match(STEP_RE);
  if (!match) {
    return null;
  }

  const keyword = match[1] as StepKeyword | undefined;
  const remainder = match[2]?.trim();
  if (!keyword || !remainder) {
    return null;
  }

  const startCharacter = line.search(/\S|$/);
  if (position.character < startCharacter || position.character > line.length) {
    return null;
  }

  return {
    keyword,
    effectiveKeyword: resolveEffectiveKeyword(lines, position.line, keyword),
    text: remainder,
    originSelectionRange: new vscode.Range(
      position.line,
      startCharacter,
      position.line,
      line.length,
    ),
  };
}

function resolveEffectiveKeyword(
  lines: readonly string[],
  line: number,
  keyword: StepKeyword,
): StepDefinitionKeyword | undefined {
  if (keyword === 'Given' || keyword === 'When' || keyword === 'Then') {
    return keyword;
  }

  for (let index = line - 1; index >= 0; index -= 1) {
    const match = lines[index]?.match(STEP_RE);
    if (!match) {
      continue;
    }

    const previousKeyword = match[1] as StepKeyword;
    if (previousKeyword === 'Given' || previousKeyword === 'When' || previousKeyword === 'Then') {
      return previousKeyword;
    }
  }

  return undefined;
}

function findMatchingStepDefinitions(
  step: GherkinStepReference,
  document: StepDefinitionDocument,
): ParsedStepDefinition[] {
  const matches: ParsedStepDefinition[] = [];

  for (const definition of parseStepDefinitions(document)) {
    if (!keywordsAreCompatible(step, definition.keyword)) {
      continue;
    }

    if (!stepTextMatches(step.text, definition.matcher)) {
      continue;
    }

    matches.push(definition);
  }

  return matches;
}

function parseStepDefinitions(document: StepDefinitionDocument): ParsedStepDefinition[] {
  const results: ParsedStepDefinition[] = [];
  const lineStarts = computeLineStarts(document.text);

  for (const match of document.text.matchAll(STEP_DEFINITION_RE)) {
    const keyword = match[1] as StepDefinitionKeyword;
    const startOffset = match.index ?? 0;
    let cursor = startOffset + match[0].length;
    cursor = skipWhitespace(document.text, cursor);

    const parsedMatcher = parseStepMatcher(document.text, cursor);
    if (!parsedMatcher) {
      continue;
    }

    const commaOffset = skipWhitespace(document.text, parsedMatcher.endOffset);
    if (document.text[commaOffset] !== ',') {
      continue;
    }

    const rangeStart = startOffset + match[0].search(/\S|$/);
    results.push({
      keyword,
      matcher: parsedMatcher.matcher,
      range: rangeFromOffsets(lineStarts, rangeStart, parsedMatcher.endOffset),
      uri: document.uri,
      score: definitionScore(document.uri, keyword),
    });
  }

  return results;
}

function parseStepMatcher(
  text: string,
  startOffset: number,
): { matcher: StepMatcher; endOffset: number } | null {
  if (text.startsWith('qr', startOffset)) {
    return parseRegexStepMatcher(text, startOffset);
  }

  const first = text[startOffset];
  if (first === "'" || first === '"') {
    return parseQuotedStepMatcher(text, startOffset, first);
  }

  return null;
}

function parseRegexStepMatcher(
  text: string,
  startOffset: number,
): { matcher: StepMatcher; endOffset: number } | null {
  let cursor = skipWhitespace(text, startOffset + 2);
  const open = text[cursor];
  if (!open || /\s/.test(open)) {
    return null;
  }

  const close = DELIMITER_PAIRS[open] ?? open;
  const paired = close !== open;
  const patternStart = cursor + 1;
  cursor = patternStart;

  let depth = paired ? 1 : 0;
  while (cursor < text.length) {
    const current = text[cursor];
    if (current === '\\') {
      cursor += 2;
      continue;
    }

    if (paired) {
      if (current === open) {
        depth += 1;
        cursor += 1;
        continue;
      }

      if (current === close) {
        depth -= 1;
        if (depth === 0) {
          const source = text.slice(patternStart, cursor);
          cursor += 1;
          const flagsStart = cursor;
          while (/[A-Za-z]/.test(text[cursor] ?? '')) {
            cursor += 1;
          }
          return {
            matcher: {
              kind: 'regex',
              source,
              flags: text.slice(flagsStart, cursor),
            },
            endOffset: cursor,
          };
        }
      }

      cursor += 1;
      continue;
    }

    if (current === close) {
      const source = text.slice(patternStart, cursor);
      cursor += 1;
      const flagsStart = cursor;
      while (/[A-Za-z]/.test(text[cursor] ?? '')) {
        cursor += 1;
      }
      return {
        matcher: {
          kind: 'regex',
          source,
          flags: text.slice(flagsStart, cursor),
        },
        endOffset: cursor,
      };
    }

    cursor += 1;
  }

  return null;
}

function parseQuotedStepMatcher(
  text: string,
  startOffset: number,
  quote: "'" | '"',
): { matcher: StepMatcher; endOffset: number } | null {
  let cursor = startOffset + 1;
  while (cursor < text.length) {
    const current = text[cursor];
    if (current === '\\') {
      cursor += 2;
      continue;
    }

    if (current === quote) {
      return {
        matcher: {
          kind: 'exact',
          text: unescapeQuotedStepText(text.slice(startOffset + 1, cursor), quote),
        },
        endOffset: cursor + 1,
      };
    }

    cursor += 1;
  }

  return null;
}

function unescapeQuotedStepText(text: string, quote: "'" | '"'): string {
  if (quote === "'") {
    return text.replace(/\\\\/g, '\\').replace(/\\'/g, "'");
  }

  return text.replace(/\\\\/g, '\\').replace(/\\"/g, '"');
}

function keywordsAreCompatible(
  step: GherkinStepReference,
  definitionKeyword: StepDefinitionKeyword,
): boolean {
  if (step.keyword === '*') {
    return true;
  }

  if (step.keyword === 'And' || step.keyword === 'But') {
    return (
      definitionKeyword === 'And' ||
      definitionKeyword === 'But' ||
      definitionKeyword === step.effectiveKeyword
    );
  }

  return (
    definitionKeyword === step.keyword || definitionKeyword === 'And' || definitionKeyword === 'But'
  );
}

function stepTextMatches(stepText: string, matcher: StepMatcher): boolean {
  if (matcher.kind === 'exact') {
    return matcher.text === stepText;
  }

  if (!isSafeRegexForStepMatching(matcher.source, stepText)) {
    return false;
  }

  try {
    return new RegExp(matcher.source, normalizeRegexFlags(matcher.flags)).test(stepText);
  } catch {
    return false;
  }
}

function isSafeRegexForStepMatching(source: string, stepText: string): boolean {
  if (source.length > MAX_MATCH_REGEX_LENGTH || stepText.length > MAX_MATCH_STEP_TEXT_LENGTH) {
    return false;
  }

  return !isPotentiallyExpensiveRegex(source);
}

function normalizeRegexFlags(flags: string): string {
  let normalized = '';
  for (const flag of flags.toLowerCase()) {
    if ((flag === 'i' || flag === 'm' || flag === 's') && !normalized.includes(flag)) {
      normalized += flag;
    }
  }
  return normalized;
}

function definitionScore(uri: vscode.Uri, keyword: StepDefinitionKeyword): number {
  let score = 0;
  const normalizedPath = uri.fsPath.replace(/\\/g, '/').toLowerCase();

  if (normalizedPath.includes('/step_definitions/')) {
    score += 20;
  }
  if (normalizedPath.endsWith('.pm')) {
    score += 10;
  } else if (normalizedPath.endsWith('.pl')) {
    score += 5;
  }
  if (keyword === 'Given' || keyword === 'When' || keyword === 'Then') {
    score += 5;
  }

  return score;
}

function createHeaderNode(
  line: string,
  lineNumber: number,
  keyword: string,
  title: string,
): OutlineNode {
  const trimmed = line.trim();
  const startCharacter = line.search(/\S|$/);
  const displayName = title.length > 0 ? `${keyword}: ${title}` : trimmed;

  return {
    name: displayName,
    detail: keyword,
    kind: outlineKindForHeader(keyword),
    level: outlineLevelForHeader(keyword),
    line: lineNumber,
    startCharacter,
    endCharacter: line.length,
    endLine: lineNumber,
    children: [],
  };
}

function createStepNode(
  line: string,
  lineNumber: number,
  keyword: string,
  remainder: string,
): OutlineNode {
  const startCharacter = line.search(/\S|$/);
  const displayName = remainder.length > 0 ? `${keyword} ${remainder}` : keyword;

  return {
    name: displayName,
    detail: keyword,
    kind: 'step',
    level: 4,
    line: lineNumber,
    startCharacter,
    endCharacter: line.length,
    endLine: lineNumber,
    children: [],
  };
}

function outlineKindForHeader(keyword: string): OutlineKind {
  switch (keyword) {
    case 'Feature':
      return 'feature';
    case 'Rule':
      return 'rule';
    case 'Background':
      return 'background';
    case 'Examples':
      return 'examples';
    default:
      return 'scenario';
  }
}

function outlineLevelForHeader(keyword: string): number {
  switch (keyword) {
    case 'Feature':
      return 0;
    case 'Rule':
      return 1;
    case 'Background':
    case 'Scenario':
    case 'Scenario Outline':
    case 'Scenario Template':
      return 2;
    case 'Examples':
      return 3;
    default:
      return 2;
  }
}

function finalizeNode(node: OutlineNode, endLine: number): void {
  node.endLine = Math.max(node.line, endLine);
}

function toDocumentSymbol(node: OutlineNode): vscode.DocumentSymbol {
  return {
    name: node.name,
    detail: node.detail,
    kind: symbolKind(node.kind),
    range: new vscode.Range(node.line, node.startCharacter, node.endLine, node.endCharacter),
    selectionRange: new vscode.Range(node.line, node.startCharacter, node.line, node.endCharacter),
    children: node.children.map(toDocumentSymbol),
  } as vscode.DocumentSymbol;
}

function symbolKind(kind: OutlineKind): vscode.SymbolKind {
  switch (kind) {
    case 'feature':
    case 'rule':
      return vscode.SymbolKind.Namespace;
    case 'background':
      return vscode.SymbolKind.Object;
    case 'scenario':
      return vscode.SymbolKind.Method;
    case 'examples':
      return vscode.SymbolKind.Array;
    case 'step':
      return vscode.SymbolKind.String;
  }
}

function skipWhitespace(text: string, offset: number): number {
  let cursor = offset;
  while (cursor < text.length && /\s/.test(text[cursor]!)) {
    cursor += 1;
  }
  return cursor;
}

function computeLineStarts(text: string): number[] {
  const starts = [0];
  for (let index = 0; index < text.length; index += 1) {
    if (text[index] === '\n') {
      starts.push(index + 1);
    }
  }
  return starts;
}

function rangeFromOffsets(
  lineStarts: readonly number[],
  startOffset: number,
  endOffset: number,
): vscode.Range {
  const start = offsetToPosition(lineStarts, startOffset);
  const end = offsetToPosition(lineStarts, endOffset);
  return new vscode.Range(start.line, start.character, end.line, end.character);
}

function offsetToPosition(
  lineStarts: readonly number[],
  offset: number,
): { line: number; character: number } {
  let low = 0;
  let high = lineStarts.length - 1;

  while (low <= high) {
    const mid = Math.floor((low + high) / 2);
    const lineStart = lineStarts[mid]!;
    const nextLineStart = lineStarts[mid + 1] ?? Number.POSITIVE_INFINITY;

    if (offset < lineStart) {
      high = mid - 1;
    } else if (offset >= nextLineStart) {
      low = mid + 1;
    } else {
      return { line: mid, character: offset - lineStart };
    }
  }

  const lastLine = Math.max(lineStarts.length - 1, 0);
  return { line: lastLine, character: Math.max(offset - (lineStarts[lastLine] ?? 0), 0) };
}

function compareRanges(left: vscode.Range, right: vscode.Range): number {
  if (left.start.line !== right.start.line) {
    return left.start.line - right.start.line;
  }
  return left.start.character - right.start.character;
}
