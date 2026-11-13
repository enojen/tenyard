<p align="center">
  <img src="docs/tenyard-icon.svg" width="96" height="96" alt="Tenyard">
</p>

# Tenyard

Tenyard is a small Kubernetes-native runtime for coding agents. Each task gets one Job and one
volume; a worker comes when a box needs one and goes when it is idle, so nothing runs when no
task runs. It is k3s on Hetzner, built from plain manifests and kept in git.
[Tenbox](https://github.com/enojen/tenbox) is the box; Tenyard is the yard it runs in.

> Early and changing fast. Manifests and layouts may break between commits.

## Why Tenyard

**Pay for the cores the agents use.** An agent waits for the model most of the time and needs
full cores only while its tests run. Sandbox providers bill the cores you allocate; on a shared
Fly CPU a 20 minute build got about one fifteenth of a core. Here a box asks for a quarter or a
half of a core with no limit, so the boxes share a node and the one that runs its tests takes
every idle core. One cx33 node (4 vCPU, 8 GB, about 10 EUR a month) carries four to six agents.

**One box per task, and nothing left over.** A run is a Job on the task's volume: the next run
continues where the last one stopped. A box that fits on no worker makes the autoscaler add one
(about 2 minutes, 20 s on a warm one); a worker with no box goes after 50 minutes.

**The whole platform in git.** OpenTofu holds the Hetzner side, Flux applies `main` to the
cluster within 2 minutes and puts back a change made by hand, the Secrets are encrypted with
SOPS, and the k3s datastore and the server's database go off the host every night. A new
Hetzner project was rebuilt from git, the age key and the backups in 13 minutes.

## How a task runs

```
tenbox machine run TEN-1 -- tenbox build TEN-1
 └─ a Job and the task's volume: the box image, one command, a resource class
     └─ no room on a worker: the autoscaler adds one, it joins in about a minute
         └─ the box opens the task, runs the command and ends; the volume stays
```

```sh
tenbox machine ls                      # the runs on the cluster
tenbox machine logs TEN-1 --follow     # the log of the newest run of a task
```

Tenyard never knows what an image does, only how to run it: what is inside the box belongs to
the agent's repository (Tenbox keeps its image in `machines/k8s/`). Any agent that fits in a
container can use it.

## Requirements

To run tasks on it: Tenbox and the cluster's kubeconfig. To build your own: a Hetzner Cloud
project, `kubectl`, Docker for the backup checks, and the tools that `tools/install.sh`
downloads (OpenTofu, Flux, sops, kustomize and the linters, each pinned by version and sha256).

## Build a cluster

```sh
tools/install.sh                               # the tools, into .tools/bin
tofu -chdir=infra apply                        # network, firewalls, a server with k3s
kubectl apply -k clusters/tenyard/flux-system  # Flux, its age key, apply again
install/autoscaler/autoscaler.sh <ip> 10.1.0.2 # the worker pools
install/check.sh                               # a pod on a new worker
```

These are the bones. The full steps, with the state of `infra/`, the Secrets and a restore from
the backups, are in [`install/README.md`](install/README.md), "Rebuild".

## Layout

| Path | What |
|---|---|
| `infra/` | the Hetzner side in OpenTofu: network, firewalls, the control-plane server |
| `clusters/tenyard/` | what Flux applies: Flux itself and four layers, `cluster`, `secrets`, `infrastructure`, `apps` |
| `install/` | one kustomization per component: CCM, CSI, autoscaler, monitoring, the egress proxy, Tetragon, the tenbox server, backups |
| `secrets/` | the Secrets of the cluster, encrypted with SOPS |
| `task/` | one run: the Job and volume templates, the resource classes, the services for tests |
| `injector/` | the proxy behind Squid that writes a run's real GitHub and tracker credentials into its requests, in Rust ([`injector/README.md`](injector/README.md)) |
| `procs/` | the sidecar that splits a box's CPU and RAM by process group, in Rust ([`procs/README.md`](procs/README.md)) |
| `tools/` | the checks of every PR, the backup check and the restore |

## Documentation

- [`docs/reference.md`](docs/reference.md): a run, its resource classes, services, Secrets and
  process metrics, and the tools, as built today.
- [`install/README.md`](install/README.md): how the cluster is built, run, backed up and rebuilt.
- [`docs/numbers.md`](docs/numbers.md): the measurements behind every number here.
- [`docs/tokens.md`](docs/tokens.md): every token the cluster holds and how to replace it.

## Roadmap

Every open task of Tenyard and Tenbox is on the
[board](https://github.com/users/enojen/projects/2). The
[milestones](https://github.com/enojen/tenyard/milestones) group them by theme: scale to zero,
cost, security and the experiments that measure a change before it is made.

## License

Tenyard is under the [Functional Source License 1.1, Apache 2.0 Future License](LICENSE)
(FSL-1.1-ALv2). You may use, change and share it for any purpose except a product or
service that competes with it. Two years after each release, that release becomes Apache 2.0.
Versions up to the `apache-2.0-final` tag stay under Apache 2.0.
