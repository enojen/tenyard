# One time: these resources were made by hand with hcloud. The first `tofu apply` takes them
# into the state and changes nothing on Hetzner; then delete this file and import_ids.
import {
  to = hcloud_network.tenyard
  id = var.import_ids.network
}

import {
  to = hcloud_network_subnet.nodes
  id = "${var.import_ids.network}-10.1.0.0/24"
}

import {
  to = hcloud_firewall.control_plane
  id = var.import_ids.firewall_control_plane
}

import {
  to = hcloud_firewall.worker
  id = var.import_ids.firewall_worker
}

import {
  to = hcloud_server.control_plane
  id = var.import_ids.server
}

import {
  to = hcloud_server_network.control_plane
  id = "${var.import_ids.server}-${var.import_ids.network}"
}
