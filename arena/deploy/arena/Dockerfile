# Built with context = repo root (see docker-compose.yml's `build.context:
# ../..`) so this Dockerfile can COPY the arena crate directly; arena has its
# own Cargo.toml and is not a member of the rs/ workspace, so nothing else
# from the repo is needed to compile it.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY arena/ arena/
RUN cd arena && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/arena/target/release/arena /usr/local/bin/arena
CMD ["arena", "serve"]
