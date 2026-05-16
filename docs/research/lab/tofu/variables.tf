variable "profile_name" {
  description = "Name of the deployment profile (single-host, replicated, federated)."
  type        = string
  default     = "single-host"
}

variable "host_name" {
  description = "Hostname / Cloudflare DNS record name (without zone)."
  type        = string
}

variable "server_type" {
  description = "Hetzner Cloud server type. CX21 minimum for V0; CCX for prod."
  type        = string
  default     = "cx22"
}

variable "location" {
  description = "Hetzner Cloud location (nbg1, fsn1, hel1, etc.)."
  type        = string
  default     = "nbg1"
}

variable "bootstrap_image" {
  description = "Bootstrap image; replaced by NixOS via nixos-anywhere post-create."
  type        = string
  default     = "ubuntu-24.04"
}

variable "cloudflare_zone_id" {
  description = "Cloudflare zone ID for the DNS record."
  type        = string
}

variable "operator_ssh_keys" {
  description = "SSH public keys allowed to reach the host."
  type = list(object({
    name       = string
    public_key = string
  }))
  default = []
}
