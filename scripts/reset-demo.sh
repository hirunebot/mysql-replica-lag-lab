#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
cd "${project_dir}"

echo "Stopping containers and deleting the demo-only MySQL volumes..."
docker compose down --volumes --remove-orphans
echo "Demo environment reset completed."
