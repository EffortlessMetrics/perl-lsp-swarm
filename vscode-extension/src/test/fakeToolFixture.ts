import * as fs from 'fs';

/**
 * Stage a regular executable file for tests that mock the actual tool process.
 * Windows fixtures still need a native executable if the test launches them.
 */
export function writeFakeTool(filePath: string, contents = ''): void {
  fs.writeFileSync(filePath, contents);
  if (process.platform !== 'win32') {
    fs.chmodSync(filePath, 0o755);
  }
}
