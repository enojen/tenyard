# Tokens

Every secret the cluster holds: what it can do, where it is made, where it lives. On a leak,
make a new one where it is made, put it where it lives, and revoke the old one. A token that
was in `secrets/` stays readable in git history to anyone with the age key, so a leaked age key
means a new token for every row marked "git".

| Secret (namespace/name) | What it can do | Made in | Lives in |
|---|---|---|---|
| `kube-system/hcloud` `token` | Hetzner project, read and write: servers, volumes, networks. The CSI driver, the CCM and the autoscaler use it | Hetzner console, project, Security, API tokens | git (decision in #67) |
| `kube-system/hcloud` `network` | not a secret: the name of the private network | | git, with the token |
| `tenbox-server/hcloud` `token` | Hetzner project, read only: the server reads each worker's type and price | Hetzner console | git |
| `tenbox/ghcr`, `tenbox-server/ghcr` | pull the private images from GHCR (`read:packages` of a classic token) | GitHub, Settings, Developer settings, Personal access tokens (classic) | git |
| `tenbox/github-token` `token` | `gh` and `git push` in the box: contents and pull requests of the box repos (fine-grained) | GitHub, fine-grained tokens | git |
| `tenbox-server/ntfy` `url` | sends a notice to the ntfy topic | the topic name itself | git |
| `monitoring/grafana-admin` `password` | the Grafana admin login | `openssl rand` | git |
| `tenbox/claude-credentials` | the claude.ai login of the boxes | `/login` on the computer | the login sync writes it (tenbox `machines/login/`); never git |
| `tenbox/project-<project id>` | a project's tracker key, ntfy URL and GitHub token | the project's `tenbox.yaml` and local layers | `tenbox machine run` writes it before each Job; never git |
| `kube-system/tenyard-autoscaler` | the k3s join token (joins workers only) and the worker cloud-init | `k3s token create` on the server | `install/autoscaler/autoscaler.sh` writes it; moves with #68 |
| `monitoring/tailscale-auth` | a Tailscale auth key, used once | Tailscale admin console | by hand; can be deleted after the first start |
| `monitoring/tailscale-grafana` | the Tailscale node key, written by the node itself | | the cluster only |

Outside the cluster:

| Secret | What it can do | Lives in |
|---|---|---|
| the age key of Tenyard (`~/.config/tenyard/age.key`) | decrypts everything in `secrets/` | the computer and a password manager; never git |
| the kubeconfig (`~/.kube/tenyard.yaml`) | admin of the cluster | the computer |
