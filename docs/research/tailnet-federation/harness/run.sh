#!/usr/bin/env bash
# Tailnet-federation prototype — hermetic harness.
#
# Proves two OpenStory-shaped nodes can federate events over a purpose-built
# Tailscale tailnet governed by a tiny tag-based ACL, with NO shared NATS token
# (the tailnet IS the auth layer). Self-contained: Headscale is the control
# server, so there's no dependency on login.tailscale.com and no secrets.
#
# SCIENTIFIC VALIDATION via path-observation + causal ablation (`./run.sh prove`).
# A test that asserts "B got the session" is worthless alone — B might reach A
# over the docker bridge. We instead prove the path DIRECTLY and demonstrate
# CAUSAL DEPENDENCE on the tailnet with negative controls:
#   - The NATS leaf dials 100.64.0.1 — a CGNAT tailnet IP (100.64.0.0/10),
#     routable ONLY via tailscale0, never a bridge IP. The destination proves
#     the path by construction.
#   - We OBSERVE the live ESTAB socket to 100.64.0.1:7422 in the leaf netns.
#   - ABLATIONS (each must FALSIFY federation): `tailscale down` -> leaf drops;
#     ACL denies :7422 -> leaf never connects; :9999 over the tailnet refused.
# (We do NOT use disjoint networks: that forces DERP relay, which a hermetic
#  HTTP Headscale can't do without TLS. Direct WireGuard on a shared underlay +
#  the ablations above is more direct evidence anyway — it shows the mechanism.)
#
#   hs-control   headscale control + embedded DERP
#   os-node-a    tailscale node, tag:os-peer  -> 100.64.0.1   nats-hub  :7422 (sidecar)
#   os-node-b    tailscale node, tag:os-peer  -> 100.64.0.2   nats-leaf dials 100.64.0.1
#
# Production mapping: each os-node-X is the Tailscale *sidecar* that gives an
# OpenStory machine its tailnet identity; NATS (and OpenStory itself) ride in
# that netns. The Rust server never embeds Go tsnet — the sidecar provides it.
#
# Usage: ./run.sh up|down|test|p4|p4test|prove
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
NET=os-tailnet
HS=headscale/headscale:latest
TS=tailscale/tailscale:latest
NATS=nats:2.10-alpine
BOX=natsio/nats-box:latest
CURL=curlimages/curl:latest

hs() { docker exec hs-control headscale "$@"; }

OS=open-story:test
SESSION=tailnet-demo-001

down() {
  docker rm -f hs-control os-node-a os-node-b nats-hub nats-leaf os-sub1 os-sub2 nc9999 \
    openstory-a openstory-b >/dev/null 2>&1 || true
  docker volume rm hs-data os-watch-a os-watch-b >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  echo "torn down"
}

# up [acl_file] — bring up the stack; optional ACL file overrides the default
# (used by ablation experiments to swap in a deny-:7422 policy).
up() {
  local acl="${1:-$HERE/headscale/acl.json}"
  down
  docker network create "$NET" >/dev/null
  echo "==> P0: headscale control server"
  docker run -d --name hs-control --network "$NET" \
    -v "$HERE/headscale/config.yaml:/etc/headscale/config.yaml:ro" \
    -v "$acl:/etc/headscale/acl.json:ro" \
    -v hs-data:/var/lib/headscale "$HS" serve >/dev/null
  sleep 4
  hs users create club >/dev/null 2>&1 || true
  # Tag is stamped on the preauth key -> nodes register pre-tagged and inherit
  # the restrictive filter immediately (the inference.club tagged-authkey model).
  KEY=$(hs preauthkeys create --user 1 --reusable --tags tag:os-peer --expiration 24h -o json 2>/dev/null \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["key"])')
  echo "    preauth key minted (tag:os-peer)"

  echo "==> P1: two tailscale nodes join"
  for n in a b; do
    docker run -d --name os-node-$n --network "$NET" --hostname os-node-$n \
      --cap-add NET_ADMIN --device /dev/net/tun \
      -e TS_AUTHKEY="$KEY" \
      -e TS_EXTRA_ARGS="--login-server=http://hs-control:8080 --accept-routes" \
      -e TS_HOSTNAME=os-node-$n -e TS_USERSPACE=false -e TS_STATE_DIR=/var/lib/tailscale "$TS" >/dev/null
  done
  sleep 6
  hs nodes list 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | awk '/os-node/ {print "    "$3" "$13" "$15}'

  echo "==> P3: NATS hub + leaf as sidecars (share node netns)"
  docker run -d --name nats-hub --network container:os-node-a \
    -v "$HERE/nats/hub.conf:/etc/nats/hub.conf:ro" "$NATS" -c /etc/nats/hub.conf -js >/dev/null
  docker run -d --name nats-leaf --network container:os-node-b \
    -v "$HERE/nats/leaf.conf:/etc/nats/leaf.conf:ro" "$NATS" -c /etc/nats/leaf.conf -js >/dev/null
  sleep 4
  echo "up."
}

test_all() {
  local fail=0
  echo "== ACL enforcement (clubhouse policy = os-peer -> os-peer:7422 only) =="
  docker rm -f nc9999 >/dev/null 2>&1 || true
  docker exec -d os-node-a sh -c 'while true; do echo OPEN | nc -l -p 9999; done'
  sleep 1
  r=$(docker exec os-node-b sh -c 'timeout 4 nc -w 3 100.64.0.1 7422 </dev/null >/dev/null 2>&1 && echo CONNECTED || echo blocked')
  echo "  :7422 allowed -> $r"; [ "$r" = CONNECTED ] || fail=1
  r=$(docker exec os-node-b sh -c 'timeout 4 nc -w 3 100.64.0.1 9999 </dev/null >/dev/null 2>&1 && echo CONNECTED || echo blocked')
  echo "  :9999 denied  -> $r"; [ "$r" = blocked ] || fail=1
  # ICMP note: Tailscale always permits ICMP echo to a host the src is granted
  # ANY port on (filter shows IPProto[6,17]:7422 only — no ICMP rule, yet ping
  # works). It's a reachability affordance, not service access. The security
  # boundary that matters is the TCP/UDP port scope above, which holds.
  r=$(docker exec os-node-b sh -c 'ping -c 1 -W 3 100.64.0.1 >/dev/null 2>&1 && echo reachable || echo blocked')
  echo "  ICMP (expected reachable, not a service path) -> $r"

  echo "== NATS federation over the tailnet (bidirectional) =="
  lc=$(docker run --rm --network container:os-node-a "$CURL" -s http://127.0.0.1:8222/leafz \
        | python3 -c 'import json,sys; print(json.load(sys.stdin).get("leafnodes"))')
  echo "  hub sees leaf_count=$lc"; [ "$lc" = 1 ] || fail=1

  docker rm -f os-sub1 >/dev/null 2>&1 || true
  docker run -d --name os-sub1 --network container:os-node-a "$BOX" \
    sh -c 'nats sub "events.>" --count=1 -s nats://127.0.0.1:4222' >/dev/null
  sleep 2
  docker run --rm --network container:os-node-b "$BOX" \
    nats pub events.session.abc "LEAF->HUB over tailnet" -s nats://127.0.0.1:4222 >/dev/null 2>&1
  sleep 2
  if docker logs os-sub1 2>&1 | grep -q 'LEAF->HUB over tailnet'; then
    echo "  leaf->hub propagated -> OK"; else echo "  leaf->hub -> FAIL"; fail=1; fi

  docker rm -f os-sub2 >/dev/null 2>&1 || true
  docker run -d --name os-sub2 --network container:os-node-b "$BOX" \
    sh -c 'nats sub "events.>" --count=1 -s nats://127.0.0.1:4222' >/dev/null
  sleep 2
  docker run --rm --network container:os-node-a "$BOX" \
    nats pub events.session.xyz "HUB->LEAF over tailnet" -s nats://127.0.0.1:4222 >/dev/null 2>&1
  sleep 2
  if docker logs os-sub2 2>&1 | grep -q 'HUB->LEAF over tailnet'; then
    echo "  hub->leaf propagated -> OK"; else echo "  hub->leaf -> FAIL"; fail=1; fi

  echo
  [ "$fail" = 0 ] && echo "ALL GREEN" || echo "FAILURES ABOVE"
  return $fail
}

os_api() { docker run --rm --network "container:os-node-$1" "$CURL" -s "http://127.0.0.1:3002$2" 2>/dev/null; }

p4() {
  echo "==> P4: two OpenStory instances federating over the tailnet"
  # Recreate NATS sidecars with current confs (max_file bump for ensure_streams).
  docker rm -f nats-hub nats-leaf openstory-a openstory-b >/dev/null 2>&1 || true
  docker run -d --name nats-hub --network container:os-node-a \
    -v "$HERE/nats/hub.conf:/etc/nats/hub.conf:ro" "$NATS" -c /etc/nats/hub.conf -js >/dev/null
  docker run -d --name nats-leaf --network container:os-node-b \
    -v "$HERE/nats/leaf.conf:/etc/nats/leaf.conf:ro" "$NATS" -c /etc/nats/leaf.conf -js >/dev/null
  sleep 3

  # Boot B FIRST (empty watch) so its consumer subscribes + the leaf stream syncs
  # BEFORE A publishes — matches the live-propagation path proven in P3.
  docker volume rm os-watch-a os-watch-b >/dev/null 2>&1 || true
  docker volume create os-watch-b >/dev/null
  docker run -d --name openstory-b --network container:os-node-b \
    -e OPEN_STORY_WATCH_BACKFILL_HOURS=0 -v os-watch-b:/watch "$OS" >/dev/null
  echo "    waiting for openstory-b API + NATS subscription..."
  for i in $(seq 1 30); do os_api b /api/sessions >/dev/null 2>&1 && break; sleep 1; done
  sleep 5  # let consumers subscribe and the leaf stream sync

  # Pre-seed A's watch volume, THEN boot A last. On boot it scans /watch, reads
  # the session, translates, publishes live -> propagates over the tailnet to B.
  docker volume create os-watch-a >/dev/null
  python3 "$HERE/gen_session.py" "$SESSION" > "$HERE/.seed.jsonl"
  docker run --rm -v os-watch-a:/w -v "$HERE/.seed.jsonl:/src.jsonl:ro" "$BOX" \
    sh -c 'cp /src.jsonl /w/'"$SESSION"'.jsonl'
  docker run -d --name openstory-a --network container:os-node-a \
    -e OPEN_STORY_WATCH_BACKFILL_HOURS=0 -v os-watch-a:/watch "$OS" >/dev/null
  echo "    waiting for openstory-a API..."
  for i in $(seq 1 30); do os_api a /api/sessions >/dev/null 2>&1 && break; sleep 1; done
  echo "p4 up."
}

p4test() {
  local fail=0
  echo "== session present at origin (openstory-a)? =="
  for i in $(seq 1 20); do
    os_api a /api/sessions | grep -q "$SESSION" && { echo "  A has $SESSION -> OK"; break; }
    [ "$i" = 20 ] && { echo "  A missing $SESSION -> FAIL"; fail=1; }; sleep 1
  done
  echo "== session FEDERATED to openstory-b over the tailnet? =="
  local got=0
  for i in $(seq 1 40); do
    if os_api b /api/sessions | grep -q "$SESSION"; then got=1; echo "  B received $SESSION after ${i}s -> OK"; break; fi
    sleep 1
  done
  [ "$got" = 1 ] || { echo "  B never received $SESSION -> FAIL"; fail=1; }
  echo
  [ "$fail" = 0 ] && echo "P4 GREEN: a real OpenStory session crossed the tailnet" || echo "P4 FAILURES ABOVE"
  return $fail
}

# ── scientific validation helpers ──────────────────────────────────────────
# Robustness lessons (from the first a1 run): the leaf needs ~10-20s to
# establish, so POLL — never fixed-sleep-then-read. And bash functions are NOT
# visible inside `sh -c`, so every predicate is a real function called directly.
leaf_count() {  # how many leaf connections the hub currently sees
  docker run --rm --network container:os-node-a "$CURL" -s http://127.0.0.1:8222/leafz 2>/dev/null \
    | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("leafnodes",0))
except Exception: print(0)' 2>/dev/null || echo 0
}
leaf_ip() {  # the remote IP the hub sees for its leaf (its testimony of the path)
  docker run --rm --network container:os-node-a "$CURL" -s http://127.0.0.1:8222/leafz 2>/dev/null \
    | python3 -c 'import json,sys
try:
    ls=json.load(sys.stdin).get("leafs",[]); print(ls[0].get("ip","") if ls else "")
except Exception: print("")' 2>/dev/null || echo ""
}
wait_leaf() {  # poll until leaf_count == $1 (or $2 secs elapse)
  local i; for i in $(seq 1 "$2"); do [ "$(leaf_count)" = "$1" ] && return 0; sleep 1; done; return 1
}
wait_no_estab() {  # poll until the socket to the hub is gone (or $1 secs elapse)
  local i; for i in $(seq 1 "$1"); do estab_to_hub || return 0; sleep 1; done; return 1
}
leaf_count_is() { [ "$(leaf_count)" = "$1" ]; }
leaf_ip_is_cgnat() {  # hub reports the leaf arriving from a 100.64.0.0/10 tailnet IP
  python3 -c 'import ipaddress,sys
ip=sys.argv[1]
sys.exit(0 if ip and ipaddress.ip_address(ip) in ipaddress.ip_network("100.64.0.0/10") else 1)' "$(leaf_ip)" 2>/dev/null
}
estab_to_hub() {  # live ESTABLISHED socket to 100.64.0.1:7422 in node-b's netns
  # /proc/net/tcp: 100.64.0.1 -> hex 01004064, port 7422 -> 1CFE, state 01.
  docker exec os-node-b cat /proc/net/tcp 2>/dev/null \
    | awk '$3=="01004064:1CFE" && $4=="01"{f=1} END{exit !f}'
}
no_estab_to_hub() { ! estab_to_hub; }
tcp_over_tailnet() {  # can node-b open TCP to 100.64.0.1:$1 over the tailnet?
  docker exec os-node-b sh -c "timeout 4 nc -w 3 100.64.0.1 $1 </dev/null" >/dev/null 2>&1
}
tcp_7422_open()     { tcp_over_tailnet 7422; }
tcp_9999_refused()  { ! tcp_over_tailnet 9999; }
tcp_7422_refused()  { ! tcp_over_tailnet 7422; }
msg_crosses() {  # publish on leaf (node-b), receive on hub (node-a)
  docker rm -f os-sub1 >/dev/null 2>&1 || true
  docker run -d --name os-sub1 --network container:os-node-a "$BOX" \
    sh -c 'nats sub "events.>" --count=1 -s nats://127.0.0.1:4222' >/dev/null 2>&1
  sleep 2
  docker run --rm --network container:os-node-b "$BOX" \
    nats pub events.probe "x" -s nats://127.0.0.1:4222 >/dev/null 2>&1
  sleep 2
  docker logs os-sub1 2>&1 | grep -q 'events.probe'
}
filter_is_7422_only() {  # node-a's compiled packet filter == TCP/UDP to :7422, nothing else
  docker exec os-node-a tailscale debug netmap 2>/dev/null | python3 -c '
import json,sys
d=json.load(sys.stdin); pf=d.get("PacketFilter",[])
ports=set()
for r in pf:
    for x in r.get("Dsts",[]): p=x.get("Ports",{}); ports.add((p.get("First"),p.get("Last")))
print("OK" if ports=={(7422,7422)} else "NO "+str(ports))
' 2>/dev/null | grep -q '^OK'
}

prove() {
  set +e   # an experiment: negative controls EXPECT failures; never abort the run
  local pass=0 total=0
  chk() { total=$((total+1)); if "$@"; then echo "  [PASS]"; pass=$((pass+1)); else echo "  [FAIL]"; fi; }

  echo "################################################################"
  echo "#  CONTROLLED EXPERIMENT"
  echo "#  H1: NATS federation rides the Tailscale tailnet (not a bridge)"
  echo "#  H2: the tag-ACL is the real permission boundary"
  echo "#  Method: direct path observation + falsifiable negative controls"
  echo "################################################################"

  echo; echo ">> setup: clean stack, clubhouse ACL (allow os-peer->os-peer:7422)"
  up >/dev/null 2>&1; wait_leaf 1 45

  echo; echo "E1  POSITIVE — federation works and the path IS the tailnet"
  echo -n "  (a) hub sees exactly 1 leaf connection ............."; chk leaf_count_is 1
  echo -n "  (b) hub reports leaf arriving from a 100.64/10 IP .."; chk leaf_ip_is_cgnat
  echo -n "  (c) live ESTAB socket to 100.64.0.1:7422 in node-b ."; chk estab_to_hub
  echo -n "  (d) a published event crosses leaf->hub ..........."; chk msg_crosses

  echo; echo "E2  NEGATIVE CONTROL — filter is port-scoped (no lateral movement)"
  docker exec -d os-node-a sh -c 'while true; do echo OPEN | nc -l -p 9999; done' 2>/dev/null || true; sleep 1
  echo -n "  (a) :7422 over tailnet connects ..................."; chk tcp_7422_open
  echo -n "  (b) :9999 over tailnet REFUSED (must fail) ........"; chk tcp_9999_refused
  echo -n "  (c) compiled filter == TCP/UDP :7422 only ........."; chk filter_is_7422_only

  echo; echo "E3  ABLATION (causal, reversible) — cut the route to the tailnet hub IP"
  echo "      drop packets to 100.64.0.1 (the CGNAT hub) in node-b. A bridge path"
  echo "      would be UNAFFECTED. Node + tailscaled stay UP — we sever only the"
  echo "      route to the tailnet peer, so federation can actually heal afterward."
  # NB: \`tailscale down\` is NOT used — it makes containerboot (PID 1) exit and
  # kills the node, which would prevent the heal. iptables is the surgical knife.
  # Block BOTH directions: an OUTPUT-only drop leaves the hub half-open (it still
  # receives our SYNs' path) and its ping-timeout lags >40s. A full bidirectional
  # partition mirrors a real tailnet outage and both sides detect within ping_max.
  docker exec os-node-b iptables -I OUTPUT -d 100.64.0.1 -j DROP 2>/dev/null || true
  docker exec os-node-b iptables -I INPUT  -s 100.64.0.1 -j DROP 2>/dev/null || true
  wait_no_estab 45  # NATS ping-timeout (~10-15s) tears the socket down; poll, don't fixed-sleep
  echo -n "  (a) ESTAB socket to 100.64.0.1:7422 is gone ......."; chk no_estab_to_hub
  wait_leaf 0 60
  echo -n "  (b) hub drops the leaf to 0 ......................."; chk leaf_count_is 0
  echo "      unblock the path — federation should heal on its own (node never died)"
  docker exec os-node-b iptables -D OUTPUT -d 100.64.0.1 -j DROP 2>/dev/null || true
  docker exec os-node-b iptables -D INPUT  -s 100.64.0.1 -j DROP 2>/dev/null || true
  wait_leaf 1 45
  echo -n "  (c) leaf reconnects to 1 (reversible tailnet switch)"; chk leaf_count_is 1

  echo; echo "E4  ABLATION (permission) — deny :7422 in the ACL, rebuild from scratch"
  echo "      identical stack, ONLY the policy port changed 7422->9999. Federation must not form."
  up "$HERE/headscale/acl-deny.json" >/dev/null 2>&1; wait_leaf 1 30  # give it a fair chance; expect it to stay 0
  echo -n "  (a) leaf NEVER connects under deny policy ........."; chk leaf_count_is 0
  echo -n "  (b) :7422 over tailnet now REFUSED ................"; chk tcp_7422_refused

  echo; echo ">> restore clubhouse ACL (leave the stack healthy)"
  up >/dev/null 2>&1; wait_leaf 1 45

  echo; echo "================================================================"
  echo "  RESULT: $pass / $total assertions passed"
  if [ "$pass" = "$total" ]; then
    echo "  H1 + H2 SUPPORTED: every positive held AND every negative control"
    echo "  falsified federation exactly when the tailnet / ACL was removed."
  else echo "  INCOMPLETE — see [FAIL] lines above."; fi
  echo "================================================================"
  [ "$pass" = "$total" ]
}

case "${1:-up}" in
  up) up "${2:-}" ;;
  down) down ;;
  test) test_all ;;
  p4) p4 ;;
  p4test) p4test ;;
  prove) prove ;;
  *) echo "usage: $0 up|down|test|p4|p4test|prove"; exit 2 ;;
esac
