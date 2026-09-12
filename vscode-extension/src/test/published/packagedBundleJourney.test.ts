import * as assert from 'assert';
import * as fs from 'fs';
import * as path from 'path';
import { randomUUID } from 'crypto';
import * as vscode from 'vscode';
import { runBoundedProcess } from '../../testAdapter';
import {
  assertProviderSucceeded,
  bundledBinaryPath,
  bundledDapPath,
  bundledServerVersion,
  pathsEquivalent,
  platformLabel,
  providerPosition,
  providerResult,
  receiptsDir,
  observeActiveDocumentReadiness,
  waitForActiveDocumentGeneration,
  sha256,
  waitForStartupMetrics,
  scanBundledDapProcessIdentities,
  withTimeout,
  type ReceiptValue,
} from './journeySupport';

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

function readinessDeferredProvider(label: string, reason: string): ReceiptValue {
  return {
    status: 'not_proven',
    label,
    reason,
  };
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
    'candidate artifact manifest platform mismatch; candidate-bound verification requires the host platform',
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

function recordPackagedDapEvidence(
  extensionPath: string,
  dapPath: string,
  session: vscode.DebugSession,
  exit: { code?: number; signal?: string },
  debuggee: OwnedDebuggee,
): void {
  const receiptPath = path.join(receiptsDir(), 'packaged_bundle_journey_receipt.json');
  if (!fs.existsSync(receiptPath)) {
    throw new Error('packaged DAP evidence requires the bundled journey receipt');
  }
  const receipt = JSON.parse(fs.readFileSync(receiptPath, 'utf8')) as ReceiptValue;
  assert.match(process.env.PERL_LSP_CURRENT_SOURCE_SHA ?? '', /^[0-9a-f]{40}$/);
  assert.ok(process.env.PERL_LSP_CANDIDATE_ID);
  assert.ok(process.env.PERL_LSP_ARTIFACT_SET_ID);
  const hashes =
    receipt.artifact_hashes && typeof receipt.artifact_hashes === 'object'
      ? (receipt.artifact_hashes as Record<string, unknown>)
      : {};
  hashes.dap_sha256 = sha256(dapPath);
  hashes.vsix_sha256 ||= process.env.PERL_LSP_VSIX_SHA256 ?? null;
  assert.match(String(hashes.vsix_sha256 ?? ''), /^[0-9a-f]{64}$/);
  receipt.artifact_hashes = hashes;
  if (Array.isArray(receipt.known_limitations)) {
    const limitations = receipt.known_limitations.filter(
      (limitation) => limitation !== 'DAP preview is not exercised by this slice.',
    );
    limitations.push(
      'DAP startup and ordinary Stop are exercised; breakpoint, stepping, variables and evaluation semantics remain not proven.',
    );
    receipt.known_limitations = limitations;
  }
  receipt.dap_startup = {
    extension_path: extensionPath,
    adapter_path: dapPath,
    session_id: session.id,
    initialize_then_launch: true,
    exit,
    debuggee,
    owned_process_cleanup: 'pass',
    candidate_id: process.env.PERL_LSP_CANDIDATE_ID ?? null,
    frozen_product_sha: process.env.PERL_LSP_CURRENT_SOURCE_SHA ?? null,
    artifact_set_id: process.env.PERL_LSP_ARTIFACT_SET_ID ?? null,
  };
  fs.writeFileSync(receiptPath, JSON.stringify(receipt, null, 2));
  writeVerifiedChildArtifact(receipt, receiptPath);
}

interface OwnedDebuggee {
  pid: number;
  creationTimeFileTime: string;
}

async function observeDebuggee(pid: number): Promise<OwnedDebuggee | null> {
  assert.ok(Number.isSafeInteger(pid) && pid > 0, 'invalid owned debuggee PID');
  const result = await runBoundedProcess(
    'powershell.exe',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `$observed = Get-Process -Id ${pid} -ErrorAction SilentlyContinue; if ($observed) { $observed.StartTime.ToFileTimeUtc().ToString() }`,
    ],
    {
      shell: false,
      windowsHide: true,
      timeoutMs: 5000,
      maxOutputBytes: 4096,
      terminationGraceMs: 1000,
      terminationWatchdogMs: 5000,
    },
  );
  if (result.outcome !== 'completed' || result.exitCode !== 0) {
    throw new Error(`owned debuggee scan failed: ${result.outcome}, ${result.exitCode}`);
  }
  const creationTimeFileTime = result.stdout.trim();
  if (!creationTimeFileTime) return null;
  assert.match(creationTimeFileTime, /^\d+$/, 'invalid process creation time');
  return { pid, creationTimeFileTime };
}

async function waitForDebuggee(pidFile: string): Promise<OwnedDebuggee> {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (fs.existsSync(pidFile)) {
      const raw = fs.readFileSync(pidFile, 'utf8').trim();
      if (/^\d+$/.test(raw)) {
        const observed = await observeDebuggee(Number(raw));
        if (observed) return observed;
      }
    }
    await new Promise<void>((resolve) => setTimeout(resolve, 100));
  }
  throw new Error('debuggee did not publish a live process identity');
}

async function waitForDebuggeeExit(debuggee: OwnedDebuggee): Promise<void> {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    const observed = await observeDebuggee(debuggee.pid);
    if (!observed || observed.creationTimeFileTime !== debuggee.creationTimeFileTime) return;
    await new Promise<void>((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`owned debuggee survived Stop: ${debuggee.pid}`);
}

async function waitForNewPackagedDap(
  directory: string,
  baseline: Set<string>,
  expectedPath: string,
): Promise<{ pid: number; path: string; creationTimeFileTime?: string }> {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    const processes = await scanBundledDapProcessIdentities(directory);
    const matches = processes.filter((entry) => {
      const key = `${entry.pid}:${entry.creationTimeFileTime ?? ''}:${entry.path.toLowerCase()}`;
      return !baseline.has(key) && pathsEquivalent(entry.path, expectedPath);
    });
    if (matches.length > 1) {
      throw new Error(
        `multiple new packaged DAP processes were observed: ${JSON.stringify(matches)}`,
      );
    }
    const match = matches[0];
    if (match) return match;
    await new Promise<void>((resolve) => setTimeout(resolve, 250));
  }
  throw new Error('packaged DAP process was not observed within 30 seconds');
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
            waitForActiveDocumentReady?: (uri: string, timeoutMs?: number) => Promise<void>;
            stop?: () => Promise<void>;
          }
        | undefined;
      const activationCompleted = performance.now();
      const bundledVersion = await bundledServerVersion(bundledServerPath);
      const readinessBefore = activation?.getActiveDocumentReadiness?.() ?? null;
      const document = await vscode.workspace.openTextDocument(workspaceFile);
      await vscode.window.showTextDocument(document);
      const position = providerPosition(document);

      const generationWait = await waitForActiveDocumentGeneration(
        activation?.getActiveDocumentReadiness,
        typeof readinessBefore?.generation === 'number' ? readinessBefore.generation : undefined,
        30_000,
      );
      const readiness =
        generationWait?.status === 'not_proven'
          ? generationWait
          : await observeActiveDocumentReadiness(
              activation?.waitForActiveDocumentReady,
              document.uri.toString(),
              30_000,
            );
      const readinessWait: ReceiptValue = {
        scope: 'active_document',
        uri: document.uri.toString(),
        ...readiness,
      };
      const readinessReady = readiness.status === 'ready';
      const readinessAfter = activation?.getActiveDocumentReadiness?.() ?? null;
      const readinessReason =
        readinessWait.status === 'ready'
          ? 'active-document readiness resolved before provider requests'
          : `provider requests were withheld: ${String(readinessWait.reason ?? 'readiness unavailable')}`;
      const readyProviders = readinessReady
        ? {
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
          }
        : {
            completion: readinessDeferredProvider('bundled completion', readinessReason),
            hover: readinessDeferredProvider('bundled hover', readinessReason),
            definition: readinessDeferredProvider('bundled definition', readinessReason),
            references: readinessDeferredProvider('bundled references', readinessReason),
            symbols: readinessDeferredProvider('bundled symbols', readinessReason),
          };

      const editStarted = performance.now();
      const edit = new vscode.WorkspaceEdit();
      edit.insert(document.uri, new vscode.Position(document.lineCount, 0), '# packaged edit\n');
      const editApplied = await vscode.workspace.applyEdit(edit);
      const editedText = document.getText();
      const afterEdit = {
        status: editApplied && editedText.includes('# packaged edit') ? 'ok' : 'error',
        duration_ms: Math.round(performance.now() - editStarted),
        immediate_requery: readinessReady
          ? await providerResult(
              'bundled completion after edit',
              'vscode.executeCompletionItemProvider',
              document.uri,
              position,
            )
          : readinessDeferredProvider('bundled completion after edit', readinessReason),
      };

      const formatting = readinessReady
        ? await providerResult(
            'bundled formatting',
            'vscode.executeFormatDocumentProvider',
            document.uri,
            { tabSize: 4, insertSpaces: true },
          )
        : readinessDeferredProvider('bundled formatting', readinessReason);

      const renameStarted = performance.now();
      let rename: ReceiptValue;
      try {
        if (!readinessReady) {
          rename = readinessDeferredProvider('bundled rename/refusal', readinessReason);
        } else {
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
        }
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
      const finalReadiness = activation?.getActiveDocumentReadiness?.() ?? null;
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
        requests: {
          immediate: readyProviders,
          immediate_phase: readinessReady ? 'after_active_document_readiness' : 'not_proven',
          after_edit: afterEdit,
          formatting,
          rename,
        },
        index_generation: 'not_observable_from_public_extension_api',
        readiness_wait: readinessWait,
        readiness_before: readinessBefore ?? 'not_observable_from_public_extension_api',
        readiness_after: readinessAfter ?? 'not_observable_from_public_extension_api',
        index_readiness: finalReadiness ?? 'not_observable_from_public_extension_api',
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
          ...(readinessReady
            ? []
            : [
                'Active-document readiness did not resolve before provider requests; provider claims are not proven.',
              ]),
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
        ['completion', readyProviders.completion],
        ['hover', readyProviders.hover],
        ['definition', readyProviders.definition],
        ['references', readyProviders.references],
        ['symbols', readyProviders.symbols],
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

  test('starts and cleanly stops the packaged DAP on Windows', async function () {
    if (process.platform !== 'win32') {
      this.skip();
      return;
    }
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    assert.ok(workspaceFolder, 'packaged DAP journey requires a workspace folder');
    const extension = vscode.extensions.getExtension('EffortlessMetrics.perl-lsp-rs');
    assert.ok(extension, 'packaged DAP journey requires the installed extension');
    const extensionPath = extension.extensionPath;
    const dapPath = bundledDapPath(extensionPath);
    assert.ok(fs.existsSync(dapPath), `packaged DAP is missing: ${dapPath}`);
    const expectedDapSha256 = sha256(dapPath);
    const dapDirectory = path.dirname(dapPath);
    const beforeProcesses = await scanBundledDapProcessIdentities(dapDirectory);
    const beforeKeys = new Set(
      beforeProcesses.map(
        (entry) => `${entry.pid}:${entry.creationTimeFileTime ?? ''}:${entry.path.toLowerCase()}`,
      ),
    );
    const workspacePath = workspaceFolder.uri.fsPath;
    const runId = randomUUID();
    const program = path.join(workspacePath, `packaged_dap_${runId}.pl`);
    const pidFile = path.join(workspacePath, `packaged_dap_${runId}.pid`);
    const releaseFile = path.join(workspacePath, `packaged_dap_${runId}.release`);
    let debuggee: OwnedDebuggee | undefined;
    const subscriptions: vscode.Disposable[] = [];

    let startedSession: vscode.DebugSession | undefined;
    const responseOrder: string[] = [];
    const requestCommands = new Map<number, string>();
    const successfulStopResponses = new Set<string>();
    let stopRequested = false;
    const protocolTrace: Array<Record<string, unknown>> = [];
    let adapterError: string | undefined;
    let adapterExit: { code?: number; signal?: string } | undefined;
    let terminated = false;
    let resolveStarted: ((session: vscode.DebugSession) => void) | undefined;
    let resolveTerminated: (() => void) | undefined;
    let resolveResponses: (() => void) | undefined;
    let resolveExit: ((exit: { code?: number; signal?: string }) => void) | undefined;
    const started = new Promise<vscode.DebugSession>((resolve) => {
      resolveStarted = resolve;
    });
    const termination = new Promise<void>((resolve) => {
      resolveTerminated = resolve;
    });
    const responses = new Promise<void>((resolve) => {
      resolveResponses = resolve;
    });
    const exitEvent = new Promise<{ code?: number; signal?: string }>((resolve) => {
      resolveExit = resolve;
    });
    try {
      fs.writeFileSync(
        program,
        [
          'use strict;',
          'use warnings;',
          'my ($pid_file, $release_file) = @ARGV;',
          'open my $pid, q{>}, $pid_file or die "pid file: $!";',
          'print {$pid} $$; close $pid or die "pid close: $!";',
          'my $deadline = time + 120;',
          'while (!-e $release_file && time < $deadline) { select undef, undef, undef, 0.1; }',
          'open my $ended, q{>}, $pid_file or die "exit marker: $!";',
          'print {$ended} "completed-without-stop"; close $ended or die "exit marker close: $!";',
          '',
        ].join('\n'),
        { flag: 'wx' },
      );
      subscriptions.push(
        vscode.debug.onDidStartDebugSession((session) => {
          if (
            session.type === 'perl' &&
            session.configuration.program === program &&
            session.configuration.request === 'launch'
          ) {
            startedSession = session;
            resolveStarted?.(session);
          }
        }),
      );
      subscriptions.push(
        vscode.debug.onDidTerminateDebugSession((session) => {
          if (session === startedSession) {
            terminated = true;
            resolveTerminated?.();
          }
        }),
      );
      subscriptions.push(
        vscode.debug.registerDebugAdapterTrackerFactory('perl', {
          createDebugAdapterTracker: (session) => {
            if (
              session.configuration.program !== program ||
              session.configuration.request !== 'launch'
            ) {
              return undefined;
            }
            startedSession = session;
            return {
              onDidSendMessage: (message: unknown) => {
                protocolTrace.push({ direction: 'out', message });
                if (!message || typeof message !== 'object') return;
                const record = message as {
                  type?: unknown;
                  command?: unknown;
                  success?: unknown;
                  request_seq?: unknown;
                };
                if (record.type !== 'response' || record.success !== true) return;
                if (
                  record.command === 'initialize' ||
                  record.command === 'launch' ||
                  record.command === 'terminate' ||
                  record.command === 'disconnect'
                ) {
                  if (
                    typeof record.request_seq !== 'number' ||
                    requestCommands.get(record.request_seq) !== record.command
                  ) {
                    adapterError = 'DAP response did not match its live request';
                    resolveResponses?.();
                    return;
                  }
                  if (record.command === 'initialize' || record.command === 'launch') {
                    responseOrder.push(record.command);
                    if (responseOrder.length >= 2) resolveResponses?.();
                  } else if (stopRequested) {
                    successfulStopResponses.add(record.command);
                  }
                }
              },
              onError: (error: Error) => {
                adapterError = error.message;
                protocolTrace.push({ direction: 'error', message: error.message });
              },
              onExit: (code: number | undefined, signal: string | undefined) => {
                adapterExit = {
                  ...(code === undefined ? {} : { code }),
                  ...(signal === undefined ? {} : { signal }),
                };
                protocolTrace.push({ direction: 'exit', ...adapterExit });
                resolveExit?.(adapterExit);
              },
              onWillReceiveMessage: (message: unknown) => {
                protocolTrace.push({ direction: 'in', message });
                if (message && typeof message === 'object') {
                  const request = message as { type?: unknown; seq?: unknown; command?: unknown };
                  if (
                    request.type === 'request' &&
                    typeof request.seq === 'number' &&
                    typeof request.command === 'string'
                  ) {
                    requestCommands.set(request.seq, request.command);
                  }
                }
              },
            };
          },
        }),
      );
      const startResult = await withTimeout(
        'packaged DAP startDebugging',
        vscode.debug.startDebugging(workspaceFolder, {
          type: 'perl',
          request: 'launch',
          name: 'Packaged DAP startup',
          program,
          args: [pidFile, releaseFile],
          cwd: workspacePath,
          stopOnEntry: false,
        }),
        30_000,
      );
      assert.equal(startResult, true, 'VS Code did not start the packaged DAP session');
      const session = await withTimeout('packaged DAP session start', started, 30_000);
      const matchingProcess = await waitForNewPackagedDap(dapDirectory, beforeKeys, dapPath);
      assert.ok(matchingProcess.creationTimeFileTime, 'packaged DAP creation time is required');
      assert.equal(
        sha256(matchingProcess.path),
        expectedDapSha256,
        'running DAP hash differs from package',
      );
      await withTimeout('packaged DAP initialize/launch', responses, 30_000);
      assert.deepEqual(responseOrder.slice(0, 2), ['initialize', 'launch']);
      debuggee = await waitForDebuggee(pidFile);
      stopRequested = true;
      await withTimeout('packaged DAP stopDebugging', vscode.debug.stopDebugging(session), 30_000);
      await withTimeout('packaged DAP termination event', termination, 30_000);
      assert.equal(adapterError, undefined, adapterError ?? 'packaged DAP adapter error');
      const observedExit = await withTimeout('packaged DAP adapter exit', exitEvent, 30_000);
      adapterExit = observedExit;
      assert.equal(adapterError, undefined, adapterError ?? 'packaged DAP adapter error');
      assert.ok(
        successfulStopResponses.has('disconnect'),
        'ordinary Stop did not complete a correlated disconnect',
      );
      const adapterExitCode = observedExit.code;
      assert.equal(
        adapterExitCode,
        0,
        `packaged DAP exit was not clean: ${JSON.stringify(adapterExit)}`,
      );
      assert.equal(
        adapterExit.signal,
        undefined,
        `packaged DAP was signaled: ${JSON.stringify(adapterExit)}`,
      );
      assert.equal(terminated, true);
      await waitForDebuggeeExit(debuggee);
      assert.equal(
        fs.readFileSync(pidFile, 'utf8').trim(),
        String(debuggee.pid),
        'debuggee completed through its safety deadline instead of Stop',
      );
      const terminalEvents = protocolTrace.filter((entry) => {
        const message = entry.message as { type?: string; event?: string } | undefined;
        return (
          entry.direction === 'out' && message?.type === 'event' && message.event === 'terminated'
        );
      });
      assert.equal(terminalEvents.length, 1, 'expected one terminal event');
      const remainingProcesses = await scanBundledDapProcessIdentities(dapDirectory);
      assert.equal(
        remainingProcesses.filter(
          (process) =>
            !beforeKeys.has(
              `${process.pid}:${process.creationTimeFileTime ?? ''}:${process.path.toLowerCase()}`,
            ),
        ).length,
        0,
        `packaged DAP process leaked: ${JSON.stringify(remainingProcesses)}`,
      );
    } catch (error) {
      protocolTrace.push({ direction: 'failure', message: String(error) });
      throw error;
    } finally {
      const cleanupErrors: unknown[] = [];
      if (fs.existsSync(program)) {
        try {
          fs.writeFileSync(releaseFile, 'release', { flag: 'wx' });
        } catch (error) {
          cleanupErrors.push(error);
        }
      }
      if (startedSession && !terminated) {
        await withTimeout(
          'packaged DAP failure cleanup',
          vscode.debug.stopDebugging(startedSession),
          30_000,
        ).catch((error: unknown) => cleanupErrors.push(error));
        await withTimeout('packaged DAP failure termination', termination, 30_000).catch(
          (error: unknown) => cleanupErrors.push(error),
        );
      }
      if (debuggee)
        await waitForDebuggeeExit(debuggee).catch((error: unknown) => cleanupErrors.push(error));
      for (const subscription of subscriptions) subscription.dispose();
      for (const file of [program, pidFile, releaseFile]) {
        try {
          fs.rmSync(file, { force: true });
        } catch (error) {
          cleanupErrors.push(error);
        }
      }
      protocolTrace.push({ direction: 'cleanup', errors: cleanupErrors.map(String) });
      fs.mkdirSync(receiptsDir(), { recursive: true });
      fs.writeFileSync(
        path.join(receiptsDir(), 'packaged_dap_protocol_trace.json'),
        JSON.stringify(protocolTrace, null, 2),
      );
      if (cleanupErrors.length)
        throw new AggregateError(cleanupErrors, 'packaged DAP cleanup failed');
    }
    assert.ok(startedSession && adapterExit && debuggee);
    recordPackagedDapEvidence(extensionPath, dapPath, startedSession, adapterExit, debuggee);
  });
});
