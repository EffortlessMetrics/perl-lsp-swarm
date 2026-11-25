"""Read-only Perl/Hermes H-authority join; no installed authorization or writer.

All ports are supplied by reviewed trusted control code. A dataclass, digest or
READY result is not authentication. The default has no portfolio/proof-to-Hermes
binding owner and therefore cannot observe an admitted H. No CLI loads ports,
accepts receipt JSON or acquires credentials.
"""
from copy import deepcopy
from dataclasses import dataclass

from . import candidate_readiness


@dataclass(frozen=True)
class Evidence:
    subject: dict
    observations: dict
    readiness: dict


@dataclass(frozen=True)
class Authorization:
    """An authenticated owner's result, not a serializable authority format.

    The owner must independently authenticate the exact evidence-to-request
    mapping and conditional H grant. References are retained provenance, never
    substitutes for that acquisition. No such production owner is installed.
    """
    evidence: Evidence
    request: dict
    app_id: int
    check_name: str
    reference: str
    mapping_reference: str


def capture(acquire_plan, authorities):
    """Record actual existing owner observations without another readiness law."""
    subject = deepcopy(acquire_plan())
    facts = {}

    class Recorded:
        def __getattr__(self, name):
            def observe(*args):
                value = getattr(authorities, name)(*args)
                facts[name] = deepcopy(value)
                return value
            return observe

    readiness = candidate_readiness.evaluate(deepcopy(subject), Recorded())
    return Evidence(subject, facts, deepcopy(readiness))


class HermesReadback:
    """Reuse the installed Hermes model and CheckPublisher through GET-only I/O.

    `model` and `publisher_type` are trusted source capabilities, never names to
    import from a candidate. `read(path)` returns the native response object.
    This adapter calls only list_checks, external_id and verify; never ensure,
    complete, fail or Admission.execute. Missing checks are not created.
    """
    def __init__(self, policy, *, model, publisher_type, read, now):
        self.policy = deepcopy(policy)
        self.model = model
        self.publisher_type = publisher_type
        self.read = read
        self.now = now

    def observe(self, authorization):
        model = self.model
        policy = deepcopy(model.validate_policy(deepcopy(self.policy)))
        subject = authorization.evidence.subject
        request = deepcopy(model.validate_request(
            deepcopy(authorization.request), policy, self.now()))
        if (policy['repository'] != subject['repository']
                or policy['repository_id'] != subject['repository_id']
                or policy['publisher_app_id'] != authorization.app_id
                or policy['check_name'] != authorization.check_name
                or request['expected_base_sha'] != subject['base']):
            raise ValueError('H authority differs from acquired subject or publisher')
        members = [row for row in request['members']
                   if row['pull_number'] == subject['pr'] and row['head_sha'] == subject['head']]
        if len(members) != 1:
            raise ValueError('H authority lacks exact member')
        member = members[0]
        prefix = '/repos/' + policy['repository']
        inventory = prefix + '/commits/' + subject['head'] + '/check-runs?'

        def get_only(method, path, body=None, **kwargs):
            check_id = path.removeprefix(prefix + '/check-runs/')
            exact_check = (path.startswith(prefix + '/check-runs/')
                           and check_id.isascii() and check_id.isdecimal() and int(check_id) > 0)
            if (method != 'GET' or body is not None or kwargs
                    or not (path.startswith(inventory) or exact_check)):
                raise ValueError('H readback capability refuses non-check GET or mutation')
            return self.read(path)

        publisher = self.publisher_type(policy, get_only)
        external_id = publisher.external_id(request['request_id'], member['pull_number'])
        rows = publisher.list_checks(subject['head'])
        matches = [row for row in rows if isinstance(row, dict)
                   and row.get('name') == authorization.check_name
                   and row.get('app', {}).get('id') == authorization.app_id
                   and row.get('external_id') == external_id]
        if len(matches) != 1 or not model.positive(matches[0].get('id')):
            raise ValueError('H readback is absent or ambiguous')
        check_id = matches[0]['id']
        check = deepcopy(get_only('GET', prefix + '/check-runs/' + str(check_id)).data)
        if not isinstance(check, dict) or type(check.get('id')) is not int or check['id'] != check_id:
            raise ValueError('H readback substituted the requested check ID')
        receipt = publisher.verify(check, member, external_id, model.sha256(request))
        if (check['status'] != 'completed' or check.get('conclusion') != 'success'
                or receipt.get('phase') != 'H_ADMITTED'
                or model.request_from_conditional_receipt(receipt) != request):
            raise ValueError('H readback is not the exact conditional admission')
        # Expiry can cross during acquisition. Preserve its original request.
        model.validate_request(deepcopy(request), policy, self.now())
        if self.policy != policy:
            raise ValueError('H publisher policy changed during observation')
        return {'check_id': check_id, 'app_id': authorization.app_id,
                'check_name': authorization.check_name, 'head': subject['head'],
                'external_id': external_id, 'receipt': deepcopy(receipt), 'policy': policy}

    def current(self, authorization, readback):
        """Closing local expiry/policy check after potentially slow owner reads."""
        policy = self.model.validate_policy(deepcopy(self.policy))
        if policy != readback['policy']:
            raise ValueError('H publisher policy changed after readback')
        self.model.validate_request(deepcopy(authorization.request), policy, self.now())


def observe(acquire_plan, authorities, *, authorization_owner=None, hermes=None):
    """Join fresh existing evidence with separately authenticated H readback.

    Even an observed H grants no effects here. Source installation, secret
    custody, atomic publication fencing and G qualification are not assessed.
    Exceptions and missing ports remain NOT_PROVEN; no write/retry path exists.
    """
    result = {'status': 'NOT_PROVEN', 'effects': [], 'publication_authorized': False,
              'execution_authorized': False, 'merge_authorized': False,
              'g_qualification_proven': False}
    try:
        before = capture(acquire_plan, authorities)
        result['readiness'] = before.readiness
        if before.readiness['classification'] != 'READY':
            return dict(result, status=before.readiness['classification'], reason='existing_readiness_refused')
        if authorization_owner is None or hermes is None:
            return dict(result, reason='authenticated_h_binding_or_readback_unavailable')
        authorization = deepcopy(authorization_owner(deepcopy(before)))
        if (not isinstance(authorization, Authorization) or authorization.evidence != before
                or type(authorization.app_id) is not int or authorization.app_id <= 0
                or not isinstance(authorization.check_name, str) or not authorization.check_name
                or not isinstance(authorization.reference, str) or not authorization.reference
                or not isinstance(authorization.mapping_reference, str) or not authorization.mapping_reference):
            return dict(result, reason='authenticated_h_binding_missing_or_mismatched')
        readback = deepcopy(hermes.observe(authorization))
        after = capture(acquire_plan, authorities)
        if after != before:
            return dict(result, reason='subject_review_proof_or_findings_changed')
        if deepcopy(authorization_owner(deepcopy(after))) != authorization:
            return dict(result, reason='h_authorization_changed')
        hermes.current(authorization, readback)
        return dict(result, status='H_AUTHORITY_OBSERVED', reason='conditional_h_only',
                    readback=readback, authorization_reference=authorization.reference,
                    mapping_reference=authorization.mapping_reference)
    except (OSError, ValueError, TypeError, KeyError, AttributeError, RuntimeError):
        return dict(result, reason='h_authority_acquisition_unavailable')
