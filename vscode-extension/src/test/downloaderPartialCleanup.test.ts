import { EventEmitter } from 'events';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import type * as vscode from 'vscode';
import { afterEach, beforeEach, describe, expect, jest, test } from '@jest/globals';
import { BinaryDownloader } from '../downloader';
import { downloadBoundedFile } from '../boundedFileDownload';
import type { CancellationTokenLike, DisposableLike } from '../boundedHttpJson';

type TestRequest = EventEmitter & {
  destroy: jest.Mock;
};

type DownloaderSeams = {
  downloadFile(
    url: string,
    dest: string,
    timeoutMs?: number,
    maxRedirects?: number,
    maxBytes?: number,
    cancellationToken?: CancellationTokenLike,
  ): Promise<void>;
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

  function makeResponse(
    statusCode: number,
    headers: Record<string, string> = {},
  ): EventEmitter & {
    statusCode: number;
    headers: Record<string, string>;
    destroy: jest.Mock;
    resume: jest.Mock;
  } {
    const response = new EventEmitter() as EventEmitter & {
      statusCode: number;
      headers: Record<string, string>;
      destroy: jest.Mock;
      resume: jest.Mock;
    };
    response.statusCode = statusCode;
    response.headers = headers;
    response.destroy = jest.fn();
    response.resume = jest.fn();
    return response;
  }

  function managedCleanupArtifacts(): string[] {
    return fs
      .readdirSync(tmpDir)
      .filter(
        (entry) => entry.startsWith('.partial-download-') || entry.startsWith('.partial-cleanup-'),
      );
  }

  async function flushManagedCleanup(): Promise<void> {
    await new Promise<void>((resolve) => setImmediate(resolve));
  }

  function observeRealManagedCleanup(downloader: DownloaderSeams): {
    delayedCleanup: Array<() => void>;
    removePartialFile: jest.SpiedFunction<DownloaderSeams['removePartialFile']>;
    stagingDestination: () => string;
    quarantinedArtifactsAtRemoval: () => string[];
  } {
    const delayedCleanup: Array<() => void> = [];
    jest.spyOn(global, 'setImmediate').mockImplementation((callback: () => void) => {
      delayedCleanup.push(callback);
      return {} as NodeJS.Immediate;
    });

    let stagingPath = '';
    let quarantinedArtifacts: string[] = [];
    const originalRemove = BinaryDownloader.prototype['removePartialFile'].bind(downloader);
    const removePartialFile = jest
      .spyOn(downloader, 'removePartialFile')
      .mockImplementation((failedStagingPath) => {
        stagingPath = failedStagingPath;
        expect(fs.existsSync(failedStagingPath)).toBe(true);

        originalRemove(failedStagingPath);

        quarantinedArtifacts = managedCleanupArtifacts().filter((entry) =>
          entry.startsWith('.partial-cleanup-'),
        );
        expect(fs.existsSync(failedStagingPath)).toBe(false);
        expect(quarantinedArtifacts).toHaveLength(1);
      });

    return {
      delayedCleanup,
      removePartialFile,
      stagingDestination: () => stagingPath,
      quarantinedArtifactsAtRemoval: () => quarantinedArtifacts,
    };
  }

  test.each([
    ['non-2xx response', 404, {}],
    ['declared oversize response', 200, { 'content-length': '11' }],
  ])('preserves an existing destination on %s', async (_name, statusCode, headers) => {
    const destination = path.join(tmpDir, 'existing.bin');
    fs.writeFileSync(destination, 'existing');
    const request = new EventEmitter() as TestRequest;
    request.destroy = jest.fn();

    await expect(
      downloadBoundedFile({
        requestFactory: (listener) => {
          process.nextTick(() => listener(makeResponse(statusCode, headers) as never));
          return request as never;
        },
        dest: destination,
        timeoutMs: 1000,
        maxBytes: 10,
      }),
    ).rejects.toThrow();

    expect(fs.readFileSync(destination, 'utf8')).toBe('existing');
  });

  test('writes and promotes a real download with a 255-byte UTF-8 destination component', async () => {
    const destination = path.join(tmpDir, `${'é'.repeat(125)}a.bin`);
    fs.writeFileSync(destination, 'existing');
    let stagingDestination = '';
    const request = new EventEmitter() as TestRequest;
    request.destroy = jest.fn();
    const downloader = new BinaryDownloader(
      makeContext(tmpDir),
      makeOutputChannel(),
    ) as unknown as DownloaderSeams;
    jest.spyOn(downloader, 'createWriteStream').mockImplementation((stagingPath) => {
      stagingDestination = stagingPath;
      return BinaryDownloader.prototype['createWriteStream'].call(downloader, stagingPath);
    });

    jest.spyOn(downloader, 'httpGet').mockImplementation((_https, _url, _options, callback) => {
      process.nextTick(() => {
        const response = makeResponse(200, { 'content-length': '11' });
        (callback as (response: unknown) => void)(response);
        response.emit('data', Buffer.from('replacement'));
        response.emit('end');
      });
      return request;
    });

    await downloader.downloadFile('http://localhost/archive', destination, 1000);

    expect(fs.readFileSync(destination, 'utf8')).toBe('replacement');
    expect(path.dirname(stagingDestination)).toBe(tmpDir);
    expect(Buffer.byteLength(path.basename(stagingDestination), 'utf8')).toBeLessThanOrEqual(255);
    expect(fs.existsSync(stagingDestination)).toBe(false);
  });

  test('preserves the existing destination when promotion fails', async () => {
    const destination = path.join(tmpDir, 'promotion-failure-existing.bin');
    fs.writeFileSync(destination, 'existing');
    let stagingDestination = '';
    const request = new EventEmitter() as TestRequest;
    request.destroy = jest.fn();
    const promotionError = new Error('promotion failed');
    const downloader = new BinaryDownloader(
      makeContext(tmpDir),
      makeOutputChannel(),
    ) as unknown as DownloaderSeams;
    jest.spyOn(downloader, 'createWriteStream').mockImplementation((stagingPath) => {
      stagingDestination = stagingPath;
      return BinaryDownloader.prototype['createWriteStream'].call(downloader, stagingPath);
    });
    jest.spyOn(downloader, 'httpGet').mockImplementation((_https, _url, _options, callback) => {
      process.nextTick(() => {
        const response = makeResponse(200, { 'content-length': '10' });
        (callback as (response: unknown) => void)(response);
        response.emit('data', Buffer.from('new-bytes'));
        response.emit('end');
      });
      return request;
    });

    await expect(
      downloadBoundedFile({
        requestFactory: (listener) => downloader.httpGet('', '', {}, listener) as never,
        dest: destination,
        timeoutMs: 1000,
        maxBytes: 10,
        createWriteStream: (stagingPath) => downloader.createWriteStream(stagingPath),
        promoteFile: () => {
          throw promotionError;
        },
      }),
    ).rejects.toBe(promotionError);

    expect(fs.readFileSync(destination, 'utf8')).toBe('existing');
    expect(stagingDestination).not.toBe('');
    expect(fs.existsSync(stagingDestination)).toBe(false);
  });

  test('preserves the existing destination after request failure', async () => {
    // Exactly 255 UTF-8 bytes: valid as a single filesystem component while
    // leaving no room for a destination-derived staging suffix.
    const destination = path.join(tmpDir, `${'é'.repeat(125)}a.bin`);
    fs.writeFileSync(destination, 'partial');

    const downloader = new BinaryDownloader(
      makeContext(tmpDir),
      makeOutputChannel(),
    ) as unknown as DownloaderSeams;
    const request = new EventEmitter() as TestRequest;
    request.destroy = jest.fn();
    const requestError = Object.assign(new Error('request failed'), { code: 'ECONNRESET' });
    const delayedCleanup: Array<() => void> = [];
    jest.spyOn(global, 'setImmediate').mockImplementation((callback: () => void) => {
      delayedCleanup.push(callback);
      return {} as NodeJS.Immediate;
    });

    const removePartialFile = jest
      .spyOn(downloader, 'removePartialFile')
      .mockImplementation((stagingPath) => {
        fs.writeFileSync(stagingPath, 'partial-generation');
        BinaryDownloader.prototype['removePartialFile'].call(downloader, stagingPath);
      });
    jest.spyOn(downloader, 'httpGet').mockImplementation(() => {
      process.nextTick(() => request.emit('error', requestError));
      return request;
    });

    await expect(
      downloader.downloadFile('http://localhost/archive', destination, 1000),
    ).rejects.toBe(requestError);

    expect(removePartialFile).toHaveBeenCalledTimes(1);
    const stagingDestination = removePartialFile.mock.calls[0]?.[0] ?? '';
    expect(path.dirname(stagingDestination)).toBe(tmpDir);
    expect(stagingDestination).not.toBe(destination);
    expect(Buffer.byteLength(path.basename(stagingDestination), 'utf8')).toBeLessThanOrEqual(255);
    fs.writeFileSync(destination, 'replacement');

    expect(delayedCleanup).toHaveLength(1);
    delayedCleanup[0]?.();
    expect(fs.readFileSync(destination, 'utf8')).toBe('replacement');
    expect(fs.existsSync(stagingDestination)).toBe(false);
  });

  test('preserves the existing destination after timeout', async () => {
    const destination = path.join(tmpDir, 'timeout-existing.bin');
    fs.writeFileSync(destination, 'existing');
    const downloader = new BinaryDownloader(
      makeContext(tmpDir),
      makeOutputChannel(),
    ) as unknown as DownloaderSeams;
    const request = new EventEmitter() as TestRequest;
    request.destroy = jest.fn();
    const cleanup = observeRealManagedCleanup(downloader);
    let createdStagingDestination = '';
    let resolveStagingOpened: () => void = () => undefined;
    const stagingOpened = new Promise<void>((resolve) => {
      resolveStagingOpened = resolve;
    });
    jest.spyOn(downloader, 'createWriteStream').mockImplementation((stagingPath) => {
      createdStagingDestination = stagingPath;
      const stream = BinaryDownloader.prototype['createWriteStream'].call(downloader, stagingPath);
      stream.once('open', resolveStagingOpened);
      return stream;
    });
    const response = makeResponse(200);
    jest.spyOn(downloader, 'httpGet').mockImplementation((_https, _url, _options, callback) => {
      process.nextTick(() => {
        (callback as (value: unknown) => void)(response);
        response.emit('data', Buffer.from('partial-generation'));
      });
      return request;
    });

    const failedDownload = downloader.downloadFile('http://localhost/archive', destination, 10);
    await stagingOpened;
    expect(createdStagingDestination).not.toBe('');
    expect(fs.existsSync(createdStagingDestination)).toBe(true);
    await expect(failedDownload).rejects.toThrow('Download timeout after 0.01 seconds');

    expect(request.destroy).toHaveBeenCalled();
    expect(fs.readFileSync(destination, 'utf8')).toBe('existing');
    expect(cleanup.removePartialFile).toHaveBeenCalledTimes(1);
    expect(cleanup.stagingDestination()).not.toBe('');
    expect(cleanup.quarantinedArtifactsAtRemoval()).toHaveLength(1);
    expect(cleanup.delayedCleanup.length).toBeGreaterThanOrEqual(1);
    expect(managedCleanupArtifacts()).toHaveLength(1);

    for (const callback of [...cleanup.delayedCleanup]) {
      callback();
    }
    expect(managedCleanupArtifacts()).toEqual([]);
    expect(fs.readFileSync(destination, 'utf8')).toBe('existing');
  });

  test('preserves the existing destination after cancellation', async () => {
    const destination = path.join(tmpDir, 'cancel-existing.bin');
    fs.writeFileSync(destination, 'existing');
    const listeners = new Set<() => void>();
    let cancelled = false;
    const token: CancellationTokenLike = {
      get isCancellationRequested() {
        return cancelled;
      },
      onCancellationRequested: (listener: () => void): DisposableLike => {
        listeners.add(listener);
        return { dispose: () => listeners.delete(listener) };
      },
    };
    const downloader = new BinaryDownloader(
      makeContext(tmpDir),
      makeOutputChannel(),
    ) as unknown as DownloaderSeams;
    const request = new EventEmitter() as TestRequest;
    request.destroy = jest.fn();
    const cleanup = observeRealManagedCleanup(downloader);
    let createdStagingDestination = '';
    let resolveStagingOpened: () => void = () => undefined;
    const stagingOpened = new Promise<void>((resolve) => {
      resolveStagingOpened = resolve;
    });
    jest.spyOn(downloader, 'createWriteStream').mockImplementation((stagingPath) => {
      createdStagingDestination = stagingPath;
      const stream = BinaryDownloader.prototype['createWriteStream'].call(downloader, stagingPath);
      stream.once('open', resolveStagingOpened);
      return stream;
    });
    const response = makeResponse(200);
    jest.spyOn(downloader, 'httpGet').mockImplementation((_https, _url, _options, callback) => {
      process.nextTick(() => {
        (callback as (value: unknown) => void)(response);
        response.emit('data', Buffer.from('partial-generation'));
      });
      return request;
    });

    const failedDownload = downloader.downloadFile(
      'http://localhost/archive',
      destination,
      1000,
      undefined,
      undefined,
      token,
    );
    await stagingOpened;
    expect(createdStagingDestination).not.toBe('');
    expect(fs.existsSync(createdStagingDestination)).toBe(true);
    cancelled = true;
    for (const listener of [...listeners]) {
      listener();
    }
    await expect(failedDownload).rejects.toThrow('Archive download cancelled');

    expect(request.destroy).toHaveBeenCalled();
    expect(fs.readFileSync(destination, 'utf8')).toBe('existing');
    expect(cleanup.removePartialFile).toHaveBeenCalledTimes(1);
    expect(cleanup.stagingDestination()).not.toBe('');
    expect(cleanup.quarantinedArtifactsAtRemoval()).toHaveLength(1);
    expect(cleanup.delayedCleanup.length).toBeGreaterThanOrEqual(1);
    expect(managedCleanupArtifacts()).toHaveLength(1);

    for (const callback of [...cleanup.delayedCleanup]) {
      callback();
    }
    expect(managedCleanupArtifacts()).toEqual([]);
    expect(fs.readFileSync(destination, 'utf8')).toBe('existing');
  });

  test('preserves the existing destination after a response stream error', async () => {
    const destination = path.join(tmpDir, 'stream-error-existing.bin');
    fs.writeFileSync(destination, 'existing');
    const downloader = new BinaryDownloader(
      makeContext(tmpDir),
      makeOutputChannel(),
    ) as unknown as DownloaderSeams;
    const request = new EventEmitter() as TestRequest;
    request.destroy = jest.fn();
    const cleanup = observeRealManagedCleanup(downloader);
    const response = makeResponse(200);
    const streamError = new Error('stream failed');
    jest.spyOn(downloader, 'httpGet').mockImplementation((_https, _url, _options, callback) => {
      process.nextTick(() => {
        (callback as (value: unknown) => void)(response);
        response.emit('data', Buffer.from('partial-generation'));
        response.emit('error', streamError);
      });
      return request;
    });

    await expect(
      downloader.downloadFile('http://localhost/archive', destination, 1000),
    ).rejects.toBe(streamError);

    expect(request.destroy).toHaveBeenCalled();
    expect(fs.readFileSync(destination, 'utf8')).toBe('existing');
    expect(cleanup.removePartialFile).toHaveBeenCalledTimes(1);
    expect(cleanup.stagingDestination()).not.toBe('');
    expect(cleanup.quarantinedArtifactsAtRemoval()).toHaveLength(1);
    expect(cleanup.delayedCleanup.length).toBeGreaterThanOrEqual(1);
    expect(managedCleanupArtifacts()).toHaveLength(1);

    for (const callback of [...cleanup.delayedCleanup]) {
      callback();
    }
    expect(managedCleanupArtifacts()).toEqual([]);
    expect(fs.readFileSync(destination, 'utf8')).toBe('existing');
  });

  test('does not quarantine a replacement installed before the failed generation is renamed', async () => {
    const destination = path.join(tmpDir, 'partial-before-rename.bin');
    fs.writeFileSync(destination, 'partial');

    const downloader = new BinaryDownloader(
      makeContext(tmpDir),
      makeOutputChannel(),
    ) as unknown as DownloaderSeams;
    const request = new EventEmitter() as TestRequest;
    request.destroy = jest.fn();
    const requestError = new Error('request failed before quarantine');
    const delayedCleanup: Array<() => void> = [];
    jest.spyOn(global, 'setImmediate').mockImplementation((callback: () => void) => {
      delayedCleanup.push(callback);
      return {} as NodeJS.Immediate;
    });

    const originalRemove = BinaryDownloader.prototype['removePartialFile'].bind(downloader);
    jest.spyOn(downloader, 'removePartialFile').mockImplementation((stagingPath) => {
      // This is the adversarial scheduling point: a new owner wins dest just
      // before cleanup claims the failed generation.
      fs.writeFileSync(stagingPath, 'partial-generation');
      fs.writeFileSync(destination, 'replacement-before-rename');
      originalRemove(stagingPath);
    });
    jest.spyOn(downloader, 'httpGet').mockImplementation(() => {
      process.nextTick(() => request.emit('error', requestError));
      return request;
    });

    await expect(
      downloader.downloadFile('http://localhost/archive', destination, 1000),
    ).rejects.toBe(requestError);

    expect(delayedCleanup).toHaveLength(1);
    delayedCleanup[0]?.();
    expect(fs.readFileSync(destination, 'utf8')).toBe('replacement-before-rename');
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
    let stagingDestination = '';
    let quarantinedArtifactsAtRemoval: string[] = [];
    let file: fs.WriteStream | undefined;
    jest.spyOn(downloader, 'createWriteStream').mockImplementation((stagingPath) => {
      stagingDestination = stagingPath;
      file = fs.createWriteStream(stagingPath, { emitClose: false });
      file.write('data');
      return file;
    });
    const originalRemove = BinaryDownloader.prototype['removePartialFile'].bind(downloader);
    const removePartialFile = jest
      .spyOn(downloader, 'removePartialFile')
      .mockImplementation((stagingPath) => {
        originalRemove(stagingPath);
        quarantinedArtifactsAtRemoval = managedCleanupArtifacts().filter((entry) =>
          entry.startsWith('.partial-cleanup-'),
        );
        expect(quarantinedArtifactsAtRemoval).toHaveLength(1);
        expect(fs.existsSync(stagingPath)).toBe(false);
      });
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
    await flushManagedCleanup();
    expect(removePartialFile).toHaveBeenCalledTimes(1);
    expect(fs.readFileSync(destination, 'utf8')).toBe('partial');
    expect(stagingDestination).not.toBe('');
    expect(quarantinedArtifactsAtRemoval).toHaveLength(1);
    expect(managedCleanupArtifacts()).toEqual([]);
  });

  test('settles immediately when a stream double closes synchronously', async () => {
    const destination = path.join(tmpDir, 'partial-sync-close.bin');
    fs.writeFileSync(destination, 'partial');

    const downloader = new BinaryDownloader(
      makeContext(tmpDir),
      makeOutputChannel(),
    ) as unknown as DownloaderSeams;
    const request = new EventEmitter() as TestRequest;
    request.destroy = jest.fn();
    const requestError = new Error('request failed after synchronous close');
    let stagingDestination = '';
    let quarantinedArtifactsAtRemoval: string[] = [];
    const file = new EventEmitter() as EventEmitter & {
      closed: boolean;
      destroy: jest.Mock;
      write: jest.Mock;
      end: jest.Mock;
    };
    file.closed = false;
    file.destroy = jest.fn(() => {
      file.emit('close');
    });
    file.write = jest.fn(() => true);
    file.end = jest.fn();
    jest.spyOn(downloader, 'createWriteStream').mockImplementation((stagingPath) => {
      stagingDestination = stagingPath;
      fs.writeFileSync(stagingPath, 'partial');
      return file as unknown as fs.WriteStream;
    });
    const originalRemove = BinaryDownloader.prototype['removePartialFile'].bind(downloader);
    const removePartialFile = jest
      .spyOn(downloader, 'removePartialFile')
      .mockImplementation((stagingPath) => {
        originalRemove(stagingPath);
        quarantinedArtifactsAtRemoval = managedCleanupArtifacts().filter((entry) =>
          entry.startsWith('.partial-cleanup-'),
        );
        expect(quarantinedArtifactsAtRemoval).toHaveLength(1);
        expect(fs.existsSync(stagingPath)).toBe(false);
      });
    jest.spyOn(downloader, 'httpGet').mockImplementation((_https, _url, _options, callback) => {
      process.nextTick(() => {
        const response = new EventEmitter() as EventEmitter & {
          statusCode: number;
          headers: Record<string, string>;
          destroy: jest.Mock;
          pause: jest.Mock;
          resume: jest.Mock;
        };
        response.statusCode = 200;
        response.headers = {};
        response.destroy = jest.fn();
        response.pause = jest.fn();
        response.resume = jest.fn();
        (callback as (response: unknown) => void)(response);
        response.emit('data', Buffer.from('payload'));
        response.emit('error', requestError);
      });
      return request;
    });

    await expect(
      downloader.downloadFile('http://localhost/archive', destination, 1000),
    ).rejects.toBe(requestError);
    await flushManagedCleanup();
    expect(removePartialFile).toHaveBeenCalledTimes(1);
    expect(file.destroy).toHaveBeenCalledTimes(1);
    expect(fs.readFileSync(destination, 'utf8')).toBe('partial');
    expect(stagingDestination).not.toBe('');
    expect(quarantinedArtifactsAtRemoval).toHaveLength(1);
    expect(managedCleanupArtifacts()).toEqual([]);
  });
});
