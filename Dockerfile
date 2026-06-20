# ── Stage 1: build ────────────────────────────────────────────────────────────
FROM rust:1.79-slim AS builder

# Install build-time system deps (needed for rav1e's NASM-based assembly).
RUN apt-get update && apt-get install -y --no-install-recommends \
    nasm \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies before copying source (layer caching).
COPY Cargo.toml Cargo.lock ./
COPY core/Cargo.toml      core/Cargo.toml
COPY cli/Cargo.toml       cli/Cargo.toml
COPY service/Cargo.toml   service/Cargo.toml

# Create stub lib/main files so `cargo fetch` succeeds without the real source.
RUN mkdir -p core/src cli/src service/src \
    && echo "fn main(){}" > cli/src/main.rs \
    && echo "fn main(){}" > service/src/main.rs \
    && echo "" > core/src/lib.rs \
    && cargo fetch

# Now copy the real source and build release binaries.
COPY core/    core/
COPY cli/     cli/
COPY service/ service/

RUN cargo build --release -p ferrox -p ferrox-service

# ── Stage 2: runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/ferrox         /usr/local/bin/ferrox
COPY --from=builder /build/target/release/ferrox-service /usr/local/bin/ferrox-service

# Default: run the HTTP service on port 8080.
ENV FERROX_ADDR=0.0.0.0:8080
ENV FERROX_LOG=info

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/ferrox-service"]
CMD ["--addr", "0.0.0.0:8080"]
