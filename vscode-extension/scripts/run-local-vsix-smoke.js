#!/usr/bin/env node

const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

const root = path.resolve(__dirname, '..');
const repoRoot = path.resolve(root, '..');
const serverPath = process.env.PERL_LSP_FIRST_HOUR_SERVER_PATH;
const serverSourceRevision = (process.env.PERL_LSP_SERVER_SOURCE_SHA || '').trim();

const npmCommand = process.platform === 'win32' ? 'npm.cmd' : 'npm';

function runNpm(args, env) {
  return spawnSync(npmCommand, args, {
    cwd: root,
    env,
    stdio: 'inherit',
    shell: process.platform === 'win32',
    windowsHide: true,
  });
}

function gitRevision() {
  const result = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' });
  if (result.status !== 0) {
    throw new Error(`Unable to determine source revision: ${result.stderr || result.error}`);
  }
  return result.stdout.trim();
}

function ensureCleanWorkingTree() {
  const result = spawnSync('git', ['status', '--porcelain'], {
    cwd: root,
    encoding: 'utf8',
  });
  if (result.status !== 0) {
    throw new Error(`Unable to inspect working tree: ${result.stderr || result.error}`);
  }
  if (result.stdout.trim()) {
    throw new Error(
      'Working tree has uncommitted changes; commit them before running the local VSIX smoke test.',
    );
  }
}

function bundleTargetForPlatform(platform = process.platform, arch = process.arch) {
  const supportedPlatforms = new Set(['darwin', 'linux', 'win32']);
  const supportedArchitectures = new Set(['x64', 'arm64']);
  if (!supportedPlatforms.has(platform) || !supportedArchitectures.has(arch)) {
    throw new Error(`Unsupported packaged server platform: ${platform}-${arch}`);
  }
  return {
    directory: `${platform}-${arch}`,
    binaryName: platform === 'win32' ? 'perllsp.exe' : 'perllsp',
  };
}

function stageServerForPackage(serverPath, extensionRoot = root) {
  const { directory, binaryName } = bundleTargetForPlatform();
  const binRoot = path.join(extensionRoot, 'bin');
  const platformRoot = path.join(binRoot, directory);
  const destination = path.join(platformRoot, binaryName);
  const existing = fs.existsSync(destination) ? fs.lstatSync(destination) : null;
  if (existing && (!existing.isFile() || existing.isSymbolicLink())) {
    throw new Error(`Refusing to replace non-regular packaged server path: ${destination}`);
  }
  const previous = existing ? { bytes: fs.readFileSync(destination), mode: existing.mode } : null;
  const createdBinRoot = !fs.existsSync(binRoot);
  const createdPlatformRoot = !fs.existsSync(platformRoot);

  const restore = () => {
    if (previous) {
      fs.writeFileSync(destination, previous.bytes);
      fs.chmodSync(destination, previous.mode);
    } else {
      fs.rmSync(destination, { force: true });
    }
    if (
      createdPlatformRoot &&
      fs.existsSync(platformRoot) &&
      fs.readdirSync(platformRoot).length === 0
    ) {
      fs.rmSync(platformRoot, { recursive: true, force: true });
    }
    if (createdBinRoot && fs.existsSync(binRoot) && fs.readdirSync(binRoot).length === 0) {
      fs.rmSync(binRoot, { recursive: true, force: true });
    }
  };

  try {
    fs.mkdirSync(platformRoot, { recursive: true });
    fs.copyFileSync(serverPath, destination);
    if (process.platform !== 'win32') {
      fs.chmodSync(destination, 0o755);
    }
  } catch (error) {
    try {
      restore();
    } catch (cleanupError) {
      process.stderr.write(
        `Unable to clean up failed VSIX server staging: ${
          cleanupError instanceof Error ? cleanupError.message : String(cleanupError)
        }\n`,
      );
    }
    throw error;
  }

  return restore;
}

function sha256File(filePath) {
  return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
}

function safeFilePart(value) {
  return String(value || 'unknown').replace(/[^A-Za-z0-9_.-]+/g, '-');
}

function receiptsRoot() {
  const configured = (process.env.PERL_LSP_SMOKE_RECEIPTS_DIR || '').trim();
  return configured
    ? path.resolve(configured)
    : path.join(repoRoot, 'target', 'receipts', 'vscode-smoke');
}

function receiptPath(revision, vscodeVersion = process.env.PERL_LSP_VSCODE_VERSION) {
  return path.join(
    receiptsRoot(),
    `current-source-orchestration-${safeFilePart(vscodeVersion)}-${safeFilePart(revision)}.json`,
  );
}

function writeJsonAtomic(destination, value) {
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  const temporary = `${destination}.${process.pid}.tmp`;
  fs.writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  fs.renameSync(temporary, destination);
}

/**
 * One stage fact inside the orchestration receipt. Stages stay independent:
 * a stage carries its own status and never inherits another stage's verdict.
 *
 * @typedef {{
 *   status: string,
 *   reason?: string,
 *   exit_code?: number | null,
 *   classification?: string,
 *   behavior_safe?: boolean,
 *   transition_state?: string,
 *   violations?: string[],
 *   transition?: unknown,
 * }} SmokeStage
 */

/**
 * @typedef {{
 *   schema_version: string,
 *   receipt_kind: string,
 *   repository_sha: string,
 *   platform: string,
 *   architecture: string,
 *   vscode_version: string,
 *   source_label: string,
 *   server: { source_sha: string | null, path: string | null, sha256: string | null },
 *   vsix: { path: string | null, sha256: string | null },
 *   stages: {
 *     package_creation: SmokeStage,
 *     package_inventory: SmokeStage,
 *     behavioral_smoke: SmokeStage,
 *     activation_failure_journey: SmokeStage,
 *     crash_recovery_journey: SmokeStage,
 *   },
 *   instrument_failure: string | null,
 *   cleanup_failure: Record<string, string> | null,
 *   overall: string,
 * }} SmokeReceipt
 */

/**
 * @param {string} revision
 * @returns {SmokeReceipt}
 */
function initialReceipt(revision) {
  const serverExists = Boolean(serverPath && fs.existsSync(serverPath));
  return {
    schema_version: 'vscode_current_source_smoke.v1',
    receipt_kind: 'vscode_current_source_smoke',
    repository_sha: revision,
    platform: process.platform,
    architecture: process.arch,
    // One default across receipt and child check: the extension-host child
    // records 'stable' when the matrix version is unset, so we do too.
    vscode_version: (process.env.PERL_LSP_VSCODE_VERSION || '').trim() || 'stable',
    source_label: (process.env.PERL_LSP_SMOKE_SOURCE_LABEL || '').trim() || 'local-current-source',
    server: {
      source_sha: serverSourceRevision || null,
      path: serverPath ? path.resolve(serverPath) : null,
      sha256: serverExists ? sha256File(serverPath) : null,
    },
    vsix: {
      path: null,
      sha256: null,
    },
    stages: {
      package_creation: { status: 'not_proven', reason: 'not_started' },
      package_inventory: {
        status: 'not_proven',
        classification: 'not_proven',
        behavior_safe: false,
        reason: 'not_started',
      },
      behavioral_smoke: { status: 'not_run', reason: 'not_started' },
      activation_failure_journey: { status: 'not_run', reason: 'not_started' },
      crash_recovery_journey: { status: 'not_run', reason: 'not_started' },
    },
    instrument_failure: null,
    cleanup_failure: null,
    overall: 'not_proven',
  };
}

function shouldRunBehavioralSmoke(stages) {
  return (
    stages.package_creation.status === 'pass' &&
    stages.package_inventory.behavior_safe === true &&
    stages.package_inventory.status !== 'not_proven'
  );
}

/**
 * The packaged activation-failure journey (#7856) needs only a behavior-safe
 * package: it installs the exact VSIX into its own isolated profile and drives
 * both legs itself. It runs independently of the first-hour behavioral stage's
 * outcome so a first-hour regression does not hide activation-recovery
 * evidence (and vice versa).
 */
function shouldRunActivationFailureJourney(stages) {
  return (
    stages.package_creation.status === 'pass' &&
    stages.package_inventory.behavior_safe === true &&
    stages.package_inventory.status !== 'not_proven'
  );
}

/**
 * The packaged crash-recovery journey (#7848) needs only a behavior-safe
 * package for the same reason: it installs the exact VSIX into its own
 * isolated profile and terminates the exact server process from the harness
 * in both legs. Its verdict composes per-row results, so an honestly
 * `not_proven` watchdog row on hosts without a suspend capability degrades
 * the stage verdict without weakening the other rows.
 */
function shouldRunCrashRecoveryJourney(stages) {
  return (
    stages.package_creation.status === 'pass' &&
    stages.package_inventory.behavior_safe === true &&
    stages.package_inventory.status !== 'not_proven'
  );
}

function computeOverallStatus(stages, instrumentFailure = null, cleanupFailure = null) {
  if (Object.values(stages).some((stage) => stage.status === 'failed')) {
    return 'failed';
  }
  if (instrumentFailure || cleanupFailure) {
    return 'not_proven';
  }
  if (
    stages.package_creation.status !== 'pass' ||
    stages.package_inventory.status !== 'pass' ||
    stages.behavioral_smoke.status !== 'pass'
  ) {
    return 'not_proven';
  }
  const activationFailure = stages.activation_failure_journey;
  if (
    activationFailure &&
    activationFailure.status !== 'pass' &&
    activationFailure.status !== 'not_run'
  ) {
    return 'not_proven';
  }
  const crashRecovery = stages.crash_recovery_journey;
  if (crashRecovery && crashRecovery.status !== 'pass' && crashRecovery.status !== 'not_run') {
    return 'not_proven';
  }
  return 'pass';
}

function persistReceipt(destination, receipt) {
  receipt.overall = computeOverallStatus(
    receipt.stages,
    receipt.instrument_failure,
    receipt.cleanup_failure,
  );
  writeJsonAtomic(destination, receipt);
}

function interpretTransitionResult(
  result,
  expectedRevision,
  expectedPlatform = process.platform,
  expectedArchitecture = process.arch,
) {
  if (result.stdout) {
    process.stdout.write(result.stdout);
  }
  if (result.stderr) {
    process.stderr.write(result.stderr);
  }
  if (result.error) {
    return {
      status: 'not_proven',
      classification: 'not_proven',
      behavior_safe: false,
      reason: result.error.message,
      exit_code: null,
      violations: [],
    };
  }
  if (result.status === null) {
    return {
      status: 'not_proven',
      classification: 'not_proven',
      behavior_safe: false,
      reason: result.signal
        ? `transition process terminated by ${result.signal}`
        : 'transition process did not return a status',
      exit_code: null,
      violations: [],
    };
  }

  let report;
  try {
    report = JSON.parse(result.stdout || '');
  } catch (error) {
    return {
      status: 'not_proven',
      classification: 'not_proven',
      behavior_safe: false,
      reason: `transition output was not valid JSON: ${
        error instanceof Error ? error.message : String(error)
      }`,
      exit_code: result.status,
      violations: [],
    };
  }

  const identityViolations = [];
  if (report.schema_version !== 'vsix_inventory_transition.v1') {
    identityViolations.push('unsupported transition receipt schema');
  }
  if (report.receipt_kind !== 'vsix_inventory_transition') {
    identityViolations.push('unexpected transition receipt kind');
  }
  if (report.candidate_sha !== expectedRevision) {
    identityViolations.push('transition receipt candidate SHA does not match the smoke subject');
  }
  if (report.platform !== expectedPlatform || report.architecture !== expectedArchitecture) {
    identityViolations.push(
      'transition receipt platform/architecture does not match the smoke host',
    );
  }
  if (identityViolations.length > 0) {
    return {
      status: 'not_proven',
      classification: 'not_proven',
      behavior_safe: false,
      reason: identityViolations.join('; '),
      exit_code: result.status,
      violations: identityViolations,
      transition: report,
    };
  }

  const classification = report.package_policy_class;
  const knownClassification = ['pass', 'size_only', 'structural', 'not_proven'].includes(
    classification,
  );
  const policyViolations = Array.isArray(report.policy_violations) ? report.policy_violations : [];
  const declarationViolations = Array.isArray(report.declaration_violations)
    ? report.declaration_violations
    : [];
  const violations = [...policyViolations, ...declarationViolations];

  if (
    result.status === 0 &&
    report.passed === true &&
    report.state !== 'not_proven' &&
    classification === 'pass' &&
    report.behavior_safe === true
  ) {
    return {
      status: 'pass',
      classification,
      behavior_safe: true,
      transition_state: report.state,
      exit_code: 0,
      violations,
      transition: report,
    };
  }

  if (
    result.status === 1 &&
    report.passed === false &&
    report.state !== 'not_proven' &&
    knownClassification &&
    classification !== 'not_proven' &&
    typeof report.behavior_safe === 'boolean'
  ) {
    return {
      status: 'failed',
      classification,
      behavior_safe: report.behavior_safe,
      transition_state: report.state,
      exit_code: 1,
      violations,
      transition: report,
    };
  }

  if (
    result.status === 2 &&
    report.state === 'not_proven' &&
    report.passed === false &&
    classification === 'not_proven' &&
    report.behavior_safe === false
  ) {
    return {
      status: 'not_proven',
      classification,
      behavior_safe: false,
      transition_state: report.state,
      reason: report.reason || 'transition instrument did not prove package state',
      exit_code: 2,
      violations,
      transition: report,
    };
  }

  return {
    status: 'not_proven',
    classification: 'not_proven',
    behavior_safe: false,
    reason: `transition exit ${result.status} contradicted state ${String(
      report.state,
    )}, classification ${String(classification)}, and passed=${String(report.passed)}`,
    exit_code: result.status,
    violations,
    transition: report,
  };
}

function runInventoryTransition(env, expectedRevision, vsixPath) {
  const scriptPath = path.join(__dirname, 'check-vsix-inventory-transition.js');
  const args = [scriptPath, '--vsix', vsixPath];
  const explicitBase = (env.PERL_LSP_PACKAGE_BASE_SHA || '').trim();
  if (explicitBase) {
    args.push('--base', explicitBase);
  }
  const result = spawnSync(process.execPath, args, {
    cwd: root,
    env,
    encoding: 'utf8',
    windowsHide: true,
  });
  return interpretTransitionResult(result, expectedRevision);
}

const CHILD_RECEIPT_NAME = 'first_hour_vscode_receipt.json';

function smokeSourceLabel() {
  return (process.env.PERL_LSP_SMOKE_SOURCE_LABEL || '').trim() || 'local-current-source';
}

function smokePlatformLabel() {
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

function childReceiptPath() {
  // The extension-host child nests its receipt by source label and platform
  // (receiptsDir() in src/test/integration/firstHourReceipt.test.ts); the
  // orchestrator must read and clear exactly where the child writes.
  return path.join(receiptsRoot(), smokeSourceLabel(), smokePlatformLabel(), CHILD_RECEIPT_NAME);
}

/**
 * A zero exit code from the extension-host smoke is not behavioral proof: a
 * no-op script, a swallowed receipt write, or a receipt left behind by an
 * earlier run would all produce it. Behavior only counts when the child wrote
 * a fresh receipt that binds itself to this candidate, this VSIX, this server,
 * and this matrix leg.
 *
 * @param {{
 *   receiptFile: string,
 *   expectedRevision: string,
 *   expectedVsixSha256: string | null,
 *   expectedServerSourceSha: string,
 *   expectedVscodeVersion: string,
 *   expectedSourceLabel: string,
 *   readFile?: (file: string) => string,
 *   exists?: (file: string) => boolean,
 * }} input
 * @returns {{ ok: true, receipt: any } | { ok: false, violations: string[] }}
 */
function validateChildSmokeReceipt({
  receiptFile,
  expectedRevision,
  expectedVsixSha256,
  expectedServerSourceSha,
  expectedVscodeVersion,
  expectedSourceLabel,
  readFile = (file) => fs.readFileSync(file, 'utf8'),
  exists = (file) => fs.existsSync(file),
}) {
  if (!exists(receiptFile)) {
    return { ok: false, violations: ['extension-host smoke did not write a first-hour receipt'] };
  }

  let receipt;
  try {
    receipt = JSON.parse(readFile(receiptFile));
  } catch (error) {
    return {
      ok: false,
      violations: [
        `first-hour receipt was not valid JSON: ${
          error instanceof Error ? error.message : String(error)
        }`,
      ],
    };
  }

  const violations = [];
  const environment =
    receipt &&
    typeof receipt === 'object' &&
    receipt.environment &&
    typeof receipt.environment === 'object'
      ? receipt.environment
      : null;
  if (!environment) {
    return { ok: false, violations: ['first-hour receipt is missing its environment identity'] };
  }

  if (receipt.outcome !== 'completed') {
    violations.push(
      `first-hour receipt outcome is ${JSON.stringify(receipt.outcome)}, not completed`,
    );
  }
  if (!Array.isArray(receipt.failures) || receipt.failures.length > 0) {
    violations.push('first-hour receipt reported failures');
  }
  if (environment.source_revision !== expectedRevision) {
    violations.push(
      `first-hour receipt source revision ${JSON.stringify(environment.source_revision)} is not the smoke subject`,
    );
  }
  if (environment.server_source_revision !== expectedServerSourceSha) {
    violations.push(
      `first-hour receipt server source revision ${JSON.stringify(environment.server_source_revision)} is not the staged server`,
    );
  }
  if (!expectedVsixSha256 || environment.vsix_sha256 !== expectedVsixSha256) {
    violations.push(
      `first-hour receipt VSIX digest ${JSON.stringify(environment.vsix_sha256)} is not the package this run created`,
    );
  }
  if (environment.requested_vscode_version !== expectedVscodeVersion) {
    violations.push(
      `first-hour receipt VS Code version ${JSON.stringify(environment.requested_vscode_version)} is not this matrix leg`,
    );
  }
  if (environment.extension_id !== 'EffortlessMetrics.perl-lsp-rs') {
    violations.push(
      `first-hour receipt extension id ${JSON.stringify(environment.extension_id)} is not the packaged extension`,
    );
  }
  if (expectedSourceLabel && receipt.source_label && receipt.source_label !== expectedSourceLabel) {
    violations.push(
      `first-hour receipt source label ${JSON.stringify(receipt.source_label)} is not this run's label`,
    );
  }

  return violations.length > 0 ? { ok: false, violations } : { ok: true, receipt };
}

/** Must match `HOST_RESOLUTION_FAILURE_RECEIPT_NAME` in vscodeHostResolution.ts. */
const HOST_RESOLUTION_FAILURE_RECEIPT = 'vscode_host_resolution_failure.json';

function hostResolutionFailurePath(root = receiptsRoot()) {
  return path.join(root, HOST_RESOLUTION_FAILURE_RECEIPT);
}

/**
 * @param {string} [root]
 * @param {{
 *   exists?: ((file: string) => boolean) | undefined,
 *   readFile?: ((file: string) => string) | undefined,
 * }} [io]
 * @returns {{ kind: 'absent' } | { kind: 'invalid' } | { kind: 'present', receipt: Record<string, unknown> }}
 */
function readHostResolutionFailureReceipt(
  root = receiptsRoot(),
  {
    exists = (file) => fs.existsSync(file),
    readFile = (file) => fs.readFileSync(file, 'utf8'),
  } = {},
) {
  const receiptFile = hostResolutionFailurePath(root);
  if (!exists(receiptFile)) {
    return { kind: 'absent' };
  }
  try {
    const receipt = JSON.parse(readFile(receiptFile));
    if (receipt && typeof receipt === 'object') {
      return { kind: 'present', receipt: /** @type {Record<string, unknown>} */ (receipt) };
    }
  } catch {
    // Invalid JSON is still a host-resolution boundary: do not relabel as product smoke.
  }
  return { kind: 'invalid' };
}

/**
 * A failed VS Code host-version resolution is not a product smoke failure.
 * The structured receipt is the visible boundary; `published_extension_smoke_failed`
 * is reserved for journeys that actually reached the extension host.
 *
 * @param {{
 *   status?: number | null,
 *   spawnError?: Error | undefined,
 *   receiptsRoot?: string,
 *   exists?: ((file: string) => boolean) | undefined,
 *   readFile?: ((file: string) => string) | undefined,
 * }} input
 * @returns {{
 *   status: string,
 *   exit_code: number | null,
 *   reason: string,
 *   host_resolution?: Record<string, unknown>,
 * }}
 */
function interpretBehavioralSmokeExit({
  status = null,
  spawnError,
  receiptsRoot: root = receiptsRoot(),
  exists,
  readFile,
}) {
  const hostFailure = readHostResolutionFailureReceipt(root, { exists, readFile });
  if (hostFailure.kind === 'present') {
    const rawDisposition = hostFailure.receipt.disposition;
    const disposition =
      rawDisposition === 'unavailable' ||
      rawDisposition === 'network' ||
      rawDisposition === 'cache' ||
      rawDisposition === 'runner'
        ? rawDisposition
        : 'runner';
    return {
      status: disposition === 'unavailable' ? 'not_proven' : 'failed',
      exit_code: status ?? null,
      reason: `vscode_host_resolution_${disposition}`,
      host_resolution: hostFailure.receipt,
    };
  }
  if (hostFailure.kind === 'invalid') {
    return {
      status: 'not_proven',
      exit_code: status ?? null,
      reason: 'vscode_host_resolution_receipt_invalid',
    };
  }
  if (spawnError) {
    return {
      status: 'not_proven',
      exit_code: null,
      reason: spawnError.message,
    };
  }
  return {
    status: 'failed',
    exit_code: status ?? null,
    reason: 'published_extension_smoke_failed',
  };
}

function exitCodeFor(overall) {
  if (overall === 'pass') {
    return 0;
  }
  return overall === 'failed' ? 1 : 2;
}

/**
 * Stage-truth projection onto the GitHub check surface (#6883).
 *
 * The receipt already keeps package creation, package inventory, and installed
 * behaviour as separate typed facts, but that evidence only exists inside an
 * uploaded artifact. A reviewer reading the check itself sees one aggregate
 * colour, so a blocking package-inventory transition still reads as though the
 * behavioural smoke failed — including when the behavioural smoke passed, or
 * never ran at all.
 *
 * Everything below is presentation derived from the finished receipt. It never
 * decides a stage verdict, never changes the aggregate, and never changes the
 * process exit code; `composeCheckSummary` is pure so the wording it produces
 * can be proven directly against a receipt.
 */
const CHECK_STAGE_ORDER = [
  'package_creation',
  'package_inventory',
  'behavioral_smoke',
  'activation_failure_journey',
  'crash_recovery_journey',
];

const CHECK_STAGE_LABELS = {
  package_creation: 'package creation',
  package_inventory: 'package inventory',
  behavioral_smoke: 'behavioral smoke',
  activation_failure_journey: 'activation-failure journey',
  crash_recovery_journey: 'crash-recovery journey',
};

/** The receipt's typed stage vocabulary, rendered as English. */
const CHECK_VERDICT_WORDS = {
  pass: 'passed',
  failed: 'failed',
  not_run: 'not run',
  not_proven: 'not proven',
};

/** Stages whose verdict describes the package rather than installed behaviour. */
const CHECK_PACKAGE_STAGES = ['package_creation', 'package_inventory'];

/**
 * Packaged journeys that can decide the aggregate on their own.
 *
 * `computeOverallStatus` degrades the run when either is failed or not proven,
 * so the headline has to be able to name them: a run whose only defect is a
 * recovery journey would otherwise read entirely green on a red check, which
 * is the misreading this projection exists to remove.
 */
const CHECK_JOURNEY_STAGES = ['activation_failure_journey', 'crash_recovery_journey'];

function checkVerdictWord(status) {
  return CHECK_VERDICT_WORDS[status] ?? String(status);
}

/**
 * Every fact this projection quotes is one line.
 *
 * Receipt text is not authored here — it carries subprocess stderr, file paths,
 * and error messages — so a line break inside it would otherwise let a stage
 * reason open a heading, a list, or a second table in the job summary, above
 * the authoritative one. Normalizing at the single point where receipt strings
 * enter the projection keeps every downstream surface (headline, annotations,
 * table cells, remaining-proof bullets) structurally safe by construction.
 */
function singleLine(value) {
  return String(value).replace(/\s+/g, ' ').trim();
}

/**
 * Why a stage reached its verdict, in one line, without repeating the verdict.
 *
 * @param {SmokeStage | undefined} stage
 */
function checkStageDetail(stage) {
  if (!stage) {
    return 'stage absent from the receipt';
  }
  const parts = [];
  if (stage.classification && stage.classification !== stage.status) {
    parts.push(singleLine(stage.classification));
  }
  if (stage.transition_state) {
    parts.push(singleLine(stage.transition_state));
  }
  if (stage.reason) {
    parts.push(singleLine(stage.reason));
  }
  if (typeof stage.exit_code === 'number') {
    parts.push(`exit ${stage.exit_code}`);
  }
  for (const violation of Array.isArray(stage.violations) ? stage.violations : []) {
    parts.push(singleLine(violation));
  }
  // The stage label and verdict already carry the fact; a stage that recorded
  // nothing further gets a placeholder rather than invented prose.
  return parts.length > 0 ? parts.join('; ') : '—';
}

/**
 * The package half of the headline: which package proposition actually
 * rejected, or that neither did.
 */
function checkPackagePhrase(stages) {
  const rejected = [];
  for (const key of CHECK_PACKAGE_STAGES) {
    const stage = stages[key];
    if (!stage) {
      // A missing stage is reported as missing, never quietly as passing.
      rejected.push(`${CHECK_STAGE_LABELS[key]} absent from the receipt`);
    } else if (stage.status !== 'pass') {
      rejected.push(`${CHECK_STAGE_LABELS[key]} ${checkVerdictWord(stage.status)}`);
    }
  }
  return rejected.length > 0
    ? rejected.join(' and ')
    : 'package creation and package inventory passed';
}

/**
 * The behavioural half of the headline. `not_run` and `not_proven` carry their
 * reason, because "did not run" and "ran and failed" are the two facts this
 * projection exists to keep apart.
 *
 * `pass` and `failed` deliberately stop at the verdict, unlike the journey
 * segments below: this stage's verdict is itself the triage surface, and its
 * failure reason is a restatement of it (`published_extension_smoke_failed`).
 * The table row carries the full `checkStageDetail` either way, so the
 * asymmetry costs no evidence — do not "fix" it into a journey-style line.
 */
function checkBehavioralPhrase(stage) {
  const label = CHECK_STAGE_LABELS.behavioral_smoke;
  if (!stage) {
    return `${label} absent from the receipt`;
  }
  const phrase = `${label} ${checkVerdictWord(stage.status)}`;
  if (stage.status === 'pass' || stage.status === 'failed' || !stage.reason) {
    return phrase;
  }
  return `${phrase}: ${singleLine(stage.reason)}`;
}

/**
 * One sentence naming the proposition that actually decided the run. This is
 * the line a reviewer reads instead of "Current-source Linux smoke failed".
 */
function checkHeadline(receipt) {
  const stages = receipt.stages ?? {};
  const segments = [checkPackagePhrase(stages), checkBehavioralPhrase(stages.behavioral_smoke)];
  // A journey that did not run is already explained by the package phrase that
  // declined it; one that reached a non-passing verdict decided this run.
  for (const key of CHECK_JOURNEY_STAGES) {
    const stage = stages[key];
    if (!stage || stage.status === 'pass' || stage.status === 'not_run') {
      continue;
    }
    const phrase = `${CHECK_STAGE_LABELS[key]} ${checkVerdictWord(stage.status)}`;
    segments.push(stage.reason ? `${phrase}: ${singleLine(stage.reason)}` : phrase);
  }
  if (receipt.instrument_failure) {
    segments.push(`smoke instrument failed: ${singleLine(receipt.instrument_failure)}`);
  }
  // An empty object is not a cleanup failure: naming one with nothing after the
  // colon would assert a failure the receipt does not record.
  const cleanupFailures = Object.keys(receipt.cleanup_failure ?? {}).sort();
  if (cleanupFailures.length > 0) {
    segments.push(`cleanup failed: ${cleanupFailures.join(', ')}`);
  }
  return segments.join('; ');
}

/** Stages that carry no verdict yet, so the summary can say what is still owed. */
function checkRemainingProof(stages) {
  return CHECK_STAGE_ORDER.filter((key) => {
    const status = stages[key]?.status;
    return status === 'not_run' || status === 'not_proven';
  }).map(
    (key) => `${CHECK_STAGE_LABELS[key]} (${stages[key].status}): ${checkStageDetail(stages[key])}`,
  );
}

/** Workflow-command data escaping, per GitHub's documented encoding. */
function escapeAnnotationData(value) {
  return String(value).replace(/%/g, '%25').replace(/\r/g, '%0D').replace(/\n/g, '%0A');
}

function escapeAnnotationProperty(value) {
  return escapeAnnotationData(value).replace(/:/g, '%3A').replace(/,/g, '%2C');
}

/** Keep a cell inside its row: a pipe or newline would otherwise reshape the table. */
function markdownCell(value) {
  return String(value)
    .replace(/\|/g, '\\|')
    .replace(/[\r\n]+/g, ' ');
}

/**
 * Compose the check-surface projection of a finished receipt.
 *
 * @param {SmokeReceipt} receipt
 * @returns {{headline: string, markdown: string, annotations: string[]}}
 */
function composeCheckSummary(receipt) {
  const stages = receipt.stages ?? {};
  const headline = checkHeadline(receipt);
  const overall = receipt.overall ?? 'not_proven';
  const present = CHECK_STAGE_ORDER.filter((key) => stages[key]);

  const annotations = [
    `::notice title=VS Code current-source smoke::${escapeAnnotationData(headline)}`,
  ];
  for (const key of present) {
    const stage = stages[key];
    if (stage.status === 'pass' || stage.status === 'not_run') {
      continue;
    }
    const level = stage.status === 'failed' ? 'error' : 'warning';
    const title = `${CHECK_STAGE_LABELS[key]} ${checkVerdictWord(stage.status)}`;
    annotations.push(
      `::${level} title=${escapeAnnotationProperty(title)}::${escapeAnnotationData(checkStageDetail(stage))}`,
    );
  }

  const rows = present.map(
    (key) =>
      `| ${markdownCell(CHECK_STAGE_LABELS[key])} | \`${markdownCell(stages[key].status)}\` | ${markdownCell(checkStageDetail(stages[key]))} |`,
  );
  const remaining = checkRemainingProof(stages);
  const lines = [
    `### VS Code current-source smoke — ${receipt.vscode_version ?? 'unknown'}`,
    '',
    headline,
    '',
    `Aggregate: \`${overall}\` · subject \`${receipt.repository_sha ?? 'unknown'}\``,
    '',
    '| stage | result | detail |',
    '| --- | --- | --- |',
    ...rows,
    '',
  ];
  if (remaining.length > 0) {
    lines.push('Remaining proof:', '', ...remaining.map((entry) => `- ${entry}`), '');
  }
  lines.push(
    'Stage results are independent: a blocking package result does not assert anything about installed behavior, and vice versa.',
    '',
  );

  return { headline, markdown: `${lines.join('\n')}\n`, annotations };
}

/**
 * Emit the projection to the live check surface.
 *
 * Presentation must not be able to change a verdict, so a failure to write the
 * job summary is reported and then dropped: the receipt remains the evidence,
 * and a summary-write error must never turn a proven run red or manufacture an
 * instrument failure.
 */
function publishCheckSummary(receipt, options = {}) {
  const {
    summaryPath = (process.env.GITHUB_STEP_SUMMARY || '').trim(),
    appendSummary = (target, text) => fs.appendFileSync(target, text),
    writeAnnotation = (line) => process.stdout.write(`${line}\n`),
    writeDiagnostic = (line) => process.stderr.write(`${line}\n`),
  } = options;

  const summary = composeCheckSummary(receipt);
  // The two channels fail independently: a closed stdout (EPIPE) must not also
  // cost the job summary, which writes to a different destination entirely.
  for (const annotation of summary.annotations) {
    try {
      writeAnnotation(annotation);
    } catch (error) {
      writeDiagnostic(
        `Unable to emit a smoke stage annotation: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }
  if (summaryPath) {
    try {
      appendSummary(summaryPath, summary.markdown);
    } catch (error) {
      writeDiagnostic(
        `Unable to append the smoke stage summary: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }
  return summary;
}

/**
 * The single terminal join: publish the stage projection, then return the
 * aggregate exit code the receipt already decided.
 *
 * Publishing is contained: presentation is the last thing a run does, and a
 * defect in it must not be able to convert a decided aggregate into an
 * uncaught exception. The receipt is already persisted, so a lost summary
 * costs readability, never evidence.
 *
 * @param {SmokeReceipt} receipt
 * @param {number} [exitCode] the code a completed run already finalized
 * @param {(receipt: SmokeReceipt) => unknown} [publish]
 */
function concludeRun(
  receipt,
  exitCode = exitCodeFor(receipt.overall),
  publish = publishCheckSummary,
) {
  try {
    publish(receipt);
  } catch (error) {
    // Direct stderr on purpose, unlike `publishCheckSummary`'s injectable
    // `writeDiagnostic`: the publisher that just threw may be the very thing
    // that closed or replaced the injectable channel, so the last-resort report
    // reaches for a fresh one. Do not "consistency-fix" this back to a
    // callback.
    try {
      process.stderr.write(
        `Unable to publish the smoke stage summary: ${error instanceof Error ? error.message : String(error)}\n`,
      );
    } catch {
      // A failsafe that can throw is not one. If even stderr is gone there is
      // nothing left to report through, and the exit code the receipt already
      // decided still has to survive; the persisted receipt remains the
      // evidence.
    }
  }
  return exitCode;
}

const ACTIVATION_FAILURE_LEG_SCHEMA = 'vscode_activation_recovery_leg.v1';
const ACTIVATION_FAILURE_RECEIPTS = {
  failure: 'activation_failure_journey_failure_receipt.json',
  retry: 'activation_failure_journey_retry_receipt.json',
  joined: 'vscode_activation_recovery_receipt.json',
};

const CRASH_RECOVERY_LEG_SCHEMA = 'vscode_crash_recovery_leg.v1';
const CRASH_RECOVERY_RECEIPTS = {
  transient: 'crash_recovery_journey_transient_receipt.json',
  breaker: 'crash_recovery_journey_breaker_receipt.json',
  joined: 'vscode_crash_recovery_receipt.json',
};

/**
 * Validate both activation-failure journey child receipts against this run's
 * exact candidate identity. A zero leg exit code is not proof by itself: the
 * children must have written fresh, candidate-bound receipts with passing
 * verdicts, exactly like the first-hour behavioral child.
 *
 * @param {{
 *   failureReceiptFile: string,
 *   retryReceiptFile: string,
 *   expectedVsixSha256: string,
 *   expectedBundledServerSha256: string,
 *   expectedExtensionVersion: string,
 *   readFile?: (file: string) => string,
 *   exists?: (file: string) => boolean,
 * }} input
 * @returns {{ ok: boolean, violations: string[], failure: any | null, retry: any | null }}
 */
function validateActivationRecoveryChildReceipts({
  failureReceiptFile,
  retryReceiptFile,
  expectedVsixSha256,
  expectedBundledServerSha256,
  expectedExtensionVersion,
  readFile = (file) => fs.readFileSync(file, 'utf8'),
  exists = (file) => fs.existsSync(file),
}) {
  const violations = [];
  const readChild = (file, leg) => {
    if (!exists(file)) {
      violations.push(`the ${leg} leg did not write its journey receipt`);
      return null;
    }
    try {
      return JSON.parse(readFile(file));
    } catch (error) {
      violations.push(
        `the ${leg} leg receipt was not valid JSON: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
      return null;
    }
  };
  const failure = readChild(failureReceiptFile, 'failure');
  const retry = readChild(retryReceiptFile, 'retry');
  const bindChild = (receipt, leg) => {
    if (!receipt) {
      return;
    }
    if (receipt.schema_version !== ACTIVATION_FAILURE_LEG_SCHEMA) {
      violations.push(`the ${leg} leg receipt schema is ${JSON.stringify(receipt.schema_version)}`);
    }
    if (receipt.leg !== leg) {
      violations.push(`the ${leg} leg receipt records leg ${JSON.stringify(receipt.leg)}`);
    }
    if (receipt.verdict !== 'pass') {
      violations.push(`the ${leg} leg verdict is ${JSON.stringify(receipt.verdict)}`);
    }
    const candidate = receipt.candidate || {};
    if (candidate.vsix_sha256 !== expectedVsixSha256) {
      violations.push(
        `the ${leg} leg receipt VSIX digest ${JSON.stringify(candidate.vsix_sha256)} is not this run's package`,
      );
    }
    if (
      !candidate.bundled_server ||
      candidate.bundled_server.sha256 !== expectedBundledServerSha256
    ) {
      violations.push(
        `the ${leg} leg receipt bundled-server digest is not this run's staged server`,
      );
    }
    if (candidate.extension_version !== expectedExtensionVersion) {
      violations.push(
        `the ${leg} leg receipt extension version ${JSON.stringify(candidate.extension_version)} is not the packaged version`,
      );
    }
  };
  bindChild(failure, 'failure');
  bindChild(retry, 'retry');
  return { ok: violations.length === 0, violations, failure, retry };
}

/**
 * Validate both crash-recovery journey child receipts against this run's
 * exact candidate identity (#7848). Exactly like the activation-failure
 * journey, a zero leg exit code is not proof: the children must have written
 * fresh, candidate-bound receipts, and the joined rows are composed from the
 * children's observations, never from exit codes alone.
 *
 * @param {{
 *   transientReceiptFile: string,
 *   breakerReceiptFile: string,
 *   expectedVsixSha256: string,
 *   expectedBundledServerSha256: string,
 *   expectedExtensionVersion: string,
 *   readFile?: (file: string) => string,
 *   exists?: (file: string) => boolean,
 * }} input
 * @returns {{
 *   ok: boolean,
 *   violations: string[],
 *   transient: any | null,
 *   breaker: any | null,
 * }}
 */
function validateCrashRecoveryChildReceipts({
  transientReceiptFile,
  breakerReceiptFile,
  expectedVsixSha256,
  expectedBundledServerSha256,
  expectedExtensionVersion,
  readFile = (file) => fs.readFileSync(file, 'utf8'),
  exists = (file) => fs.existsSync(file),
}) {
  const violations = [];
  const readChild = (file, leg) => {
    if (!exists(file)) {
      violations.push(`the ${leg} leg did not write its journey receipt`);
      return null;
    }
    try {
      return JSON.parse(readFile(file));
    } catch (error) {
      violations.push(
        `the ${leg} leg receipt was not valid JSON: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
      return null;
    }
  };
  const transient = readChild(transientReceiptFile, 'transient');
  const breaker = readChild(breakerReceiptFile, 'breaker');
  const bindChild = (receipt, leg) => {
    if (!receipt) {
      return;
    }
    if (receipt.schema_version !== CRASH_RECOVERY_LEG_SCHEMA) {
      violations.push(`the ${leg} leg receipt schema is ${JSON.stringify(receipt.schema_version)}`);
    }
    if (receipt.leg !== leg) {
      violations.push(`the ${leg} leg receipt records leg ${JSON.stringify(receipt.leg)}`);
    }
    const candidate = receipt.candidate || {};
    if (candidate.vsix_sha256 !== expectedVsixSha256) {
      violations.push(
        `the ${leg} leg receipt VSIX digest ${JSON.stringify(
          candidate.vsix_sha256,
        )} is not this run's package`,
      );
    }
    if (
      !candidate.bundled_server ||
      candidate.bundled_server.sha256 !== expectedBundledServerSha256
    ) {
      violations.push(
        `the ${leg} leg receipt bundled-server digest is not this run's staged server`,
      );
    }
    if (candidate.extension_version !== expectedExtensionVersion) {
      violations.push(
        `the ${leg} leg receipt extension version ${JSON.stringify(
          candidate.extension_version,
        )} is not the packaged version`,
      );
    }
    // The crash must be an unexpected external termination, never the
    // extension's user restart command (#7848 negative control).
    const fault = receipt.fault;
    if (!fault || typeof fault.method !== 'string' || !/harness-external/.test(fault.method)) {
      violations.push(
        `the ${leg} leg receipt does not record a harness-external process termination as the failure injection`,
      );
    }
  };
  bindChild(transient, 'transient');
  bindChild(breaker, 'breaker');
  return { ok: violations.length === 0, violations, transient, breaker };
}

/**
 * Derive one pass|failed|not_proven row from a child observation. An observed
 * product failure is `failed`; missing evidence is `not_proven`; a passing
 * child observation only counts when its leg also exited cleanly.
 */
function crashRowFromObservation(value, legExitCode, isPass) {
  if (value === undefined || value === null) {
    return 'not_proven';
  }
  if (!isPass(value)) {
    return 'failed';
  }
  return legExitCode === 0 ? 'pass' : 'not_proven';
}

/**
 * Compose the joined `vscode_crash_recovery.v1` receipt (#7848) from the two
 * child legs and the orchestrator's own post-host-exit process scan. The
 * verdict is fail-closed: any missing or contradictory child evidence leaves
 * the affected row `not_proven`, an observed product failure fails its row
 * outright, and any failed row fails the receipt while an honestly
 * `not_proven` row (for example the watchdog row on hosts that cannot suspend
 * a process) keeps the overall verdict `not_proven` without weakening the
 * other rows.
 *
 * @param {{
 *   vsixSha256: string,
 *   extensionVersion: string,
 *   bundledServerSha256: string,
 *   serverSourceRevision: string | null,
 *   repositorySha: string,
 *   vscodeVersion: string,
 *   transient: any | null,
 *   breaker: any | null,
 *   violations: string[],
 *   legExitCodes: { transient: number | null, breaker: number | null },
 *   postHostExitProcesses: string[],
 * }} input
 */
function composeCrashRecoveryReceipt({
  vsixSha256,
  extensionVersion,
  bundledServerSha256,
  serverSourceRevision,
  repositorySha,
  vscodeVersion,
  transient,
  breaker,
  violations,
  legExitCodes,
  postHostExitProcesses,
}) {
  const transientObservations = (transient && transient.observations) || {};
  const breakerObservations = (breaker && breaker.observations) || {};
  const childrenBound = violations.length === 0 && transient && breaker;
  const breakerBound = childrenBound;
  // Every observation-derived row is gated on binding: a receipt that failed
  // schema/digest/version/leg validation is not this candidate's evidence,
  // and its observations must leave the affected rows `not_proven` rather
  // than turning an unbound observation into a product-regression `failed`.
  const boundRow = (row) => (childrenBound ? row : 'not_proven');

  const isReplayRowPass = (value) =>
    typeof value === 'object' &&
    value !== null &&
    Object.keys(value).length > 0 &&
    Object.values(value).every((row) => row === 'ready_in_replacement_generation');
  const replayRow = boundRow(
    crashRowFromObservation(transientObservations.replay, legExitCodes.transient, isReplayRowPass),
  );

  const providerAfter = transientObservations.provider_after_recovery;
  const isProviderRowPass = (value) =>
    value && typeof value === 'object' && value.provider && value.provider.status === 'ok';
  const providerRow = boundRow(
    crashRowFromObservation(providerAfter, legExitCodes.transient, isProviderRowPass),
  );

  const episodes = Array.isArray(breakerObservations.episodes) ? breakerObservations.episodes : [];
  const automaticRestarts = episodes.filter((episode) => episode && episode.replacement_pid);
  const exhausted = breakerObservations.exhausted === true;
  const finalEpisode = episodes.length > 0 ? episodes[episodes.length - 1] : null;
  const backgroundAfterExhaustion =
    finalEpisode && Array.isArray(finalEpisode.background_server_processes)
      ? finalEpisode.background_server_processes.length > 0
      : null;
  const isExplicitRetryPass = (value) =>
    value &&
    typeof value === 'object' &&
    value.binary_resolution_source_after === 'bundled' &&
    value.readiness === 'ready_in_retry_generation' &&
    value.provider &&
    value.provider.status === 'ok';
  const explicitRetryRow = boundRow(
    crashRowFromObservation(
      breakerObservations.explicit_retry,
      legExitCodes.breaker,
      isExplicitRetryPass,
    ),
  );

  const watchdogStatus =
    transientObservations.watchdog && typeof transientObservations.watchdog.status === 'string'
      ? transientObservations.watchdog.status
      : 'not_proven';
  const watchdogRow = boundRow(
    ['pass', 'failed', 'not_proven'].includes(watchdogStatus) ? watchdogStatus : 'not_proven',
  );

  const legsExitedCleanly = legExitCodes.transient === 0 && legExitCodes.breaker === 0;
  const hostExitClean = postHostExitProcesses.length === 0;
  const observedChildFailure =
    childrenBound &&
    ((transient && transient.verdict === 'failed') || (breaker && breaker.verdict === 'failed'));
  let cleanupRow;
  if (!childrenBound) {
    cleanupRow = 'not_proven';
  } else if (hostExitClean === false) {
    cleanupRow = 'failed';
  } else if (!legsExitedCleanly) {
    cleanupRow = 'not_proven';
  } else {
    cleanupRow = 'pass';
  }

  const circuitBreaker = {
    attempts: childrenBound ? automaticRestarts.length : null,
    exhausted: breakerBound ? exhausted : null,
    background_restart_after_exhaustion: backgroundAfterExhaustion,
    explicit_retry: explicitRetryRow,
    budget:
      breakerBound && typeof breakerObservations.automatic_budget === 'number'
        ? breakerObservations.automatic_budget
        : null,
    action_required_dialog_observable:
      breakerBound && breakerObservations.action_required_dialog?.observable === true,
  };

  const exhaustedRow = !breakerBound
    ? 'not_proven'
    : crashRowFromObservation(
        breakerObservations.exhausted,
        legExitCodes.breaker,
        (value) => value === true,
      );
  const backgroundRow = !breakerBound
    ? 'not_proven'
    : crashRowFromObservation(
        backgroundAfterExhaustion,
        legExitCodes.breaker,
        (value) => value === false,
      );

  const negativeControls = {
    user_restart_not_used_for_failure_injection: childrenBound
      ? typeof transient.fault?.method === 'string' &&
        /harness-external process termination/.test(transient.fault.method) &&
        typeof breaker.fault?.method === 'string' &&
        /harness-external/.test(breaker.fault.method)
      : null,
    replacement_servers_never_overlapped: childrenBound
      ? Number(transientObservations.recovery_samples?.max_simultaneous_server_processes ?? 0) <=
          1 && episodes.every((episode) => (episode?.max_simultaneous_server_processes ?? 0) <= 1)
      : null,
    budget_exhaustion_spawned_no_background_server: childrenBound
      ? backgroundAfterExhaustion === false
      : null,
    explicit_retry_did_not_substitute_binary_source: childrenBound
      ? breakerObservations.explicit_retry?.binary_resolution_source_after === 'bundled'
      : null,
    failed_process_not_resurrected: childrenBound
      ? transientObservations.quiet_window?.failed_pid_resurrected === false
      : null,
  };

  const rows = [
    replayRow,
    providerRow,
    exhaustedRow,
    backgroundRow,
    explicitRetryRow,
    watchdogRow,
    cleanupRow,
  ];
  let verdict;
  if (observedChildFailure || rows.includes('failed')) {
    verdict = 'failed';
  } else if (rows.includes('not_proven')) {
    verdict = 'not_proven';
  } else {
    verdict = 'pass';
  }

  return {
    schema_version: 'vscode_crash_recovery.v1',
    receipt_kind: 'vscode_crash_recovery',
    repository_sha: repositorySha,
    candidate: {
      extension_id: 'EffortlessMetrics.perl-lsp-rs',
      extension_version: extensionVersion,
      vsix_sha256: vsixSha256,
      bundled_server_sha256: bundledServerSha256,
      server_source_sha: serverSourceRevision || null,
      vscode_version: vscodeVersion,
      platform: process.platform,
      architecture: process.arch,
      arbiter_contract: 'CrashRecoveryArbiter (#7845, 6fb693c64a)',
      inventory_baseline_binding: 'stages.package_inventory (vsix_inventory_transition.v1)',
    },
    host: {
      repository_sha: repositorySha,
      vscode_version: vscodeVersion,
      platform: process.platform,
      architecture: process.arch,
      source_label: smokeSourceLabel(),
      failure_injection: 'harness-external process termination (taskkill /F or SIGKILL)',
    },
    transient_crash: {
      failed_generation: childrenBound ? (transientObservations.failed_generation ?? null) : null,
      replacement_generation: childrenBound
        ? (transientObservations.replacement_generation ?? null)
        : null,
      replay: replayRow,
      provider_after_recovery: providerRow,
    },
    circuit_breaker: circuitBreaker,
    watchdog: watchdogRow,
    negative_controls: negativeControls,
    cleanup: cleanupRow,
    legs: {
      transient_exit_code: legExitCodes.transient,
      breaker_exit_code: legExitCodes.breaker,
      transient_receipt: transient,
      breaker_receipt: breaker,
    },
    orchestrator_observations: {
      post_host_exit_bundled_server_processes: postHostExitProcesses,
    },
    instrument_violations: violations,
    verdict,
  };
}

/**
 * Compose the joined `vscode_activation_recovery.v1` receipt (#7856) from the
 * two child legs and the orchestrator's own post-host-exit process scan. The
 * verdict is fail-closed: any missing or contradictory child evidence leaves
 * the affected row `not_proven`, and a surviving bundled-server process after
 * the retry host exited fails the deactivation row outright.
 *
 * @param {{
 *   vsixSha256: string,
 *   extensionVersion: string,
 *   bundledServerSha256: string,
 *   serverSourceRevision: string | null,
 *   repositorySha: string,
 *   vscodeVersion: string,
 *   workspaceFixtureSha256: string,
 *   failure: any | null,
 *   retry: any | null,
 *   violations: string[],
 *   legExitCodes: { failure: number | null, retry: number | null },
 *   postHostExitProcesses: string[],
 * }} input
 */
function composeActivationRecoveryReceipt({
  vsixSha256,
  extensionVersion,
  bundledServerSha256,
  serverSourceRevision,
  repositorySha,
  vscodeVersion,
  workspaceFixtureSha256,
  failure,
  retry,
  violations,
  legExitCodes,
  postHostExitProcesses,
}) {
  const failureObservations = (failure && failure.observations) || {};
  const retryObservations = (retry && retry.observations) || {};
  // Both legs must have exited cleanly AND written passing, bound receipts; a
  // nonzero leg exit (runner or host teardown failure) cannot stand behind a
  // passing verdict even when the child observations passed.
  const legsExitedCleanly = legExitCodes.failure === 0 && legExitCodes.retry === 0;
  const childrenBound =
    violations.length === 0 &&
    legsExitedCleanly &&
    failure &&
    retry &&
    failure.verdict === 'pass' &&
    retry.verdict === 'pass';
  // An OBSERVED product failure (a child verdict of failed, or a leg that
  // exited nonzero after writing evidence) is a failed receipt, not an
  // unproven one; not_proven is reserved for missing or unbound evidence.
  const observedFailure =
    (failure && failure.verdict === 'failed') || (retry && retry.verdict === 'failed');

  const countObservation = (value) => (Array.isArray(value) ? value.length : null);
  const failureProcessesAfterDemand = countObservation(
    failureObservations.bundled_server_processes_after_demand_window,
  );
  const retryRunningProcesses = countObservation(
    retryObservations.bundled_server_processes_running,
  );
  const retryProcessesAfterSecondDemand = countObservation(
    retryObservations.bundled_server_processes_after_second_demand,
  );
  const retryProcessesAfterStop = countObservation(
    retryObservations.bundled_server_processes_after_stop,
  );

  // The two process observations describe the SAME resource set at two
  // moments, not two independent sets: one surplus server process that
  // persists across both windows is ONE duplicate, counted once (max, not
  // sum). Unobserved counts leave the row not_proven.
  const observedCounts = [retryRunningProcesses, retryProcessesAfterSecondDemand].map((count) =>
    count !== null && count >= 1 ? count : null,
  );
  const duplicateObserved = observedCounts.every((count) => count !== null);
  const duplicateResources = duplicateObserved ? Math.max(...observedCounts) - 1 : 0;

  // A missing stop observation is missing evidence, not an observed dirty
  // stop: it must yield not_proven, never failed.
  const stopObserved = retryProcessesAfterStop !== null;
  const stopClean = stopObserved && retryProcessesAfterStop === 0;
  const hostExitClean = postHostExitProcesses.length === 0;
  const crashBudgetEvidence =
    Array.isArray(failureObservations.crash_budget_evidence) &&
    failureObservations.crash_budget_evidence.length > 0;

  const legRow = (child, exitCode) => {
    if (!child) {
      return 'not_proven';
    }
    if (child.verdict !== 'pass') {
      return 'failed';
    }
    // The child observed passing behavior, but its leg run did not exit
    // cleanly: an execution-integrity gap is not a product failure, so the
    // row cannot claim a completed pass either.
    return exitCode === 0 ? 'pass' : 'not_proven';
  };
  const cleanupRow = legRow(failure, legExitCodes.failure);
  const retryRow = legRow(retry, legExitCodes.retry);
  const deactivationRow =
    !childrenBound || !stopObserved ? 'not_proven' : stopClean && hostExitClean ? 'pass' : 'failed';

  // Derived rows: the receipt states what the children observed, not what the
  // composer assumes. Missing evidence degrades to not_proven.
  const evidenceComplete = childrenBound && stopObserved && crashBudgetEvidence;
  let verdict;
  if (observedFailure) {
    verdict = 'failed';
  } else if (!evidenceComplete) {
    verdict = 'not_proven';
  } else if (!stopClean || !hostExitClean) {
    verdict = 'failed';
  } else {
    verdict = 'pass';
  }

  return {
    schema_version: 'vscode_activation_recovery.v1',
    receipt_kind: 'vscode_activation_recovery',
    repository_sha: repositorySha,
    candidate: {
      extension_id: 'EffortlessMetrics.perl-lsp-rs',
      extension_version: extensionVersion,
      vsix_sha256: vsixSha256,
      bundled_server_sha256: bundledServerSha256,
      server_source_sha: serverSourceRevision || null,
      vscode_version: vscodeVersion,
      platform: process.platform,
      architecture: process.arch,
      workspace_fixture_sha256: workspaceFixtureSha256,
      profile: 'one isolated shared profile across the failure and retry legs',
      activation_schema: 'ExtensionActivationOwner/ActivationTransaction (#7854 wiring)',
    },
    failure: {
      phase:
        failure && failure.fault && failure.fault.boundary
          ? `${failure.fault.boundary} (harness-injected pre-commit boundary)`
          : 'not_proven',
      terminal_state: childrenBound ? 'activation_failed' : 'not_proven',
      process_remaining:
        failureProcessesAfterDemand === null ? null : failureProcessesAfterDemand > 0,
      crash_budget_consumed: childrenBound
        ? crashBudgetEvidence
          ? false
          : 'not_proven'
        : 'not_proven',
      crash_budget_evidence: failureObservations.crash_budget_evidence || [],
      cleanup: cleanupRow,
    },
    retry: {
      activation: retryRow,
      duplicate_resources: duplicateObserved ? duplicateResources : 'not_proven',
      provider_smoke: retryRow,
      server_identity:
        retry && retry.observations && retry.observations.startup
          ? String(retry.observations.startup.binary_resolution_source ?? 'not_proven')
          : 'not_proven',
    },
    deactivation: deactivationRow,
    legs: {
      failure_exit_code: legExitCodes.failure,
      retry_exit_code: legExitCodes.retry,
      failure_receipt: failure,
      retry_receipt: retry,
    },
    orchestrator_observations: {
      post_host_exit_bundled_server_processes: postHostExitProcesses,
    },
    instrument_violations: violations,
    verdict,
  };
}

/**
 * Orchestrator-side bundled-server process scan. Mirrors the in-host scan from
 * journeySupport.ts but runs in plain Node after the extension host exited,
 * proving the terminal deactivate path left no candidate server behind.
 * Fail-closed: a nonzero scanner exit is an instrument failure, never an
 * observation of zero processes.
 */
function scanBundledServerProcesses(directory) {
  const resolved = path.resolve(directory);
  let command;
  let args;
  if (process.platform === 'win32') {
    command = 'powershell.exe';
    args = [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      '(Get-Process -Name perllsp,perl-lsp -ErrorAction SilentlyContinue).Path',
    ];
  } else {
    command = 'ps';
    args = ['-eo', 'args='];
  }
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    windowsHide: true,
    timeout: 30_000,
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error) {
    throw new Error(`bundled-server process scan failed to spawn: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(
      `bundled-server process scan exited ${result.status}: ${(result.stderr || '').slice(0, 200)}`,
    );
  }
  const caseInsensitive = process.platform === 'win32' || process.platform === 'darwin';
  const needle = caseInsensitive ? resolved.toLowerCase() : resolved;
  return (result.stdout || '')
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => {
      if (line.length === 0) {
        return false;
      }
      const haystack = caseInsensitive ? line.toLowerCase() : line;
      return haystack.startsWith(needle);
    });
}

/**
 * Environment for one activation-failure journey leg. Both legs install the
 * exact packaged VSIX into the SAME isolated profile; only the failure leg
 * carries the harness fault, so the retry leg is the explicit reload path a
 * user takes with the fault removed.
 */
function activationFailureLegEnv(baseEnv, leg, fault, context) {
  const env = {
    ...baseEnv,
    PERL_LSP_ACTIVATION_FAILURE_SMOKE: '1',
    PERL_LSP_ACTIVATION_FAILURE_LEG: leg,
    PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'vsix',
    PERL_LSP_PUBLISHED_VSIX_PATH: context.vsixPath,
    PERL_LSP_SMOKE_WORKSPACE: context.workspacePath,
    PERL_LSP_SMOKE_USER_DATA_DIR: context.userDataDir,
    PERL_LSP_SMOKE_EXTENSIONS_DIR: context.extensionsDir,
    PERL_LSP_SMOKE_RECEIPTS_DIR: receiptsRoot(),
    PERL_LSP_SMOKE_SOURCE_LABEL: smokeSourceLabel(),
    PERL_LSP_VSIX_SHA256: context.vsixSha256,
    // PERL_LSP_CURRENT_SOURCE_SHA deliberately NOT set: it switches the
    // published smoke into candidate-bound mode, which is Linux-only; the
    // repository SHA rides on this orchestration receipt instead.
    PERL_LSP_SERVER_SOURCE_SHA: context.serverSourceRevision,
  };
  if (fault) {
    env.PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE = 'debugger';
  } else {
    delete env.PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE;
  }
  // This journey selects its own suite and resolves the bundled candidate;
  // the first-hour selectors and the current-source server override must not
  // leak into it.
  delete env.PERL_LSP_CURRENT_SOURCE_SMOKE;
  delete env.PERL_LSP_PACKAGED_BUNDLE_SMOKE;
  delete env.PERL_LSP_FIRST_HOUR_SERVER_PATH;
  delete env.PERL_LSP_CURRENT_SOURCE_SHA;
  return env;
}

/**
 * Run the two-leg packaged activation-failure journey (#7856) against the
 * exact VSIX this run packaged: failure leg (fault armed), retry leg (fault
 * removed, same installed profile — the explicit reload path), then the
 * orchestrator's post-host-exit scan and the joined receipt.
 */
function runActivationFailureJourneyStage(baseEnv, revision, vsixPath, vsixSha256) {
  const packageJson = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  const extensionVersion = packageJson.version;
  const bundledServerSha256 = sha256File(serverPath);
  const workspacePath = fs.mkdtempSync(
    path.join(os.tmpdir(), 'perl-lsp-activation-failure-workspace-'),
  );
  const profilePath = fs.mkdtempSync(
    path.join(os.tmpdir(), 'perl-lsp-activation-failure-profile-'),
  );
  const userDataDir = path.join(profilePath, 'user-data');
  const extensionsDir = path.join(profilePath, 'extensions');
  fs.mkdirSync(userDataDir, { recursive: true });
  fs.mkdirSync(extensionsDir, { recursive: true });
  const fixturePath = path.join(workspacePath, 'activation_recovery.pl');
  fs.writeFileSync(fixturePath, 'use strict;\nuse warnings;\n\nmy $value = 42;\nprint $value;\n');
  const platformReceiptsDir = path.join(receiptsRoot(), smokeSourceLabel(), smokePlatformLabel());
  const failureReceiptFile = path.join(platformReceiptsDir, ACTIVATION_FAILURE_RECEIPTS.failure);
  const retryReceiptFile = path.join(platformReceiptsDir, ACTIVATION_FAILURE_RECEIPTS.retry);
  const joinedReceiptFile = path.join(platformReceiptsDir, ACTIVATION_FAILURE_RECEIPTS.joined);
  // Stale receipts from an earlier run can never stand in for this run's
  // evidence, exactly like the first-hour child receipt guard.
  for (const stale of [failureReceiptFile, retryReceiptFile, joinedReceiptFile]) {
    fs.rmSync(stale, { force: true });
  }
  const context = {
    vsixPath,
    vsixSha256,
    revision,
    serverSourceRevision,
    workspacePath,
    userDataDir,
    extensionsDir,
  };
  const paths = {
    failureReceiptFile,
    retryReceiptFile,
    joinedReceiptFile,
    extensionVersion,
    bundledServerSha256,
    fixturePath,
  };

  /** @type {string | null} */
  let cleanupFailure = null;
  let stage;
  try {
    stage = runActivationFailureJourneyAttempt(baseEnv, context, paths);
  } finally {
    for (const directory of [workspacePath, profilePath]) {
      try {
        fs.rmSync(directory, { recursive: true, force: true });
      } catch (error) {
        cleanupFailure = error instanceof Error ? error.message : String(error);
        process.stderr.write(
          `[activation-failure-journey] cleanup failed for ${directory}: ${cleanupFailure}\n`,
        );
      }
    }
  }
  // A journey whose isolated profile/workspace could not be removed is not a
  // clean pass — the same cleanup contract the outer VSIX/server staging
  // follows. An observed product failure stays failed; anything else that was
  // passing becomes not_proven.
  if (cleanupFailure && stage.status !== 'failed') {
    return {
      ...stage,
      status: 'not_proven',
      reason: `journey directories could not be cleaned: ${cleanupFailure}`,
    };
  }
  return stage;
}

/**
 * The two legs and the joined receipt, isolated from directory lifecycle so
 * the owning stage can apply its cleanup contract to the result.
 */
function runActivationFailureJourneyAttempt(baseEnv, context, paths) {
  const {
    failureReceiptFile,
    retryReceiptFile,
    joinedReceiptFile,
    extensionVersion,
    bundledServerSha256,
    fixturePath,
  } = paths;
  const revision = context.revision;
  const vsixSha256 = context.vsixSha256;
  const extensionsDir = context.extensionsDir;
  /** @type {{ failure: number | null, retry: number | null }} */
  const legExitCodes = { failure: null, retry: null };
  const failureResult = runNpm(
    ['run', 'test:published'],
    activationFailureLegEnv(baseEnv, 'failure', true, context),
  );
  legExitCodes.failure = failureResult.error ? null : (failureResult.status ?? null);
  if (failureResult.error) {
    return {
      status: 'not_proven',
      exit_codes: legExitCodes,
      reason: `failure leg could not run: ${failureResult.error.message}`,
    };
  }
  const retryResult = runNpm(
    ['run', 'test:published'],
    activationFailureLegEnv(baseEnv, 'retry', false, context),
  );
  legExitCodes.retry = retryResult.error ? null : (retryResult.status ?? null);
  if (retryResult.error) {
    return {
      status: 'not_proven',
      reason: `retry leg could not run: ${retryResult.error.message}`,
      exit_codes: legExitCodes,
    };
  }

  let postHostExitProcesses;
  try {
    postHostExitProcesses = scanBundledServerProcesses(extensionsDir);
  } catch (error) {
    return {
      status: 'not_proven',
      exit_codes: legExitCodes,
      reason: `post-host-exit process scan failed: ${
        error instanceof Error ? error.message : String(error)
      }`,
    };
  }

  const validation = validateActivationRecoveryChildReceipts({
    failureReceiptFile,
    retryReceiptFile,
    expectedVsixSha256: vsixSha256,
    expectedBundledServerSha256: bundledServerSha256,
    expectedExtensionVersion: extensionVersion,
  });
  const joined = composeActivationRecoveryReceipt({
    vsixSha256,
    extensionVersion,
    bundledServerSha256,
    serverSourceRevision,
    repositorySha: revision,
    vscodeVersion: (process.env.PERL_LSP_VSCODE_VERSION || '').trim() || 'stable',
    workspaceFixtureSha256: sha256File(fixturePath),
    failure: validation.failure,
    retry: validation.retry,
    violations: validation.violations,
    legExitCodes,
    postHostExitProcesses,
  });
  writeJsonAtomic(joinedReceiptFile, joined);

  if (legExitCodes.failure !== 0 || legExitCodes.retry !== 0) {
    // Aligned with the composer: a leg that did not exit cleanly is an
    // execution-integrity gap (not_proven), not an observed product failure.
    return {
      status: 'not_proven',
      exit_codes: legExitCodes,
      reason: 'activation_failure_journey_leg_did_not_exit_cleanly',
      recovery_verdict: joined.verdict,
    };
  }
  if (!validation.ok) {
    return {
      status: 'not_proven',
      exit_codes: legExitCodes,
      reason: 'journey child receipts did not bind this run',
      violations: validation.violations,
      recovery_verdict: joined.verdict,
    };
  }
  return {
    status: joined.verdict === 'pass' ? 'pass' : 'not_proven',
    exit_codes: legExitCodes,
    recovery_verdict: joined.verdict,
    receipt: path.relative(repoRoot, joinedReceiptFile).replaceAll('\\', '/'),
  };
}

/**
 * Environment for one crash-recovery journey leg (#7848). Both legs install
 * the exact packaged VSIX into the SAME isolated profile; each leg runs in
 * its own fresh extension host so the crash budget, episode state, and demand
 * lifecycle start clean, and each leg terminates the exact server process
 * from the harness.
 */
function crashRecoveryLegEnv(baseEnv, leg, context) {
  const env = {
    ...baseEnv,
    PERL_LSP_CRASH_RECOVERY_SMOKE: '1',
    PERL_LSP_CRASH_RECOVERY_LEG: leg,
    PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'vsix',
    PERL_LSP_PUBLISHED_VSIX_PATH: context.vsixPath,
    PERL_LSP_SMOKE_WORKSPACE: context.workspacePath,
    PERL_LSP_SMOKE_USER_DATA_DIR: context.userDataDir,
    PERL_LSP_SMOKE_EXTENSIONS_DIR: context.extensionsDir,
    PERL_LSP_SMOKE_RECEIPTS_DIR: receiptsRoot(),
    PERL_LSP_SMOKE_SOURCE_LABEL: smokeSourceLabel(),
    PERL_LSP_VSIX_SHA256: context.vsixSha256,
    // PERL_LSP_CURRENT_SOURCE_SHA deliberately NOT set: it switches the
    // published smoke into candidate-bound mode, which is Linux-only; the
    // repository SHA rides on this orchestration receipt instead.
    PERL_LSP_SERVER_SOURCE_SHA: context.serverSourceRevision,
  };
  // This journey selects its own suite and resolves the bundled candidate;
  // the first-hour selectors and the current-source server override must not
  // leak into it.
  delete env.PERL_LSP_CURRENT_SOURCE_SMOKE;
  delete env.PERL_LSP_PACKAGED_BUNDLE_SMOKE;
  delete env.PERL_LSP_FIRST_HOUR_SERVER_PATH;
  delete env.PERL_LSP_CURRENT_SOURCE_SHA;
  delete env.PERL_LSP_ACTIVATION_FAILURE_SMOKE;
  delete env.PERL_LSP_ACTIVATION_FAILURE_LEG;
  delete env.PERL_LSP_EXTENSION_TEST_FAIL_ACTIVATION_PHASE;
  return env;
}

/**
 * Run the two-leg packaged crash-recovery journey (#7848) against the exact
 * VSIX this run packaged: transient leg (one unexpected crash through
 * harness-external termination), breaker leg (budget exhaustion plus explicit
 * retry), then the orchestrator's post-host-exit scan and the joined
 * `vscode_crash_recovery.v1` receipt.
 */
function runCrashRecoveryJourneyStage(baseEnv, revision, vsixPath, vsixSha256) {
  const packageJson = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  const extensionVersion = packageJson.version;
  const bundledServerSha256 = sha256File(serverPath);
  const workspacePath = fs.mkdtempSync(
    path.join(os.tmpdir(), 'perl-lsp-crash-recovery-workspace-'),
  );
  const profilePath = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-crash-recovery-profile-'));
  const userDataDir = path.join(profilePath, 'user-data');
  const extensionsDir = path.join(profilePath, 'extensions');
  fs.mkdirSync(userDataDir, { recursive: true });
  fs.mkdirSync(extensionsDir, { recursive: true });
  const platformReceiptsDir = path.join(receiptsRoot(), smokeSourceLabel(), smokePlatformLabel());
  const transientReceiptFile = path.join(platformReceiptsDir, CRASH_RECOVERY_RECEIPTS.transient);
  const breakerReceiptFile = path.join(platformReceiptsDir, CRASH_RECOVERY_RECEIPTS.breaker);
  const joinedReceiptFile = path.join(platformReceiptsDir, CRASH_RECOVERY_RECEIPTS.joined);
  // Stale receipts from an earlier run can never stand in for this run's
  // evidence, exactly like the first-hour child receipt guard.
  for (const stale of [transientReceiptFile, breakerReceiptFile, joinedReceiptFile]) {
    fs.rmSync(stale, { force: true });
  }
  const context = {
    vsixPath,
    vsixSha256,
    revision,
    serverSourceRevision,
    workspacePath,
    userDataDir,
    extensionsDir,
  };
  const paths = {
    transientReceiptFile,
    breakerReceiptFile,
    joinedReceiptFile,
    extensionVersion,
    bundledServerSha256,
  };

  /** @type {string | null} */
  let cleanupFailure = null;
  let stage;
  try {
    stage = runCrashRecoveryJourneyAttempt(baseEnv, context, paths);
  } finally {
    for (const directory of [workspacePath, profilePath]) {
      try {
        fs.rmSync(directory, { recursive: true, force: true });
      } catch (error) {
        cleanupFailure = error instanceof Error ? error.message : String(error);
        process.stderr.write(
          `[crash-recovery-journey] cleanup failed for ${directory}: ${cleanupFailure}\n`,
        );
      }
    }
  }
  // A journey whose isolated profile/workspace could not be removed is not a
  // clean pass — the same cleanup contract the outer VSIX/server staging
  // follows. An observed product failure stays failed; anything else that was
  // passing becomes not_proven.
  if (cleanupFailure && stage.status !== 'failed') {
    return {
      ...stage,
      status: 'not_proven',
      reason: `journey directories could not be cleaned: ${cleanupFailure}`,
    };
  }
  return stage;
}

/**
 * The two crash-recovery legs and the joined receipt, isolated from directory
 * lifecycle so the owning stage can apply its cleanup contract to the result.
 */
function runCrashRecoveryJourneyAttempt(baseEnv, context, paths) {
  const {
    transientReceiptFile,
    breakerReceiptFile,
    joinedReceiptFile,
    extensionVersion,
    bundledServerSha256,
  } = paths;
  const revision = context.revision;
  const vsixSha256 = context.vsixSha256;
  const extensionsDir = context.extensionsDir;
  /** @type {{ transient: number | null, breaker: number | null }} */
  const legExitCodes = { transient: null, breaker: null };
  const transientResult = runNpm(
    ['run', 'test:published'],
    crashRecoveryLegEnv(baseEnv, 'transient', context),
  );
  legExitCodes.transient = transientResult.error ? null : (transientResult.status ?? null);
  if (transientResult.error) {
    return {
      status: 'not_proven',
      exit_codes: legExitCodes,
      reason: `transient leg could not run: ${transientResult.error.message}`,
    };
  }
  const breakerResult = runNpm(
    ['run', 'test:published'],
    crashRecoveryLegEnv(baseEnv, 'breaker', context),
  );
  legExitCodes.breaker = breakerResult.error ? null : (breakerResult.status ?? null);
  if (breakerResult.error) {
    return {
      status: 'not_proven',
      reason: `breaker leg could not run: ${breakerResult.error.message}`,
      exit_codes: legExitCodes,
    };
  }

  let postHostExitProcesses;
  try {
    postHostExitProcesses = scanBundledServerProcesses(extensionsDir);
  } catch (error) {
    return {
      status: 'not_proven',
      exit_codes: legExitCodes,
      reason: `post-host-exit process scan failed: ${
        error instanceof Error ? error.message : String(error)
      }`,
    };
  }

  const validation = validateCrashRecoveryChildReceipts({
    transientReceiptFile,
    breakerReceiptFile,
    expectedVsixSha256: vsixSha256,
    expectedBundledServerSha256: bundledServerSha256,
    expectedExtensionVersion: extensionVersion,
  });
  const joined = composeCrashRecoveryReceipt({
    vsixSha256,
    extensionVersion,
    bundledServerSha256,
    serverSourceRevision,
    repositorySha: revision,
    vscodeVersion: (process.env.PERL_LSP_VSCODE_VERSION || '').trim() || 'stable',
    transient: validation.transient,
    breaker: validation.breaker,
    violations: validation.violations,
    legExitCodes,
    postHostExitProcesses,
  });
  writeJsonAtomic(joinedReceiptFile, joined);

  if (legExitCodes.transient !== 0 || legExitCodes.breaker !== 0) {
    if (joined.verdict === 'failed') {
      // A leg that recorded observed product failures and then exited nonzero
      // produced evidence of a real regression; downgrading that to
      // instrumentation uncertainty would hide it. Only exits without a
      // bound failure verdict are integrity gaps.
      return {
        status: 'failed',
        exit_codes: legExitCodes,
        reason: 'crash_recovery_journey_leg_observed_failure',
        recovery_verdict: joined.verdict,
      };
    }
    // Aligned with the composer: a leg that did not exit cleanly is an
    // execution-integrity gap (not_proven), not an observed product failure.
    return {
      status: 'not_proven',
      exit_codes: legExitCodes,
      reason: 'crash_recovery_journey_leg_did_not_exit_cleanly',
      recovery_verdict: joined.verdict,
    };
  }
  if (!validation.ok) {
    return {
      status: 'not_proven',
      exit_codes: legExitCodes,
      reason: 'journey child receipts did not bind this run',
      violations: validation.violations,
      recovery_verdict: joined.verdict,
    };
  }
  return {
    // An observed product failure in the joined receipt (a failed row, for
    // example the watchdog row) is a failed stage, not instrumentation
    // uncertainty; only a missing/contradictory verdict stays not_proven.
    status:
      joined.verdict === 'pass' ? 'pass' : joined.verdict === 'failed' ? 'failed' : 'not_proven',
    exit_codes: legExitCodes,
    recovery_verdict: joined.verdict,
    receipt: path.relative(repoRoot, joinedReceiptFile).replaceAll('\\', '/'),
  };
}

function finalizeSmokeRun(
  destination,
  receipt,
  vsixPath,
  restoreStagedServer,
  removeVsix = (target) => fs.rmSync(target, { force: true }),
  persist = persistReceipt,
) {
  const cleanupFailure = {};

  try {
    removeVsix(vsixPath);
  } catch (error) {
    cleanupFailure.vsix_deletion = error instanceof Error ? error.message : String(error);
    process.stderr.write(`Unable to delete staged VSIX: ${cleanupFailure.vsix_deletion}\n`);
  }

  try {
    restoreStagedServer();
  } catch (error) {
    cleanupFailure.staged_server_restoration =
      error instanceof Error ? error.message : String(error);
    process.stderr.write(
      `Unable to restore staged VSIX server: ${cleanupFailure.staged_server_restoration}\n`,
    );
  }

  receipt.cleanup_failure = Object.keys(cleanupFailure).length > 0 ? cleanupFailure : null;
  persist(destination, receipt);
  return exitCodeFor(receipt.overall);
}

function main() {
  let revision = 'unknown';
  try {
    revision = gitRevision();
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  }

  const destination = receiptPath(revision);
  const receipt = initialReceipt(revision);
  persistReceipt(destination, receipt);

  const failInstrument = (error) => {
    const message = error instanceof Error ? error.message : String(error);
    receipt.instrument_failure = message;
    if (receipt.stages.package_creation.status === 'not_proven') {
      receipt.stages.package_creation = { status: 'not_proven', reason: message };
    }
    if (receipt.stages.package_inventory.reason === 'not_started') {
      receipt.stages.package_inventory = {
        status: 'not_proven',
        classification: 'not_proven',
        behavior_safe: false,
        reason: 'package_creation_or_input_validation_failed',
      };
    }
    if (receipt.stages.behavioral_smoke.reason === 'not_started') {
      receipt.stages.behavioral_smoke = {
        status: 'not_run',
        reason: 'package_creation_or_input_validation_failed',
      };
    }
    process.stderr.write(`${message}\n`);
    persistReceipt(destination, receipt);
  };

  if (!serverPath || !fs.existsSync(serverPath)) {
    failInstrument(
      new Error(
        'PERL_LSP_FIRST_HOUR_SERVER_PATH must point to an existing server built from the current source revision.',
      ),
    );
    return concludeRun(receipt);
  }
  if (!serverSourceRevision) {
    failInstrument(
      new Error(
        'PERL_LSP_SERVER_SOURCE_SHA must identify the source revision used to build the server.',
      ),
    );
    return concludeRun(receipt);
  }

  try {
    ensureCleanWorkingTree();
    if (revision === 'unknown') {
      throw new Error('Extension source revision is not available.');
    }
    if (serverSourceRevision !== revision) {
      throw new Error(
        `Server source revision ${serverSourceRevision} does not match extension source revision ${revision}`,
      );
    }
  } catch (error) {
    failInstrument(error);
    return concludeRun(receipt);
  }

  const packageJson = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  const vsixPath = path.join(root, `${packageJson.name}-${packageJson.version}.vsix`);
  let restoreStagedServer = () => {};

  const runStageBody = () => {
    try {
      restoreStagedServer = stageServerForPackage(serverPath);
      const packageEnv = {
        ...process.env,
        PERL_LSP_CURRENT_SOURCE_SMOKE: '1',
        PERL_LSP_SMOKE_RECEIPTS_DIR: receiptsRoot(),
      };
      const packageResult = runNpm(
        ['exec', '--offline', '--no', '--', '@vscode/vsce', 'package'],
        packageEnv,
      );
      if (packageResult.error) {
        receipt.stages.package_creation = {
          status: 'not_proven',
          reason: packageResult.error.message,
          exit_code: null,
        };
        receipt.stages.package_inventory = {
          status: 'not_proven',
          classification: 'not_proven',
          behavior_safe: false,
          reason: 'package_creation_not_proven',
        };
        receipt.stages.behavioral_smoke = {
          status: 'not_run',
          reason: 'package_creation_not_proven',
        };
        persistReceipt(destination, receipt);
        return;
      }
      if (packageResult.status !== 0) {
        receipt.stages.package_creation = {
          status: 'failed',
          exit_code: packageResult.status ?? null,
          reason: 'vsce_package_failed',
        };
        receipt.stages.package_inventory = {
          status: 'not_proven',
          classification: 'not_proven',
          behavior_safe: false,
          reason: 'package_creation_failed',
        };
        receipt.stages.behavioral_smoke = {
          status: 'not_run',
          reason: 'package_creation_failed',
        };
        persistReceipt(destination, receipt);
        return;
      }
      if (!fs.existsSync(vsixPath)) {
        receipt.stages.package_creation = {
          status: 'failed',
          exit_code: 0,
          reason: `vsce did not produce the expected VSIX: ${vsixPath}`,
        };
        receipt.stages.package_inventory = {
          status: 'not_proven',
          classification: 'not_proven',
          behavior_safe: false,
          reason: 'expected_vsix_missing',
        };
        receipt.stages.behavioral_smoke = {
          status: 'not_run',
          reason: 'expected_vsix_missing',
        };
        persistReceipt(destination, receipt);
        return;
      }

      receipt.vsix = {
        path: path.relative(repoRoot, vsixPath).replaceAll('\\', '/'),
        sha256: sha256File(vsixPath),
      };
      receipt.stages.package_creation = { status: 'pass', exit_code: 0 };
      persistReceipt(destination, receipt);

      receipt.stages.package_inventory = runInventoryTransition(packageEnv, revision, vsixPath);
      persistReceipt(destination, receipt);

      if (shouldRunBehavioralSmoke(receipt.stages)) {
        const smokeEnv = {
          ...process.env,
          PERL_LSP_CURRENT_SOURCE_SHA: revision,
          PERL_LSP_CURRENT_SOURCE_SMOKE: '1',
          PERL_LSP_FIRST_HOUR_ONLY: '1',
          PERL_LSP_FIRST_HOUR_RECEIPT: '1',
          PERL_LSP_FIRST_HOUR_SERVER_PATH: path.resolve(serverPath),
          PERL_LSP_PUBLISHED_EXTENSION_SOURCE: 'vsix',
          PERL_LSP_PUBLISHED_VSIX_PATH: vsixPath,
          PERL_LSP_SERVER_SOURCE_SHA: serverSourceRevision,
          PERL_LSP_SMOKE_RECEIPTS_DIR: receiptsRoot(),
          PERL_LSP_SMOKE_SOURCE_LABEL: smokeSourceLabel(),
          PERL_LSP_VSIX_SHA256: receipt.vsix.sha256,
        };

        // Clear any receipt left by an earlier run so a stale artifact can
        // never be mistaken for this run's behavioral evidence.
        const childReceiptFile = childReceiptPath();
        try {
          fs.rmSync(childReceiptFile, { force: true });
          fs.rmSync(hostResolutionFailurePath(), { force: true });
        } catch (error) {
          receipt.stages.behavioral_smoke = {
            status: 'not_proven',
            exit_code: null,
            reason: `unable to clear the previous first-hour receipt: ${
              error instanceof Error ? error.message : String(error)
            }`,
          };
          persistReceipt(destination, receipt);
          return;
        }

        const smokeResult = runNpm(['run', 'test:published'], smokeEnv);
        if (smokeResult.error || smokeResult.status !== 0) {
          receipt.stages.behavioral_smoke = interpretBehavioralSmokeExit({
            status: smokeResult.status,
            spawnError: smokeResult.error,
            receiptsRoot: receiptsRoot(),
          });
        } else if (smokeResult.status === 0) {
          const childReceipt = validateChildSmokeReceipt({
            receiptFile: childReceiptFile,
            expectedRevision: revision,
            expectedVsixSha256: receipt.vsix.sha256,
            expectedServerSourceSha: serverSourceRevision,
            // Mirror the child's own default so an unset matrix version is not
            // reported as an identity mismatch.
            expectedVscodeVersion: (process.env.PERL_LSP_VSCODE_VERSION || '').trim() || 'stable',
            expectedSourceLabel: receipt.source_label,
          });
          receipt.stages.behavioral_smoke = childReceipt.ok
            ? { status: 'pass', exit_code: 0 }
            : {
                status: 'not_proven',
                exit_code: 0,
                reason: 'child_receipt_did_not_bind_this_run',
                violations: childReceipt.violations,
              };
        }
      } else {
        receipt.stages.behavioral_smoke = {
          status: 'not_run',
          reason:
            receipt.stages.package_creation.status !== 'pass'
              ? 'package_creation_not_passed'
              : `inventory_${
                  receipt.stages.package_inventory.transition_state ||
                  receipt.stages.package_inventory.classification ||
                  'not_proven'
                }`,
        };
      }

      // The packaged activation-failure journey (#7856): two legs against the
      // exact VSIX this run packaged, joined into one candidate-bound
      // vscode_activation_recovery.v1 receipt.
      if (shouldRunActivationFailureJourney(receipt.stages)) {
        receipt.stages.activation_failure_journey = runActivationFailureJourneyStage(
          packageEnv,
          revision,
          vsixPath,
          receipt.vsix.sha256,
        );
      } else {
        receipt.stages.activation_failure_journey = {
          status: 'not_run',
          reason:
            receipt.stages.package_creation.status !== 'pass'
              ? 'package_creation_not_passed'
              : `inventory_${
                  receipt.stages.package_inventory.transition_state ||
                  receipt.stages.package_inventory.classification ||
                  'not_proven'
                }`,
        };
      }

      // The packaged crash-recovery journey (#7848): transient and breaker
      // legs against the exact VSIX this run packaged, joined into one
      // candidate-bound vscode_crash_recovery.v1 receipt.
      if (shouldRunCrashRecoveryJourney(receipt.stages)) {
        receipt.stages.crash_recovery_journey = runCrashRecoveryJourneyStage(
          packageEnv,
          revision,
          vsixPath,
          receipt.vsix.sha256,
        );
      } else {
        receipt.stages.crash_recovery_journey = {
          status: 'not_run',
          reason:
            receipt.stages.package_creation.status !== 'pass'
              ? 'package_creation_not_passed'
              : `inventory_${
                  receipt.stages.package_inventory.transition_state ||
                  receipt.stages.package_inventory.classification ||
                  'not_proven'
                }`,
        };
      }
      persistReceipt(destination, receipt);
    } catch (error) {
      failInstrument(error);
    }
  };

  runStageBody();
  const exitCode = finalizeSmokeRun(destination, receipt, vsixPath, restoreStagedServer);
  return concludeRun(receipt, exitCode);
}

if (require.main === module) {
  process.exit(main());
}

module.exports = {
  ACTIVATION_FAILURE_RECEIPTS,
  CRASH_RECOVERY_RECEIPTS,
  activationFailureLegEnv,
  bundleTargetForPlatform,
  childReceiptPath,
  composeActivationRecoveryReceipt,
  composeCheckSummary,
  composeCrashRecoveryReceipt,
  computeOverallStatus,
  concludeRun,
  crashRecoveryLegEnv,
  finalizeSmokeRun,
  initialReceipt,
  interpretBehavioralSmokeExit,
  interpretTransitionResult,
  publishCheckSummary,
  readHostResolutionFailureReceipt,
  receiptPath,
  scanBundledServerProcesses,
  shouldRunActivationFailureJourney,
  shouldRunBehavioralSmoke,
  shouldRunCrashRecoveryJourney,
  validateActivationRecoveryChildReceipts,
  validateChildSmokeReceipt,
  validateCrashRecoveryChildReceipts,
  stageServerForPackage,
  writeJsonAtomic,
};
