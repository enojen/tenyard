#!/usr/bin/env bash
# Checks infra/ with no token and no state: the format, then `tofu validate` after an init
# without the backend (it downloads the provider of the lock file). Run tools/install.sh first.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$root/.tools/bin"
cd "$root/infra"
"$bin/tofu" fmt -check -diff
TF_DATA_DIR="$(mktemp -d)"
export TF_DATA_DIR
trap 'rm -rf "$TF_DATA_DIR"' EXIT
"$bin/tofu" init -backend=false -input=false -lockfile=readonly > /dev/null
"$bin/tofu" validate
