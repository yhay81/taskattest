#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
result_path="${1:-${root_dir}/benchmark-results.json}"
binary="${root_dir}/target/release/taskattest"
generator="${root_dir}/benchmarks/generate_workspace.py"

for dependency in cargo git jq python3 stat timeout uname; do
  command -v "${dependency}" >/dev/null || {
    printf 'missing benchmark dependency: %s\n' "${dependency}" >&2
    exit 1
  }
done

if ! /usr/bin/time --version 2>&1 | grep -qi 'GNU time'; then
  printf 'benchmarks/run.sh requires GNU /usr/bin/time (the Ubuntu runner provides it)\n' >&2
  exit 1
fi

temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT

workspace="${temp_dir}/workspace"
state_dir="${temp_dir}/state"
receipt="${temp_dir}/receipt.json"
setup_output="${temp_dir}/setup-receipt.json"
discovery_metrics="${temp_dir}/discover.metrics.json"
discovery_output="${temp_dir}/discover.output.json"
verify_metrics="${temp_dir}/verify.metrics.json"
verify_output="${temp_dir}/verify.output.json"

cd "${root_dir}"
cargo build --release --locked
python3 "${generator}" --files 10000 --checks 100 --output "${workspace}"

"${binary}" \
  --workspace "${workspace}" \
  --state-dir "${state_dir}" \
  --format json \
  --quiet \
  run \
  --receipt-out "${receipt}" >"${setup_output}"

measure() {
  local metrics_path="$1"
  local output_path="$2"
  shift 2

  /usr/bin/time \
    -f '{"wall_seconds": %e, "max_rss_kib": %M, "exit_code": %x}' \
    -o "${metrics_path}" \
    timeout --signal=KILL 45s "$@" >"${output_path}"
  jq -e . "${metrics_path}" >/dev/null
  jq -e . "${output_path}" >/dev/null
}

measure "${discovery_metrics}" "${discovery_output}" \
  "${binary}" \
  --workspace "${workspace}" \
  --format json \
  discover

measure "${verify_metrics}" "${verify_output}" \
  "${binary}" \
  --workspace "${workspace}" \
  --state-dir "${state_dir}" \
  --format json \
  verify "${receipt}"

mkdir -p "$(dirname "${result_path}")"
jq -n \
  --arg generated_at "$(date -u +'%Y-%m-%dT%H:%M:%SZ')" \
  --arg git_sha "$(git rev-parse HEAD)" \
  --arg fixture_commit "$(git -C "${workspace}" rev-parse HEAD)" \
  --arg runner_os "${RUNNER_OS:-Linux}" \
  --arg runner_arch "$(uname -m)" \
  --arg runner_image "${ImageOS:-unknown}" \
  --arg runner_image_version "${ImageVersion:-unknown}" \
  --argjson receipt_bytes "$(stat -c '%s' "${receipt}")" \
  --argjson discovery_output_bytes "$(stat -c '%s' "${discovery_output}")" \
  --argjson verify_output_bytes "$(stat -c '%s' "${verify_output}")" \
  --slurpfile setup_output "${setup_output}" \
  --slurpfile discovery_metrics "${discovery_metrics}" \
  --slurpfile discovery_output "${discovery_output}" \
  --slurpfile verify_metrics "${verify_metrics}" \
  --slurpfile verify_output "${verify_output}" \
  '{
    schema_version: "taskattest.benchmark.v1",
    generated_at: $generated_at,
    git_sha: $git_sha,
    runner: {
      os: $runner_os,
      arch: $runner_arch,
      image: $runner_image,
      image_version: $runner_image_version
    },
    fixture: {
      id: "workspace_10k_files_100_checks",
      generator: "benchmarks/generate_workspace.py",
      git_commit: $fixture_commit,
      workspace_files: $discovery_output[0].source.workspace_file_count,
      discovered_checks: ($discovery_output[0].checks | length),
      selected_checks:
        ([$discovery_output[0].selection[] | select(.selected)] | length),
      receipt_bytes: $receipt_bytes,
      receipt_checks: ($setup_output[0].checks | length),
      receipt_log_references:
        ([
          $setup_output[0].checks[]
          | .stdout, .stderr
          | select(. != null)
        ] | length)
    },
    measurements: [
      {
        id: "discover_10k_files_100_checks",
        process: $discovery_metrics[0],
        output_bytes: $discovery_output_bytes,
        result: {
          schema_version: $discovery_output[0].schema_version,
          workspace_files: $discovery_output[0].source.workspace_file_count,
          checks: ($discovery_output[0].checks | length),
          selected_checks:
            ([$discovery_output[0].selection[] | select(.selected)] | length),
          coverage_gaps: ($discovery_output[0].coverage_gaps | length)
        }
      },
      {
        id: "verify_100_check_receipt",
        process: $verify_metrics[0],
        output_bytes: $verify_output_bytes,
        result: {
          schema_version: $verify_output[0].schema_version,
          valid: $verify_output[0].valid,
          blobs_verified: ($verify_output[0].blobs | length),
          problems: $verify_output[0].problems
        }
      }
    ],
    derived: {
      peak_rss_mib:
        ([
          $discovery_metrics[0].max_rss_kib,
          $verify_metrics[0].max_rss_kib
        ] | max | . / 1024)
    },
    threshold_status: "observation_only"
  }' >"${result_path}"

jq -e '
  .schema_version == "taskattest.benchmark.v1"
  and .fixture.workspace_files == 10000
  and .fixture.discovered_checks == 100
  and .fixture.selected_checks == 100
  and .fixture.receipt_checks == 100
  and .fixture.receipt_log_references == 200
  and all(
    .measurements[];
    .process.exit_code == 0
      and .process.wall_seconds >= 0
      and .process.max_rss_kib > 0
      and .output_bytes > 0
  )
  and any(
    .measurements[];
    .id == "discover_10k_files_100_checks"
      and .result.workspace_files == 10000
      and .result.checks == 100
      and .result.selected_checks == 100
      and .result.coverage_gaps == 0
  )
  and any(
    .measurements[];
    .id == "verify_100_check_receipt"
      and .result.valid
      and .result.blobs_verified == 200
      and (.result.problems | length == 0)
  )
' "${result_path}" >/dev/null

printf 'wrote %s\n' "${result_path}"
