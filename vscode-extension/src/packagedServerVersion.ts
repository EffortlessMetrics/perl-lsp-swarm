/**
 * Parse a packaged `perllsp --version` probe while preserving stdout/stderr
 * provenance. Only stdout's first line is authoritative for identity.
 */
export function parsePackagedServerVersionStdout(stdout: string): string | null {
  const firstLine = stdout.split(/\r?\n/, 1)[0]?.trim() ?? '';
  const match =
    /^(?:perllsp|perl-lsp)\s+v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)(?:\s|$)/.exec(
      firstLine,
    );
  return match?.[1] ?? null;
}
