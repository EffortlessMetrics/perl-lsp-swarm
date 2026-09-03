import type { ActivationResourceCensus } from './activationTransaction';
import type {
  ClientResourceMeasurement,
  VscodeClientMeasurementRecorder,
} from './clientMeasurement';
import { notProvenResource, observedResource, sortResourceMeasurements } from './clientMeasurement';

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
 *
 * What the registry *can* count exactly is its own membership, so that is the
 * one observed row: `extension_owned_activation_resources`, the resources this
 * attempt registered and has not confirmed released. It is deliberately not
 * reported as `extension_owned_disposables` — the ledger holds `ownCleanup()`
 * callbacks that are not disposables, and post-commit resources go to the host
 * subscription array instead of the ledger, so a disposable figure derived from
 * it would be wrong in both directions.
 */

/** Why the ownership registry cannot break its entries down by resource kind. */
export const RESOURCE_KIND_NOT_CLASSIFIED_REASON =
  'activation ownership registry records resource id, phase, and class but no resource kind';

/**
 * Why the registry cannot report a disposable count specifically.
 *
 * Two independent gaps, either of which alone makes an exact figure impossible:
 * the ledger mixes `own()` disposables with `ownCleanup()` callbacks that are
 * not disposables at all, and resources created after the attempt commits go
 * straight to the host `context.subscriptions` and never enter the ledger.
 */
export const DISPOSABLE_COUNT_UNAVAILABLE_REASON =
  'ownership registry mixes disposables with cleanup callbacks and excludes post-commit host-owned resources';

/** Why extension-host memory cannot be attributed to this extension. */
export const EXTENSION_HOST_MEMORY_SHARED_REASON =
  'extension-host memory is shared across installed extensions and is not attributable';

/** Why no census is available when no activation attempt is owned. */
export const NO_OWNED_ACTIVATION_REASON =
  'no activation attempt is currently owned by this extension';

/**
 * Projects `census` into the `vscode_client_measurement.v1` resource rows.
 *
 * Returns rows only. A resource census carries no measurement subject of its
 * own, and #7866 requires exact subject/scenario identity on a snapshot, so
 * this producer never invents one to borrow the recorder's serializer; the
 * caller that owns a real subject records these rows through
 * {@link recordExtensionOwnedResources}.
 */
export function extensionOwnedResourceMeasurements(
  census: ActivationResourceCensus | null,
): ClientResourceMeasurement[] {
  return sortResourceMeasurements([
    census === null
      ? notProvenResource('extension_owned_activation_resources', NO_OWNED_ACTIVATION_REASON)
      : observedResource('extension_owned_activation_resources', census.live_total),
    notProvenResource('extension_owned_disposables', DISPOSABLE_COUNT_UNAVAILABLE_REASON),
    notProvenResource('extension_owned_timers', RESOURCE_KIND_NOT_CLASSIFIED_REASON),
    notProvenResource('extension_owned_event_listeners', RESOURCE_KIND_NOT_CLASSIFIED_REASON),
    notProvenResource('extension_host_rss_bytes', EXTENSION_HOST_MEMORY_SHARED_REASON),
  ]);
}

/**
 * Records the extension-owned resource counters on a recorder that already owns
 * an exact measurement subject, so a full `vscode_client_measurement.v1`
 * snapshot can carry them alongside its phases.
 *
 * Delegates to {@link extensionOwnedResourceMeasurements} so the rows have one
 * definition; the recorder re-validates each row against the closed resource-id
 * set as it stores it.
 */
export function recordExtensionOwnedResources(
  recorder: VscodeClientMeasurementRecorder,
  census: ActivationResourceCensus | null,
): void {
  for (const row of extensionOwnedResourceMeasurements(census)) {
    if (row.availability === 'observed' && row.value !== null) {
      recorder.observeResource(row.id, row.value);
    } else {
      recorder.markResourceNotProven(row.id, row.reason ?? 'unavailable');
    }
  }
}
