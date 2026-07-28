#!/usr/bin/env python3
"""Generate a deterministic Git workspace for TaskAttest benchmarks."""

from __future__ import annotations

import argparse
import os
import pathlib
import subprocess


def positive_integer(value: str) -> int:
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("value must be at least 1")
    return parsed


def run_git(workspace: pathlib.Path, *arguments: str) -> None:
    environment = os.environ.copy()
    environment.update(
        {
            "GIT_AUTHOR_DATE": "2020-01-01T00:00:00Z",
            "GIT_COMMITTER_DATE": "2020-01-01T00:00:00Z",
        }
    )
    subprocess.run(
        ["git", "-C", str(workspace), *arguments],
        check=True,
        env=environment,
        stdout=subprocess.DEVNULL,
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--files", required=True, type=positive_integer)
    parser.add_argument("--checks", required=True, type=positive_integer)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()

    if args.files < 2:
        parser.error("files must include at least one payload plus .taskattest.toml")
    if args.output.exists():
        parser.error("output must not already exist")

    args.output.mkdir(parents=True)
    for index in range(args.files - 1):
        path = args.output / "files" / f"{index // 100:04d}" / f"{index:09d}.txt"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f"fixture-{index:09d}\n", encoding="utf-8")

    config_lines = ["version = 1", ""]
    for index in range(args.checks):
        config_lines.extend(
            [
                "[[checks]]",
                f'id = "benchmark-{index:03d}"',
                f'label = "Benchmark check {index:03d}"',
                'kind = "test"',
                "command = [",
                '  "python3",',
                '  "-c",',
                f'  "print(\'taskattest-benchmark-{index:03d}\')",',
                "]",
                'reason = "deterministic benchmark fixture"',
                'coverage_paths = ["**"]',
                "",
            ]
        )
    (args.output / ".taskattest.toml").write_text(
        "\n".join(config_lines),
        encoding="utf-8",
    )

    subprocess.run(
        ["git", "init", "--quiet", "--initial-branch=main", str(args.output)],
        check=True,
    )
    run_git(args.output, "config", "user.email", "benchmark@example.invalid")
    run_git(args.output, "config", "user.name", "TaskAttest Benchmark")
    run_git(args.output, "config", "commit.gpgsign", "false")
    run_git(args.output, "config", "core.autocrlf", "false")
    run_git(args.output, "add", ".")
    run_git(args.output, "commit", "--quiet", "-m", "deterministic fixture")


if __name__ == "__main__":
    main()
