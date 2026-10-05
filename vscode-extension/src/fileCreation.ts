/** Pure starter content for an explicitly chosen new file. */
import * as path from 'path';

export const enum FileKind {
  Module = 'module',
  Test = 'test',
}

/** A module name is earned from the owning workspace's lib tree. */
export function scaffoldContent(
  kind: FileKind,
  rootPath: string,
  targetPath: string,
): string | null {
  const relative = path.relative(rootPath, targetPath).split(path.sep).join('/');
  if (relative === '..' || relative.startsWith('../') || path.isAbsolute(relative)) return null;

  if (kind === FileKind.Test) {
    return path.extname(relative).toLowerCase() === '.t'
      ? "use strict;\nuse warnings;\nuse lib 'lib';\nuse Test::More;\n\n\n\ndone_testing;\n"
      : null;
  }

  if (path.extname(relative).toLowerCase() !== '.pm' || !relative.startsWith('lib/')) return null;
  const segments = relative.slice(4, -3).split('/');
  if (!segments.every((part) => /^[A-Za-z_]\w*$/.test(part))) return null;
  return `package ${segments.join('::')};\nuse strict;\nuse warnings;\n\n\n\n1;\n`;
}
