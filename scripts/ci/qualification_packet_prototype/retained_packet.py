"""Read delivered campaign artifacts through the existing evaluator (#17897).

This is an offline diagnostic adapter. Supplied API records are evidence inputs,
not authenticated attestations; byte agreement cannot invent execution authority.
No extraction, execution, downloading, admission, or policy activation occurs.
"""
import argparse
import hashlib
import json
import io
import math
from pathlib import Path, PurePosixPath
import re
import zipfile
import zlib

from evaluator import EvidenceError, evaluate_observed_report, require

MAX_JSON_BYTES = 16 * 1024 * 1024
MAX_ARCHIVE_BYTES = 128 * 1024 * 1024
MISSING = (
    'authenticated_delivery_custody', 'artifact_to_execution_attempt_binding',
    'requested_experiment_manifest', 'execution_time_source_operation_attestation',
    'policy_clock_mapping', 'whole_envelope_accounting_through_packaging',
)


def strict_json(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, 'duplicate JSON key: ' + key)
            result[key] = value
        return result

    def constant(value):
        raise EvidenceError('nonfinite JSON: ' + value)

    def finite_float(value):
        number = float(value)
        require(math.isfinite(number), 'nonfinite JSON numeric exponent')
        return number

    return json.loads(raw, object_pairs_hook=pairs, parse_constant=constant, parse_float=finite_float)


def delivered_archive_bytes(path):
    # Hash and parse exactly the same immutable bounded bytes. Reopening the
    # pathname would allow replacement between digest validation and ZIP reads.
    with Path(path).open('rb') as stream:
        raw = stream.read(MAX_ARCHIVE_BYTES + 1)
    require(len(raw) <= MAX_ARCHIVE_BYTES, 'archive exceeds bounded diagnostic size')
    return raw


def member_bytes(archive, name):
    path = PurePosixPath(name)
    require(not path.is_absolute() and path.as_posix() == name
            and '..' not in path.parts and '\\' not in name, 'canonical member name')
    matches = [info for info in archive.infolist() if info.filename == name]
    require(len(matches) == 1, 'missing or duplicate archive member: ' + name)
    info = matches[0]
    require(not info.is_dir() and info.file_size <= MAX_JSON_BYTES,
            'report member exceeds bounded JSON size')
    with archive.open(info) as stream:
        raw = stream.read(MAX_JSON_BYTES + 1)
    require(len(raw) == info.file_size and len(raw) <= MAX_JSON_BYTES, 'member length mismatch')
    return raw


def inspect_archive(path, artifact, run, *, expected_run, expected_attempt,
                    expected_artifact, expected_head, report_members,
                    expected_argv=None):
    """Compare caller expectations, API records, delivered bytes and reports.

    The caller obtains expectations from the existing campaign owner. They are
    not a new manifest schema, and agreement does not authenticate that caller.
    A run's attempt field does not bind an artifact to that attempt: the Actions
    artifact record has no attempt field. Keep that gap explicit even on reruns.
    """
    result = {'scope': 'retained-campaign-packet-diagnostic',
              'production_qualification': 'NOT_PROVEN',
              'delivery': 'NOT_PROVEN', 'reports': [], 'time': 'NOT_PROVEN',
              'missing_authority': list(MISSING), 'reasons': []}
    try:
        for value in (expected_run, expected_attempt, expected_artifact):
            require(type(value) is int and value > 0, 'positive typed expected identity')
        require(isinstance(expected_head, str) and re.fullmatch('[0-9a-f]{40}', expected_head), 'expected head')
        require(type(run['id']) is int and run['id'] == expected_run, 'run identity')
        require(type(run['run_attempt']) is int and run['run_attempt'] == expected_attempt, 'run attempt')
        require(run['head_sha'] == expected_head, 'run head')
        require(type(artifact['id']) is int and artifact['id'] == expected_artifact, 'artifact identity')
        require(artifact['expired'] is False, 'artifact expired or unavailable')
        require(type(artifact['workflow_run']['id']) is int
                and artifact['workflow_run']['id'] == expected_run, 'artifact run')
        require(artifact['workflow_run']['head_sha'] == expected_head, 'artifact head')
        expected_digest = artifact['digest']
        require(isinstance(expected_digest, str) and re.fullmatch('sha256:[0-9a-f]{64}', expected_digest), 'artifact digest unavailable')
        delivered = delivered_archive_bytes(path)
        actual_digest = hashlib.sha256(delivered).hexdigest()
        result['delivered_bytes'] = {'sha256': actual_digest, 'size_bytes': len(delivered)}
        require('sha256:' + actual_digest == expected_digest, 'delivered artifact digest mismatch')
        require(type(artifact['size_in_bytes']) is int
                and artifact['size_in_bytes'] == result['delivered_bytes']['size_bytes'], 'delivered artifact size mismatch')
        require(isinstance(report_members, list) and bool(report_members)
                and len(set(report_members)) == len(report_members), 'explicit unique report selection')
        result['delivery'] = 'CONSISTENT'
        result['declared_identity'] = {'run_id': expected_run, 'run_attempt': expected_attempt,
                                       'artifact_id': expected_artifact, 'head': expected_head}
        with zipfile.ZipFile(io.BytesIO(delivered)) as archive:
            for name in report_members:
                item = {'member': name, 'binding': 'NOT_PROVEN'}
                result['reports'].append(item)
                try:
                    raw = member_bytes(archive, name)
                    item['sha256'] = hashlib.sha256(raw).hexdigest()
                    report = strict_json(raw)
                    require(isinstance(report, dict), 'report object')
                    require(report['head'] == expected_head, 'report head differs from requested source')
                    if expected_argv is not None:
                        require(isinstance(expected_argv, list) and bool(expected_argv)
                                and all(isinstance(arg, str) for arg in expected_argv), 'expected argv list')
                        argv_digest = hashlib.sha256(json.dumps(expected_argv, ensure_ascii=True,
                            separators=(',', ':')).encode('utf-8')).hexdigest()
                        scope = report['terminal_owner_binding']['descriptor']['scope']
                        require(scope['cargo_argv_sha256'] == argv_digest, 'effective argv digest mismatch')
                        require(scope['operation'] == expected_argv[0], 'effective operation mismatch')
                    else:
                        item['missing_authority'] = ['requested_effective_argv']
                    item['binding'] = 'CONSISTENT'
                    # The original reporter exports only a hash. Absence of the
                    # projection must not be replaced by an empty or toy graph.
                    item['observations'] = evaluate_observed_report(report, report.get('unit_projection'))
                    if 'unit_projection' not in report:
                        item.setdefault('missing_authority', []).append('producer_canonical_unit_projection')
                    storage = report.get('storage', {})
                    require(isinstance(storage, dict), 'storage summary object')
                    item['clock_observations'] = {
                        'command_elapsed_seconds': storage.get('command_elapsed_seconds'),
                        'sampler_elapsed_seconds': storage.get('elapsed_seconds'),
                        'gate_cumulative_elapsed_seconds': None,
                        'whole_experiment_elapsed_seconds': None,
                        'policy_mapping': 'NOT_PROVEN',
                    }
                except (EvidenceError, KeyError, TypeError, ValueError, OSError, RuntimeError, zlib.error, EOFError) as error:
                    item['binding'] = 'NOT_PROVEN' if isinstance(error, KeyError) else 'REFUSED'
                    item['reason'] = str(error)
    except (EvidenceError, KeyError, TypeError, ValueError, OSError, RuntimeError, zipfile.BadZipFile, zlib.error, EOFError) as error:
        result['delivery'] = 'NOT_PROVEN' if isinstance(error, (KeyError, OSError)) else 'REFUSED'
        result['reasons'].append(str(error))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    parser.add_argument('--artifact-record', required=True, type=Path)
    parser.add_argument('--run-record', required=True, type=Path)
    parser.add_argument('--run-id', required=True, type=int)
    parser.add_argument('--attempt', required=True, type=int)
    parser.add_argument('--artifact-id', required=True, type=int)
    parser.add_argument('--head', required=True)
    parser.add_argument('--report', action='append', required=True)
    parser.add_argument('--expected-argv', type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    try:
        result = inspect_archive(args.archive, strict_json(args.artifact_record.read_bytes()),
            strict_json(args.run_record.read_bytes()), expected_run=args.run_id,
            expected_attempt=args.attempt, expected_artifact=args.artifact_id,
            expected_head=args.head, report_members=args.report,
            expected_argv=strict_json(args.expected_argv.read_bytes()) if args.expected_argv else None)
    except (OSError, ValueError) as error:
        result = {'production_qualification': 'NOT_PROVEN', 'delivery': 'NOT_PROVEN', 'reasons': [str(error)]}
    print(json.dumps(result, indent=2, sort_keys=True, allow_nan=False))
    # A readable diagnostic is not qualification. No --check success exists
    # until execution-time authorities are implemented by the existing owners.
    return 1 if args.check else 0


if __name__ == '__main__':
    raise SystemExit(main())
