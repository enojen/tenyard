#!/usr/bin/env bash
# Decrypts every Secret in git, secrets/<namespace>/<name>.sops.yaml, and applies it. With
# --diff it shows what would change instead (kubectl hides the values). Needs KUBECONFIG and the
# age key of Tenyard: SOPS_AGE_KEY_FILE, default ~/.config/tenyard/age.key. Run tools/install.sh
# first.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$root/.tools/bin"
cd "$root"
export SOPS_AGE_KEY_FILE="${SOPS_AGE_KEY_FILE:-$HOME/.config/tenyard/age.key}"
action=apply
if [ "${1:-}" = --diff ]; then action=diff; fi

status=0
while IFS= read -r file; do
  echo "$file" >&2
  "$bin/sops" --decrypt "$file" | kubectl "$action" -f - || status=$?
done < <(git ls-files 'secrets/*.sops.yaml')
# kubectl diff exits 1 when something differs.
exit "$status"
