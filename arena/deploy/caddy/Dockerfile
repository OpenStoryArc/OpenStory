# Custom Caddy build with the Cloudflare DNS module, needed for the DNS-01
# challenge that issues the wildcard cert for *.{ARENA_BASE_DOMAIN}. Stock
# `caddy:2` has no DNS provider modules compiled in and will fail to parse
# (let alone validate) the `tls { dns cloudflare ... }` directive below —
# always validate/run the Caddyfile against THIS image, never `caddy:2`.
FROM caddy:2-builder AS builder
RUN xcaddy build --with github.com/caddy-dns/cloudflare

FROM caddy:2
COPY --from=builder /usr/bin/caddy /usr/bin/caddy
