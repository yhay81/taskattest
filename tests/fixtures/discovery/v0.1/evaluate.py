#!/usr/bin/env python3
"""Evaluate TaskAttest discovery against the versioned labeled corpus."""

from __future__ import annotations

import argparse
import collections
import difflib
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import tempfile
from typing import Any


METRICS_SCHEMA_VERSION = "taskattest.discovery-metrics/v0.1"
SUPPORTED_ECOSYSTEMS = ("javascript", "python", "rust", "go")


def run(
    command: list[str],
    *,
    cwd: Path | None = None,
    environment: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        env=environment,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def safe_fixture_path(root: Path, relative: str) -> Path:
    portable = PurePosixPath(relative)
    if portable.is_absolute() or ".." in portable.parts or not portable.parts:
        raise ValueError(f"unsafe fixture path: {relative!r}")
    return root.joinpath(*portable.parts)


def initialize_workspace(root: Path, files: dict[str, str]) -> None:
    result = run(["git", "init", "--quiet"], cwd=root)
    if result.returncode:
        raise RuntimeError(result.stderr.strip())
    for relative, contents in files.items():
        destination = safe_fixture_path(root, relative)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(contents, encoding="utf-8")
    result = run(["git", "add", "."], cwd=root)
    if result.returncode:
        raise RuntimeError(result.stderr.strip())
    environment = os.environ.copy()
    environment.update(
        {
            "GIT_AUTHOR_DATE": "2000-01-01T00:00:00Z",
            "GIT_COMMITTER_DATE": "2000-01-01T00:00:00Z",
        }
    )
    result = run(
        [
            "git",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=TaskAttest Corpus",
            "-c",
            "user.email=corpus@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
        cwd=root,
        environment=environment,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip())


def ratio(numerator: int, denominator: int) -> float:
    return round(numerator / denominator, 6) if denominator else 1.0


def metric(counter: collections.Counter[str]) -> dict[str, int | float]:
    true_positive = counter["true_positive"]
    false_positive = counter["false_positive"]
    false_negative = counter["false_negative"]
    return {
        "expected": true_positive + false_negative,
        "predicted": true_positive + false_positive,
        "true_positive": true_positive,
        "false_positive": false_positive,
        "false_negative": false_negative,
        "precision": ratio(true_positive, true_positive + false_positive),
        "recall": ratio(true_positive, true_positive + false_negative),
    }


def actual_ecosystem(
    case: dict[str, Any], check_id: str
) -> str:
    declared = case["check_ecosystems"].get(check_id)
    if declared:
        return declared
    for prefix, ecosystem in (
        ("js-", "javascript"),
        ("python-", "python"),
        ("rust-", "rust"),
        ("go-", "go"),
    ):
        if check_id.startswith(prefix):
            return ecosystem
    return "workflow"


def validate_corpus(corpus: dict[str, Any]) -> list[str]:
    violations: list[str] = []
    if corpus.get("schema_version") != "taskattest.discovery-corpus/v0.1":
        violations.append(
            f"unsupported corpus schema: {corpus.get('schema_version')!r}"
        )
    cases = corpus.get("cases", [])
    requirements = corpus["requirements"]
    ids = [case.get("id") for case in cases]
    if len(cases) < requirements["minimum_projects"]:
        violations.append(
            f"project count {len(cases)} is below {requirements['minimum_projects']}"
        )
    if len(ids) != len(set(ids)):
        violations.append("case IDs are not unique")
    ecosystem_counts = collections.Counter(
        ecosystem
        for case in cases
        for ecosystem in set(case["ecosystems"])
        if ecosystem in SUPPORTED_ECOSYSTEMS
    )
    for ecosystem in SUPPORTED_ECOSYSTEMS:
        minimum = requirements["minimum_projects_per_ecosystem"]
        if ecosystem_counts[ecosystem] < minimum:
            violations.append(
                f"{ecosystem} project count {ecosystem_counts[ecosystem]} is below {minimum}"
            )
    shape_counts = collections.Counter(case["shape"] for case in cases)
    for shape, requirement_key in (
        ("mixed", "minimum_mixed_projects"),
        ("monorepo", "minimum_monorepo_projects"),
    ):
        if shape_counts[shape] < requirements[requirement_key]:
            violations.append(
                f"{shape} project count {shape_counts[shape]} "
                f"is below {requirements[requirement_key]}"
            )
    for case in cases:
        expected = case["expected_check_ids"]
        mapping = case["check_ecosystems"]
        missing = sorted(set(expected) - set(mapping))
        if missing:
            violations.append(
                f"{case['id']} lacks ecosystem labels for: {', '.join(missing)}"
            )
    return violations


def evaluate(binary: Path, corpus_path: Path) -> dict[str, Any]:
    corpus_bytes = corpus_path.read_bytes()
    corpus = json.loads(corpus_bytes)
    violations = validate_corpus(corpus)
    overall: collections.Counter[str] = collections.Counter()
    ecosystem_metrics: dict[str, collections.Counter[str]] = {
        ecosystem: collections.Counter()
        for ecosystem in (*SUPPORTED_ECOSYSTEMS, "workflow", "other")
    }
    shape_counts: collections.Counter[str] = collections.Counter()
    ecosystem_project_counts: collections.Counter[str] = collections.Counter()
    mismatches: list[dict[str, Any]] = []
    gap_mismatches: list[dict[str, Any]] = []
    tripwire_failures: list[dict[str, str]] = []

    for case in corpus["cases"]:
        shape_counts[case["shape"]] += 1
        ecosystem_project_counts.update(set(case["ecosystems"]))
        with tempfile.TemporaryDirectory(prefix=f"taskattest-{case['id']}-") as raw:
            workspace = Path(raw)
            initialize_workspace(workspace, case["files"])
            result = run(
                [
                    str(binary),
                    "--workspace",
                    str(workspace),
                    "discover",
                    "--format",
                    "json",
                ]
            )
            if result.returncode:
                violations.append(
                    f"{case['id']} discovery exited {result.returncode}: "
                    f"{result.stderr.strip()}"
                )
                actual_ids: set[str] = set()
                actual_gaps: list[str] = []
                actual_observations: list[dict[str, Any]] = []
            else:
                try:
                    report = json.loads(result.stdout)
                except json.JSONDecodeError as error:
                    violations.append(
                        f"{case['id']} returned invalid JSON: {error}"
                    )
                    report = {"checks": [], "coverage_gaps": []}
                actual_ids = {check["id"] for check in report["checks"]}
                actual_gaps = sorted(report["coverage_gaps"])
                actual_observations = report.get("workflow_observations", [])

            expected_ids = set(case["expected_check_ids"])
            true_positive = expected_ids & actual_ids
            false_positive = actual_ids - expected_ids
            false_negative = expected_ids - actual_ids
            overall.update(
                {
                    "true_positive": len(true_positive),
                    "false_positive": len(false_positive),
                    "false_negative": len(false_negative),
                }
            )
            for check_id in true_positive:
                ecosystem = case["check_ecosystems"][check_id]
                ecosystem_metrics.setdefault(ecosystem, collections.Counter())[
                    "true_positive"
                ] += 1
            for check_id in false_negative:
                ecosystem = case["check_ecosystems"][check_id]
                ecosystem_metrics.setdefault(ecosystem, collections.Counter())[
                    "false_negative"
                ] += 1
            for check_id in false_positive:
                ecosystem = actual_ecosystem(case, check_id)
                ecosystem_metrics.setdefault(ecosystem, collections.Counter())[
                    "false_positive"
                ] += 1
            if false_positive or false_negative:
                mismatches.append(
                    {
                        "case_id": case["id"],
                        "false_negative": sorted(false_negative),
                        "false_positive": sorted(false_positive),
                    }
                )

            expected_gaps = sorted(case["expected_coverage_gaps"])
            if actual_gaps != expected_gaps:
                gap_mismatches.append(
                    {
                        "case_id": case["id"],
                        "actual": actual_gaps,
                        "expected": expected_gaps,
                        "workflow_observations": actual_observations,
                    }
                )
            for relative in case["tripwire_paths"]:
                path = safe_fixture_path(workspace, relative)
                if path.exists():
                    tripwire_failures.append(
                        {"case_id": case["id"], "path": relative}
                    )

    overall_metric = metric(overall)
    requirements = corpus["requirements"]
    if overall_metric["precision"] < requirements["minimum_precision"]:
        violations.append(
            f"precision {overall_metric['precision']} is below "
            f"{requirements['minimum_precision']}"
        )
    if overall_metric["recall"] < requirements["minimum_recall"]:
        violations.append(
            f"recall {overall_metric['recall']} is below "
            f"{requirements['minimum_recall']}"
        )
    if gap_mismatches:
        violations.append(
            f"{len(gap_mismatches)} case(s) emitted unexpected coverage gaps"
        )
    if tripwire_failures:
        violations.append(
            f"{len(tripwire_failures)} discovery tripwire(s) were executed"
        )

    per_ecosystem = {
        ecosystem: metric(counter)
        for ecosystem, counter in sorted(ecosystem_metrics.items())
        if sum(counter.values())
    }
    return {
        "schema_version": METRICS_SCHEMA_VERSION,
        "corpus": {
            "schema_version": corpus["schema_version"],
            "sha256": hashlib.sha256(corpus_bytes).hexdigest(),
        },
        "projects": {
            "total": len(corpus["cases"]),
            "by_ecosystem": dict(sorted(ecosystem_project_counts.items())),
            "by_shape": dict(sorted(shape_counts.items())),
        },
        "accuracy": {
            "overall": overall_metric,
            "per_ecosystem": per_ecosystem,
        },
        "check_mismatches": mismatches,
        "coverage_gap_mismatches": gap_mismatches,
        "tripwire_failures": tripwire_failures,
        "requirements": requirements,
        "violations": sorted(violations),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument(
        "--corpus",
        type=Path,
        default=Path(__file__).with_name("corpus.json"),
    )
    parser.add_argument("--expected", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    metrics = evaluate(args.binary.resolve(), args.corpus)
    encoded = json.dumps(metrics, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(encoded, encoding="utf-8")
    baseline_matches = True
    if args.expected:
        expected = args.expected.read_text(encoding="utf-8")
        if encoded != expected:
            baseline_matches = False
            print("discovery metrics differ from the pinned baseline:")
            print(
                "".join(
                    difflib.unified_diff(
                        expected.splitlines(keepends=True),
                        encoded.splitlines(keepends=True),
                        fromfile=str(args.expected),
                        tofile=str(args.output or "actual metrics"),
                    )
                ),
                end="",
            )
    print(
        "evaluated "
        f"{metrics['projects']['total']} projects: "
        f"precision={metrics['accuracy']['overall']['precision']:.3f}, "
        f"recall={metrics['accuracy']['overall']['recall']:.3f}"
    )
    if metrics["violations"]:
        for violation in metrics["violations"]:
            print(f"violation: {violation}")
        return 1
    return 0 if baseline_matches else 1


if __name__ == "__main__":
    raise SystemExit(main())
