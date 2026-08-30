# syntax=docker/dockerfile:1
# --- build open-story ---
FROM rust:1-bookworm AS oss-build
WORKDIR /src
COPY rs/ rs/
RUN cd rs && cargo build --release -p open-story-cli

# --- runtime ---
FROM node:22-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
      tmux git curl ca-certificates procps \
      jq sqlite3 unzip zip vim less tree \
    && rm -rf /var/lib/apt/lists/*

# tmux reads /etc/tmux.conf as its system-wide config for every user. Placed
# outside /home/dev deliberately: the DockerDriver mounts /home/dev as a
# per-user volume at runtime, which shadows anything baked there at build
# time (see the ~/.scratch / ~/.local/bin boot-time mkdir in welcome.sh for
# the same issue). /etc is never shadowed, so this survives without a
# boot-time copy.
COPY arena/sandbox/tmux.conf /etc/tmux.conf

# Same shadow class, different mechanism: a login shell (bash -lc) sources
# /etc/profile, which unconditionally rebuilds PATH from scratch and then
# sources /etc/profile.d/*.sh — discarding whatever PATH was inherited from
# this Dockerfile's ENV. Normally ~/.profile re-appends ~/.local/bin
# afterward, but ~/.profile is itself a build-time file under /home/dev and
# is shadowed on a returning user's already-populated volume exactly like
# ~/.tmux.conf was. Fix it the same way: put the append in /etc/profile.d,
# which every login shell sources regardless of volume history.
RUN printf '%s\n' \
      'case ":$PATH:" in' \
      '  *":$HOME/.local/bin:"*) ;;' \
      '  *) PATH="$HOME/.local/bin:$PATH" ;;' \
      'esac' \
    > /etc/profile.d/local-bin-path.sh

# ttyd: not packaged in Debian bookworm's apt repos (verified — apt-cache policy
# shows no candidate on amd64 or arm64, incl. backports). Install the upstream
# static release binary instead, matching the image's architecture.
ARG TTYD_VERSION=1.7.7
RUN set -eux; \
    arch="$(dpkg --print-architecture)"; \
    case "$arch" in \
      amd64) ttyd_arch=x86_64 ;; \
      arm64) ttyd_arch=aarch64 ;; \
      *) echo "unsupported architecture: $arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL -o /usr/local/bin/ttyd \
      "https://github.com/tsl0922/ttyd/releases/download/${TTYD_VERSION}/ttyd.${ttyd_arch}"; \
    chmod +x /usr/local/bin/ttyd

# nats-server (open-story --manage-nats spawns it)
ARG NATS_VERSION=2.10.22
RUN curl -fsSL "https://github.com/nats-io/nats-server/releases/download/v${NATS_VERSION}/nats-server-v${NATS_VERSION}-linux-$(dpkg --print-architecture).tar.gz" \
    | tar -xz -C /tmp \
 && mv /tmp/nats-server-v${NATS_VERSION}-linux-*/nats-server /usr/local/bin/ \
 && rm -rf /tmp/nats-server-v*

RUN npm install -g @anthropic-ai/claude-code

COPY --from=oss-build /src/rs/target/release/open-story /usr/local/bin/open-story
COPY arena/sandbox/welcome.sh /usr/local/bin/welcome.sh
RUN chmod +x /usr/local/bin/welcome.sh

# curated starter workspace baked at /opt/workspace (seeded into $HOME on first boot)
RUN mkdir -p /opt/workspace && git -C /opt/workspace init -q
COPY arena/sandbox/skel/CLAUDE.md arena/sandbox/skel/README.md /opt/workspace/
RUN cd /opt/workspace && git add -A && git -c user.email=arena@localhost -c user.name=arena commit -qm seed
COPY arena/sandbox/skel/settings.json /opt/skel/claude-settings.json

# node:22-bookworm-slim ships a uid-1000 `node` user; drop it and create `dev`
# so paths read as the plan says (matches DockerDriver's HOME_MOUNT = "/home/dev").
RUN userdel -r node 2>/dev/null || true; useradd -m -u 1000 -s /bin/bash dev
USER dev
ENV HOME=/home/dev
ENV PATH="/home/dev/.local/bin:${PATH}"
RUN mkdir -p /home/dev/.local/bin /home/dev/.scratch
WORKDIR /home/dev

EXPOSE 7681 3002
# ttyd only spawns its child command when a client connects to the web
# terminal — it does not run anything at boot on its own. Since welcome.sh
# starts open-story (which must be observing from container start, not from
# "whenever someone happens to open a browser tab"), create the tmux session
# eagerly and have ttyd attach to whatever's already running. `new-session -A`
# is idempotent: it creates the session if missing, or just attaches if a
# client dials in after it already exists.
CMD ["sh", "-c", "tmux new-session -d -s main /usr/local/bin/welcome.sh; exec ttyd -p 7681 -W tmux new-session -A -s main /usr/local/bin/welcome.sh"]
