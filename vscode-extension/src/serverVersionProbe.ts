import { execFile } from 'child_process';

export type VersionExecutor = (
  path: string,
  args: string[],
  options: { timeout: number },
  callback: (error: Error | null, stdout: string) => void,
) => void;

export interface ServerVersionProbeBinding {
  /** Identity of the lifecycle owner that captured this probe. */
  readonly lifecycle: object | null;
  readonly serverPath: string | null;
  readonly generation: number | undefined;
}

/** Probe one captured server identity and discard a result after its identity changes. */
export function probeServerVersion(
  capture: () => ServerVersionProbeBinding,
  execute: VersionExecutor = execFile as VersionExecutor,
): Promise<string> {
  const captured = capture();
  const serverPath = captured.serverPath;
  if (!serverPath || captured.lifecycle === null || captured.generation === undefined) {
    return Promise.resolve('unavailable');
  }

  return new Promise((resolve) => {
    execute(serverPath, ['--version'], { timeout: 3000 }, (error, stdout) => {
      const current = capture();
      if (
        current.lifecycle !== captured.lifecycle ||
        current.serverPath !== captured.serverPath ||
        current.generation !== captured.generation
      ) {
        resolve('unavailable');
        return;
      }
      if (error) {
        resolve('unavailable');
        return;
      }
      resolve(stdout.trim().split('\n')[0]?.trim() || 'unavailable');
    });
  });
}
