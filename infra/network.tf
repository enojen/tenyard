# The private network of the nodes. The control plane is 10.1.0.2; a worker takes a free
# address. The API and the kubelets talk over it, so a worker needs no open port.
resource "hcloud_network" "tenyard" {
  name     = "tenyard-net"
  ip_range = "10.1.0.0/16"
  labels   = { app = "tenyard" }
}

resource "hcloud_network_subnet" "nodes" {
  network_id   = hcloud_network.tenyard.id
  type         = "cloud"
  network_zone = "eu-central"
  ip_range     = "10.1.0.0/24"
}
