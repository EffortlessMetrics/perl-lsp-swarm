import * as path from 'path';
import { classifyLaunchPath } from '../workspaceTrustRuntimeState';

/**
 * Direct pins for the launch-configuration path classification (#17336).
 *
 * `classifyLaunchPath` feeds the workspace-trust runtime guidance's view of
 * `launch.json` shapes (`perlPath`, `program`, `cwd`, `includePaths`); the
 * module previously had zero test references, so reclassifications could ship
 * unnoticed.
 */

describe('classifyLaunchPath', () => {
  test.each(['', '   ', '\t'])('classifies %j as empty', (value) => {
    expect(classifyLaunchPath(value)).toBe('empty');
  });

  test('classifies workspace variables before any path shape', () => {
    expect(classifyLaunchPath('${workspaceFolder}/bin/perl')).toBe('workspace_variable');
    expect(classifyLaunchPath('${workspaceFolder}')).toBe('workspace_variable');
    expect(classifyLaunchPath('${workspaceFolderW}\\perl.exe')).toBe('workspace_variable');
  });

  test('classifies file variables', () => {
    expect(classifyLaunchPath('${file}')).toBe('file_variable');
    expect(classifyLaunchPath('${fileDirname}/../script.pl')).toBe('file_variable');
  });

  test('classifies other variable expansions', () => {
    expect(classifyLaunchPath('${env:PERL5LIB}/bin')).toBe('other_variable');
    expect(classifyLaunchPath('${userHome}/perl/bin')).toBe('other_variable');
    expect(classifyLaunchPath('${relativePath}')).toBe('other_variable');
  });

  test('classifies home-relative paths', () => {
    expect(classifyLaunchPath('~/perl/bin')).toBe('home_relative');
    expect(classifyLaunchPath('~')).toBe('home_relative');
  });

  test('classifies absolute paths per the host platform', () => {
    expect(classifyLaunchPath(path.resolve('somewhere', 'perl'))).toBe('absolute');
  });

  test('classifies bare commands without separators', () => {
    expect(classifyLaunchPath('perl')).toBe('command');
    expect(classifyLaunchPath('perl -d:Trace')).toBe('command');
  });

  test('classifies slash-qualified relative paths', () => {
    expect(classifyLaunchPath('bin/perl')).toBe('relative');
    expect(classifyLaunchPath('./script.pl')).toBe('relative');
  });

  test('classification order: variables win over absolute-looking text', () => {
    // `${workspaceFolder}` contains no real separators of its own in the
    // prefix; the workspace-variable branch must win before the variable is
    // even inspected as "other".
    expect(classifyLaunchPath('  ${workspaceFolder}\\bin  ')).toBe('workspace_variable');
    // A home-relative path that also contains a variable stays variable-class.
    expect(classifyLaunchPath('~/${fileBaseName}')).toBe('file_variable');
  });
});
