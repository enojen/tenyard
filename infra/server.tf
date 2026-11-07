# The control plane: k3s server, the CCM, the CSI controller, the autoscaler, monitoring and the
# tenbox server (install/README.md). The GitHub runners of other projects share it, so it is
# never destroyed or rebuilt from here. A new one (a rebuild, a rehearsal) gets k3s from
# control-plane.yaml; Hetzner does not return the image, the SSH keys or the cloud-init of a
# server, so a change to them never touches the running one.
resource "hcloud_server" "control_plane" {
  name         = "tenbox-ci"
  server_type  = "cx33"
  location     = "hel1"
  image        = "ubuntu-24.04"
  ssh_keys     = var.ssh_keys
  firewall_ids = [hcloud_firewall.control_plane.id]
  user_data    = file("${path.module}/control-plane.yaml")

  lifecycle {
    prevent_destroy = true
    ignore_changes  = [image, ssh_keys, user_data]
  }
}

# The server's address on tenyard-net. k3s listens there (install/README.md, step 11).
resource "hcloud_server_network" "control_plane" {
  server_id  = hcloud_server.control_plane.id
  network_id = hcloud_network.tenyard.id
  ip         = "10.1.0.2"

  depends_on = [hcloud_network_subnet.nodes]
}
