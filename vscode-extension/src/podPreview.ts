/**
 * POD Preview Panel (issue #2062)
 *
 * Provides a VSCode webview panel that renders POD documentation from the
 * currently active Perl file as HTML. Auto-updates on file save.
 *
 * The POD-to-HTML conversion is implemented in pure TypeScript so the feature
 * has zero runtime dependencies (no Perl executable required).
 */

import * as vscode from 'vscode';

// ---------------------------------------------------------------------------
// POD → HTML conversion
// ---------------------------------------------------------------------------

/**
 * Convert a POD source string (or a Perl source file containing POD sections)
 * to an HTML fragment. Non-POD lines are skipped. Returns a complete HTML
 * document body, not a full <html> skeleton — the caller wraps it.
 */
export function podToHtml(source: string): string {
  const podLines = extractPodLines(source);
  if (podLines.length === 0) {
    return '<p class="no-pod">No POD documentation found in this file.</p>';
  }

  const blocks = parseBlocks(podLines);
  return renderBlocks(blocks);
}

// ---------------------------------------------------------------------------
// Step 1: extract POD lines from mixed Perl+POD source
// ---------------------------------------------------------------------------

/**
 * POD begins at a =command that starts at column 0 and ends at =cut.
 * Multiple POD sections per file are supported.
 */
function extractPodLines(source: string): string[] {
  const result: string[] = [];
  let inPod = false;

  for (const line of source.split('\n')) {
    if (!inPod) {
      // Any =word at start of line begins a POD section
      if (/^=[a-zA-Z]/.test(line)) {
        inPod = true;
        // =cut at the very start before any other pod would be odd,
        // but handle it: don't add the line, stay out of pod
        if (/^=cut\b/.test(line)) {
          inPod = false;
          continue;
        }
        result.push(line);
      }
    } else {
      if (/^=cut\b/.test(line)) {
        inPod = false;
        // Don't include the =cut line itself
      } else {
        result.push(line);
      }
    }
  }

  return result;
}

// ---------------------------------------------------------------------------
// Step 2: parse POD lines into logical blocks
// ---------------------------------------------------------------------------

type Block =
  | { kind: 'command'; cmd: string; text: string }
  | { kind: 'verbatim'; lines: string[] }
  | { kind: 'para'; text: string };

function parseBlocks(lines: string[]): Block[] {
  const blocks: Block[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    if (line === undefined) {
      break;
    }

    // Command paragraph: line starting with =
    if (/^=[a-zA-Z]/.test(line)) {
      const m = line.match(/^=(\S+)\s*(.*)/);
      const cmd = m?.[1] ?? '';
      const text = m?.[2]?.trim() ?? '';
      blocks.push({ kind: 'command', cmd, text });
      i++;
      continue;
    }

    // Verbatim paragraph: indented line
    if (/^\s+\S/.test(line)) {
      const verbLines: string[] = [];
      while (i < lines.length) {
        const current = lines[i];
        if (current === undefined || (!/^\s/.test(current) && current !== '')) {
          break;
        }

        // Stop collecting if we hit a blank line followed by non-indented
        if (current === '') {
          // Peek: is the next non-empty line indented?
          let j = i + 1;
          while (j < lines.length && lines[j] === '') {
            j++;
          }
          const next = lines[j];
          if (next === undefined || !/^\s/.test(next)) {
            break;
          }
        }
        verbLines.push(current);
        i++;
      }
      // Skip trailing blank lines inside the verbatim block
      while (verbLines.length > 0 && verbLines[verbLines.length - 1] === '') {
        verbLines.pop();
      }
      if (verbLines.length > 0) {
        blocks.push({ kind: 'verbatim', lines: verbLines });
      }
      continue;
    }

    // Blank line: paragraph separator — skip
    if (line.trim() === '') {
      i++;
      continue;
    }

    // Ordinary paragraph: collect until blank line or command
    const paraLines: string[] = [];
    while (i < lines.length) {
      const current = lines[i];
      if (
        current === undefined ||
        current.trim() === '' ||
        /^=[a-zA-Z]/.test(current) ||
        /^\s+\S/.test(current)
      ) {
        break;
      }

      paraLines.push(current);
      i++;
    }
    if (paraLines.length > 0) {
      blocks.push({ kind: 'para', text: paraLines.join(' ') });
    }
  }

  return blocks;
}

// ---------------------------------------------------------------------------
// Step 3: render blocks to HTML
// ---------------------------------------------------------------------------

/** Emitted for `=over` and rewritten once the first `=item` fixes the list type. */
const LIST_PLACEHOLDER = '<!-- over -->';

/** An `=item` marker that opens an ordered list: `1.`, `2)`, or a bare `1`. */
const ORDERED_ITEM_MARKER = /^\d+(?:[.)](?:\s|$)|$)/;

/** An `=item` marker that opens a bullet list: `*` or `-`. */
const UNORDERED_ITEM_MARKER = /^[*-](?:\s|$)/;

/** One open `=over` list level. */
interface ListFrame {
  tag: 'ul' | 'ol';
  /** Index in `parts` holding this list's opening tag or its placeholder. */
  openIdx: number;
  /** True once the opening tag reflects a real `=item` marker. */
  decided: boolean;
  /** True while an `<li>` is open and can still absorb the item's body. */
  itemOpen: boolean;
  /** Index in `parts` of the currently open `<li>`, or -1 when none. */
  itemIdx: number;
  /** True once this list has emitted at least one `=item`. */
  hasItems: boolean;
}

function renderBlocks(blocks: Block[]): string {
  const parts: string[] = [];
  const listStack: ListFrame[] = [];

  /**
   * Close the open `<li>`.
   *
   * An item with no body keeps its original single-element `<li>…</li>` shape;
   * an item that absorbed paragraphs or verbatim blocks gets its own closing
   * tag after them, so the body stays inside the item rather than terminating
   * the list.
   */
  function closeItem(frame: ListFrame): void {
    if (!frame.itemOpen) {
      return;
    }
    const openTag = parts[frame.itemIdx];
    if (frame.itemIdx === parts.length - 1 && openTag !== undefined) {
      parts[frame.itemIdx] = `${openTag}</li>`;
    } else {
      parts.push('</li>');
    }
    frame.itemOpen = false;
    frame.itemIdx = -1;
  }

  function closeList(): void {
    const frame = listStack.pop();
    if (!frame) {
      return;
    }
    closeItem(frame);
    if (!frame.hasItems) {
      // `=over` with no `=item` contributes no list; the placeholder is
      // dropped below rather than emitted as an empty `<ul></ul>`.
      return;
    }
    if (!frame.decided) {
      parts[frame.openIdx] = `<${frame.tag}>`;
      frame.decided = true;
    }
    parts.push(`</${frame.tag}>`);
  }

  function closeAllLists(): void {
    while (listStack.length > 0) {
      closeList();
    }
  }

  /** The innermost list item that a body block belongs to, when one is open. */
  function openItemFrame(): ListFrame | undefined {
    const frame = listStack[listStack.length - 1];
    return frame?.itemOpen === true ? frame : undefined;
  }

  /**
   * Render an item body (paragraph or verbatim block).
   *
   * POD item text and its explanatory paragraphs are one list item. Closing the
   * list here would split every `=item` into its own single-entry list.
   */
  function pushBodyBlock(html: string): void {
    if (!openItemFrame()) {
      closeAllLists();
    }
    parts.push(html);
  }

  for (const block of blocks) {
    if (block.kind === 'command') {
      const { cmd, text } = block;

      // Heading commands
      if (/^head[1-4]$/.test(cmd)) {
        closeAllLists();
        const level = cmd[4];
        parts.push(`<h${level}>${renderInline(text)}</h${level}>`);
        continue;
      }

      // =pod: no-op (marks start of pod, already extracted)
      if (cmd === 'pod') {
        continue;
      }

      // =over: begin a list
      if (cmd === 'over') {
        // defer list type until we see the first =item
        listStack.push({
          tag: 'ul',
          openIdx: parts.length,
          decided: false,
          itemOpen: false,
          itemIdx: -1,
          hasItems: false,
        });
        parts.push(LIST_PLACEHOLDER);
        continue;
      }

      // =item: list item
      if (cmd === 'item') {
        // Determine / correct list type from item marker
        const isOrdered = ORDERED_ITEM_MARKER.test(text);
        const isUnordered = UNORDERED_ITEM_MARKER.test(text);

        // If we are not inside any list context, open one
        let frame = listStack[listStack.length - 1];
        if (!frame) {
          frame = {
            tag: isOrdered ? 'ol' : 'ul',
            openIdx: parts.length,
            decided: true,
            itemOpen: false,
            itemIdx: -1,
            hasItems: false,
          };
          listStack.push(frame);
          parts.push(`<${frame.tag}>`);
        } else if (!frame.decided) {
          // Fix the placeholder now that the first item names the list type
          frame.tag = isOrdered ? 'ol' : 'ul';
          parts[frame.openIdx] = `<${frame.tag}>`;
          frame.decided = true;
        }

        closeItem(frame);

        // Strip leading marker
        let itemText = text;
        if (isOrdered) {
          itemText = text.replace(/^\d+[.)]?\s*/, '').trim();
        } else if (isUnordered) {
          itemText = text.replace(/^[*-]\s*/, '').trim();
        }

        frame.itemIdx = parts.length;
        frame.itemOpen = true;
        frame.hasItems = true;
        parts.push(`<li>${renderInline(itemText)}`);
        continue;
      }

      // =back: end a list
      if (cmd === 'back') {
        closeList();
        continue;
      }

      // =begin / =end / =for / =encoding: skip
      if (['begin', 'end', 'for', 'encoding'].includes(cmd)) {
        continue;
      }

      // Unknown command: treat as paragraph
      if (text) {
        pushBodyBlock(`<p>${renderInline(text)}</p>`);
      }
      continue;
    }

    if (block.kind === 'verbatim') {
      // Compute common indentation to strip
      const stripped = stripCommonIndent(block.lines);
      pushBodyBlock(`<pre><code>${escapeHtml(stripped.join('\n'))}</code></pre>`);
      continue;
    }

    if (block.kind === 'para') {
      pushBodyBlock(`<p>${renderInline(block.text)}</p>`);
      continue;
    }
  }

  closeAllLists();

  // Remove any remaining placeholder comments (e.g. =over with no =item)
  return parts.filter((p) => p !== LIST_PLACEHOLDER).join('\n');
}

// ---------------------------------------------------------------------------
// Inline formatting: B<>, I<>, C<>, L<>, E<>, S<>, X<>
// ---------------------------------------------------------------------------

/** Letters that introduce a POD formatting code. */
const INLINE_CODE_LETTERS = 'BICLESFZX';

/** One located formatting code: `C<…>`, `C<< … >>`, `C<<< … >>>`, … */
interface InlineSequence {
  /** The code letter, e.g. `B`. */
  code: string;
  /** Content between the delimiters, with the required padding removed. */
  inner: string;
  /** Index just past the closing delimiter. */
  end: number;
}

/**
 * Find the `>` that closes a single-angle code, skipping nested codes.
 *
 * `B<C<fetch>>` closes at the second `>`, not the first: the first belongs to
 * the nested `C<…>`. Stopping at the first `>` splits the sequence and leaves
 * the remainder as prose.
 */
function findSingleAngleEnd(text: string, from: number): number {
  let depth = 1;
  for (let i = from; i < text.length; i += 1) {
    const ch = text[i] ?? '';
    if (INLINE_CODE_LETTERS.includes(ch) && text[i + 1] === '<') {
      depth += 1;
      i += 1;
      continue;
    }
    if (ch === '>') {
      depth -= 1;
      if (depth === 0) {
        return i;
      }
    }
  }
  return -1;
}

/**
 * Match a formatting code starting at `at`, or return undefined.
 *
 * perlpodspec allows any number of angle brackets so long as the counts match,
 * and requires whitespace just inside both delimiters once there are two or
 * more. That whitespace is padding, not content, so it is stripped here.
 */
function matchInlineSequence(text: string, at: number): InlineSequence | undefined {
  const code = text[at] ?? '';
  if (!INLINE_CODE_LETTERS.includes(code) || text[at + 1] !== '<') {
    return undefined;
  }

  let opening = 0;
  while (text[at + 1 + opening] === '<') {
    opening += 1;
  }
  const contentStart = at + 1 + opening;

  if (opening === 1) {
    const close = findSingleAngleEnd(text, contentStart);
    return close === -1
      ? undefined
      : { code, inner: text.slice(contentStart, close), end: close + 1 };
  }

  // Two or more angles: padding whitespace is mandatory on both sides.
  if (!/\s/.test(text[contentStart] ?? '')) {
    return undefined;
  }
  const closer = '>'.repeat(opening);
  for (let i = contentStart + 1; i + opening <= text.length; i += 1) {
    if (text.startsWith(closer, i) && /\s/.test(text[i - 1] ?? '')) {
      return { code, inner: text.slice(contentStart, i).trim(), end: i + opening };
    }
  }
  return undefined;
}

/**
 * Render POD inline formatting codes and escape everything else.
 *
 * Literal text between sequences is escaped as it is scanned. Escaping cannot
 * run over the finished output instead: the rendered codes are real HTML by
 * then, so any pattern that spares them also spares prose that happens to look
 * like a tag — `Use <angle> markers` would reach the webview as markup and its
 * text would silently vanish from the preview.
 */
function renderInline(text: string): string {
  let out = '';
  let i = 0;

  while (i < text.length) {
    const sequence = matchInlineSequence(text, i);
    if (sequence === undefined) {
      out += escapeHtml(text[i] ?? '');
      i += 1;
      continue;
    }
    out += renderInlineCode(sequence.code, sequence.inner);
    i = sequence.end;
  }

  return out;
}

function renderInlineCode(code: string, inner: string): string {
  switch (code) {
    // B, I and S may contain further formatting codes, so their content is
    // rendered rather than escaped. C and F are literal text by definition.
    case 'B':
      return `<strong>${renderInline(inner)}</strong>`;
    case 'I':
      return `<em>${renderInline(inner)}</em>`;
    case 'C':
      return `<code>${escapeHtml(inner)}</code>`;
    case 'F':
      return `<code class="filename">${escapeHtml(inner)}</code>`;
    case 'L':
      return renderLinkCode(inner);
    case 'E':
      return renderEscapeCode(inner);
    case 'S':
      return `<span class="no-break">${renderInline(inner)}</span>`;
    case 'Z':
      return ''; // zero-width
    case 'X':
      return ''; // index entry, invisible
    default:
      return escapeHtml(inner);
  }
}

function renderLinkCode(inner: string): string {
  // L<text|url> or L<name> or L<name/section>
  const pipeIdx = inner.indexOf('|');
  if (pipeIdx !== -1) {
    const label = inner.slice(0, pipeIdx).trim();
    const target = inner.slice(pipeIdx + 1).trim();
    if (/^https?:\/\//.test(target)) {
      return `<a href="${escapeAttr(target)}">${renderInline(label)}</a>`;
    }
    return `<a href="#${escapeAttr(target.replace(/\s+/g, '-').toLowerCase())}">${renderInline(label)}</a>`;
  }

  if (/^https?:\/\//.test(inner)) {
    return `<a href="${escapeAttr(inner)}">${escapeHtml(inner)}</a>`;
  }

  // Module/manpage reference
  const slashIdx = inner.indexOf('/');
  if (slashIdx !== -1) {
    const mod = inner.slice(0, slashIdx);
    const section = inner.slice(slashIdx + 1);
    return `<a href="https://perldoc.perl.org/${escapeAttr(mod)}">${escapeHtml(mod)}/${escapeHtml(section)}</a>`;
  }

  return `<a href="https://perldoc.perl.org/${escapeAttr(inner)}">${escapeHtml(inner)}</a>`;
}

function renderEscapeCode(name: string): string {
  const namedEntities: Record<string, string> = {
    lt: '&lt;',
    gt: '&gt;',
    sol: '/',
    verbar: '|',
    amp: '&amp;',
    apos: "'",
    quot: '&quot;',
  };
  if (name in namedEntities) {
    const entity = namedEntities[name];
    if (entity !== undefined) {
      return entity;
    }
  }
  if (/^\d+$/.test(name)) {
    return `&#${name};`;
  }
  if (/^0x[0-9a-fA-F]+$/i.test(name)) {
    return `&#x${name.slice(2)};`;
  }
  // Unicode name (U+xxxx) — best effort
  const unicodeMatch = name.match(/^U\+([0-9A-Fa-f]+)$/);
  const codePoint = unicodeMatch?.[1];
  if (codePoint) {
    return `&#x${codePoint};`;
  }
  return `&amp;${escapeHtml(name)};`;
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function escapeAttr(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

function stripCommonIndent(lines: string[]): string[] {
  const nonEmptyLines = lines.filter((l) => l.trim() !== '');
  if (nonEmptyLines.length === 0) {
    return lines;
  }

  const minIndent = nonEmptyLines.reduce((min, l) => {
    const m = l.match(/^(\s*)/);
    const indent = m?.[1]?.length ?? 0;
    return Math.min(min, indent);
  }, Infinity);

  if (minIndent === 0 || !isFinite(minIndent)) {
    return lines;
  }
  return lines.map((l) => (l.trim() === '' ? '' : l.slice(minIndent)));
}

// ---------------------------------------------------------------------------
// Webview panel
// ---------------------------------------------------------------------------

let podPreviewPanel: vscode.WebviewPanel | undefined;

/**
 * Open (or reveal) the POD preview panel for the given document.
 */
export function showPodPreview(
  context: vscode.ExtensionContext,
  document: vscode.TextDocument,
): void {
  const column = vscode.ViewColumn.Beside;

  if (podPreviewPanel) {
    podPreviewPanel.reveal(column);
  } else {
    podPreviewPanel = vscode.window.createWebviewPanel('perlPodPreview', 'POD Preview', column, {
      enableScripts: false,
      retainContextWhenHidden: true,
      // Defense in depth: POD preview renders workspace text, so restrict
      // resource access to prevent any content injection from loading
      // external resources (#6047).
      localResourceRoots: [],
    });

    podPreviewPanel.onDidDispose(
      () => {
        podPreviewPanel = undefined;
      },
      null,
      context.subscriptions,
    );
  }

  updatePodPreviewContent(document);
}

function updatePodPreviewContent(document: vscode.TextDocument): void {
  if (!podPreviewPanel) {
    return;
  }

  const source = document.getText();
  const bodyHtml = podToHtml(source);
  const fileName = document.fileName.split(/[\\/]/).pop() ?? document.fileName;

  podPreviewPanel.title = `POD: ${fileName}`;
  podPreviewPanel.webview.html = buildWebviewHtml(fileName, bodyHtml);
}

function buildWebviewHtml(title: string, bodyHtml: string): string {
  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline';">
  <title>${escapeHtml(title)}</title>
  <style>
    body {
      font-family: var(--vscode-editor-font-family, 'Segoe UI', Tahoma, sans-serif);
      font-size: var(--vscode-editor-font-size, 14px);
      line-height: 1.6;
      color: var(--vscode-editor-foreground, #ccc);
      background: var(--vscode-editor-background, #1e1e1e);
      padding: 1.5em 2em;
      max-width: 860px;
    }
    h1, h2, h3, h4 {
      color: var(--vscode-textLink-foreground, #4ec9b0);
      border-bottom: 1px solid var(--vscode-panel-border, #333);
      padding-bottom: 0.2em;
      margin-top: 1.5em;
    }
    h1 { font-size: 1.8em; }
    h2 { font-size: 1.4em; }
    h3 { font-size: 1.2em; }
    h4 { font-size: 1.05em; }
    pre {
      background: var(--vscode-textCodeBlock-background, #252526);
      border: 1px solid var(--vscode-panel-border, #333);
      border-radius: 4px;
      padding: 0.8em 1em;
      overflow-x: auto;
    }
    code {
      font-family: var(--vscode-editor-font-family, 'Courier New', monospace);
      font-size: 0.92em;
      background: var(--vscode-textCodeBlock-background, #252526);
      padding: 0.1em 0.3em;
      border-radius: 2px;
    }
    pre code {
      background: transparent;
      padding: 0;
    }
    a {
      color: var(--vscode-textLink-foreground, #4ec9b0);
      text-decoration: none;
    }
    a:hover { text-decoration: underline; }
    ul, ol { padding-left: 1.5em; }
    li { margin: 0.3em 0; }
    p { margin: 0.6em 0; }
    .no-pod {
      color: var(--vscode-disabledForeground, #888);
      font-style: italic;
    }
    .no-break { white-space: nowrap; }
  </style>
</head>
<body>
${bodyHtml}
</body>
</html>`;
}

/**
 * Register the preview command and the on-save auto-update watcher.
 * Call this from extension.ts activate().
 */
export function registerPodPreview(context: vscode.ExtensionContext): vscode.Disposable[] {
  const previewCommand = vscode.commands.registerCommand('perl-lsp.previewPod', () => {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== 'perl') {
      vscode.window.showErrorMessage('No active Perl file to preview POD documentation');
      return;
    }
    showPodPreview(context, editor.document);
  });

  const saveWatcher = vscode.workspace.onDidSaveTextDocument((document) => {
    if (document.languageId !== 'perl') {
      return;
    }
    if (!podPreviewPanel) {
      return;
    }
    updatePodPreviewContent(document);
  });

  return [previewCommand, saveWatcher];
}
