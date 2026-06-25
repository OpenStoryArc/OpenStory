#!/usr/bin/env bash
# Spike: validate JetStream cross-domain *sources* as a federation transport.
#
# "Shift prototyping left" step for Idea A
# (docs/research/jetstream-sources-federation.md). Stands up a hub + two leaves
# with JetStream DOMAINS over leafnode connections and validates:
#
#   RISK 1 (cross-domain API reachability): can a leaf reach `$JS.hub.API`?
#           (prereq for the self-registration topology / Option 3)
#   RISK 2 (source loops): the design's loop trap is that a leaf cannot both
#           PUBLISH events.> locally AND SOURCE events.> back from the aggregate
#           — it would re-pull its own messages. The fix this spike proves:
#           split roles into two streams per leaf —
#             • `events`        : local publish target (own events only)
#             • `events-mirror` : source-only, pulls the fleet from hub agg
#           so the fleet view converges with NO loop and NO app-level catch-up.
#
# Success = an event published ONLY to leaf-0 lands in leaf-1's `events-mirror`
# via hub aggregation, each stream holding exactly the expected count.
#
# Usage: scripts/spike_jetstream_sources.sh [--keep]
# Requires: docker (nats:2-alpine + natsio/nats-box).
set -euo pipefail

KEEP=0
[[ "${1:-}" == "--keep" ]] && KEEP=1
PROJECT="js-sources-spike-$$"
WORK="$(mktemp -d)"
COMPOSE="$WORK/docker-compose.yml"
TOKEN="spike-token"

cleanup() {
  if [[ $KEEP -eq 0 ]]; then
    docker compose -f "$COMPOSE" -p "$PROJECT" down -v --remove-orphans >/dev/null 2>&1 || true
    rm -rf "$WORK"
  else
    echo "  [--keep] up. Tear down: docker compose -f $COMPOSE -p $PROJECT down -v"
  fi
}
trap cleanup EXIT

# --- server configs: each NATS gets a JetStream DOMAIN -----------------------
cat > "$WORK/hub.conf" <<EOF
listen: 0.0.0.0:4222
jetstream { store_dir: /data/js, domain: hub }
authorization { token: "$TOKEN" }
leafnodes { listen: "0.0.0.0:7422" }
EOF
mk_leaf_conf() {
  cat > "$WORK/leaf-$1.conf" <<EOF
listen: 0.0.0.0:4222
jetstream { store_dir: /data/js, domain: "leaf-$1" }
leafnodes { remotes [ { url: "nats://$TOKEN@nats-hub:7422" } ] }
EOF
}
mk_leaf_conf 0
mk_leaf_conf 1

# --- stream config JSON (avoids all interactive CLI prompts) -----------------
# Hub aggregate: source-only, pulls each leaf's local `events` across domains.
# (Spike enumerates the 2 leaves to prove the mechanic; production decentralizes
#  this via self-registration — see RISK 1 check below.)
cat > "$WORK/events-agg.json" <<'EOF'
{
  "name": "events-agg",
  "retention": "limits", "storage": "file", "max_bytes": 536870912,
  "discard": "old", "num_replicas": 1,
  "sources": [
    { "name": "events", "external": { "api": "$JS.leaf-0.API" } },
    { "name": "events", "external": { "api": "$JS.leaf-1.API" } }
  ]
}
EOF
# Per-leaf local publish target — binds ONLY its own namespace events.<leaf>.>
# so the racy core leafnode subject propagation cannot cross-pollinate streams.
# (Schema implication for prod: subjects need a per-node component.)
for i in 0 1; do
cat > "$WORK/events-local-$i.json" <<EOF
{ "name": "events", "subjects": ["events.leaf-$i.>"], "retention": "limits",
  "storage": "file", "max_bytes": 536870912, "discard": "old", "num_replicas": 1 }
EOF
done
# Per-leaf fleet mirror: source-only from hub agg (no subjects → no publishers
# → cannot loop). This is the convergent "every machine sees all" view.
cat > "$WORK/events-mirror.json" <<'EOF'
{ "name": "events-mirror", "retention": "limits", "storage": "file",
  "max_bytes": 536870912, "discard": "old", "num_replicas": 1,
  "sources": [ { "name": "events-agg", "external": { "api": "$JS.hub.API" } } ] }
EOF

# --- compose -----------------------------------------------------------------
{
  echo "services:"
  echo "  nats-hub:"
  echo "    image: nats:2-alpine"
  echo "    volumes: [ \"$WORK/hub.conf:/etc/nats.conf:ro\" ]"
  echo "    command: [\"-c\", \"/etc/nats.conf\"]"
  for i in 0 1; do
    echo "  nats-leaf-$i:"
    echo "    image: nats:2-alpine"
    echo "    depends_on: [ nats-hub ]"
    echo "    volumes: [ \"$WORK/leaf-$i.conf:/etc/nats.conf:ro\" ]"
    echo "    command: [\"-c\", \"/etc/nats.conf\"]"
  done
  echo "  box:"
  echo "    image: natsio/nats-box:latest"
  echo "    depends_on: [ nats-hub, nats-leaf-0, nats-leaf-1 ]"
  echo "    volumes: [ \"$WORK:/cfg:ro\" ]"
  echo "    entrypoint: [ \"sh\", \"-c\", \"sleep 100000\" ]"
} > "$COMPOSE"

echo "▶ booting hub + 2 leaves with JetStream domains…"
docker compose -f "$COMPOSE" -p "$PROJECT" up -d --remove-orphans >/dev/null
sleep 6

box() { docker compose -f "$COMPOSE" -p "$PROJECT" exec -T box "$@"; }
leaf_url() { echo "nats://nats-leaf-$1:4222"; }

PASS=1
note() { echo "  $*"; }
check() { if eval "$2"; then note "✓ $1"; else note "✗ $1"; PASS=0; fi; }

# --- RISK 1: leaf reaches the hub-domain JS API ------------------------------
echo "▶ RISK 1: can leaf-0 reach \$JS.hub.API (list hub-domain streams)…"
set +e
box nats --server "$(leaf_url 0)" --js-domain hub account info >/tmp/r1 2>&1
R1=$?
set -e
check "leaf→hub cross-domain JS API reachable over leafnode" "[ $R1 -eq 0 ]"
[ $R1 -ne 0 ] && note "    └ $(tail -2 /tmp/r1 | tr '\n' '|')"

# --- create streams via JSON config ------------------------------------------
echo "▶ creating per-leaf local 'events' + source-only 'events-mirror'…"
for i in 0 1; do
  box nats --server "$(leaf_url $i)" stream add events --config /cfg/events-local-$i.json  >/dev/null 2>&1 \
    || note "    ✗ leaf-$i events create failed"
  box nats --server "$(leaf_url $i)" stream add events-mirror --config /cfg/events-mirror.json >/dev/null 2>&1 \
    || note "    ✗ leaf-$i events-mirror create failed"
done
echo "▶ creating hub aggregate 'events-agg' (sources both leaves)…"
box nats --server "$(leaf_url 0)" --js-domain hub stream add events-agg --config /cfg/events-agg.json >/dev/null 2>&1 \
  || { note "    ✗ agg create failed"; PASS=0; }

# --- publish ONLY to leaf-0 --------------------------------------------------
echo "▶ publishing 5 events to leaf-0 only…"
for n in 1 2 3 4 5; do
  box nats --server "$(leaf_url 0)" pub "events.leaf-0.proj.main" "evt-$n" >/dev/null 2>&1
done
sleep 6  # let agg source from leaf-0, then mirrors source from agg

msgs() { box nats --server "$1" ${3:+--js-domain $3} stream info "$2" --json 2>/dev/null \
  | tr ',' '\n' | grep -m1 '"messages"' | grep -o '[0-9]*'; }

L0=$(msgs   "$(leaf_url 0)" events);               L1=$(msgs   "$(leaf_url 1)" events)
M0=$(msgs   "$(leaf_url 0)" events-mirror);        M1=$(msgs   "$(leaf_url 1)" events-mirror)
AGG=$(msgs  "$(leaf_url 0)" events-agg hub)
echo "▶ counts → leaf-0 events:${L0:-?} mirror:${M0:-?} | leaf-1 events:${L1:-?} mirror:${M1:-?} | hub agg:${AGG:-?}"

# --- assertions --------------------------------------------------------------
check "leaf-0 local 'events' has exactly its own 5 (no loop inflation)" "[ \"${L0:-0}\" -eq 5 ]"
check "leaf-1 local 'events' has 0 (it published nothing)"              "[ \"${L1:-0}\" -eq 0 ]"
check "hub aggregate pulled leaf-0's 5 across domain"                   "[ \"${AGG:-0}\" -ge 5 ]"
check "leaf-1 'events-mirror' sees leaf-0's 5 via transport (no catch-up)" "[ \"${M1:-0}\" -ge 5 ]"
# JetStream loop prevention: a leaf's mirror correctly EXCLUDES events that
# originated on that leaf (won't pull its own data back through the fleet). So
# the complete fleet view on any leaf is: local 'events' ∪ 'events-mirror'.
check "leaf-0 mirror excludes self-origin (loop prevention) → 0"        "[ \"${M0:-0}\" -eq 0 ]"
FLEET0=$(( ${L0:-0} + ${M0:-0} )); FLEET1=$(( ${L1:-0} + ${M1:-0} ))
note  "fleet view (local ∪ mirror) → leaf-0:$FLEET0  leaf-1:$FLEET1"
check "leaf-0 complete fleet view = 5 (own 5 + mirrored 0)"             "[ $FLEET0 -eq 5 ]"
check "leaf-1 complete fleet view = 5 (own 0 + mirrored 5)"             "[ $FLEET1 -eq 5 ]"

echo
if [[ $PASS -eq 1 ]]; then
  echo "✅ SPIKE PASS — cross-domain sources converge leaf→hub→leaf, loop-free, no app catch-up."
  echo "   Topology proven: local 'events' (publish) + source-only 'events-mirror' (fleet view)."
  exit 0
else
  echo "❌ SPIKE INCOMPLETE — see ✗ above. Re-run with --keep to poke the live stack."
  exit 1
fi
