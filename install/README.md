# Install

One Hetzner server runs k3s with the control plane and a worker on the same node. The
first cluster lives on `tenbox-ci` (cx33, 4 vCPU, 8 GB, Ubuntu 24.04), next to the GitHub
Actions runners and Docker that were already there. k3s reserves one core and 1.5 GiB for
them, so pods see 3 cores and about 6 GB.

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

3. Install the Hetzner CSI driver, so a PersistentVolumeClaim becomes a Hetzner volume.
   It needs a Hetzner API token in a Secret; pipe it in, do not put it on the command line:

   ```
   cat hcloud-token | k3s kubectl -n kube-system create secret generic hcloud --from-file=token=/dev/stdin
   k3s kubectl apply -f https://raw.githubusercontent.com/hetznercloud/csi-driver/v2.24.0/deploy/kubernetes/hcloud-csi.yml
   ```

4. Make `hcloud-volumes` the default StorageClass and take the default away from
   `local-path`:

   ```
   k3s kubectl patch storageclass local-path -p '{"metadata":{"annotations":{"storageclass.kubernetes.io/is-default-class":"false"}}}'
   k3s kubectl patch storageclass hcloud-volumes -p '{"metadata":{"annotations":{"storageclass.kubernetes.io/is-default-class":"true"}}}'
   ```

5. Create the namespace the agents run in:

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

8. Give a container without resources the `plan` class (`task/classes.yaml`), from your
   computer:

   ```
   KUBECONFIG=~/.kube/tenyard.yaml kubectl apply -f install/limitrange.yaml
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
    kubectl -n kube-system patch deployment hcloud-csi-controller --patch-file install/csi-controller.patch.yaml
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
    kubectl apply -f install/ccm/ccm.yaml
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
| Hetzner CSI driver | v2.24.0 |
| Hetzner cloud-controller-manager | v1.39.2 |
| cluster-autoscaler | v1.36.1 |
| kubectl on the computer | v1.36.5 |
