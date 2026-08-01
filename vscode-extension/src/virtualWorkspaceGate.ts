/**
 * Virtual-workspace startup gate.
 *
 * The Perl language server is a native binary that reads files through the
 * operating-system file system, and the language client's document selector is
 * restricted to the `file` and `untitled` schemes. A workspace whose folders
 * are all served by a virtual file-system provider (`vscode-vfs:` on
 * vscode.dev / github.dev, `git:`, and similar) therefore has nothing the
 * server can open: starting it would download a binary, spawn a process, and
 * attach it to zero documents while the status bar claimed the extension was
 * running.
 *
 * This module owns the decision only. It does not download, spawn, log, or
 * mutate editor state — `extension.ts` applies the decision, and
 * `workspaceTopology.ts` reports the same boundary in receipts.
 */

/** Minimal shape of the workspace folders this gate reads. */
export interface VirtualWorkspaceGateFolder {
  uri: {
    scheme?: string | undefined;
    fsPath?: string | undefined;
  };
}

export interface VirtualWorkspaceGateInput {
  folders: readonly VirtualWorkspaceGateFolder[];
}

export type VirtualWorkspaceGateDecision =
  | {
      /** At least one file-backed folder exists, or the window has no folders. */
      kind: 'start';
    }
  | {
      /** Every open folder is virtual — the server has no file-backed root. */
      kind: 'defer';
      /** Distinct URI schemes of the open folders, in first-seen order. */
      folderSchemes: string[];
      /** Operator-facing single line for the output channel. */
      logMessage: string;
      /** User-facing reason shown in the status bar tooltip and command errors. */
      userMessage: string;
    };

function folderScheme(folder: VirtualWorkspaceGateFolder): string {
  const scheme = folder.uri.scheme?.trim();
  if (scheme) {
    return scheme;
  }
  return folder.uri.fsPath ? 'file' : 'unknown';
}

/**
 * Decide whether language-server startup may proceed for the current folders.
 *
 * Deferral requires open folders: a window with no folder at all (a single
 * loose `file:` document, or an empty window the user is about to add a folder
 * to) still starts, because those documents are file-backed and the server
 * serves them.
 *
 * A mixed workspace — at least one `file:` folder alongside virtual ones —
 * also starts. The file-backed folders are fully served; the virtual folders
 * are reported as a limitation by `describeWorkspaceTopology` rather than
 * blocking the folders that do work.
 */
export function decideVirtualWorkspaceGate(
  input: VirtualWorkspaceGateInput,
): VirtualWorkspaceGateDecision {
  if (input.folders.length === 0) {
    return { kind: 'start' };
  }

  const schemes: string[] = [];
  for (const folder of input.folders) {
    const scheme = folderScheme(folder);
    if (scheme === 'file') {
      return { kind: 'start' };
    }
    if (!schemes.includes(scheme)) {
      schemes.push(scheme);
    }
  }

  const schemeList = schemes.map((scheme) => `${scheme}:`).join(', ');
  return {
    kind: 'defer',
    folderSchemes: schemes,
    logMessage:
      `[startup] Every workspace folder is virtual (${schemeList}) — the Perl language server ` +
      'needs a file-backed folder. Deferring startup until one is opened.',
    userMessage:
      `Perl Language Server is unavailable in this virtual workspace (${schemeList}). ` +
      'The server is a native binary and reads files from disk, so syntax highlighting and ' +
      'snippets work here but code intelligence, diagnostics, tests, and debugging need a ' +
      'file-backed folder — clone the repository locally or open it through a remote host.',
  };
}
