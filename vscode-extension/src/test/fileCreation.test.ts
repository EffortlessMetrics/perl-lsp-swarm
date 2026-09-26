import { FileKind, scaffoldContent } from '../fileCreation';

test('module declaration follows the owning lib tree', () => {
  const content = scaffoldContent(FileKind.Module, '/project', '/project/lib/Foo/Bar.pm');
  expect(content).toContain('package Foo::Bar;');
  expect(content).toContain('use strict;');
  expect(content?.trimEnd()).toMatch(/1;$/);
});

test('a test scaffold has no package declaration', () => {
  const content = scaffoldContent(FileKind.Test, '/project', '/project/t/example.t');
  expect(content).toContain('use Test::More;');
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
