#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
cd "${project_dir}"

mysql_in() {
  local service="$1"
  shift
  docker compose exec -T --env MYSQL_PWD=root "${service}" \
    mysql --user=root --batch --skip-column-names "$@"
}

wait_for_mysql() {
  local service="$1"

  for _ in $(seq 1 60); do
    if mysql_in "${service}" --execute="SELECT 1" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done

  echo "${service} did not become ready" >&2
  return 1
}

echo "Waiting for Source and Replica..."
wait_for_mysql source
wait_for_mysql replica

echo "Configuring GTID auto-position replication..."
mysql_in replica --execute="
STOP REPLICA;
RESET REPLICA ALL;
CHANGE REPLICATION SOURCE TO
  SOURCE_HOST='source',
  SOURCE_PORT=3306,
  SOURCE_USER='repl',
  SOURCE_PASSWORD='replpass',
  SOURCE_AUTO_POSITION=1,
  SOURCE_CONNECT_RETRY=1,
  GET_SOURCE_PUBLIC_KEY=1;
START REPLICA;
SET PERSIST read_only = ON;
SET PERSIST super_read_only = ON;
"

for _ in $(seq 1 60); do
  connection_state="$(mysql_in replica --execute="
    SELECT SERVICE_STATE
    FROM performance_schema.replication_connection_status
    LIMIT 1;
  " 2>/dev/null || true)"
  applier_state="$(mysql_in replica --execute="
    SELECT SERVICE_STATE
    FROM performance_schema.replication_applier_status
    LIMIT 1;
  " 2>/dev/null || true)"

  if [[ "${connection_state}" == "ON" && "${applier_state}" == "ON" ]]; then
    seconds_behind="$(mysql_in replica --execute="
      SELECT COALESCE(
        MAX(TIMESTAMPDIFF(SECOND, LAST_APPLIED_TRANSACTION_ORIGINAL_COMMIT_TIMESTAMP, NOW(6))),
        0
      )
      FROM performance_schema.replication_applier_status_by_worker
      WHERE LAST_APPLIED_TRANSACTION <> '';
    " 2>/dev/null || true)"
    echo "Replication is running."
    echo "  Source_Host: source"
    echo "  Replica_IO_Running: Yes"
    echo "  Replica_SQL_Running: Yes"
    echo "  Approximate lag: ${seconds_behind:-0}s"
    exit 0
  fi
  sleep 1
done

echo "Replication did not start successfully." >&2
docker compose exec -T --env MYSQL_PWD=root replica \
  mysql --user=root --execute="SHOW REPLICA STATUS\\G" >&2 || true
exit 1
