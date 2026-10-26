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
