/**
 * `Perl: Show @INC Paths` must not present PATH-perl's `@INC` as the roots
 * `perl-lsp` searches (#16581).
 *
 * The command spawns bare `perl -e 'print join("\n", @INC)'`, so what it prints
 * is the raw `@INC` of whatever `perl` is first on `PATH`. That is not the
 * server's effective include-path set: it omits workspace `includePaths` and the
 * PERL5LIB policy, and may be a different interpreter than the one the server
 * resolved. The reader most harmed by an unqualified list is someone asking
 * "why can't the LSP find my module?", for whom the list omits exactly the
 * roots in dispute and carries no caveat.
 */

import { showIncPathsCommand } from '../documentCommands';
import * as vscode from 'vscode';

type ExecFileLike = Parameters<typeof showIncPathsCommand>[0];

interface CapturedChannel {
  appendLine: jest.Mock;
  clear: jest.Mock;
  show: jest.Mock;
}

let measuredFrom = 0;

function channelFactory(): jest.Mock {
  return vscode.window.createOutputChannel as unknown as jest.Mock;
}

function firstChannel(create: jest.Mock): CapturedChannel {
  const fromResult = create.mock.results[0]?.value as CapturedChannel | undefined;
  const fromCall = create.mock.calls[0]?.[0] as CapturedChannel | undefined;
  const channel = fromResult ?? fromCall;
  expect(channel).toBeDefined();
  return channel as CapturedChannel;
}

async function invokeAndCapture(stdout: string): Promise<string> {
  const create = channelFactory();
  // Ensure the channel exists before measuring where this call starts.
  await showIncPathsCommand(fakeExec(stdout));
  const all = firstChannel(create).appendLine.mock.calls.map((call) => String(call[0]));
  return all.slice(measuredFrom).join('\n');
}

function fakeExec(stdout: string): ExecFileLike {
  return ((
    _file: string,
    _args: string[],
    _options: unknown,
    callback: (error: Error | null, stdout: string, stderr: string) => void,
  ) => {
    callback(null, stdout, '');
  }) as unknown as ExecFileLike;
}

describe('showIncPathsCommand output scope', () => {
  it('states that the listing is the PATH perl before printing any path', async () => {
    const create = channelFactory();
    if (create.mock.results.length === 0) {
      // First invocation in this module creates the channel.
      await showIncPathsCommand(fakeExec('seed\n'));
    }
    measuredFrom = firstChannel(create).appendLine.mock.calls.length;

    const output = await invokeAndCapture('/usr/lib/perl5\n/site/lib\n');

    // The caveat has to be visible without scrolling: assert the first
    // non-empty line already scopes the list.
    const firstLine = output.split('\n').find((line) => line.trim().length > 0);
    expect(firstLine).toBeDefined();
    expect(firstLine).toContain('PATH');
    expect(firstLine).toMatch(/perl/i);
  });

  it('names what the listing is NOT, so the reader is not misled about the roots in dispute', async () => {
    const output = await invokeAndCapture('/usr/lib/perl5\n/site/lib\n');

    expect(output).toContain('includePaths');
    expect(output).toContain('PERL5LIB');
    // A negative claim in words, not only an omission: "this is not the set of
    // roots the server searches" is the whole point of the change.
    expect(output).toMatch(/NOT the set of roots/i);
  });

  it('points at the command that does answer the server-effective question', async () => {
    const output = await invokeAndCapture('/usr/lib/perl5\n/site/lib\n');

    // Quoted exactly as `package.nls.json` registers the palette entry
    // (`command.showWorkspaceTrustReport.title`), so the pointer resolves by name
    // rather than approximately.
    expect(output).toContain('Show Workspace Trust Report');
  });

  it('still lists the @INC entries themselves', async () => {
    const output = await invokeAndCapture('/usr/lib/perl5\n/site/lib\n');

    expect(output).toContain('/usr/lib/perl5');
    expect(output).toContain('/site/lib');
  });
});
