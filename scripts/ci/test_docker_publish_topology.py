#!/usr/bin/env python3
"""Topology proof for .github/workflows/docker-publish.yml (#9595/#12888).

The trusted-anchor gate (#9595) constrains *which* commit may publish; this
module pins the digest-split *topology* that keeps registry authority away from
the runner that interprets repository-controlled build configuration:

- build jobs hold no registry credentials (no login step at all);
- publication jobs hold no repository checkout and never rebuild;
- publication jobs verify the OCI artifact digest BEFORE the first login step;
- every checkout sets persist-credentials: false and builds only the
  gate-approved anchor;
- every job declares its own permissions and the workflow default carries no
  write scope, so no job can silently inherit registry authority.

The parser is stdlib-only and understands just enough of the GitHub Actions
YAML subset used by this workflow; anything it cannot interpret is a test
failure rather than a pass.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/docker-publish.yml"

CHECKOUT_PREFIX = "actions/checkout"
LOGIN_PREFIX = "docker/login-action"
PUSH_STEP_HINT = "skopeo copy --multi-arch all"
APPROVED_ANCHOR_OUTPUT = "approved_sha"

# The Rust build toolchain retired from product publication (#8980).
BUILDER_DOCKERFILE_DIR = ".docker/rust"
BUILDER_CI_REPOSITORY = "perl-lsp-ci"
# Repository basenames that mean "the product" to a consumer. The runtime is
# published as `effortlessmetrics/perl-lsp` with `-perl` tags and as
# `ghcr.io/<repo>-perl`, so these bare names must never denote the builder.
PRODUCT_IMAGE_REPOSITORIES = frozenset({"perl-lsp", "perllsp"})


def _noncomment_lines(lines: list[str]) -> list[tuple[int, str]]:
    """Return (index, line) pairs that are not blank or pure comments."""
    return [(i, line) for i, line in enumerate(lines) if line.strip() and not line.lstrip().startswith("#")]


def _job_blocks(text: str) -> dict[str, list[str]]:
    """Split the workflow into `jobs:` blocks keyed by job id.

    Job ids sit at exactly two spaces of indentation under a top-level
    `jobs:` key. Anything else at that indentation terminates a job block.
    """
    lines = text.splitlines()
    jobs_header = next(
        (i for i, line in enumerate(lines) if re.match(r"^jobs:\s*$", line)),
        None,
    )
    if jobs_header is None:
        raise AssertionError("workflow has no top-level `jobs:` key")

    headers: list[tuple[int, str]] = []
    for index, line in _noncomment_lines(lines):
        if index <= jobs_header:
            continue
        match = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if match:
            headers.append((index, match.group(1)))

    blocks: dict[str, list[str]] = {}
    for position, (start, job_id) in enumerate(headers):
        end = headers[position + 1][0] if position + 1 < len(headers) else len(lines)
        blocks[job_id] = lines[start + 1 : end]
    if not blocks:
        raise AssertionError("workflow declares no jobs")
    return blocks


def _steps(job_body: list[str]) -> list[dict[str, object]]:
    """Extract ordered step descriptors from a job body.

    Steps live under a four-space `steps:` key; each step begins with a
    six-space `- ` list marker. Descriptor fields are best-effort: `name`,
    `uses`, whether the step has a `run:` block, and the run block text.
    """
    steps_key = next(
        (i for i, line in enumerate(job_body) if re.match(r"^    steps:\s*$", line)),
        None,
    )
    if steps_key is None:
        raise AssertionError("job body has no `steps:` key")

    region: list[tuple[int, str]] = [
        (i, line)
        for i, line in enumerate(job_body)
        if i > steps_key and line.strip() and not line.lstrip().startswith("#")
    ]
    region = [
        (i, line)
        for i, line in region
        if len(line) - len(line.lstrip()) > 4
    ]

    starts = [i for i, line in region if re.match(r"^      - ", line)]
    if not starts:
        raise AssertionError("steps block declares no `- ` entries")

    steps: list[dict[str, object]] = []
    for position, start in enumerate(starts):
        end = starts[position + 1] if position + 1 < len(starts) else len(job_body)
        body = [line for i, line in region if start <= i < end]
        step: dict[str, object] = {"run_lines": [], "lines": body}
        in_run = False
        for line in body:
            name = re.match(r"^\s+(?:- )?name:\s*(.+?)\s*$", line)
            uses = re.match(r"^\s+uses:\s*(\S+)", line)
            run = re.match(r"^\s+run:\s*(\S.*)?$", line)
            if name and "name" not in step:
                step["name"] = name.group(1)
            if uses:
                step["uses"] = uses.group(1)
                in_run = False
            if run:
                step["has_run"] = True
                in_run = True
                continue
            if in_run and not re.match(r"^\s+[A-Za-z-]+:", line) and line.strip():
                step["run_lines"].append(line.strip())  # type: ignore[union-attr]
            elif in_run:
                in_run = False
            if "persist-credentials: false" in line:
                step["persist_false"] = True
        steps.append(step)
    return steps


def _enclosing_step(lines: list[str], index: int) -> list[tuple[int, str]]:
    """Return the (index, line) pairs of the workflow step containing `index`.

    A step begins at a `- ` list marker and ends before the next marker at the
    same indentation or the first line indented less than the marker. Used to
    scan a whole step rather than one line, so a value hoisted into the step's
    `env:` block is still in scope.
    """
    marker = re.compile(r"^(\s*)- ")
    start = index
    while start >= 0 and not marker.match(lines[start]):
        start -= 1
    if start < 0:
        return [(index, lines[index])]

    indent = len(marker.match(lines[start]).group(1))  # type: ignore[union-attr]
    end = start + 1
    while end < len(lines):
        line = lines[end]
        if line.strip():
            current = len(line) - len(line.lstrip())
            if current < indent or (current == indent and marker.match(line)):
                break
        end += 1
    return [
        (i, lines[i])
        for i in range(start, end)
        if lines[i].strip() and not lines[i].lstrip().startswith("#")
    ]


def _job_declares_permissions(job_body: list[str]) -> bool:
    return any(re.match(r"^    permissions:\s*$", line) for line in job_body)


def _workflow_default_write_scopes(text: str) -> list[str]:
    lines = text.splitlines()
    jobs_header = next(
        (i for i, line in enumerate(lines) if re.match(r"^jobs:\s*$", line)),
        len(lines),
    )
    scopes: list[str] = []
    in_permissions = False
    for _, line in _noncomment_lines(lines[:jobs_header]):
        if re.match(r"^permissions:\s*$", line):
            in_permissions = True
            continue
        if in_permissions:
            if re.match(r"^\S", line):
                in_permissions = False
                continue
            scope = re.match(r"^  ([a-z-]+):\s*write\s*$", line)
            if scope:
                scopes.append(scope.group(1))
    return scopes


class DigestSplitTopology(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        if not WORKFLOW.exists():
            raise AssertionError(f"workflow missing: {WORKFLOW}")
        cls.text = WORKFLOW.read_text(encoding="utf-8")
        cls.jobs = _job_blocks(cls.text)
        cls.steps = {job_id: _steps(body) for job_id, body in cls.jobs.items()}

    def uses_of(self, job_id: str, prefix: str) -> list[dict[str, object]]:
        return [
            step
            for step in self.steps[job_id]
            if str(step.get("uses", "")).startswith(prefix)
        ]

    # ── workflow-level authority ────────────────────────────────────────────

    def test_workflow_default_grants_no_write_scope(self) -> None:
        # A workflow-default write scope plus any future job that forgets its
        # own `permissions:` silently re-creates the inherited-authority shape
        # that #5989 closed elsewhere. The xtask policy lint enforces the job
        # side; this pins the workflow side.
        self.assertEqual(
            [],
            _workflow_default_write_scopes(self.text),
            "docker-publish.yml must not carry workflow-default write scopes",
        )

    def test_every_job_declares_its_own_permissions(self) -> None:
        for job_id, body in self.jobs.items():
            self.assertTrue(
                _job_declares_permissions(body),
                f"job {job_id} must declare explicit permissions",
            )

    # ── credential-free builds ──────────────────────────────────────────────

    def test_no_job_combines_checkout_with_registry_login(self) -> None:
        for job_id in self.jobs:
            checkouts = self.uses_of(job_id, CHECKOUT_PREFIX)
            logins = self.uses_of(job_id, LOGIN_PREFIX)
            if logins:
                self.assertEqual(
                    [],
                    checkouts,
                    f"job {job_id} logs in to a registry and must not check out source (#9595/#12888)",
                )
            if checkouts:
                self.assertEqual(
                    [],
                    logins,
                    f"job {job_id} checks out source and must not log in to a registry (#9595/#12888)",
                )

    def test_login_jobs_never_rebuild_images(self) -> None:
        # Publication must push the prebuilt, digest-verified subject. A
        # `docker/build-push-action` step in a login job would re-interpret
        # build configuration next to credentials.
        for job_id in self.jobs:
            if not self.uses_of(job_id, LOGIN_PREFIX):
                continue
            builders = [
                step
                for step in self.steps[job_id]
                if str(step.get("uses", "")).startswith("docker/build-push-action")
            ]
            self.assertEqual(
                [],
                builders,
                f"login job {job_id} must not run image builds",
            )

    def test_checkout_builds_pin_the_approved_anchor(self) -> None:
        # init is exempt: it checks out only to run the read-only metadata
        # validator and holds no registry authority. Every other checkout that
        # feeds a build must name the gate-approved anchor.
        for job_id in self.jobs:
            if job_id == "init":
                continue
            for step in self.uses_of(job_id, CHECKOUT_PREFIX):
                step_text = "\n".join(str(line) for line in step["lines"])  # type: ignore[union-attr]
                self.assertIn(
                    APPROVED_ANCHOR_OUTPUT,
                    step_text,
                    f"checkout in {job_id} must pin the {APPROVED_ANCHOR_OUTPUT} anchor",
                )
                self.assertTrue(
                    step.get("persist_false"),
                    f"checkout in {job_id} must set persist-credentials: false",
                )

    # ── verify-then-login publication ───────────────────────────────────────

    def test_publication_jobs_verify_artifact_digest_before_login(self) -> None:
        for job_id in self.jobs:
            logins = self.uses_of(job_id, LOGIN_PREFIX)
            if not logins:
                continue
            login_index = self.steps[job_id].index(logins[0])
            verifiers = [
                (index, step)
                for index, step in enumerate(self.steps[job_id])
                if index < login_index
                and step.get("has_run")
                and self._is_digest_verification(step)
            ]
            self.assertTrue(
                verifiers,
                f"publication job {job_id} must verify the artifact digest "
                "in a run step before its first login",
            )
            before_login = self.steps[job_id][:login_index]
            self.assertTrue(
                any("refusing to log in" in line for step in before_login for line in step["run_lines"]),  # type: ignore[union-attr]
                f"digest verification in {job_id} must fail closed before login",
            )

    def _is_digest_verification(self, step: dict[str, object]) -> bool:
        run_text = "\n".join(str(line) for line in step["run_lines"])  # type: ignore[union-attr]
        name = str(step.get("name", "")).lower()
        return "sha256sum" in run_text and "digest" in name

    def test_publication_jobs_push_the_verified_subject_only(self) -> None:
        for job_id in self.jobs:
            if not self.uses_of(job_id, LOGIN_PREFIX):
                continue
            run_text = "\n".join(
                line for step in self.steps[job_id] for line in step["run_lines"]  # type: ignore[union-attr]
            )
            self.assertIn(
                PUSH_STEP_HINT,
                run_text,
                f"publication job {job_id} must push the prebuilt OCI layout ({PUSH_STEP_HINT})",
            )
            self.assertIn(
                "imagetools inspect",
                run_text,
                f"publication job {job_id} must re-inspect the pushed subject (platforms + attestations)",
            )
            self.assertIn(
                "not the verified subject",
                run_text,
                f"publication job {job_id} must prove every tag resolves to the pushed digest",
            )

    # ── the topology covers every registry surface ──────────────────────────

    def test_both_registries_are_covered_by_the_split(self) -> None:
        login_jobs = [
            job_id for job_id in self.jobs if self.uses_of(job_id, LOGIN_PREFIX)
        ]
        self.assertEqual(
            ["publish-dockerhub", "publish-ghcr"],
            sorted(login_jobs),
            "exactly one publication job per registry surface is expected",
        )
        build_jobs = [
            job_id
            for job_id in self.jobs
            if self.uses_of(job_id, CHECKOUT_PREFIX)
            and any(
                str(step.get("uses", "")).startswith("docker/build-push-action")
                for step in self.steps[job_id]
            )
        ]
        self.assertEqual(
            ["build-perl-runtime"],
            sorted(build_jobs),
            "the runtime image is the only published subject (#8980) and its "
            "build must remain a credential-free build job",
        )
        for job_id in build_jobs:
            run_text = "\n".join(
                line for step in self.steps[job_id] for line in step["run_lines"]  # type: ignore[union-attr]
            )
            self.assertNotIn(
                "docker/login-action",
                run_text,
                f"build job {job_id} must not log in anywhere in its run steps",
            )
            self.assertNotIn(
                "secrets.",
                run_text,
                f"build job {job_id} must not consume a secrets context",
            )
            self.assertNotIn(
                "${{",
                run_text,
                f"build job {job_id} run steps must receive expressions via env (workflow-security ratchet)",
            )


class RustBuilderIsNotAProduct(unittest.TestCase):
    """The Rust build toolchain is retired from product publication (#8980).

    `.docker/rust/Dockerfile` builds a Rust toolchain over a copy of the
    repository source. It ships no `perllsp`, yet it used to publish under the
    unsuffixed `perl-lsp:<version>` tags on both registries, so the obvious
    product reference resolved to an image that cannot run the product. These
    are the negative controls for that retirement: each one fails against the
    pre-retirement workflows rather than merely restating the current shape.

    The Dockerfile itself is retained — `trivy-docker-scan` in ci-security.yml
    still builds it locally — so absence of the file is not the invariant.
    What is pinned is that no product identity ever denotes it.
    """

    @classmethod
    def setUpClass(cls) -> None:
        cls.publish_text = WORKFLOW.read_text(encoding="utf-8")
        cls.jobs = _job_blocks(cls.publish_text)
        cls.steps = {job_id: _steps(body) for job_id, body in cls.jobs.items()}
        cls.workflows = sorted(WORKFLOW.parent.glob("*.yml"))
        if not cls.workflows:
            raise AssertionError(f"no workflows found under {WORKFLOW.parent}")

    def test_publication_workflow_never_builds_the_rust_builder(self) -> None:
        # Comments are stripped deliberately: the surviving prose explains the
        # retirement, so a substring scan over the raw file would pass for the
        # wrong reason.
        live = [
            line
            for _, line in _noncomment_lines(self.publish_text.splitlines())
            if BUILDER_DOCKERFILE_DIR in line
        ]
        self.assertEqual(
            [],
            live,
            "docker-publish.yml must not build or publish the Rust builder "
            f"({BUILDER_DOCKERFILE_DIR}); it is retired from product publication",
        )

    def test_no_workflow_gives_the_builder_a_product_image_name(self) -> None:
        # The publication path is not the only way the canonical name can come
        # to mean the builder: a local `docker build -t perl-lsp:<sha>` for a
        # security scan asserts the same false identity inside CI.
        #
        # The whole enclosing step is scanned, not the `docker build` line, so
        # that hoisting the tag into `env:` — which the workflow-security
        # ratchet requires for expressions in run source — cannot launder a
        # product name past this control.
        offenders: list[str] = []
        for workflow in self.workflows:
            lines = workflow.read_text(encoding="utf-8").splitlines()
            for index, line in _noncomment_lines(lines):
                if BUILDER_DOCKERFILE_DIR not in line:
                    continue
                for offset, step_line in _enclosing_step(lines, index):
                    for reference in re.findall(r"[\w./-]+:[\w${}.-]+", step_line):
                        repository = reference.split(":", 1)[0].rsplit("/", 1)[-1]
                        if repository in PRODUCT_IMAGE_REPOSITORIES:
                            offenders.append(
                                f"{workflow.name}:{offset + 1}: {reference}"
                            )
        self.assertEqual(
            [],
            sorted(set(offenders)),
            "the canonical product image name must never denote the Rust "
            f"builder; use a CI-toolchain name such as {BUILDER_CI_REPOSITORY}",
        )

    def test_each_publication_job_pushes_exactly_one_subject(self) -> None:
        for job_id in self.jobs:
            pushes = [
                line
                for step in self.steps[job_id]
                for line in step["run_lines"]  # type: ignore[union-attr]
                if line.startswith("publish_layout ")
            ]
            if not pushes:
                continue
            self.assertEqual(
                1,
                len(pushes),
                f"publication job {job_id} must push only the runtime subject, "
                f"got: {pushes}",
            )
            self.assertTrue(
                pushes[0].startswith("publish_layout runtime-layout "),
                f"publication job {job_id} must push the runtime layout, "
                f"got: {pushes[0]}",
            )

    def test_every_needs_edge_resolves_to_a_declared_job(self) -> None:
        # Removing a job is exactly the edit that strands a `needs:` edge in
        # some other job, and GitHub rejects the whole workflow when that
        # happens. This caught a real stranded `build` edge on
        # `publish-dockerhub` during the #8980 retirement, which none of the
        # identity controls above would have noticed.
        declared = set(self.jobs)
        stranded: list[str] = []
        for job_id, body in self.jobs.items():
            for line in body:
                match = re.match(r"^    needs:\s*(.+?)\s*$", line)
                if not match:
                    continue
                raw = match.group(1).strip()
                names = (
                    raw[1:-1].split(",")
                    if raw.startswith("[") and raw.endswith("]")
                    else [raw]
                )
                for name in (n.strip() for n in names):
                    if name and name not in declared:
                        stranded.append(f"{job_id} -> {name}")
        self.assertEqual(
            [],
            stranded,
            f"every needs edge must name a declared job; declared={sorted(declared)}",
        )

    def test_summary_advertises_no_builder_image(self) -> None:
        summary_text = "\n".join(
            line
            for step in self.steps["summary"]
            for line in step["run_lines"]  # type: ignore[union-attr]
        )
        self.assertNotIn(
            "Builder image",
            summary_text,
            "the publish summary must not advertise a builder image to consumers",
        )
        # The runtime section must survive; an empty summary would otherwise
        # satisfy the assertion above.
        self.assertIn(
            "Runtime image",
            summary_text,
            "the publish summary must still advertise the runtime image",
        )


if __name__ == "__main__":
    unittest.main()
