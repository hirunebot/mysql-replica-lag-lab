#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
cd "${project_dir}"

echo "== Rust formatting =="
cargo fmt --check

echo "== Rust Clippy =="
cargo clippy --all-targets --all-features -- -D warnings

echo "== Rust unit tests =="
cargo test

echo "== PHP syntax =="
while IFS= read -r -d '' file; do
  php -l "${file}"
done < <(find php -name '*.php' -type f -print0)

echo "== PHP unit tests =="
php php/tests/run.php

echo "== Shell syntax =="
bash -n scripts/configure-replication.sh
bash -n scripts/reset-demo.sh
bash -n scripts/test-unit.sh
bash -n scripts/test-e2e.sh

echo "== Compose configuration =="
docker compose config --quiet
docker compose --profile php config --quiet

echo "All unit and static tests passed."
