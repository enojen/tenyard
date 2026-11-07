# CLAUDE.md

Tenyard: the Kubernetes-native runtime of the coding agents, on k3s on Hetzner. `README.md` says
what is here; `install/README.md` how the cluster is built, run and rebuilt.

## Rules of the cluster

- A change reaches the cluster only through a merge to `main`: Flux applies it within 2 minutes
  and puts back a change by hand within 10 (`install/README.md`, "The cluster in git: Flux").
  No `kubectl apply`, `edit` or `patch` of an object in git; `flux diff` shows what a branch
  would change.
- Never `kubectl rollout restart` a Deployment of Flux's: Flux removes its annotation and the
  pods restart once more. Delete the pod: `kubectl delete pod -l app=<name>`.
- A Secret goes to `secrets/<namespace>/<name>.sops.yaml`, encrypted with sops; the pre-commit
  hook (`tools/install.sh` sets it) refuses a plain one. Never print, log or write a decrypted
  value. `docs/tokens.md` lists every token.
- The Hetzner side is OpenTofu in `infra/`; its state and values stay in
  `~/.config/tenyard/tofu/`. Apply only a saved plan read first; a plan that destroys or
  replaces anything is not applied.
- A rehearsal or any second cluster runs in its own Hetzner project, never in this one: the
  autoscaler finds its workers by their pool label, and the Secrets in git hold this project's
  token.
- This repo is public: no IP, server or volume id, token or client name in code, commits,
  issues or PRs.
- Every tool is pinned by version and sha256 in `tools/install.sh`; an image by tag and digest.
- Before a PR: `tools/check-manifests.sh`, `check-scripts.sh`, `check-secrets.sh` and
  `check-infra.sh` (the checks of `tenbox.yaml` and CI).
