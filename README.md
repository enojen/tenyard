# Tenyard

Tenbox is the box. Tenyard is the yard it runs in.

Tenyard is a small Kubernetes runtime for coding agents: one Job and one volume per
task, low requests and high limits so that several agents share a node and still burst,
and nothing running when no task runs. It is built on k3s on Hetzner, from plain
manifests. [Tenbox](https://github.com/enojen/tenbox) is its first client; any agent that
fits in a container can be the next.

The line between the two: what is inside the box belongs to the agent's own repository
(Tenbox keeps its image, `Dockerfile` and `boot.sh`, in `machines/k8s/`); where and how the
box runs belongs to Tenyard. Tenyard never knows what an image does, only how to run it.

## What is here today

- `install/`: how the cluster is set up, on one Hetzner server, with a rollback.

Everything else (the Job template, resource classes, snapshots, the autoscaler, the
queue, the preview URLs, the resize controller) comes step by step; the README grows with
the code.

## Why not a sandbox provider

Fly Machines, Daytona and Vercel Sandbox start a box in seconds too, but they bill the
cores you allocate, not the cores you use, and an agent uses almost none while it waits
for the model. On a shared Fly CPU a 20 minute build gets throttled to about one
fifteenth of a core. One cx33 node (4 vCPU, 8 GB, about 10 EUR a month) carries four to
six agents at once, with full cores for the one that is running its tests. The numbers
behind this are in `docs/numbers.md` once they are measured.
