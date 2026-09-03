import type { ActivationResourceCensus } from './activationTransaction';
import type { ClientResourceMeasurement } from './clientMeasurement';
import { VscodeClientMeasurementRecorder } from './clientMeasurement';

/**
 * Production producer for the `extension_owned_*` counters of
 * `vscode_client_measurement.v1` (#14678, parent #7866).
 *
 * The only honest source for "extension-owned" is the activation ownership
 * registry: `ActivationTransaction` records exactly the resources this
 * extension registered during an attempt, each with a phase and a resource
 * class, and marks them cleaned on rollback or deactivation. Counting host
 * disposables or listeners instead would attribute VS Code's own resources to
 * this extension, which #7866 names as a negative control.
 *
 * The registry deliberately carries no resource-*kind* discriminator: an entry
 * is a bounded id plus a cleanup callback, so it cannot say whether a given
 * resource is a timer or an event subscription. Those counters therefore stay
 * `not_proven` with a reason rather than being reported as a fabricated `0`,
 * which #7866 also names as a negative control ("unavailable metric is
 * serialized as zero"). Extension-host memory is shared across every installed
 * extension and is likewise not attributable here.
 */

/** Why the ownership registry cannot break its entries down by resource kind. */
export const RESOURCE_KIND_NOT_CLASSIFIED_REASON =
  'activation ownership registry records resource id, phase, and class but no resource kind';

/** Why extension-host memory cannot be attributed to this extension. */
export const EXTENSION_HOST_MEMORY_SHARED_REASON =
  'extension-host memory is shared across installed extensions and is not attributable';

/** Why no census is available when no activation attempt is owned. */
export const NO_OWNED_ACTIVATION_REASON =
  'no activation attempt is currently owned by this extension';

/**
 * Records the extension-owned resource counters on `recorder` from `census`.
 *
 * Passing `null` (no attempt owned yet, or the module-level owner was cleared)
 * records the ownership counter as `not_proven` rather than `0`: zero owned
 * resources and an unobservable census are different facts.
 */
export function recordExtensionOwnedResources(
  recorder: VscodeClientMeasurementRecorder,
  census: ActivationResourceCensus | null,
): void {
  if (census === null) {
    recorder.markResourceNotProven('extension_owned_disposables', NO_OWNED_ACTIVATION_REASON);
  } else {
    recorder.observeResource('extension_owned_disposables', census.live_total);
  }

  recorder.markResourceNotProven('extension_owned_timers', RESOURCE_KIND_NOT_CLASSIFIED_REASON);
  recorder.markResourceNotProven(
    'extension_owned_event_listeners',
    RESOURCE_KIND_NOT_CLASSIFIED_REASON,
  );
  recorder.markResourceNotProven('extension_host_rss_bytes', EXTENSION_HOST_MEMORY_SHARED_REASON);
}

/**
 * Projects `census` into the `vscode_client_measurement.v1` resource rows.
 *
 * Serialization stays owned by {@link VscodeClientMeasurementRecorder}, so the
 * closed `ClientResourceId` set, the non-negative-value rule, and the
 * deterministic row ordering are enforced in exactly one place.
 */
export function extensionOwnedResourceMeasurements(
  census: ActivationResourceCensus | null,
): ClientResourceMeasurement[] {
  const recorder = new VscodeClientMeasurementRecorder(
    {
      candidate: 'unknown',
      vscode_version: 'unknown',
      platform: process.platform,
      architecture: process.arch,
      host_kind: 'unknown',
      scenario: 'extension_owned_resource_census',
      cold_warm: 'warm',
      binary_role: 'unknown',
      server_candidate: null,
    },
    0,
  );
  recordExtensionOwnedResources(recorder, census);
  return recorder.snapshot().resources;
}
