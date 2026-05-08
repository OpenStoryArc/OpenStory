# V0 profile — one Hetzner box, all-in-one (k3s + openstory + nats hub).
#
# Required env vars before `tofu apply -var-file=profiles/single-host.tfvars`:
#   HCLOUD_TOKEN          Hetzner Cloud API token
#   CLOUDFLARE_API_TOKEN  Cloudflare API token (DNS:Edit on the zone)
#
# Replace cloudflare_zone_id and operator_ssh_keys before deploying.

profile_name = "single-host"
host_name    = "lab"
server_type  = "cx22"
location     = "nbg1"

# Replace with the real Cloudflare zone ID.
cloudflare_zone_id = "REPLACE_ME"

# Operator SSH keys. Add real keys before `tofu apply`.
operator_ssh_keys = [
  # {
  #   name       = "max-laptop"
  #   public_key = "ssh-ed25519 AAAA... max@laptop"
  # },
]
