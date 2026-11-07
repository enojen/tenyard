# The control plane: SSH from anywhere, the Kubernetes API from your computer only.
resource "hcloud_firewall" "control_plane" {
  name = "tenbox-ci-fw"

  rule {
    description = "ssh"
    direction   = "in"
    protocol    = "tcp"
    port        = "22"
    source_ips  = ["0.0.0.0/0", "::/0"]
  }

  rule {
    description = "kubernetes api from the computer"
    direction   = "in"
    protocol    = "tcp"
    port        = "6443"
    source_ips  = var.admin_ips
  }
}

# The workers: no inbound rule. The private network is not filtered, and a worker needs nothing
# from the internet. The autoscaler puts each new worker in it (HCLOUD_FIREWALL in
# install/autoscaler/autoscaler.yaml), so this code leaves the list of its servers alone.
resource "hcloud_firewall" "worker" {
  name   = "tenyard-worker"
  labels = { app = "tenyard" }

  lifecycle {
    ignore_changes = [apply_to]
  }
}
