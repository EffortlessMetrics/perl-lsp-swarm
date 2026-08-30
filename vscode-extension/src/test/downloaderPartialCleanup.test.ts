import { EventEmitter } from 'events';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import type * as vscode from 'vscode';
import { afterEach, beforeEach, describe, expect, jest, test } from '@jest/globals';
import { BinaryDownloader } from '../downloader';

type TestRequest = EventEmitter & {
  destroy: jest.Mock;
};

type DownloaderSeams = {
  downloadFile(url: string, dest: string, timeoutMs?: number): Promise<void>;
  httpGet(...args: unknown[]): TestRequest;
  removePartialFile(dest: string): void;
};

function makeContext(storagePath: string): vscode.ExtensionContext {
  return {
    globalStorageUri: { fsPath: storagePath } as vscode.Uri,
    extensionPath: storagePath,
    subscriptions: [],
  } as unknown as vscode.ExtensionContext;
}

function makeOutputChannel(): vscode.OutputChannel {
  return {
    appendLine: jest.fn(),
    show: jest.fn(),
    dispose: jest.fn(),
  } as unknown as vscode.OutputChannel;
}

describe('BinaryDownloader partial-file cleanup ordering', () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'downloader-cleanup-'));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
    jest.restoreAllMocks();
  });

  test('rejects only after the partial destination is absent', async () => {
    const destination = path.join(tmpDir, 'partial.bin');
    fs.writeFileSync(destination, 'partial');

    const downloader = new BinaryDownloader(
      makeContext(tmpDir),
      makeOutputChannel(),
    ) as unknown as DownloaderSeams;
    const request = new EventEmitter() as TestRequest;
    request.destroy = jest.fn();
    const requestError = Object.assign(new Error('request failed'), { code: 'ECONNRESET' });

    const removePartialFile = jest
      .spyOn(downloader, 'removePartialFile')
      .mockImplementation((filePath) => {
        // Model the production callback-based fs.unlink seam: it returns now
        // and would remove the file only on a later event-loop turn.
        setImmediate(() => {
          try {
            fs.unlinkSync(filePath);
          } catch {
            // The bounded helper may already have completed the cleanup.
          }
        });
      });
    jest.spyOn(downloader, 'httpGet').mockImplementation(() => {
      process.nextTick(() => request.emit('error', requestError));
      return request;
    });

    await expect(
      downloader.downloadFile('http://localhost/archive', destination, 1000),
    ).rejects.toBe(requestError);

    expect(removePartialFile).toHaveBeenCalledTimes(1);
    expect(removePartialFile).toHaveBeenCalledWith(destination);
    expect(fs.existsSync(destination)).toBe(false);

    await new Promise<void>((resolve) => setImmediate(resolve));
  });
});
