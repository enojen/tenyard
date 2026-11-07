terraform {
  required_version = ">= 1.10"
  required_providers {
    hcloud = {
      source  = "hetznercloud/hcloud"
      version = "~> 1.70"
    }
  }
  # The state stays outside this public repo, as it holds addresses and ids:
  #   tofu init -backend-config=path=$HOME/.config/tenyard/tofu/terraform.tfstate
  backend "local" {}
}

# The token comes from HCLOUD_TOKEN. The Hetzner project holds other servers too: this code
# owns only the ones below, and never an SSH key or a volume (the CSI driver owns those).
provider "hcloud" {}
