variable "admin_ips" {
  description = "The addresses, as CIDRs, that may reach the Kubernetes API on 6443: your computer."
  type        = list(string)
}

variable "ssh_keys" {
  description = "The names of the Hetzner SSH keys of a new control-plane server."
  type        = list(string)
}
