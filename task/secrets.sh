#!/usr/bin/env bash
# Writes the five Secrets a task Job reads, from the files on this computer. Run it again
# after a login or a token change: each Secret is replaced. Needs KUBECONFIG and kubectl.
# Until the tenbox login sync writes them itself (TEN-462), this is the way.
#
#   ghcr               the pull secret of the private image (GitHub token with read:packages)
#   github-token       token: gh and git push in the box
#   claude-credentials credentials.json: ~/.claude/.credentials.json
#   tracker            api_key: the tracker key, from tenbox.local.yaml of the main checkout
#   ntfy               url: the ntfy topic, from the same file
#
# Env: NAMESPACE (tenbox), GITHUB_USER (enojen), GITHUB_TOKEN_FILE (~/.config/tenbox/github-token),
# CLAUDE_CREDENTIALS_FILE (~/.claude/.credentials.json), LOCAL_MANIFEST (~/tenbox/tenbox.local.yaml).
set -euo pipefail
ns="${NAMESPACE:-tenbox}"
user="${GITHUB_USER:-enojen}"
token_file="${GITHUB_TOKEN_FILE:-$HOME/.config/tenbox/github-token}"
login_file="${CLAUDE_CREDENTIALS_FILE:-$HOME/.claude/.credentials.json}"
manifest="${LOCAL_MANIFEST:-$HOME/tenbox/tenbox.local.yaml}"
[ -s "$token_file" ] || { echo "no token at $token_file" >&2; exit 1; }
[ -s "$login_file" ] || { echo "no login at $login_file" >&2; exit 1; }

# `apply` replaces an existing Secret; nothing goes on a command line.
apply() { kubectl -n "$ns" create secret "$@" --dry-run=client -o yaml | kubectl -n "$ns" apply -f - ; }
# One value of the local manifest: `  key: "value"` under its section.
value() { sed -n "s/^ *$1: *\"\{0,1\}\([^\"]*\)\"\{0,1\} *\$/\1/p" "$manifest" | tr -d '\n'; }

umask 077
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
jq -n --rawfile t "$token_file" --arg u "$user" \
  '($t | gsub("\\s"; "")) as $tok | { auths: { "ghcr.io": { username: $u, password: $tok, auth: (($u + ":" + $tok) | @base64) } } }' \
  > "$tmp/dockerconfig.json"
apply generic ghcr --type=kubernetes.io/dockerconfigjson --from-file=.dockerconfigjson="$tmp/dockerconfig.json"
apply generic github-token --from-file=token="$token_file"
apply generic claude-credentials --from-file=credentials.json="$login_file"
if [ -s "$manifest" ]; then
  value api_key > "$tmp/api_key"
  value ntfy > "$tmp/url"
  [ -s "$tmp/api_key" ] && apply generic tracker --from-file=api_key="$tmp/api_key"
  [ -s "$tmp/url" ] && apply generic ntfy --from-file=url="$tmp/url"
fi
echo "login refresh expiry: $(jq -r '.claudeAiOauth.refreshTokenExpiresAt / 1000 | todate' "$login_file")"
kubectl -n "$ns" get secrets
