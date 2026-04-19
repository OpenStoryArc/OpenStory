FROM rust:1.88-slim AS builder

# Optional cargo features to enable in the release build. Set via the
# docker-compose `args:` block. Default empty (SQLite-only). Use
# `FEATURES=mongo` to include the MongoDB EventStore backend.
ARG FEATURES=""

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev build-essential protobuf-compiler && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy workspace manifests first for layer caching
COPY Cargo.toml Cargo.lock* ./
COPY bus/Cargo.toml bus/Cargo.toml
COPY cli/Cargo.toml cli/Cargo.toml
COPY core/Cargo.toml core/Cargo.toml
COPY patterns/Cargo.toml patterns/Cargo.toml
COPY server/Cargo.toml server/Cargo.toml
COPY store/Cargo.toml store/Cargo.toml
COPY views/Cargo.toml views/Cargo.toml
COPY schemas/Cargo.toml schemas/Cargo.toml

# Create dummy sources to cache dependency builds
RUN mkdir -p src bus/src cli/src core/src patterns/src schemas/src/bin server/src store/src views/src && \
    echo "pub fn dummy() {}" > src/lib.rs && \
    echo "pub fn dummy() {}" > bus/src/lib.rs && \
    echo "fn main() {}" > cli/src/main.rs && \
    echo "pub fn dummy() {}" > core/src/lib.rs && \
    echo "pub fn dummy() {}" > patterns/src/lib.rs && \
    echo "pub fn dummy() {}" > schemas/src/lib.rs && \
    echo "fn main() {}" > schemas/src/bin/generate.rs && \
    echo "pub fn dummy() {}" > server/src/lib.rs && \
    echo "pub fn dummy() {}" > store/src/lib.rs && \
    echo "pub fn dummy() {}" > views/src/lib.rs && \
    if [ -n "$FEATURES" ]; then \
        cargo build --release -p open-story-cli --features "$FEATURES" 2>/dev/null || true; \
    else \
        cargo build --release -p open-story-cli 2>/dev/null || true; \
    fi

# Copy real source and build (touch forces Cargo to see changed sources)
COPY src/ src/
COPY bus/src/ bus/src/
COPY cli/src/ cli/src/
COPY core/src/ core/src/
COPY patterns/src/ patterns/src/
COPY server/src/ server/src/
COPY store/src/ store/src/
COPY views/src/ views/src/
COPY schemas/src/ schemas/src/
RUN touch src/lib.rs bus/src/lib.rs cli/src/main.rs core/src/lib.rs patterns/src/lib.rs schemas/src/lib.rs server/src/lib.rs store/src/lib.rs views/src/lib.rs && \
    if [ -n "$FEATURES" ]; then \
        cargo build --release -p open-story-cli --features "$FEATURES"; \
    else \
        cargo build --release -p open-story-cli; \
    fi

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/open-story /usr/local/bin/open-story

RUN groupadd --gid 1000 openstory && \
    useradd --uid 1000 --gid openstory --create-home openstory && \
    mkdir -p /data /watch && \
    chown openstory:openstory /data /watch

USER openstory

EXPOSE 3002

ENTRYPOINT ["open-story"]
CMD ["serve", "--host", "0.0.0.0", "--port", "3002", "--data-dir", "/data", "--watch-dir", "/watch"]
