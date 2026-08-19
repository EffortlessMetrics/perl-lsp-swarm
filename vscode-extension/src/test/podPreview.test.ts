/**
 * Contract tests for the POD preview panel feature (issue #2062).
 *
 * These are static contract tests — they verify that package.json declares
 * the command with correct metadata and that the POD-to-HTML conversion
 * logic handles common POD constructs correctly.
 * No live VSCode extension host is needed.
 */

import * as fs from 'fs';
import * as path from 'path';
import { podToHtml } from '../podPreview';

const EXT_ROOT = path.resolve(__dirname, '..', '..');

type CommandContribution = {
  command: string;
  category?: string;
  title?: string;
};

type PaletteEntry = {
  command: string;
  when?: string;
};

type PackageManifest = {
  contributes: {
    commands: CommandContribution[];
    menus?: {
      commandPalette?: PaletteEntry[];
    };
  };
};

function readPackageJson(): PackageManifest {
  return JSON.parse(
    fs.readFileSync(path.join(EXT_ROOT, 'package.json'), 'utf8'),
  ) as PackageManifest;
}

// ---------------------------------------------------------------------------
// package.json contract: command registration
// ---------------------------------------------------------------------------
describe('perl-lsp.previewPod command (issue #2062)', () => {
  let pkg: PackageManifest;
  let commandIds: string[];
  let paletteEntries: PaletteEntry[];

  beforeAll(() => {
    pkg = readPackageJson();
    commandIds = pkg.contributes.commands.map((c: CommandContribution) => c.command);
    paletteEntries = pkg.contributes.menus?.commandPalette ?? [];
  });

  test('perl-lsp.previewPod is registered in contributes.commands', () => {
    expect(commandIds).toContain('perl-lsp.previewPod');
  });

  test('perl-lsp.previewPod has category Perl', () => {
    const cmd = pkg.contributes.commands.find(
      (c: CommandContribution) => c.command === 'perl-lsp.previewPod',
    );
    expect(cmd?.category).toBe('Perl');
  });

  test('perl-lsp.previewPod has a non-empty title', () => {
    const cmd = pkg.contributes.commands.find(
      (c: CommandContribution) => c.command === 'perl-lsp.previewPod',
    );
    expect(cmd?.title).toBeTruthy();
  });

  test('perl-lsp.previewPod appears in command palette', () => {
    const entry = paletteEntries.find((e: PaletteEntry) => e.command === 'perl-lsp.previewPod');
    expect(entry).toBeDefined();
  });

  test('perl-lsp.previewPod is guarded by editorLangId == perl', () => {
    const entry = paletteEntries.find((e: PaletteEntry) => e.command === 'perl-lsp.previewPod');
    expect(entry?.when).toContain('editorLangId == perl');
  });
});

// ---------------------------------------------------------------------------
// POD-to-HTML conversion logic
// ---------------------------------------------------------------------------
describe('podToHtml', () => {
  test('renders =head1 as <h1>', () => {
    const html = podToHtml('=head1 NAME\n');
    expect(html).toContain('<h1>NAME</h1>');
  });

  test('renders =head2 as <h2>', () => {
    const html = podToHtml('=head2 SYNOPSIS\n');
    expect(html).toContain('<h2>SYNOPSIS</h2>');
  });

  test('renders =head3 as <h3>', () => {
    const html = podToHtml('=head3 Details\n');
    expect(html).toContain('<h3>Details</h3>');
  });

  test('renders =head4 as <h4>', () => {
    const html = podToHtml('=head4 Notes\n');
    expect(html).toContain('<h4>Notes</h4>');
  });

  test('renders =over/=item/=back as a list', () => {
    const pod = '=over 4\n\n=item * First\n\n=item * Second\n\n=back\n';
    const html = podToHtml(pod);
    expect(html).toContain('<ul>');
    expect(html).toContain('<li>First</li>');
    expect(html).toContain('<li>Second</li>');
    expect(html).toContain('</ul>');
  });

  test('renders =over/=item with numbers as <ol>', () => {
    const pod = '=over 4\n\n=item 1. First\n\n=item 2. Second\n\n=back\n';
    const html = podToHtml(pod);
    expect(html).toContain('<ol>');
    expect(html).toContain('</ol>');
  });

  test('renders verbatim block (indented) as <pre><code>', () => {
    const pod = '=pod\n\n    my $x = 1;\n    print $x;\n\n=cut\n';
    const html = podToHtml(pod);
    expect(html).toContain('<pre><code>');
    expect(html).toContain('my $x = 1;');
  });

  test('renders plain paragraph as <p>', () => {
    const pod = '=pod\n\nThis is a plain paragraph.\n\n=cut\n';
    const html = podToHtml(pod);
    expect(html).toContain('<p>This is a plain paragraph.</p>');
  });

  test('renders B<bold> as <strong>', () => {
    const pod = '=pod\n\nThis is B<bold> text.\n\n=cut\n';
    const html = podToHtml(pod);
    expect(html).toContain('<strong>bold</strong>');
  });

  test('renders I<italic> as <em>', () => {
    const pod = '=pod\n\nThis is I<italic> text.\n\n=cut\n';
    const html = podToHtml(pod);
    expect(html).toContain('<em>italic</em>');
  });

  test('renders C<code> as <code>', () => {
    const pod = '=pod\n\nUse C<my $x> for a scalar.\n\n=cut\n';
    const html = podToHtml(pod);
    expect(html).toContain('<code>my $x</code>');
  });

  test('renders L<link> as anchor', () => {
    const pod = '=pod\n\nSee L<perldoc>.\n\n=cut\n';
    const html = podToHtml(pod);
    expect(html).toContain('<a');
    expect(html).toContain('perldoc');
  });

  test('renders E<lt> as &lt;', () => {
    const pod = '=pod\n\nUse E<lt>tag E<gt> syntax.\n\n=cut\n';
    const html = podToHtml(pod);
    expect(html).toContain('&lt;');
    expect(html).toContain('&gt;');
  });

  test('skips non-POD code before =pod marker', () => {
    const source = 'sub foo { return 1; }\n\n=head1 NAME\n\nMyModule\n\n=cut\n\nsub bar { }\n';
    const html = podToHtml(source);
    expect(html).toContain('<h1>NAME</h1>');
    expect(html).not.toContain('sub foo');
  });

  test('returns empty content message when no POD found', () => {
    const html = podToHtml('# just a comment\nmy $x = 1;\n');
    expect(html).toContain('No POD documentation found');
  });

  test('handles =pod/=cut markers', () => {
    const pod = '=pod\n\nThis is documentation.\n\n=cut\n';
    const html = podToHtml(pod);
    expect(html).toContain('<p>This is documentation.</p>');
  });

  test('escapes HTML special characters in plain text', () => {
    const pod = '=pod\n\nUse 3 < 5 & check > 0.\n\n=cut\n';
    const html = podToHtml(pod);
    expect(html).toContain('&lt;');
    expect(html).toContain('&amp;');
    expect(html).toContain('&gt;');
  });

  test('escapes prose that looks like an HTML tag instead of emitting markup', () => {
    const pod = '=pod\n\nUse <angle> markers around a name.\n\n=cut\n';
    const html = podToHtml(pod);
    // A raw `<angle>` reaches the webview as an unknown element, and the text
    // between the angle brackets disappears from the rendered preview.
    expect(html).not.toContain('<angle>');
    expect(html).toContain('Use &lt;angle&gt; markers around a name.');
  });

  test('escapes a closing-tag-shaped run in prose', () => {
    const pod = '=pod\n\nClose it with </sub> when done.\n\n=cut\n';
    const html = podToHtml(pod);
    expect(html).not.toContain('</sub>');
    expect(html).toContain('&lt;/sub&gt;');
  });

  test('renders inline formatting codes inside headings', () => {
    const html = podToHtml('=head1 The C<fetch> helper\n');
    expect(html).toContain('<h1>The <code>fetch</code> helper</h1>');
  });

  test('keeps an item body inside its own list item', () => {
    const pod =
      '=over 4\n\n=item * First\n\nExplains the first item.\n\n' +
      '=item * Second\n\nExplains the second item.\n\n=back\n';
    const html = podToHtml(pod);

    // One list, not one list per item: the explanatory paragraphs must not
    // terminate the list and restart it for the next =item.
    expect(html.match(/<ul>/g)).toHaveLength(1);
    expect(html.match(/<\/ul>/g)).toHaveLength(1);
    expect(html.indexOf('Explains the first item.')).toBeGreaterThan(html.indexOf('<ul>'));
    expect(html.indexOf('Explains the second item.')).toBeLessThan(html.indexOf('</ul>'));
    expect(html).toContain('<li>First\n<p>Explains the first item.</p>\n</li>');
  });

  test('keeps a verbatim item body inside its list item', () => {
    const pod = '=over 4\n\n=item * Example\n\n    my $x = 1;\n\n=back\n';
    const html = podToHtml(pod);
    expect(html.match(/<ul>/g)).toHaveLength(1);
    expect(html).toContain('<li>Example\n<pre><code>my $x = 1;</code></pre>\n</li>');
  });

  test('treats a dotted numeric =item marker as an ordered list and strips the marker', () => {
    const pod = '=over 4\n\n=item 1.\n\nFirst step.\n\n=item 2.\n\nSecond step.\n\n=back\n';
    const html = podToHtml(pod);
    expect(html.match(/<ol>/g)).toHaveLength(1);
    expect(html.match(/<\/ol>/g)).toHaveLength(1);
    // The marker is a list marker, not item text.
    expect(html).not.toContain('<li>1.');
    expect(html).not.toContain('<li>2.');
  });

  test('treats a bare numeric =item marker as an ordered list', () => {
    const pod = '=over 4\n\n=item 1\n\nFirst step.\n\n=item 2\n\nSecond step.\n\n=back\n';
    const html = podToHtml(pod);
    expect(html.match(/<ol>/g)).toHaveLength(1);
    expect(html).not.toContain('<ul>');
    expect(html).not.toContain('<li>1');
  });

  test('does not treat numeric prose as an ordered =item marker', () => {
    const pod = '=over 4\n\n=item 1996 was a year\n\n=back\n';
    const html = podToHtml(pod);
    expect(html).toContain('<ul>');
    expect(html).toContain('1996 was a year');
  });

  test('renders a formatting code nested inside another', () => {
    // `B<C<fetch>>` closes at the second `>`. Stopping at the first splits the
    // sequence and leaves the remainder as stray prose.
    const html = podToHtml('=pod\n\nCall B<C<fetch>> first.\n\n=cut\n');
    expect(html).toContain('<strong><code>fetch</code></strong>');
    expect(html).not.toContain('&gt;');
  });

  test('renders repeated-angle delimiters and strips their padding', () => {
    const html = podToHtml('=pod\n\nUse C<<< $x >>> here.\n\n=cut\n');
    expect(html).toContain('<code>$x</code>');
    expect(html).not.toContain('&gt;');
    expect(html).not.toContain('&lt;');
  });

  test('keeps a literal > inside a repeated-angle code', () => {
    // The whole point of the `<< >>` form: content may contain a bare `>`.
    const html = podToHtml('=pod\n\nCompare C<< $a > $b >> now.\n\n=cut\n');
    expect(html).toContain('<code>$a &gt; $b</code>');
  });

  test('renders formatting codes inside a link label', () => {
    const html = podToHtml('=pod\n\nSee L<B<the docs>|https://example.com/x>.\n\n=cut\n');
    expect(html).toContain('<a href="https://example.com/x"><strong>the docs</strong></a>');
  });

  test('leaves an unterminated formatting code as escaped text', () => {
    const html = podToHtml('=pod\n\nA stray B<open code.\n\n=cut\n');
    expect(html).toContain('B&lt;open code.');
    expect(html).not.toContain('<strong>');
  });

  test('emits no empty list for an =over block with no =item', () => {
    const html = podToHtml('=over 4\n\n=back\n\n=head1 NAME\n');
    expect(html).not.toContain('<ul>');
    expect(html).not.toContain('<ol>');
    expect(html).toContain('<h1>NAME</h1>');
  });
});
