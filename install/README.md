# Install

One Hetzner server runs k3s with the control plane and a worker on the same node. The
first cluster lives on `tenbox-ci` (cx33, 4 vCPU, 8 GB, Ubuntu 24.04), next to the GitHub
Actions runners and Docker that were already there. k3s reserves one core and 1.5 GiB for
them, so pods see 3 cores and about 6 GB.

## The cluster in git

`clusters/tenyard/kustomization.yaml` holds every object of the cluster that lives in git, one
kustomization per component under `install/`: `cluster` (the namespace `tenbox` and the
LimitRange), `ccm`, `csi`, `autoscaler`, `monitoring` (with Grafana and its tailnet node) and
`server`. An upstream file (the CSI driver, the CCM) is kept as its release ships it, and every
change to it is a patch next to it. Secrets stay outside; the steps below name each one.

```
kubectl diff -k clusters/tenyard    # what an apply would change; nothing, when git and the cluster agree
kubectl apply -k clusters/tenyard
```

Apply with no box running, after a snapshot. The steps below are how the cluster was built; each
one applies its own component.

## Hetzner resources: `infra/`

OpenTofu with the hcloud provider holds the Hetzner side of the cluster: the network
`tenyard-net` and its subnet, the control-plane server and its address there, the firewall of the
control plane (SSH, and the Kubernetes API from your computer) and the empty firewall of the
workers. The Hetzner project holds other servers too; `infra/` owns only these. It never owns an
SSH key, a volume (the CSI driver makes them), a worker (the autoscaler makes them) or a
snapshot. The control-plane server has `prevent_destroy`, and a change to its image, SSH keys or
cloud-init never touches the running one: GitHub runners of other projects share it.

The state and the values stay outside this public repo, in `~/.config/tenyard/tofu/`:
`terraform.tfstate`, and `tenyard.tfvars` with `admin_ips` (your computer, for 6443) and
`ssh_keys` (names, for a new server). The token is `HCLOUD_TOKEN`.

```
cd infra
tofu init -backend-config=path=$HOME/.config/tenyard/tofu/terraform.tfstate
tofu plan -var-file=$HOME/.config/tenyard/tofu/tenyard.tfvars    # read only
tofu apply -var-file=$HOME/.config/tenyard/tofu/tenyard.tfvars
```

The resources were made by hand (steps 1, 7 and 9 below) and imported into the state once, with
an `imports.tf` that is in git history. A plan that wants to destroy or replace anything is not
applied. When your IP changes, change `admin_ips` and apply.

`infra/control-plane.yaml` is the cloud-init of a new control-plane server: k3s with the flags
of steps 2 and 11. It has not run on a fresh server yet. The worker cloud-init and the
autoscaler's pool config still come from `install/autoscaler/autoscaler.sh`.

## Secrets

The Secrets live in `secrets/<namespace>/<name>.sops.yaml`, encrypted with SOPS to the age key
of Tenyard; only `data` and `stringData` are encrypted (`.sops.yaml`). `docs/tokens.md` lists
every token, and which ones stay out of git. The key is `~/.config/tenyard/age.key` on the
computer, with a copy in a password manager; without it nothing in `secrets/` can be read again.
It is not the key of any other project.

```
export SOPS_AGE_KEY_FILE=~/.config/tenyard/age.key
.tools/bin/sops secrets/<namespace>/<name>.sops.yaml   # make or change one in $EDITOR
tools/apply-secrets.sh --diff                          # what would change, values hidden
tools/apply-secrets.sh
```

A Secret that is already on the cluster goes to git with:

```
kubectl -n <namespace> get secret <name> -o json \
  | jq '{apiVersion, kind, type, data, metadata: {name: .metadata.name, namespace: .metadata.namespace}}' \
  | .tools/bin/sops --encrypt --input-type json --output-type yaml \
      --filename-override secrets/<namespace>/<name>.sops.yaml /dev/stdin \
  > secrets/<namespace>/<name>.sops.yaml
```

`.gitignore` keeps any other file in `secrets/` out of git, and `tools/check-secrets.sh` fails on
a file there that sops did not encrypt and on a Secret with values in `clusters/tenyard`.

## Steps

All commands run on the server as root, except where noted.

1. Snapshot the server first, for a rollback (from your computer):

   ```
   hcloud server create-image --type snapshot --description "before k3s" <server>
   ```

2. Install k3s. `--tls-san` adds the public IP to the API certificate so `kubectl` works
   from your computer; traefik is off because the ingress comes later with its own
   controller; `system-reserved` keeps the share of the other services on the node:

   ```
   curl -sfL https://get.k3s.io | INSTALL_K3S_EXEC="server \
     --tls-san <public ip> \
     --disable traefik \
     --kubelet-arg=system-reserved=cpu=1,memory=1.5Gi" sh -
   ```

   `k3s kubectl get nodes` shows the node Ready after about 20 seconds.

3. The Hetzner CSI driver (step 8) makes a PersistentVolumeClaim a Hetzner volume. It needs a
   Hetzner API token in a Secret; pipe it in, do not put it on the command line:

   ```
   cat hcloud-token | k3s kubectl -n kube-system create secret generic hcloud --from-file=token=/dev/stdin
   ```

4. Take the default StorageClass away from `local-path`, so `hcloud-volumes` of the CSI driver
   is the only default:

   ```
   k3s kubectl patch storageclass local-path -p '{"metadata":{"annotations":{"storageclass.kubernetes.io/is-default-class":"false"}}}'
   ```

5. Create the namespace the agents run in (`install/cluster` has it too, so step 8 keeps it):

   ```
   k3s kubectl create namespace tenbox
   ```

6. Copy the kubeconfig to your computer and point it at the public IP:

   ```
   ssh root@<server> cat /etc/rancher/k3s/k3s.yaml \
     | sed 's|https://127.0.0.1:6443|https://<public ip>:6443|' > ~/.kube/tenyard.yaml
   chmod 600 ~/.kube/tenyard.yaml
   KUBECONFIG=~/.kube/tenyard.yaml kubectl get nodes
   ```

7. Open the API port on the server's Hetzner firewall, only to your own IP. k3s listens
   on 6443; the Hetzner firewall drops it until a rule allows it:

   ```
   hcloud firewall add-rule <firewall id> --direction in --protocol tcp --port 6443 \
     --source-ips <your ip>/32 --description "kubernetes api from the computer"
   ```

   When your IP changes, change the rule. `kubectl` on the computer: download the
   binary of the cluster's version from `https://dl.k8s.io/release/<version>/bin/<os>/<arch>/kubectl`
   into `~/.local/bin`.

8. From your computer: the namespace and a LimitRange that gives a container without resources
   the `plan` class (`task/classes.yaml`), then the CSI driver v2.24.0:

   ```
   export KUBECONFIG=~/.kube/tenyard.yaml
   kubectl apply -k install/cluster
   kubectl apply -k install/csi
   ```

## Workers

Boxes run on workers that the cluster autoscaler adds and removes; the server keeps the
control plane, the CCM, the CSI controller and the autoscaler, and gets a taint so no box runs
there. Three things make the Hetzner autoscaler work with k3s, all found in a rehearsal on a
throwaway server before tenbox-ci (2026-10-26):

- The autoscaler finds a node's server by `spec.providerID` = `hcloud://<server id>`. k3s
  writes `k3s://<node name>`, and the autoscaler then deletes every new worker after 15 minutes
  as unregistered. The Hetzner cloud-controller-manager (CCM) writes the right one; k3s runs
  with `--disable-cloud-controller` and the kubelet with `cloud-provider=external`.
- The nodes talk over the private network `tenyard-net`, so a worker needs no open port and the
  API stays closed to the internet. k3s cannot change the IP of a running node: the node is
  deleted and registers again, which is also the only way to change its `providerID`.
- A new worker carries the CCM's `uninitialized` taint for a few seconds; the autoscaler must
  know it as a startup taint, or it adds a second worker.

From your computer, once, on a server set up with the steps above:

9. Make the network and the worker firewall (no inbound rule: the private network is not
   filtered, and a worker needs nothing from the internet):

   ```
   hcloud network create --name tenyard-net --ip-range 10.1.0.0/16 --label app=tenyard
   hcloud network add-subnet tenyard-net --network-zone eu-central --type cloud --ip-range 10.1.0.0/24
   hcloud server attach-to-network <server> --network tenyard-net --ip 10.1.0.2
   hcloud firewall create --name tenyard-worker --label app=tenyard
   ```

   `hc-utils` on the Hetzner Ubuntu image brings the new interface up after about 20
   seconds (`ip -br addr`: `enp7s0` with `10.1.0.2/32`).

10. Snapshot the server, keep the task volumes whatever happens, and let the CSI controller
    run on the tainted server:

    ```
    hcloud server create-image --type snapshot --description "before workers" <server>
    for pv in $(kubectl get pv -o name); do
      kubectl patch "$pv" -p '{"spec":{"persistentVolumeReclaimPolicy":"Retain"}}'
    done
    kubectl apply -k install/csi   # controller.patch.yaml: the CSI controller tolerates the control plane
    ```

11. With no box running, on the server: add the network name to the `hcloud` Secret, write
    the new flags, delete the node and start k3s again. The node registers with its private IP,
    no `providerID` and the `uninitialized` taint; Secrets, volumes and the namespace stay:

    ```
    k3s kubectl -n kube-system patch secret hcloud -p '{"stringData":{"network":"tenyard-net"}}'
    curl -sfL https://get.k3s.io | INSTALL_K3S_VERSION=v1.36.5+k3s1 INSTALL_K3S_SKIP_START=true \
      INSTALL_K3S_EXEC="server --tls-san <public ip> --disable traefik \
        --kubelet-arg=system-reserved=cpu=1,memory=1.5Gi \
        --node-ip 10.1.0.2 --advertise-address 10.1.0.2 --flannel-iface enp7s0 \
        --disable-cloud-controller --kubelet-arg=cloud-provider=external" sh -
    k3s kubectl delete node <server>
    systemctl restart k3s
    ```

    If `k3s kubectl get nodes` stays empty for 30 seconds, the old kubelet registered before
    the delete ended: run `systemctl restart k3s` once more.

12. From your computer: the CCM, then the taint. The CCM sets `hcloud://<id>`, the private
    `InternalIP` and the public `ExternalIP`, and removes the `uninitialized` taint:

    ```
    kubectl apply -k install/ccm
    kubectl get nodes -o custom-columns=NAME:.metadata.name,ID:.spec.providerID
    kubectl taint node <server> node-role.kubernetes.io/control-plane:NoSchedule
    ```

13. The autoscaler (`install/autoscaler/`): three pools in hel1, from 0, in the order cx43,
    cpx32, cpx42; a worker goes after 50 minutes unneeded:

    ```
    install/autoscaler/autoscaler.sh <server public ip> 10.1.0.2
    ```

    The worker joins with a bootstrap token (`k3s token create`), which can join workers only;
    it is in the Secret `tenyard-autoscaler` and in each worker's cloud-init.

    The kubelet of a worker keeps 1 GiB and 250m out of the boxes (`system-reserved`
    768Mi, eviction under 256Mi free): a cx43 or cpx42 holds 7 `build` boxes, a cpx32 3. With
    no reserve, boxes that fill their requests take the whole RAM: on a cpx42 with 6 `build` and
    2 `plan` boxes at their limit, the kernel killed the CSI node plugin and a `plan` box under
    its limit (2026-11-05). After a change of the cloud-init, run `autoscaler.sh` again; only new
    workers get it.

## Metrics

VictoriaMetrics single node keeps the cluster's metrics for a year, on the server next to the
control plane (`install/monitoring/monitoring.yaml`). It scrapes every 30 s:

- kube-state-metrics: what exists. Workers with their server type (`kube_node_labels`, label
  `node.kubernetes.io/instance-type`) and creation time, box pods with their `task`,
  `project`, `class` and `command` labels (two projects can have the same task id), their requests,
  start and end. The cost report counts paid and busy worker hours from these.
- cAdvisor of every kubelet, through the API server proxy: what a pod uses. CPU, working set
  and RSS memory, CPU throttling, OOM events and the time its processes waited for CPU, memory
  or IO (pressure stall information: `container_pressure_*_seconds_total`); nothing else, as
  cAdvisor has hundreds more.
- the tenbox server's `/metrics`: `tenbox_node_price_eur_hourly{node, server_type, location}`, the
  hourly list price of each worker that is up, from its `node.added` (enojen/tenbox#198). The
  exact paid euros stay in the server's cost report, which counts started hours.
- the `processes` sidecar of each running box pod, every 10 s, from the pod IP: CPU, anonymous
  memory and process count per process group inside the box, with the pod's `task`, `project`,
  `command` and `class` (README, "Process metrics"). A Role in namespace `tenbox` lets
  VictoriaMetrics list the pods there; it reads no other namespace's pods.

```
kubectl apply -f install/monitoring/monitoring.yaml
kubectl patch pv "$(kubectl -n monitoring get pvc victoria-metrics -o jsonpath='{.spec.volumeName}')" \
  -p '{"spec":{"persistentVolumeReclaimPolicy":"Retain"}}'
kubectl -n monitoring port-forward svc/victoria-metrics 8428   # then http://localhost:8428/vmui
```

The data lives on its own 10 GB Hetzner volume, kept with `Retain`, so it outlives the pod and
the claim. Together the two pods ask for 150m and 320Mi of the server.

### Grafana

Grafana shows one dashboard, Tenyard, over VictoriaMetrics (`install/monitoring/grafana/`):

- **In numbers**: the whole cluster in the time range as plain numbers: tasks, projects, machine
  runs, agent hours (from the process groups), machine hours, the most runs at once, workers
  opened, machine cost and cost per task, OOM kills, CPU hours, and the share of worker time with
  no run on it.
- **Workers**: the machines that were up with their server type, their hourly list price, and
  per machine its CPU and memory used and the memory the running boxes reserved, in % of it.
- **Box runs**: a table with one row per run (task, command, project, class, machine, start, run
  time, memory peak and its share of the class limit, CPU seconds, OOM kills), then the memory,
  CPU and CPU waiting of each run over time.
- **Inside the runs**: CPU and anon memory per process group (README, "Process metrics"), and the
  CPU seconds and memory peak per group over the time range.

The variables `machine`, `project`, `task` and `command` filter the rows below the numbers; a
worker links to its runs, and a task in the table to the process groups of that run, in its own
time range. The dashboard is `tenyard.json` in git, applied as a ConfigMap:
change the JSON (or export it from the UI) and apply again; Grafana reads it within a minute. The
UI cannot save over it. Grafana keeps no other state: its database is an emptyDir, so a restart
loses only logins. It asks for 50m and 256Mi on the server (about 260 MiB used with the
dashboard open); it downloads no plugins.

```
P=$(openssl rand -base64 18); echo "$P"; kubectl -n monitoring create secret generic grafana-admin --from-literal=password="$P"
kubectl apply -k install/monitoring/grafana
kubectl -n monitoring port-forward svc/grafana 3000   # then http://localhost:3000, user admin
```

On the tailnet (#59) Grafana is `https://grafana.<tailnet>.ts.net`, from any device of the
tailnet and from nowhere else: `install/monitoring/tailnet.yaml` runs one Tailscale node
`grafana` in userspace mode (no privilege) that serves HTTPS to the Grafana Service. Once, in the
Tailscale admin console: turn on MagicDNS and HTTPS certificates (DNS), add
`"tagOwners": { "tag:k8s": ["autogroup:admin"] }` to the access controls, and make an auth key
(not reusable, not ephemeral, tag `tag:k8s`). Then, in your own terminal so the key stays out of
any log:

```
read -rs K; kubectl -n monitoring create secret generic tailscale-auth --from-literal=TS_AUTHKEY="$K"; unset K
kubectl apply -f install/monitoring/tailnet.yaml
```

The key is used once: the node key then lives in Secret `tailscale-grafana`, and a tagged node
does not expire. Delete `tailscale-auth` after the first start if you like.

## Server

The tenbox server (`server/` of enojen/tenbox, image `ghcr.io/enojen/tenbox-server`, private)
runs in namespace `tenbox-server` on the server, next to the control plane
(`install/server/server.yaml`):

- Its SQLite file is `/data/server.db` on a 10 GB Hetzner volume kept with `Retain`.
- The CronJob `backup` writes `/data/backups/server-<date>.db` at 03:00 UTC with
  `tenbox-server backup` (read-only, no migration) and deletes copies older than 7 days. The
  copies sit on the same volume: they cover a broken database, not a lost volume.
- The Service `tenbox-server:4300` answers inside the cluster only; tokens travel over plain
  HTTP until TLS.
- Its account reads nodes, and pods, their logs, Jobs and volume claims of namespace `tenbox`,
  to record workers, machine runs and task volumes: the box's last log line counts the
  processes killed by OOM inside a container that lived on. It reads no Secret and creates
  nothing. It deletes one thing: the volume claim of a task whose `tenbox close` ended with
  exit code 0, 3 days after the close (`TENBOX_SERVER_VOLUME_GRACE_DAYS`; enojen/tenbox#212).
- `TENBOX_SERVER_NTFY` from the Secret `ntfy` (key `url`, optional): the ntfy topic URL for the
  notice about an open task's volume with no run for 7 days. Without the Secret no notice goes.
- `enableServiceLinks: false`: else Kubernetes sets `TENBOX_SERVER_PORT=tcp://...` for the
  Service of the same name and the server reads it as its port.
- `HCLOUD_TOKEN` from the Secret `hcloud`: a **read-only** Hetzner API token of the project.
  The server asks Hetzner for the type, location and hourly price of each new worker. In the
  cluster it does not start without it. Make a new token for it, not the CSI one (that one
  can write).

```
cat hcloud-readonly-token | kubectl -n tenbox-server create secret generic hcloud --from-file=token=/dev/stdin
read -rs U; kubectl -n tenbox-server create secret generic ntfy --from-literal=url="$U"; unset U
kubectl apply -f install/server/server.yaml
kubectl -n tenbox get secret ghcr -o json \
  | jq '{apiVersion, kind, type, data, metadata: {name: .metadata.name, namespace: "tenbox-server"}}' \
  | kubectl apply -f -
kubectl patch pv "$(kubectl -n tenbox-server get pvc tenbox-server -o jsonpath='{.spec.volumeName}')" \
  -p '{"spec":{"persistentVolumeReclaimPolicy":"Retain"}}'
kubectl get --raw /api/v1/namespaces/tenbox-server/services/tenbox-server:4300/proxy/health
```

Both images (the Deployment and the `backup` CronJob) name one commit of enojen/tenbox and its
digest, so a restart never pulls another server and a rollback is one revert. A new server is a
commit here that changes both lines; tenbox's `server.yml` pushes every commit of `main` with its
sha as the tag, and the digest of a tag is listed by

```
gh api /user/packages/container/tenbox-server/versions -q '.[] | "\(.name) \(.metadata.container.tags)"'
```

Then `kubectl apply -f install/server/server.yaml`.

## Check

`install/check.sh` runs a pod that writes a file to a new volume, reads it back, reaches
the internet and prints its cores, then deletes the pod and the volume. Run it from your
computer with `KUBECONFIG` set. The volume costs money while it exists; the script
removes it.

## Rollback

Hetzner console or CLI: rebuild the server from the snapshot taken in step 1. Everything
k3s wrote is gone; the runners and Docker are as before.

```
hcloud server rebuild --image <snapshot id> <server>
```

## Versions

| | |
|---|---|
| k3s | v1.36.5+k3s1 (stable channel, 2026-10-21) |
| Hetzner CSI driver | v2.24.0, `install/csi/hcloud-csi.yml` |
| Hetzner cloud-controller-manager | v1.39.2, `install/ccm/ccm-networks.yaml` |
| cluster-autoscaler | v1.36.1 |
| VictoriaMetrics single node | v1.154.1 |
| kube-state-metrics | v2.20.0 |
| procs (box sidecar) | `procs/`, image by commit |
| tenbox server | enojen/tenbox commit and digest, `install/server/server.yaml` |
| kubectl on the computer | v1.36.5 |
