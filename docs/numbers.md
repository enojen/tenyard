# Numbers

Measured on the cluster, not estimated. Each section names its source so that a later run
can repeat it. One row per Job; a Job is one tenbox command and the claude session it leaves.

## Start timeline (TEN-519)

Source: the pod events of namespace `tenbox` (`kubectl get events`), 2026-10-26, node
`tenbox-ci` (cx33), image `ghcr.io/enojen/tenbox-box:latest` (415 MB compressed), task TEN-476
of project tenbox. Times from the Job's `creationTimestamp`.

| Job (UTC) | volume attached | image pull | container started | note |
|---|---|---|---|---|
| 17:59:59 | +20 s | 19.1 s | +40 s | image pushed just before: real pull |
| 18:06:37 | | 0.8 s | +21 s | |
| 18:08:09 | +16 s | 0.8 s | +17 s | `build`, 1 round, ran 15 min |
| 18:23:33 | +16 s | 0.8 s | +20 s | |
| 18:24:49 | +16 s | 0.8 s | +17 s | |
| 18:34:00 | +16 s | 0.8 s | +19 s | |
| 18:51:09 | +16 s | 18.2 s | +37 s | image pushed just before: real pull |

`task/measure.sh <job>` prints this timeline for one Job from the events and the box log.
Two more runs on 2026-10-26 with it (project tenbox-demo, task DEMO-1, read-only boxes):

| Job (UTC) | scheduled | volume attached | image pull | container started | ready | note |
|---|---|---|---|---|---|---|
| 20:44:14 | +10 s | +26 s | 15.5 s | +45 s | +48 s | new volume: provisioning 10 s; image pushed before: real pull |
| 20:45:49 | +0 s | +16 s | 0.8 s | +18 s | | same volume, same image |

Inside the first run, from the log: container start to `ready at` 2 s (gh login, clone,
`tenbox install`); `ready at` to the claude session 4 s (`bun install` and `claude --bg`).

What it says:

- Job to a running box is 17–21 s on a warm node, and 45 s when the volume is new and the
  image changed: provisioning a Hetzner volume adds 10 s before the pod is scheduled. The Hetzner volume attach is 16 s of it
  and is the same in every run.
- `imagePullPolicy: Always` costs 0.8 s when the image did not change. The full pull of
  415 MB is 18–19 s and happens after a push, and will happen on every fresh worker node.
- boot.sh and setup are 6 s together. The pod is not where a task spends its time: the
  build run above worked 15 min, the whole task 51 min (TEN-476, 3 rounds).

## Memory and CPU of a box (TEN-520)

Source: the `memory peak` line of boot.sh (cgroup `memory.peak`, counts the file cache)
and `kubectl top`, 2026-10-26, image 7b3ab60; recorded on TEN-466.

| box | memory peak | CPU time | duration |
|---|---|---|---|
| `start TEN-476` (first boot: clone + bun install) | 1434 MiB | 32 s | 2.5 min |
| `approve` + `build TEN-476` (1 round, 5 checks) | 1290 MiB | 1114 s (~1.2 cores) | 15 min |
| a box that only reads files | 87–109 MiB | 1–2 s | 1.5 min |

`kubectl top` samples: plan ~440 MiB, build 564 MiB at 1.3 cores. Not measured yet: the
PID peak, and the heavy project (one `pnpm test` sweep held ~4 GB on 2026-10-26 and a build
pod went over 5Gi, see TEN-466).

## No ServiceAccount token in the box (TEN-522)

Source: two read-only boxes of tenbox-demo on 2026-10-26 (Jobs `task-demo-1-1026-204413` and
`task-demo-1-1026-204548`), command `ls /var/run/secrets/kubernetes.io/serviceaccount` then
the project's checks (`bun install`, `bunx tsc --noEmit`, `bun test`).

| Job template | `serviceaccount` dir | pod volumes | checks |
|---|---|---|---|
| before | `ca.crt`, `namespace`, `token` | `home`, `kube-api-access-kqwhh` | 4 pass |
| `automountServiceAccountToken: false` | does not exist | `home` | 4 pass |

Decision: keep it. The box never calls the cluster API; the token was only something to steal.
`KUBERNETES_SERVICE_HOST` and `_PORT` stay in the env, they come from the Service, not the token.

## A GitHub token for one repo in the box (TEN-521)

Source: one read-only box of tenbox-demo on 2026-10-26 (Job `task-demo-1-1026-212650`), with
a fine-grained token in the `github-token` Secret: repository access `enojen/tenbox-demo` only,
contents and pull requests read and write, metadata read, no user permissions. Before it, the
box token reached 12 repos, 2 of them private.

| in the box | result |
|---|---|
| `gh repo create leak-test --public` | refused: `Resource not accessible by personal access token` |
| `gh api repos/enojen/tenbox-archive` (private, outside the scope) | 404 |
| create a branch on `enojen/tenbox` over the API (public, outside the scope) | 403 |
| `git push` a branch of tenbox-demo, `gh pr create`, `gh pr close --delete-branch` | all work (PR #3) |

Decision: keep it. A box can now only push to its own repo and open PRs there; the s1ngularity
leak path (push the secrets to a new public repo) is closed. `boot.sh` logs in with it as before.
One token per repo; `task/secrets.sh` reads it from `~/.config/tenbox/github-box-token`. The pull
Secret `ghcr` needs a token with `read:packages`, which a fine-grained token cannot have:
`secrets.sh` reads it from `~/.config/tenbox/ghcr-token` (the first run of this experiment failed
on `ImagePullBackOff` because the script took the pull token from the box token file).

## A box on a new worker (TEN-464)

Source: a rehearsal cluster on 2026-10-26 (a cx23 control plane in hel1 with k3s v1.36.5+k3s1,
the CCM, the CSI driver and cluster-autoscaler v1.36.1, `install/autoscaler/`), a Job with the
`build` class (500m / 2Gi) on a 10 GB task volume, busybox, no worker running. Times from the
Job's `creationTimestamp`, from the autoscaler log and the pod events.

| step | run 1 | run 3 |
|---|---|---|
| cx43 tried, Hetzner: `resource_unavailable` | +7 s | +10 s |
| cpx32 ordered | +17 s | +20 s |
| worker joined the cluster (boot, k3s agent) | +62 s | +61 s |
| volume attached on the worker | +101 s | +97 s (one `FailedAttachVolume` first: the volume was still on the worker that had just gone) |
| container started | +104 s | +100 s |

What it says:

- A box with no worker waits about 1 min 40 s, against 17–21 s on a warm node. Most of it is
  the server: boot and the k3s agent take about 45 s, the volume attach 16 s as before.
- The cx line (cx33, cx43, cx53) had no server in any EU location that evening, so a pool of
  one type would not have started at all. The autoscaler tries the next pool 10 s later.
- Run 2 started two workers for one box: the new node lacked `csi.hetzner.cloud/location`
  for about 25 s, until the CSI node plugin wrote it, and the task volume asks for it. The
  worker now registers with the label; run 3 started one.
- Scale-down: an empty worker went 3 min after it was unneeded (`UNNEEDED_TIME=3m` in the
  rehearsal; 50 min on the cluster), its Hetzner server with it.

On the cluster after the move (tenbox-ci as control plane only), two read-only boxes of
tenbox-demo with the box image (`task/run.sh DEMO-1 ls`, `task/measure.sh`), 2026-10-26:

| Job (UTC) | class | worker | scheduled | volume attached | image pull | ready |
|---|---|---|---|---|---|---|
| 22:40:01 | `plan` | new cpx32 (cx43 sold out) | +66 s | +102 s | 16.5 s | +120 s |
| 22:42:27 | `build` | the same, warm | +0 s | +16 s | 0.9 s | +19 s |

A box on a new worker is ready in about 2 minutes, on a warm one in 20 s as before.

## A rebuild rehearsal (#69)

A new Hetzner project, `tenyard-rehearsal`, built from git, the age key and the backups of
2026-11-07 (install/README.md, "Rebuild"), then deleted. 2026-11-07, UTC:

| step | time |
|---|---|
| `tofu apply`: network, subnet, firewalls | 16:05:45 |
| cx33 in hel1, then fsn1: `resource_unavailable`; `location = "nbg1"` | 16:06:36 |
| k3s Ready (cloud-init of `infra/control-plane.yaml`, first run) | 16:07:39 |
| `kubectl apply -k clusters/tenyard`, then the Secrets | 16:08:22 – 16:08:46 |
| every pod Running but the autoscaler; two volumes made and bound | 16:09:49 |
| `autoscaler.sh` | 16:10:09 |
| `tools/restore-server.sh 2026-11-07` | 16:10:56 – 16:12:04 |
| `install/check.sh` passes; a cpx32 worker in hel1 started for it | 16:12:49 – 16:14:56 |
| a task Job of tenbox-demo (`tenbox status DEMO-R1`): box ready | 16:15:43 – 16:16:23 |
| the k3s datastore of the backup, on the server, `--disable-agent`: API ready | 16:18:12 |
| every resource of the project deleted | 16:18:55 |

13 minutes from nothing to a task Job. The restored server database held the 285 events of
the backup; the server then recorded the old cluster's 2 nodes and 22 volumes as removed, and
the rehearsal's worker, volume and Job as new. The restored k3s datastore held 19 Secrets,
11 Deployments and 25 volume claims.

Cost, every started hour billed: cx33 €0.0136, cpx32 €0.0569, four 10 GB volumes and one IPv4
for an hour, about €0.08 in all.

## Flux on the cluster (#70)

Flux v2.9.6, its source and kustomize controllers on the control-plane server, 2026-11-07, UTC:

| step | time |
|---|---|
| first `kubectl apply -k clusters/tenyard/flux-system`: CRDs and controllers | 16:35:10 |
| controllers ready | 16:35:30 |
| second apply, after the Secret `sops-age`: the GitRepository and the root Kustomization | 16:37:11 |
| all five Kustomizations `Ready` at `main@674aefd` | 16:38:48 |

| | RAM in use | request | limit |
|---|---|---|---|
| kustomize-controller | 29 MiB | 64 MiB | 1 GiB |
| source-controller | 24 MiB | 64 MiB | 1 GiB |

The node went from 3580 to 3619 MiB in use. Taking the objects over restarted three pods once:
cluster-autoscaler, victoria-metrics and tenbox-server had a `kubectl.kubernetes.io/restartedAt`
annotation in their pod template from an earlier `kubectl rollout restart`, which git does not
have; no other pod changed, and a running task Job went on.

A field changed by hand (the schedule of the CronJob `offsite-backup`) came back in 3 s with
`flux reconcile kustomization apps`, and in 134 s on its own, at the next run of the layer.

## PIDs and local disk of a box (#16, #88)

Source: 2026-11-07. The procs series `tenyard_procs_processes` of the last 30 days (120 runs;
the sum of each group's own peak, so an upper bound), and `pids.peak` of the box's cgroup and
the kubelet's `stats/summary` of a running build of the heavy project.

| | value |
|---|---|
| processes in a run, highest (a `decide` of the heavy project) | 70 |
| processes in a build, highest | 46 |
| tasks (processes and threads) of a heavy build, `pids.peak` while it ran | 134 |
| threads per process in that box | ~3.4 |
| the box's disk outside its volume, heavy build | 58 MiB, mostly `/tmp` |
| before: kubelet `podPidsLimit` | none; each container had `pids.max` 9208, 15 % of the node's `threads-max` (systemd) |

The Postgres service allows 300 connections, one process each, so a build with it may add up
to 300. Decision: 2048 PIDs per pod, about 4 times the worst case seen (~240 in the box plus
300); 4Gi of local disk, request 256Mi.

A pod that wrote 1 GiB to `/tmp` with an `ephemeral-storage` limit of 512Mi was evicted ("Pod
ephemeral local storage usage exceeds the total limit of containers 512Mi."); the same pod
with no limit went on. boot.sh logs `pids peak` next to `memory peak` from tenbox#223 on.

## Tetragon on a worker (#94)

Source: 2026-11-08. A test install of `install/tetragon` in its own namespace, one task of the
demo project (start, approve, build) on a new cpx32 worker, test pods that write a watched file
every 20 ms from their first instruction, VictoriaMetrics (cAdvisor) and VictoriaLogs.

| | value |
|---|---|
| agent CPU during a plan and a build | 2–11m |
| agent memory, the 2 policies of git | ~99 MiB |
| agent memory, 6 policies | 221–241 MiB |
| Fluent Bit | 8–11 MiB, 0–4m |
| runtime hook | 36 MiB, 0–1m |
| events of the three runs (start, approve, build of one small task) | 602, 133, 629 |
| connections of the three runs to anything but the egress proxy | 0 |
| one event in VictoriaLogs | ~100 B on disk (~4.7 KB as JSON) |
| new worker: node, agent started, agent ready, first container of the box pod, box container | 0, +8 s, +28 s, +34 s, +49 s |
| writes a namespaced policy missed after a container start, no hook (4 pods) | 1–2 of 100, the first 22–45 ms |
| the same with the hook (8 pods) | 0 |
| pod create to first exec, no hook / hook | 2033–2083 / 2010–2233 ms |
| open and close of a file, on a computer: no policy / the file policy cluster-wide | 2.35 / 5.45 µs |

The agent was killed once at its 512Mi limit, only while unfiltered `tetra getevents` streams
were open; with the same tests read from VictoriaLogs it stayed flat. With the hook failing
closed, a pod of `tenbox` started while the agent was gone got `StartError` and stayed so; with
`tenbox` in `failAllowNamespaces` it started. A cluster-wide file policy instead of the hook would
also close the gap, at about 3 µs on every open of every process of the node.
