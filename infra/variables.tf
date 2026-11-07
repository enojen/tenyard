variable "admin_ips" {
  description = "The addresses, as CIDRs, that may reach the Kubernetes API on 6443: your computer."
  type        = list(string)
}

variable "ssh_keys" {
  description = "The names of the Hetzner SSH keys of a new control-plane server."
  type        = list(string)
}

variable "import_ids" {
  description = "The ids of the resources made by hand, for imports.tf only."
  type = object({
    network                = string
    firewall_control_plane = string
    firewall_worker        = string
    server                 = string
  })
}
