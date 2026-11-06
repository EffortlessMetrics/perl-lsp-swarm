import * as path from 'path';
import { FileKind, scaffoldContent } from '../fileCreation';

test('module declaration follows the owning lib tree', () => {
  const content = scaffoldContent(FileKind.Module, '/project', '/project/lib/Foo/Bar.pm');
  expect(content).toContain('package Foo::Bar;');
  expect(content).toContain('use strict;');
  expect(content?.trimEnd()).toMatch(/1;$/);
  expect(content).not.toContain("use lib 'lib';");
});

test('a test scaffold has no package declaration', () => {
  const content = scaffoldContent(FileKind.Test, '/project', '/project/t/example.t');
  expect(content).toBe(
    "use strict;\nuse warnings;\nuse lib 'lib';\nuse Test::More;\n\n\n\ndone_testing;\n",
  );
  expect(content).not.toContain('package ');
  expect(content?.trimEnd()).toMatch(/done_testing;$/);
});

test.each([
  ['/project/other/Foo.pm', FileKind.Module],
  ['/project/lib/Foo-Bar.pm', FileKind.Module],
  ['/project/lib/Foo.pl', FileKind.Module],
  ['/other/lib/Foo.pm', FileKind.Module],
  ['/project/lib/Foo.pm', FileKind.Test],
] as const)('rejects ambiguous or ineligible target %s', (target, kind) => {
  expect(scaffoldContent(kind, '/project', target)).toBeNull();
});

test('a backslash in a POSIX module filename is never read as a separator', () => {
  if (path.sep !== '/') return; // On Windows a backslash is a real separator.
  expect(scaffoldContent(FileKind.Module, '/project', '/project/lib/Foo\\Bar.pm')).toBeNull();
});
