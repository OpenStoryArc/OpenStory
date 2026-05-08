output "lab_ipv4" {
  description = "Public IPv4 address of the lab host."
  value       = hcloud_server.lab.ipv4_address
}

output "lab_ipv6" {
  description = "Public IPv6 address of the lab host."
  value       = hcloud_server.lab.ipv6_address
}

output "lab_fqdn" {
  description = "Fully-qualified DNS name of the lab host."
  value       = cloudflare_record.lab_a.hostname
}

output "ssh_command" {
  description = "Shortcut for sshing in as the operator."
  value       = "ssh root@${hcloud_server.lab.ipv4_address}"
}

output "nixos_anywhere_command" {
  description = "Run this after `tofu apply` to install NixOS."
  value       = "nixos-anywhere --flake ../#lab-host root@${hcloud_server.lab.ipv4_address}"
}
