#!/usr/bin/env bash
# Runs shellcheck on every shell script in git, the git hooks too. Run tools/install.sh first.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
git ls-files '*.sh' '.githooks/*' | xargs "$root/.tools/bin/shellcheck"
