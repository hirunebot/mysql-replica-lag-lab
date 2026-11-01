#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
cd "${project_dir}"

mode="${1:-all}"
case "${mode}" in
  rust|php|all) ;;
  *)
    echo "Usage: $0 [rust|php|all]" >&2
    exit 2
    ;;
esac

e2e_rows="${E2E_ROWS:-200000}"
e2e_seed_batch_size="${E2E_SEED_BATCH_SIZE:-1000}"
e2e_heavy_batch_size="${E2E_HEAVY_BATCH_SIZE:-50000}"
e2e_marker_after_batches="${E2E_MARKER_AFTER_BATCHES:-2}"
e2e_poll_interval_ms="${E2E_POLL_INTERVAL_MS:-250}"
e2e_timeout_seconds="${E2E_TIMEOUT_SECS:-600}"
e2e_total_batches=$(((e2e_rows + e2e_heavy_batch_size - 1) / e2e_heavy_batch_size))

if ((e2e_total_batches <= e2e_marker_after_batches)); then
  echo "E2E configuration needs at least one heavy batch after the marker" >&2
  exit 2
fi

test_tmp_dir="$(mktemp -d)"
cleanup() {
  rm -f -- "${test_tmp_dir}/rust.log" "${test_tmp_dir}/php.log"
  rmdir "${test_tmp_dir}" 2>/dev/null || true
}
trap cleanup EXIT

mysql_in() {
  local service="$1"
  local query="$2"
  docker compose exec -T --env MYSQL_PWD=root "${service}" \
    mysql --user=root --batch --skip-column-names --execute="${query}"
}

fail() {
  echo "not ok - $*" >&2
  exit 1
}

assert_output() {
  local language="$1"
  local log_file="$2"

  grep -Fq "Initial synchronization completed" "${log_file}" \
    || fail "${language}: initial synchronization was not confirmed"
  grep -Fq "${e2e_marker_after_batches} heavy batches committed" "${log_file}" \
    || fail "${language}: marker was not placed after the expected batch"
  grep -Eq '\|[[:space:]]*1[[:space:]]*\|[[:space:]]*0[[:space:]]*\|' "${log_file}" \
    || fail "${language}: Source=1 / Replica=0 was not observed"
  grep -Fq "SUCCESS: Replica returned the old marker" "${log_file}" \
    || fail "${language}: stale read success was not reported"
  grep -Fq "Heavy workload: ${e2e_total_batches} batches, ${e2e_rows} affected rows" "${log_file}" \
    || fail "${language}: heavy workload summary did not match"
  grep -Fq "Demo completed with Source and Replica in sync" "${log_file}" \
    || fail "${language}: final synchronization was not reported"

  local marker_line
  local final_batch_line
  marker_line="$(grep -n -m1 "Updating the independent marker" "${log_file}" | cut -d: -f1 || true)"
  final_batch_line="$(grep -n "heavy batch ${e2e_total_batches}/${e2e_total_batches}:" "${log_file}" | tail -n1 | cut -d: -f1 || true)"
  [[ -n "${marker_line}" && -n "${final_batch_line}" ]] \
    || fail "${language}: could not determine marker and final batch order"
  ((final_batch_line > marker_line)) \
    || fail "${language}: no heavy batch continued after marker insertion"
}

assert_database_state() {
  local language="$1"
  local expected="1:${e2e_rows}:${e2e_rows}"
  local source_state
  local replica_state
  local replication_state
  local replica_settings

  source_state="$(mysql_in source "
    SELECT CONCAT(
      (SELECT value FROM lag_demo.marker WHERE id = 1), ':',
      (SELECT COUNT(*) FROM lag_demo.items), ':',
      (SELECT COUNT(*) FROM lag_demo.items WHERE version = 2)
    );
  ")"
  replica_state="$(mysql_in replica "
    SELECT CONCAT(
      (SELECT value FROM lag_demo.marker WHERE id = 1), ':',
      (SELECT COUNT(*) FROM lag_demo.items), ':',
      (SELECT COUNT(*) FROM lag_demo.items WHERE version = 2)
    );
  ")"
  replication_state="$(mysql_in replica "
    SELECT CONCAT(
      (SELECT SERVICE_STATE FROM performance_schema.replication_connection_status LIMIT 1), ':',
      (SELECT SERVICE_STATE FROM performance_schema.replication_applier_status LIMIT 1)
    );
  ")"
  replica_settings="$(mysql_in replica \
    "SELECT CONCAT(@@read_only, ':', @@super_read_only, ':', @@replica_parallel_workers);")"

  [[ "${source_state}" == "${expected}" ]] \
    || fail "${language}: unexpected Source state ${source_state}, expected ${expected}"
  [[ "${replica_state}" == "${expected}" ]] \
    || fail "${language}: unexpected Replica state ${replica_state}, expected ${expected}"
  [[ "${replication_state}" == "ON:ON" ]] \
    || fail "${language}: replication is not healthy (${replication_state})"
  [[ "${replica_settings}" == "1:1:1" ]] \
    || fail "${language}: Replica safety/worker settings changed (${replica_settings})"

  echo "ok - ${language}: stale read and final database state verified"
}

run_demo() {
  local language="$1"
  local log_file="${test_tmp_dir}/${language}.log"

  echo "== ${language} E2E (${e2e_rows} rows, ${e2e_total_batches} batches) =="
  if [[ "${language}" == "rust" ]]; then
    env \
      DEMO_ROWS="${e2e_rows}" \
      SEED_BATCH_SIZE="${e2e_seed_batch_size}" \
      HEAVY_BATCH_SIZE="${e2e_heavy_batch_size}" \
      MARKER_AFTER_BATCHES="${e2e_marker_after_batches}" \
      POLL_INTERVAL_MS="${e2e_poll_interval_ms}" \
      INITIAL_SYNC_TIMEOUT_SECS="${e2e_timeout_seconds}" \
      FINAL_SYNC_TIMEOUT_SECS="${e2e_timeout_seconds}" \
      cargo run --release 2>&1 | tee "${log_file}"
  else
    env \
      DEMO_ROWS="${e2e_rows}" \
      SEED_BATCH_SIZE="${e2e_seed_batch_size}" \
      HEAVY_BATCH_SIZE="${e2e_heavy_batch_size}" \
      MARKER_AFTER_BATCHES="${e2e_marker_after_batches}" \
      POLL_INTERVAL_MS="${e2e_poll_interval_ms}" \
      INITIAL_SYNC_TIMEOUT_SECS="${e2e_timeout_seconds}" \
      FINAL_SYNC_TIMEOUT_SECS="${e2e_timeout_seconds}" \
      php php/bin/demo.php 2>&1 | tee "${log_file}"
  fi

  assert_output "${language}" "${log_file}"
  assert_database_state "${language}"
}

command -v docker >/dev/null || fail "docker command is required"
if [[ "${mode}" == "php" || "${mode}" == "all" ]]; then
  command -v php >/dev/null || fail "php command is required"
fi

echo "== Starting MySQL test environment =="
docker info >/dev/null
docker compose up -d --wait
./scripts/configure-replication.sh

if [[ "${mode}" == "rust" || "${mode}" == "all" ]]; then
  run_demo rust
fi
if [[ "${mode}" == "php" || "${mode}" == "all" ]]; then
  run_demo php
fi

echo "All requested E2E tests passed."
