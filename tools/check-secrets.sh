#!/usr/bin/env bash
# Fails on a Secret that is not encrypted: a file in secrets/ that is not
# <namespace>/<name>.sops.yaml or that sops did not encrypt, and a Secret with data in the build
# of clusters/tenyard or of one of its layers. Needs no key. Run tools/install.sh first. With
# --staged (the pre-commit hook) it reads the files of secrets/ as the next commit holds them,
# not as they are on disk.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$root/.tools/bin"
cd "$root"
staged=0
if [ "${1:-}" = --staged ]; then staged=1; fi
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

failed=0
while IFS= read -r file; do
  case "$file" in
    secrets/*/*.sops.yaml) ;;
    *) echo "$file: secrets/ takes only <namespace>/<name>.sops.yaml" >&2; failed=1; continue ;;
  esac
  copy="$file"
  if [ "$staged" = 1 ]; then copy="$tmp/staged.yaml"; git show ":$file" > "$copy"; fi
  if [ "$("$bin/sops" filestatus "$copy" 2> /dev/null)" != '{"encrypted":true}' ]; then
    echo "$file: not encrypted by sops" >&2
    failed=1
  fi
done < <(git ls-files secrets)

# An empty Secret (a state Secret a pod writes into) may live in the build; one with values not.
plain="$(for dir in clusters/tenyard install/cluster clusters/tenyard/infrastructure clusters/tenyard/apps; do
  "$bin/kustomize" build "$dir" || exit 1
  echo ---
done | awk '
  /^---/ { if (secret && values) print name; secret = 0; values = 0; name = ""; next }
  /^kind: Secret$/ { secret = 1 }
  /^  name: / && name == "" { name = $2 }
  /^(data|stringData):/ && $0 !~ /: \{\}$/ { values = 1 }
  END { if (secret && values) print name }')"
if [ -n "$plain" ]; then
  echo "a Secret with values in clusters/tenyard: $plain (it goes to secrets/, encrypted)" >&2
  failed=1
fi
exit "$failed"
