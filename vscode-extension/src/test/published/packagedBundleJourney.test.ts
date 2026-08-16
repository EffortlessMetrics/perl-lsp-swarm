import * as assert from 'assert';
import * as crypto from 'crypto';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { parsePackagedServerVersionStdout } from '../../packagedServerVersion';
import { runBoundedProcess } from '../../testAdapter';

type ReceiptValue = Record<string, unknown>;

interface VerifiedChildArtifact {
  owner_issue: '#4346';
  schema_version: 'verified_child_receipt.v1';
  receipt_schema_version: 'installed_acceptance.v1';
  candidate_id: string;
  frozen_product_sha: string;
  artifact_set_id: string;
  status: 'pass' | 'limited' | 'blocked' | 'not_proven';
  claim_boundary: string;
  limitation: string | null;
  source_receipt_sha256: string;
  artifact_hashes: ArtifactHashes;
}

interface CandidateArtifactManifest {
  candidate_id: string;
  frozen_product_sha: string;
  artifact_set_id: string;
  platform: string;
  vsix_sha256: string;
  bundled_server_sha256: string;
}

interface ArtifactHashes {
  vsix_sha256: string;
  bundled_server_sha256: string;
}

const SOURCE_CLAIM_BOUNDARY =
  'Packaged VSIX and bundled-server journey exercised by the VS Code extension host.';

function platformLabel(): string {
  switch (process.platform) {
    case 'win32':
      return 'windows';
    case 'darwin':
      return 'macos';
    case 'linux':
      return 'linux';
    default:
      return process.platform;
  }
}

function receiptsDir(): string {
  const root =
    process.env.PERL_LSP_SMOKE_RECEIPTS_DIR ??
    path.resolve(__dirname, '..', '..', '..', '..', 'target', 'receipts', 'vscode-smoke');
  const label = process.env.PERL_LSP_SMOKE_SOURCE_LABEL ?? 'packaged-bundle';
  assert.match(
    label,
    /^[A-Za-z0-9_-]+$/,
    'smoke receipt label must be a single safe path component',
  );
  const directory = path.join(root, label, platformLabel());
  fs.mkdirSync(directory, { recursive: true });
  return directory;
}

function sha256(filePath: string): string {
  return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
}

type BundledServerVersion =
  | {
      status: 'ok';
      version: string;
      stdout: string;
      stderr: string;
      outcome: 'completed';
      output_truncated: boolean;
      termination_confirmed: boolean;
    }
  | {
      status: 'error';
      version?: never;
      stdout: string;
      stderr: string;
      outcome: string;
      output_truncated: boolean;
      termination_confirmed: boolean;
      message: string;
    };

const VERSION_PROBE_TIMEOUT_MS = 15_000;
const VERSION_PROBE_OUTPUT_MAX_BYTES = 64 * 1024;

async function bundledServerVersion(binaryPath: string): Promise<BundledServerVersion> {
  const result = await runBoundedProcess(binaryPath, ['--version'], {
    shell: false,
    timeoutMs: VERSION_PROBE_TIMEOUT_MS,
    maxOutputBytes: VERSION_PROBE_OUTPUT_MAX_BYTES,
    terminationGraceMs: 500,
    terminationWatchdogMs: 5_000,
    windowsHide: true,
  });
  const terminationConfirmed =
    result.outcome !== 'spawn_error' && result.outcome !== 'termination_failed';
  const base = {
    stdout: result.stdout,
    stderr: result.stderr,
    outcome: result.outcome,
    output_truncated: result.outcome === 'output_limit',
    termination_confirmed: terminationConfirmed,
  };
  if (result.outcome !== 'completed') {
    return {
      status: 'error',
      ...base,
      message:
        result.diagnostic ??
        `bundled server --version ended with ${result.outcome} before a clean completion`,
    };
  }
  const completedBase = { ...base, outcome: 'completed' as const };
  try {
    const version = parsePackagedServerVersionStdout(result.stdout);
    if (!version) {
      return {
        status: 'error',
        ...completedBase,
        message: 'bundled server --version did not contain a semantic version',
      };
    }
    return {
      status: 'ok',
      version,
      ...completedBase,
    };
  } catch (error: unknown) {
    return {
      status: 'error',
      ...completedBase,
      message: error instanceof Error ? error.message : String(error),
    };
  }
}

function requireCandidateArtifactManifest(
  observedVsixSha256: string | undefined,
  observedBundledServerSha256: string,
): CandidateArtifactManifest | undefined {
  const serialized = process.env.PERL_LSP_CANDIDATE_ARTIFACT_MANIFEST?.trim();
  const candidateId = process.env.PERL_LSP_CANDIDATE_ID?.trim();
  const frozenProductSha = process.env.PERL_LSP_CURRENT_SOURCE_SHA?.trim();
  const artifactSetId = process.env.PERL_LSP_ARTIFACT_SET_ID?.trim();
  const candidateBound = Boolean(serialized || candidateId || frozenProductSha || artifactSetId);
  if (!candidateBound) {
    return undefined;
  }
  assert.ok(serialized, 'candidate-bound packaged smoke requires an artifact manifest');
  let manifest: CandidateArtifactManifest;
  try {
    manifest = JSON.parse(serialized) as CandidateArtifactManifest;
  } catch (error: unknown) {
    throw new Error(
      `candidate artifact manifest must be valid JSON: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  for (const [field, value] of Object.entries(manifest)) {
    assert.equal(typeof value, 'string', `candidate artifact manifest ${field} must be a string`);
    assert.ok(value.trim(), `candidate artifact manifest ${field} is required`);
  }
  assert.equal(
    manifest.candidate_id,
    candidateId,
    'candidate artifact manifest candidate mismatch',
  );
  assert.equal(
    manifest.frozen_product_sha,
    frozenProductSha,
    'candidate artifact manifest frozen SHA mismatch',
  );
  assert.equal(
    manifest.artifact_set_id,
    artifactSetId,
    'candidate artifact manifest artifact-set mismatch',
  );
  assert.equal(
    manifest.platform,
    platformLabel(),
    'candidate artifact manifest platform mismatch; candidate-bound verification is Linux-only',
  );
  assert.match(manifest.vsix_sha256, /^[0-9a-f]{64}$/i, 'manifest VSIX SHA-256 is invalid');
  assert.match(
    manifest.bundled_server_sha256,
    /^[0-9a-f]{64}$/i,
    'manifest bundled-server SHA-256 is invalid',
  );
  assert.ok(observedVsixSha256, 'candidate-bound smoke could not observe a VSIX SHA-256');
  assert.equal(
    manifest.vsix_sha256,
    observedVsixSha256,
    'observed VSIX identity differs from manifest',
  );
  assert.equal(
    manifest.bundled_server_sha256,
    observedBundledServerSha256,
    'observed bundled-server identity differs from manifest',
  );
  return manifest;
}

function writeVerifiedChildArtifact(receipt: ReceiptValue, sourceReceiptPath: string): void {
  const outputPath = process.env.PERL_LSP_VERIFIED_OUTPUT;
  if (!outputPath) {
    return;
  }

  const candidateId = process.env.PERL_LSP_CANDIDATE_ID;
  const frozenProductSha = process.env.PERL_LSP_CURRENT_SOURCE_SHA;
  const artifactSetId = process.env.PERL_LSP_ARTIFACT_SET_ID;
  assert.ok(candidateId, 'PERL_LSP_CANDIDATE_ID is required for a verified artifact');
  assert.ok(frozenProductSha, 'PERL_LSP_CURRENT_SOURCE_SHA is required for a verified artifact');
  assert.match(frozenProductSha, /^[0-9a-f]{40}$/i, 'frozen product SHA must be 40 hex characters');
  assert.ok(artifactSetId, 'PERL_LSP_ARTIFACT_SET_ID is required for a verified artifact');

  const knownLimitations = Array.isArray(receipt.known_limitations)
    ? receipt.known_limitations.filter((value): value is string => typeof value === 'string')
    : [];
  const outcome = receipt.outcome;
  const artifactHashes = receipt.artifact_hashes;
  assert.ok(
    artifactHashes && typeof artifactHashes === 'object',
    'packaged artifact hashes are required',
  );
  const vsixSha256 = (artifactHashes as Record<string, unknown>).vsix_sha256;
  const bundledServerSha256 = (artifactHashes as Record<string, unknown>).bundled_server_sha256;
  assert.match(vsixSha256 as string, /^[0-9a-f]{64}$/i, 'observed VSIX SHA-256 is required');
  assert.match(
    bundledServerSha256 as string,
    /^[0-9a-f]{64}$/i,
    'observed bundled-server SHA-256 is required',
  );
  const mandatoryEvidenceIsMissing = knownLimitations.some(
    (limitation) =>
      limitation === 'DAP preview is not exercised by this slice.' ||
      limitation ===
        'The public VS Code API does not expose server index generation or semantic exactness.',
  );
  const status: VerifiedChildArtifact['status'] =
    outcome === 'failed'
      ? 'blocked'
      : outcome !== 'completed' || mandatoryEvidenceIsMissing
        ? 'not_proven'
        : knownLimitations.length > 0
          ? 'limited'
          : 'pass';
  const artifact: VerifiedChildArtifact = {
    owner_issue: '#4346',
    schema_version: 'verified_child_receipt.v1',
    receipt_schema_version: 'installed_acceptance.v1',
    candidate_id: candidateId,
    frozen_product_sha: frozenProductSha,
    artifact_set_id: artifactSetId,
    status,
    claim_boundary: SOURCE_CLAIM_BOUNDARY,
    limitation:
      knownLimitations.length > 0
        ? knownLimitations.join(' ')
        : status === 'blocked'
          ? 'The packaged journey reported one or more product blockers.'
          : null,
    source_receipt_sha256: sha256(sourceReceiptPath),
    artifact_hashes: {
      vsix_sha256: vsixSha256 as string,
      bundled_server_sha256: bundledServerSha256 as string,
    },
  };
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, JSON.stringify(artifact, null, 2));
}

async function withTimeout<T>(
  label: string,
  operation: PromiseLike<T>,
  timeoutMs: number,
): Promise<T> {
  let timeout: NodeJS.Timeout | undefined;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timeout = setTimeout(
      () => reject(new Error(`${label} timed out after ${timeoutMs}ms`)),
      timeoutMs,
    );
  });
  try {
    return await Promise.race([Promise.resolve(operation), timeoutPromise]);
  } finally {
    if (timeout) {
      clearTimeout(timeout);
    }
  }
}

async function waitForStartupMetrics(
  getMetrics: () => ReceiptValue,
  timeoutMs: number,
): Promise<ReceiptValue> {
  const deadline = Date.now() + timeoutMs;
  let metrics = getMetrics();
  while (
    Date.now() < deadline &&
    [metrics.binary_resolution_status, metrics.server_start_status, metrics.initialize_status].some(
      (status) => status === 'running',
    )
  ) {
    await new Promise((resolve) => setTimeout(resolve, 100));
    metrics = getMetrics();
  }
  return metrics;
}

function bundledBinaryPath(extensionPath: string): string {
  const directory = path.join(extensionPath, 'bin', `${process.platform}-${process.arch}`);
  const names =
    process.platform === 'win32' ? ['perllsp.exe', 'perl-lsp.exe'] : ['perllsp', 'perl-lsp'];
  const binary = names
    .map((name) => path.join(directory, name))
    .find((candidate) => fs.existsSync(candidate));
  assert.ok(binary, `packaged VSIX must contain a bundled server in ${directory}`);
  return binary;
}

function pathsEquivalent(left: unknown, right: string): boolean {
  if (typeof left !== 'string' || left.length === 0) {
    return false;
  }
  const normalizedLeft = path.resolve(left);
  const normalizedRight = path.resolve(right);
  if (process.platform === 'win32' || process.platform === 'darwin') {
    return normalizedLeft.toLowerCase() === normalizedRight.toLowerCase();
  }
  return normalizedLeft === normalizedRight;
}

async function providerResult(
  label: string,
  command: string,
  ...args: unknown[]
): Promise<ReceiptValue> {
  const started = performance.now();
  try {
    const result = await withTimeout(
      label,
      vscode.commands.executeCommand(command, ...args),
      15_000,
    );
    const record: ReceiptValue = {
      status: 'ok',
      duration_ms: Math.round(performance.now() - started),
    };
    if (Array.isArray(result)) {
      record.item_count = result.length;
    } else if (result && typeof result === 'object' && 'items' in result) {
      const items = (result as { items?: unknown }).items;
      record.item_count = Array.isArray(items) ? items.length : 0;
    } else if (result === undefined || result === null) {
      record.result = 'empty';
    } else {
      record.result = 'present';
    }
    return record;
  } catch (error: unknown) {
    return {
      status: 'error',
      duration_ms: Math.round(performance.now() - started),
      message: error instanceof Error ? error.message : String(error),
    };
  }
}

function providerPosition(document: vscode.TextDocument): vscode.Position {
  const offset = document.getText().indexOf('$value');
  assert.notEqual(offset, -1, 'packaged journey fixture must contain the $value probe');
  return document.positionAt(offset);
}

function assertProviderSucceeded(label: string, result: ReceiptValue): void {
  assert.notEqual(result.status, 'error', `${label}: ${JSON.stringify(result)}`);
}

suite('Packaged VSIX bundled-server journey', function () {
  this.timeout(240_000);

  test('records bundled identity, provider use, edit re-query, and safe mutation outcomes', async function () {
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    assert.ok(workspaceFolder, 'packaged journey requires a workspace folder');
    const workspacePath = workspaceFolder.uri.fsPath;
    const extension = vscode.extensions.getExtension('EffortlessMetrics.perl-lsp-rs');
    assert.ok(extension, 'packaged journey requires the installed extension');

    const bundledServerPath = bundledBinaryPath(extension.extensionPath);
    const expectedVersion = extension.packageJSON?.version ?? null;
    const workspaceFile = path.join(workspacePath, 'packaged_daily_driver.pl');
    fs.writeFileSync(
      workspaceFile,
      ['use strict;', 'use warnings;', '', 'my $value = 42;', 'print $value;', ''].join('\n'),
    );

    const config = vscode.workspace.getConfiguration('perl-lsp');
    const configurationContributions = extension.packageJSON?.contributes?.configuration;
    const registeredConfigurationKeys = new Set(
      (Array.isArray(configurationContributions)
        ? configurationContributions
        : configurationContributions
          ? [configurationContributions]
          : []
      ).flatMap((section: { properties?: Record<string, unknown> }) =>
        Object.keys(section.properties ?? {}),
      ),
    );
    const inspectedSettings = ['autoDownload', 'serverPath', 'critic.enabled']
      .filter((key) => registeredConfigurationKeys.has(`perl-lsp.${key}`))
      .map((key) => ({
        key,
        value: config.inspect<unknown>(key)?.globalValue,
      }));
    const criticSettingRegistered = registeredConfigurationKeys.has('perl-lsp.critic.enabled');

    try {
      if (registeredConfigurationKeys.has('perl-lsp.autoDownload')) {
        await config.update('autoDownload', false, vscode.ConfigurationTarget.Global);
      }
      if (registeredConfigurationKeys.has('perl-lsp.serverPath')) {
        await config.update('serverPath', '', vscode.ConfigurationTarget.Global);
      }
      if (criticSettingRegistered) {
        await config.update('critic.enabled', false, vscode.ConfigurationTarget.Global);
      }

      const activationStarted = performance.now();
      const activation = (await withTimeout(
        'packaged extension activation',
        extension.activate(),
        90_000,
      )) as
        | {
            getLanguageClientStartupMetrics?: () => ReceiptValue;
            getActiveDocumentReadiness?: () => {
              generation: number;
              indexState: string;
              indexReason?: string;
              fullyReady: boolean;
            };
            stop?: () => Promise<void>;
          }
        | undefined;
      const activationCompleted = performance.now();
      const bundledVersion = await bundledServerVersion(bundledServerPath);
      const document = await vscode.workspace.openTextDocument(workspaceFile);
      await vscode.window.showTextDocument(document);
      const position = providerPosition(document);

      const immediate = {
        completion: await providerResult(
          'bundled completion',
          'vscode.executeCompletionItemProvider',
          document.uri,
          position,
        ),
        hover: await providerResult(
          'bundled hover',
          'vscode.executeHoverProvider',
          document.uri,
          position,
        ),
        definition: await providerResult(
          'bundled definition',
          'vscode.executeDefinitionProvider',
          document.uri,
          position,
        ),
        references: await providerResult(
          'bundled references',
          'vscode.executeReferenceProvider',
          document.uri,
          position,
          { includeDeclaration: true },
        ),
        symbols: await providerResult(
          'bundled symbols',
          'vscode.executeDocumentSymbolProvider',
          document.uri,
        ),
      };

      const editStarted = performance.now();
      const edit = new vscode.WorkspaceEdit();
      edit.insert(document.uri, new vscode.Position(document.lineCount, 0), '# packaged edit\n');
      const editApplied = await vscode.workspace.applyEdit(edit);
      const editedText = document.getText();
      const afterEdit = {
        status: editApplied && editedText.includes('# packaged edit') ? 'ok' : 'error',
        duration_ms: Math.round(performance.now() - editStarted),
        immediate_requery: await providerResult(
          'bundled completion after edit',
          'vscode.executeCompletionItemProvider',
          document.uri,
          position,
        ),
      };

      const formatting = await providerResult(
        'bundled formatting',
        'vscode.executeFormatDocumentProvider',
        document.uri,
        { tabSize: 4, insertSpaces: true },
      );

      const renameStarted = performance.now();
      let rename: ReceiptValue;
      try {
        const result = (await withTimeout(
          'bundled rename/refusal',
          vscode.commands.executeCommand(
            'vscode.executeDocumentRenameProvider',
            document.uri,
            position,
            'renamed_value',
          ),
          15_000,
        )) as vscode.WorkspaceEdit | undefined;
        const entries = result?.entries() ?? [];
        const workspaceResolved = path.resolve(workspacePath);
        const workspacePrefix = workspaceResolved + path.sep;
        const caseInsensitive = process.platform === 'win32' || process.platform === 'darwin';
        const safe = entries.every(([uri]) => {
          const resolved = path.resolve(uri.fsPath);
          if (caseInsensitive) {
            const normalized = resolved.toLowerCase();
            const normalizedWorkspace = workspaceResolved.toLowerCase();
            const normalizedPrefix = workspacePrefix.toLowerCase();
            return normalized === normalizedWorkspace || normalized.startsWith(normalizedPrefix);
          }
          return resolved === workspaceResolved || resolved.startsWith(workspacePrefix);
        });
        rename = {
          status: result ? (safe ? 'offered_not_applied' : 'unsafe_refusal') : 'safe_refusal',
          edit_count: entries.length,
          duration_ms: Math.round(performance.now() - renameStarted),
        };
      } catch (error: unknown) {
        rename = {
          status: 'error',
          duration_ms: Math.round(performance.now() - renameStarted),
          message: error instanceof Error ? error.message : String(error),
        };
      }

      const diagnostics = vscode.languages.getDiagnostics(document.uri);
      const metrics = activation?.getLanguageClientStartupMetrics
        ? await waitForStartupMetrics(activation.getLanguageClientStartupMetrics, 30_000)
        : {};
      const readiness = activation?.getActiveDocumentReadiness?.() ?? null;
      const receipt: ReceiptValue = {
        schema_version: 1,
        outcome: 'completed',
        repository_sha: process.env.PERL_LSP_CURRENT_SOURCE_SHA ?? null,
        artifact_hashes: {
          vsix_sha256: process.env.PERL_LSP_VSIX_SHA256 ?? null,
          bundled_server_sha256: sha256(bundledServerPath),
        },
        server_identity: {
          path: bundledServerPath,
          source: 'packaged_vsix_bundle',
          version: bundledVersion.version ?? null,
          expected_version: expectedVersion,
          version_stdout: bundledVersion.stdout,
          version_stderr: bundledVersion.stderr,
          version_output_truncated: bundledVersion.output_truncated,
          version_probe_termination_confirmed: bundledVersion.termination_confirmed,
          version_match:
            bundledVersion.status === 'ok' && expectedVersion !== null
              ? bundledVersion.version === expectedVersion
              : false,
          activated_version: metrics.server_version ?? null,
          activated_version_match:
            bundledVersion.status === 'ok' &&
            expectedVersion !== null &&
            metrics.server_version === bundledVersion.version &&
            metrics.server_version === expectedVersion,
          activated_path: metrics.binary_resolution_path ?? null,
          activated_path_match: pathsEquivalent(metrics.binary_resolution_path, bundledServerPath),
          startup_source: metrics.binary_resolution_source ?? null,
        },
        claim_boundary: SOURCE_CLAIM_BOUNDARY,
        startup: metrics,
        vsix_identity: {
          extension_id: extension.id,
          version: extension.packageJSON?.version ?? null,
          path: extension.extensionPath,
        },
        vscode_version: vscode.version,
        workspaces: [
          { path: workspacePath, mode: 'single-root', trust: vscode.workspace.isTrusted },
        ],
        requests: { immediate, after_edit: afterEdit, formatting, rename },
        index_generation: 'not_observable_from_public_extension_api',
        index_readiness: readiness ?? 'not_observable_from_public_extension_api',
        answering_tier: 'bundled_server_provider',
        fallback_or_refusal_reason:
          rename.status === 'safe_refusal' ? 'rename provider returned no edit' : null,
        latency: { activation_ms: Math.round(activationCompleted - activationStarted) },
        false_exact: 'not_scored',
        stale_exact: 'not_scored',
        unsafe_edits: rename.status === 'unsafe_refusal' ? 1 : 0,
        unexplained_empty: 'not_scored',
        known_limitations: [
          'DAP preview is not exercised by this slice.',
          'The public VS Code API does not expose server index generation or semantic exactness.',
          'A rename edit is never applied by this receipt; offered edits are checked for workspace containment first.',
          ...(criticSettingRegistered
            ? []
            : [
                'The published artifact does not register perl-lsp.critic.enabled; the journey leaves its published default unchanged.',
              ]),
        ],
        product_blockers: [],
        diagnostics: { count: diagnostics.length },
        shutdown: 'pending',
      };

      requireCandidateArtifactManifest(
        typeof receipt.artifact_hashes === 'object' && receipt.artifact_hashes !== null
          ? ((receipt.artifact_hashes as Record<string, unknown>).vsix_sha256 as string | undefined)
          : undefined,
        (receipt.artifact_hashes as Record<string, unknown>).bundled_server_sha256 as string,
      );

      if (activation?.stop) {
        try {
          await withTimeout('packaged extension shutdown', activation.stop(), 30_000);
          receipt.shutdown = 'stopped';
        } catch (error: unknown) {
          receipt.shutdown = 'timeout';
          receipt.shutdown_error = error instanceof Error ? error.message : String(error);
        }
      } else {
        receipt.shutdown = 'not_observable';
      }

      const providerResults = [
        ['completion', immediate.completion],
        ['hover', immediate.hover],
        ['definition', immediate.definition],
        ['references', immediate.references],
        ['symbols', immediate.symbols],
        ['completion after edit', afterEdit.immediate_requery],
        ['formatting', formatting],
        ['rename', rename],
      ] as const;
      const providerFailures = providerResults.filter(
        ([label, result]) =>
          result.status === 'error' || (label === 'rename' && result.status === 'unsafe_refusal'),
      );
      const lifecycleExpectations: Array<[string, string]> = [
        ['binary_resolution_source', 'bundled'],
        ['binary_resolution_status', 'ok'],
        ['server_start_status', 'ok'],
        ['initialize_status', 'ok'],
      ];
      const lifecycleFailures = lifecycleExpectations
        .filter(([field, expected]) => metrics[field] !== expected)
        .map(([field, expected]) => ({
          label: `lifecycle.${field}`,
          result: {
            expected,
            actual: metrics[field] ?? null,
            metrics,
          },
        }));
      const bundledVersionBlocker =
        bundledVersion.status === 'error'
          ? {
              label: 'bundled_server_version',
              result: {
                expected: expectedVersion,
                actual: null,
                message: bundledVersion.message,
                stdout: bundledVersion.stdout,
                stderr: bundledVersion.stderr,
                output_truncated: bundledVersion.output_truncated,
                termination_confirmed: bundledVersion.termination_confirmed,
              },
            }
          : expectedVersion === null || bundledVersion.version !== expectedVersion
            ? {
                label: 'bundled_server_version',
                result: {
                  expected: expectedVersion,
                  actual: bundledVersion.version,
                  stdout: bundledVersion.stdout,
                  stderr: bundledVersion.stderr,
                  output_truncated: bundledVersion.output_truncated,
                  termination_confirmed: bundledVersion.termination_confirmed,
                },
              }
            : null;
      const activatedPath = metrics.binary_resolution_path;
      const activatedPathBlocker = pathsEquivalent(activatedPath, bundledServerPath)
        ? null
        : {
            label: 'activated_server_path',
            result: {
              expected: bundledServerPath,
              actual: activatedPath ?? null,
              source: metrics.binary_resolution_source ?? null,
              message: 'initialized server path did not resolve to the packaged bundled binary',
            },
          };
      const activatedVersion = metrics.server_version;
      const activatedVersionBlocker =
        bundledVersion.status !== 'ok' || expectedVersion === null || !activatedVersion
          ? {
              label: 'activated_server_version',
              result: {
                expected: expectedVersion ?? bundledVersion.version,
                actual: activatedVersion ?? null,
                message: 'initialized server did not report a comparable semantic version',
              },
            }
          : activatedVersion !== bundledVersion.version || activatedVersion !== expectedVersion
            ? {
                label: 'activated_server_version',
                result: {
                  expected: { package: expectedVersion, bundled: bundledVersion.version },
                  actual: activatedVersion,
                  message: 'initialized server version disagrees with packaged identities',
                },
              }
            : null;
      const productBlockers = [
        ...(bundledVersionBlocker ? [bundledVersionBlocker] : []),
        ...(activatedPathBlocker ? [activatedPathBlocker] : []),
        ...(activatedVersionBlocker ? [activatedVersionBlocker] : []),
        ...lifecycleFailures,
        ...providerFailures.map(([label, result]) => ({ label, result })),
      ];
      receipt.outcome = productBlockers.length > 0 ? 'failed' : 'not_proven';
      receipt.product_blockers = productBlockers;

      const sourceReceiptPath = path.join(receiptsDir(), 'packaged_bundle_journey_receipt.json');
      fs.writeFileSync(sourceReceiptPath, JSON.stringify(receipt, null, 2));
      writeVerifiedChildArtifact(receipt, sourceReceiptPath);

      assert.equal(productBlockers.length, 0, JSON.stringify(productBlockers));
      assert.equal(metrics.binary_resolution_source, 'bundled', JSON.stringify(metrics));
      assert.equal(metrics.binary_resolution_status, 'ok', JSON.stringify(metrics));
      assert.equal(metrics.server_start_status, 'ok', JSON.stringify(metrics));
      assert.equal(metrics.initialize_status, 'ok', JSON.stringify(metrics));
      assert.equal(afterEdit.status, 'ok', JSON.stringify(afterEdit));
      for (const [label, result] of providerResults) {
        assertProviderSucceeded(label, result);
      }
      assert.notEqual(rename.status, 'unsafe_refusal', JSON.stringify(rename));
    } finally {
      await Promise.all(
        inspectedSettings.map(({ key, value }) =>
          config.update(key, value, vscode.ConfigurationTarget.Global),
        ),
      );
    }
  });
});
