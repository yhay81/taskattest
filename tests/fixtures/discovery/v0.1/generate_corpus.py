#!/usr/bin/env python3
"""Generate the deterministic TaskAttest discovery accuracy corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "taskattest.discovery-corpus/v0.1"
NO_CHECKS_GAP = (
    "no checks discovered; add a supported manifest or .taskattest.toml"
)


def package_json(scripts: dict[str, str]) -> str:
    return json.dumps(
        {"name": "discovery-fixture", "private": True, "scripts": scripts},
        indent=2,
        sort_keys=True,
    ) + "\n"


def cargo_manifest(index: int) -> str:
    if index % 4 == 0:
        return (
            '[workspace]\nresolver = "2"\nmembers = ["crates/core"]\n\n'
            "[workspace.package]\nversion = \"0.1.0\"\n"
        )
    return (
        "[package]\n"
        f'name = "discovery-fixture-{index:02d}"\n'
        'version = "0.1.0"\n'
        'edition = "2024"\n'
    )


def go_manifest(index: int) -> str:
    return f"module example.invalid/discovery/fixture{index:02d}\n\ngo 1.23\n"


def explicit_config(checks: list[dict[str, str]]) -> str:
    lines = ["version = 1", ""]
    for check in checks:
        lines.extend(
            [
                "[[checks]]",
                f'id = {json.dumps(check["id"])}',
                f'label = {json.dumps(check["label"])}',
                f'kind = {json.dumps(check["kind"])}',
                f'command = {json.dumps(check["command"])}',
                f'working_directory = {json.dumps(check["working_directory"])}',
                f'reason = {json.dumps(check["reason"])}',
                f'coverage_paths = {json.dumps(check["coverage_paths"])}',
                "",
            ]
        )
    return "\n".join(lines)


def workflow(step_name: str, run: str) -> str:
    run_value = json.dumps(run)
    return (
        "name: Discovery corpus\n"
        "on: push\n"
        "jobs:\n"
        "  quality:\n"
        "    runs-on: ubuntu-latest\n"
        "    steps:\n"
        f"      - name: {step_name}\n"
        f"        run: {run_value}\n"
    )


def infer_ecosystem(check_id: str) -> str:
    for prefix, ecosystem in (
        ("js-", "javascript"),
        ("python-", "python"),
        ("rust-", "rust"),
        ("go-", "go"),
    ):
        if check_id.startswith(prefix):
            return ecosystem
    return "workflow"


def build_corpus() -> dict[str, Any]:
    cases: list[dict[str, Any]] = []

    def add(
        case_id: str,
        ecosystems: list[str],
        shape: str,
        description: str,
        files: dict[str, str],
        expected_check_ids: list[str],
        *,
        expected_coverage_gaps: list[str] | None = None,
        tripwire_paths: list[str] | None = None,
        check_ecosystems: dict[str, str] | None = None,
    ) -> None:
        mapping = {
            check_id: infer_ecosystem(check_id)
            for check_id in expected_check_ids
        }
        mapping.update(check_ecosystems or {})
        cases.append(
            {
                "id": case_id,
                "ecosystems": ecosystems,
                "shape": shape,
                "description": description,
                "files": files,
                "expected_check_ids": sorted(expected_check_ids),
                "check_ecosystems": {
                    check_id: mapping[check_id]
                    for check_id in sorted(mapping)
                },
                "expected_coverage_gaps": sorted(expected_coverage_gaps or []),
                "tripwire_paths": sorted(tripwire_paths or []),
            }
        )

    javascript_cases = [
        (
            {"test": "vitest run", "lint": "eslint .", "build": "tsc",
             "typecheck": "tsc --noEmit", "format:check": "prettier --check ."},
            ["js-build", "js-format-check", "js-lint", "js-test", "js-typecheck"],
            {"package-lock.json": "{}\n"},
            "standard npm verification scripts",
        ),
        ({"test": "vitest run"}, ["js-test"], {"pnpm-lock.yaml": "lockfileVersion: '9.0'\n"}, "pnpm test"),
        ({"lint:ci": "eslint .", "build": "vite build"}, ["js-build", "js-lint-ci"], {"yarn.lock": "# fixture\n"}, "Yarn lint and build"),
        ({"typecheck": "tsc --noEmit", "format:check": "prettier --check ."}, ["js-format-check", "js-typecheck"], {"bun.lock": "fixture\n"}, "Bun type and format checks"),
        ({"test:unit": "node --test", "lint": "eslint ."}, ["js-lint", "js-test-unit"], {"npm-shrinkwrap.json": "{}\n"}, "named npm checks"),
        ({"test:watch": "vitest", "dev": "vite", "start": "node app.js"}, [], {}, "interactive scripts are omitted"),
        ({"test": "vitest --watch", "lint": "eslint . --fix", "format:check": "prettier . --write"}, [], {}, "mutating and watch commands are omitted"),
        ({"check-types:ci": "tsc --noEmit"}, ["js-check-types-ci"], {}, "check-types alias"),
        ({"type-check": "tsc --noEmit"}, ["js-type-check"], {}, "type-check alias"),
        ({"tsc": "tsc --noEmit"}, ["js-tsc"], {}, "tsc alias"),
        ({"fmt:check": "dprint check"}, ["js-fmt-check"], {}, "fmt check name"),
        ({"format": "prettier --check ."}, ["js-format"], {}, "format check command"),
        ({"test:ci": "node --test", "lint:ci": "eslint .", "build:prod": "vite build"}, ["js-build-prod", "js-lint-ci", "js-test-ci"], {}, "colon-qualified checks"),
        ({"contest": "node contest.js", "rebuild": "node rebuild.js", "start:test": "node app.js"}, [], {}, "verification substrings are not check names"),
        ({"test": "python3 -c \"from pathlib import Path; Path('taskattest-package-script-tripwire').write_text('ran')\""}, ["js-test"], {}, "package scripts are described but never run"),
        ({}, [], {}, "empty package scripts"),
        ({"test": "node --test", "test:snapshot": "vitest -u"}, ["js-test"], {}, "snapshot updater is omitted"),
        ({"lint": "eslint .", "lint:update": "eslint . --fix"}, ["js-lint"], {}, "lint updater is omitted"),
        ({"build:ci": "vite build", "dev:build": "vite"}, ["js-build-ci"], {}, "build and development distinction"),
        ({"test": "node --test", "lint": "eslint .", "build": "tsc", "typecheck": "tsc --noEmit"}, ["js-build", "js-lint", "js-test", "js-typecheck"], {"package-lock.json": "{}\n"}, "complete npm baseline"),
    ]
    for index, (scripts, expected, extra_files, description) in enumerate(
        javascript_cases, start=1
    ):
        files = {"package.json": package_json(scripts), **extra_files}
        tripwires = (
            ["taskattest-package-script-tripwire"] if index == 15 else []
        )
        add(
            f"javascript-{index:02d}",
            ["javascript"],
            "single-package",
            description,
            files,
            expected,
            expected_coverage_gaps=[] if expected else [NO_CHECKS_GAP],
            tripwire_paths=tripwires,
        )

    python_cases = [
        ('[project]\nname = "fixture"\nversion = "0.1.0"\ndependencies = ["pytest>=8", "ruff>=0.5", "mypy>=1"]\n', ["python-mypy", "python-ruff", "python-test"], {}, "PEP 621 dependencies"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\n[project.optional-dependencies]\ntest = ["pytest>=8"]\n', ["python-test"], {}, "optional dependency group"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\n[dependency-groups]\ndev = ["ruff", "mypy"]\n', ["python-mypy", "python-ruff"], {}, "PEP 735 dependency group"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\n[tool.pytest.ini_options]\ntestpaths = ["tests"]\n[tool.ruff]\nline-length = 100\n[tool.mypy]\nstrict = true\n', ["python-mypy", "python-ruff", "python-test"], {}, "tool configuration tables"),
        ('[tool.poetry]\nname = "fixture"\nversion = "0.1.0"\n[tool.poetry.group.dev.dependencies]\npytest = "^8"\nruff = "^0.5"\n', ["python-ruff", "python-test"], {}, "Poetry dependency group"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\n[tool.uv]\ndev-dependencies = ["pytest", "mypy"]\n', ["python-mypy", "python-test"], {}, "uv development dependencies"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\n[tool.pdm.dev-dependencies]\nquality = ["ruff", "mypy"]\n', ["python-mypy", "python-ruff"], {}, "PDM development dependencies"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\n[tool.hatch.envs.default]\ndependencies = ["pytest", "ruff"]\n', ["python-ruff", "python-test"], {}, "Hatch environment dependencies"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\n', ["python-ruff", "python-test"], {"requirements.txt": "pytest==8.3\nruff>=0.5\n"}, "requirements file"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\n', ["python-mypy", "python-test"], {"requirements-dev.txt": "mypy>=1\n", "tox.ini": "[tox]\nenv_list = py\n"}, "requirements-dev and tox"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\n', ["python-test"], {"tox.ini": "[tox]\nenv_list = py\n"}, "tox configuration"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\ndependencies = ["pytest-cov>=5"]\n', ["python-test"], {}, "pytest plugin evidence"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\ndependencies = ["ruff @ https://example.invalid/ruff.whl"]\n', ["python-ruff"], {}, "PEP 508 direct reference"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\ndependencies = ["PyTest>=8", "RUFF==0.5", "MyPy~=1.10"]\n', ["python-mypy", "python-ruff", "python-test"], {}, "case-normalized distribution names"),
        ('[project]\nname = "scruffy"\nversion = "0.1.0"\ndependencies = ["scruffy>=0.3", "mypy-boto3-s3>=1"]\n', [], {}, "irrelevant dependency substrings"),
        ('[build-system]\nrequires = ["ruff-build-helper"]\nbuild-backend = "fixture.backend"\n[project]\nname = "pytest-mypy-not-a-tool"\nversion = "0.1.0"\ndescription = "ruff and pytest are words, not declared tools"\n', [], {}, "metadata substrings are not tool evidence"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\n', [], {"requirements.txt": "https://example.invalid/ruff.whl\n-e ../pytest-helper\n"}, "bare URLs and editable paths are not distributions"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\ndependencies = ["tox", "ruff"]\n', ["python-ruff", "python-test"], {}, "tox distribution fallback"),
        ('[tool.poetry]\nname = "fixture"\nversion = "0.1.0"\n[tool.poetry.dependencies]\npython = "^3.11"\npytest = "^8"\nmypy = "^1"\n', ["python-mypy", "python-test"], {}, "Poetry main dependencies"),
        ('[project]\nname = "fixture"\nversion = "0.1.0"\n[project.optional-dependencies]\ntest = ["pytest"]\nquality = ["ruff", "mypy"]\n', ["python-mypy", "python-ruff", "python-test"], {}, "multiple optional groups"),
    ]
    for index, (pyproject, expected, extra_files, description) in enumerate(
        python_cases, start=1
    ):
        files = {"pyproject.toml": pyproject, **extra_files}
        add(
            f"python-{index:02d}",
            ["python"],
            "single-package",
            description,
            files,
            expected,
            expected_coverage_gaps=[] if expected else [NO_CHECKS_GAP],
        )

    rust_checks = ["rust-build", "rust-format", "rust-lint", "rust-test"]
    for index in range(1, 21):
        files = {"Cargo.toml": cargo_manifest(index)}
        if index % 2 == 0:
            files["Cargo.lock"] = "version = 4\n"
        if index == 4:
            files["crates/core/Cargo.toml"] = (
                '[package]\nname = "core"\nversion = "0.1.0"\nedition = "2024"\n'
            )
            files["crates/core/src/lib.rs"] = "pub fn value() -> u8 { 1 }\n"
        if index == 15:
            files["build.rs"] = (
                'fn main() { std::fs::write("taskattest-rust-tripwire", "ran").unwrap(); }\n'
            )
        add(
            f"rust-{index:02d}",
            ["rust"],
            "single-package",
            "Rust manifest discovery" + (" with lockfile" if index % 2 == 0 else ""),
            files,
            rust_checks,
            tripwire_paths=["taskattest-rust-tripwire"] if index == 15 else [],
        )

    go_checks = ["go-build", "go-test", "go-vet"]
    for index in range(1, 21):
        files = {"go.mod": go_manifest(index)}
        if index % 2 == 0:
            files["go.sum"] = (
                "example.invalid/dependency v0.0.0 "
                "h1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\n"
            )
        if index == 15:
            files["tripwire_test.go"] = (
                "package fixture\n"
                "import (\"os\"; \"testing\")\n"
                "func TestTripwire(t *testing.T) { "
                "os.WriteFile(\"taskattest-go-tripwire\", []byte(\"ran\"), 0o600) }\n"
            )
        add(
            f"go-{index:02d}",
            ["go"],
            "single-package",
            "Go module discovery" + (" with checksum file" if index % 2 == 0 else ""),
            files,
            go_checks,
            tripwire_paths=["taskattest-go-tripwire"] if index == 15 else [],
        )

    add(
        "mixed-01", ["javascript", "python"], "mixed",
        "JavaScript and Python root project",
        {
            "package.json": package_json({"test": "node --test"}),
            "pyproject.toml": '[project]\nname = "fixture"\nversion = "0.1.0"\ndependencies = ["ruff"]\n',
        },
        ["js-test", "python-ruff"],
    )
    add(
        "mixed-02", ["javascript", "rust"], "mixed",
        "JavaScript and Rust root project",
        {"package.json": package_json({"lint": "eslint ."}), "Cargo.toml": cargo_manifest(21)},
        ["js-lint", *rust_checks],
    )
    add(
        "mixed-03", ["javascript", "go"], "mixed",
        "JavaScript and Go root project",
        {"package.json": package_json({"build": "tsc"}), "go.mod": go_manifest(21)},
        ["js-build", *go_checks],
    )
    add(
        "mixed-04", ["python", "rust"], "mixed",
        "Python and Rust root project",
        {"pyproject.toml": '[project]\nname = "fixture"\nversion = "0.1.0"\ndependencies = ["mypy"]\n', "Cargo.toml": cargo_manifest(22)},
        ["python-mypy", *rust_checks],
    )
    add(
        "mixed-05", ["python", "go"], "mixed",
        "Python and Go root project",
        {"pyproject.toml": '[project]\nname = "fixture"\nversion = "0.1.0"\ndependencies = ["pytest"]\n', "go.mod": go_manifest(22)},
        ["python-test", *go_checks],
    )
    add(
        "mixed-06", ["rust", "go"], "mixed",
        "Rust and Go root project",
        {"Cargo.toml": cargo_manifest(23), "go.mod": go_manifest(23)},
        [*rust_checks, *go_checks],
    )
    add(
        "mixed-07", ["javascript", "python", "rust", "go"], "mixed",
        "all four root ecosystems",
        {
            "package.json": package_json({"test": "node --test", "typecheck": "tsc --noEmit"}),
            "pyproject.toml": '[project]\nname = "fixture"\nversion = "0.1.0"\ndependencies = ["pytest", "ruff"]\n',
            "Cargo.toml": cargo_manifest(24),
            "go.mod": go_manifest(24),
        },
        ["js-test", "js-typecheck", "python-ruff", "python-test", *rust_checks, *go_checks],
    )
    add(
        "mixed-08", ["javascript"], "mixed",
        "safe workflow command matches a package check",
        {
            "package.json": package_json({"test": "node --test"}),
            ".github/workflows/ci.yml": workflow("Test", "npm run test"),
        },
        ["js-test"],
    )
    unsafe_workflow = workflow(
        "Test tripwire",
        "python3 -c \"from pathlib import Path; Path('discovery-workflow-tripwire').write_text('ran')\"",
    )
    add(
        "mixed-09", ["go", "workflow"], "mixed",
        "unsafe workflow verification is a coverage gap and is not run",
        {"go.mod": go_manifest(25), ".github/workflows/ci.yml": unsafe_workflow},
        go_checks,
        expected_coverage_gaps=[
            "verification workflow step is not safely modeled: "
            ".github/workflows/ci.yml#quality#Test tripwire"
        ],
        tripwire_paths=["discovery-workflow-tripwire"],
    )
    safe_run = "cargo test --locked"
    safe_id = "ci-test-" + hashlib.sha256(safe_run.encode()).hexdigest()[:8]
    add(
        "mixed-10", ["rust", "workflow"], "mixed",
        "safe workflow-only Rust check",
        {".github/workflows/ci.yml": workflow("Test", safe_run)},
        [safe_id],
        check_ecosystems={safe_id: "rust"},
    )

    monorepo_specs = [
        (
            ["javascript"],
            [("mono-js-test", "JavaScript package tests", "test", ["npm", "run", "test"], "packages/web", ["packages/web/**"], "javascript")],
            {"packages/web/package.json": package_json({"test": "node --test"})},
        ),
        (
            ["python"],
            [("mono-python-test", "Python package tests", "test", ["python3", "-m", "pytest"], "packages/api", ["packages/api/**"], "python")],
            {"packages/api/pyproject.toml": '[project]\nname = "api"\nversion = "0.1.0"\ndependencies = ["pytest"]\n'},
        ),
        (
            ["rust"],
            [("mono-rust-test", "Rust package tests", "test", ["cargo", "test", "--locked"], "crates/core", ["crates/core/**"], "rust")],
            {"crates/core/Cargo.toml": cargo_manifest(31)},
        ),
        (
            ["go"],
            [("mono-go-test", "Go package tests", "test", ["go", "test", "./..."], "services/api", ["services/api/**"], "go")],
            {"services/api/go.mod": go_manifest(31)},
        ),
        (
            ["javascript", "python"],
            [
                ("mono-js-lint", "JavaScript package lint", "lint", ["npm", "run", "lint"], "packages/web", ["packages/web/**"], "javascript"),
                ("mono-python-lint", "Python package lint", "lint", ["python3", "-m", "ruff", "check", "."], "packages/api", ["packages/api/**"], "python"),
            ],
            {"packages/web/package.json": package_json({"lint": "eslint ."}), "packages/api/pyproject.toml": '[project]\nname = "api"\nversion = "0.1.0"\ndependencies = ["ruff"]\n'},
        ),
        (
            ["rust", "go"],
            [
                ("mono-rust-build", "Rust package build", "build", ["cargo", "build", "--locked"], "crates/core", ["crates/core/**"], "rust"),
                ("mono-go-build", "Go service build", "build", ["go", "build", "./..."], "services/api", ["services/api/**"], "go"),
            ],
            {"crates/core/Cargo.toml": cargo_manifest(32), "services/api/go.mod": go_manifest(32)},
        ),
        (
            ["javascript", "rust"],
            [
                ("mono-js-typecheck", "JavaScript package types", "type_check", ["npm", "run", "typecheck"], "packages/web", ["packages/web/**"], "javascript"),
                ("mono-rust-lint", "Rust package lint", "lint", ["cargo", "clippy", "--all-targets"], "crates/core", ["crates/core/**"], "rust"),
            ],
            {"packages/web/package.json": package_json({"typecheck": "tsc --noEmit"}), "crates/core/Cargo.toml": cargo_manifest(33)},
        ),
        (
            ["python", "go"],
            [
                ("mono-python-types", "Python package types", "type_check", ["python3", "-m", "mypy", "."], "packages/api", ["packages/api/**"], "python"),
                ("mono-go-vet", "Go service vet", "lint", ["go", "vet", "./..."], "services/api", ["services/api/**"], "go"),
            ],
            {"packages/api/pyproject.toml": '[project]\nname = "api"\nversion = "0.1.0"\ndependencies = ["mypy"]\n', "services/api/go.mod": go_manifest(33)},
        ),
        (
            ["javascript", "python", "rust", "go"],
            [
                ("mono-js-test-all", "JavaScript package tests", "test", ["npm", "run", "test"], "packages/web", ["packages/web/**"], "javascript"),
                ("mono-python-test-all", "Python package tests", "test", ["python3", "-m", "pytest"], "packages/api", ["packages/api/**"], "python"),
                ("mono-rust-test-all", "Rust package tests", "test", ["cargo", "test"], "crates/core", ["crates/core/**"], "rust"),
                ("mono-go-test-all", "Go service tests", "test", ["go", "test", "./..."], "services/go-api", ["services/go-api/**"], "go"),
            ],
            {
                "packages/web/package.json": package_json({"test": "node --test"}),
                "packages/api/pyproject.toml": '[project]\nname = "api"\nversion = "0.1.0"\ndependencies = ["pytest"]\n',
                "crates/core/Cargo.toml": cargo_manifest(34),
                "services/go-api/go.mod": go_manifest(34),
            },
        ),
        (
            ["python"],
            [("mono-python-quality", "Python package quality", "custom", ["python3", "-m", "compileall", "."], "packages/worker", ["packages/worker/**"], "python")],
            {"packages/worker/pyproject.toml": '[project]\nname = "worker"\nversion = "0.1.0"\n'},
        ),
    ]
    for index, (ecosystems, checks, files) in enumerate(monorepo_specs, start=1):
        config_checks = [
            {
                "id": check[0],
                "label": check[1],
                "kind": check[2],
                "command": check[3],
                "working_directory": check[4],
                "coverage_paths": check[5],
                "reason": "the monorepo declares an explicit argv-only package check",
            }
            for check in checks
        ]
        files = {".taskattest.toml": explicit_config(config_checks), **files}
        expected = [check[0] for check in checks]
        add(
            f"monorepo-{index:02d}",
            ecosystems,
            "monorepo",
            "explicit argv-only checks for nested packages",
            files,
            expected,
            check_ecosystems={check[0]: check[6] for check in checks},
        )

    assert len(cases) == 100, len(cases)
    assert len({case["id"] for case in cases}) == len(cases)
    return {
        "schema_version": SCHEMA_VERSION,
        "license": "MIT",
        "labeling_methodology": (
            "Expected check IDs and coverage gaps are hand-labeled from declared "
            "verification intent; generator logic only serializes those labels."
        ),
        "requirements": {
            "minimum_projects": 100,
            "minimum_projects_per_ecosystem": 20,
            "minimum_mixed_projects": 10,
            "minimum_monorepo_projects": 10,
            "minimum_precision": 0.95,
            "minimum_recall": 0.90,
        },
        "cases": cases,
    }


def encoded_corpus() -> bytes:
    return (
        json.dumps(build_corpus(), indent=2, sort_keys=True, ensure_ascii=False)
        + "\n"
    ).encode()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(__file__).with_name("corpus.json"),
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if --output differs from deterministic generator output",
    )
    args = parser.parse_args()
    generated = encoded_corpus()
    if args.check:
        actual = args.output.read_bytes() if args.output.is_file() else b""
        if actual != generated:
            raise SystemExit(
                f"{args.output} is stale; run {Path(__file__).name}"
            )
        print(f"verified {args.output} ({len(build_corpus()['cases'])} cases)")
        return 0
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(generated)
    print(f"wrote {args.output} ({len(build_corpus()['cases'])} cases)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
