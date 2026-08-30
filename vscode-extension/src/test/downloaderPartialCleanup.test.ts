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
  createWriteStream(dest: string): fs.WriteStream;
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

  test('does not wait forever when the production write stream suppresses close', async () => {
    const destination = path.join(tmpDir, 'partial-emit-close-false.bin');
    fs.writeFileSync(destination, 'partial');

    const downloader = new BinaryDownloader(
      makeContext(tmpDir),
      makeOutputChannel(),
    ) as unknown as DownloaderSeams;
    const request = new EventEmitter() as TestRequest;
    request.destroy = jest.fn();
    const requestError = new Error('request failed after data');
    const file = fs.createWriteStream(destination, { emitClose: false });
    file.write('data');
    jest.spyOn(downloader, 'createWriteStream').mockReturnValue(file);
    jest.spyOn(downloader, 'removePartialFile').mockImplementation(() => {});
    jest.spyOn(downloader, 'httpGet').mockImplementation((_https, _url, _options, callback) => {
      process.nextTick(() => {
        const response = new EventEmitter() as EventEmitter & {
          statusCode: number;
          headers: Record<string, string>;
          destroy: jest.Mock;
        };
        response.statusCode = 200;
        response.headers = {};
        response.destroy = jest.fn();
        (callback as (response: unknown) => void)(response);
        response.emit('data', Buffer.from('payload'));
        response.emit('error', requestError);
      });
      return request;
    });

    await expect(
      downloader.downloadFile('http://localhost/archive', destination, 1000),
    ).rejects.toBe(requestError);
    expect(fs.existsSync(destination)).toBe(false);
  });
});
