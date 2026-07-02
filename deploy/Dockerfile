# ──────────────────────────────────────────────
# Stage 1: Prepare recipe (dependency fingerprint)
# ──────────────────────────────────────────────
FROM rust:1.85-slim AS chef

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/* \
    && cargo install cargo-chef

WORKDIR /app

# ──────────────────────────────────────────────
# Stage 2: Compute dependency recipe
# ──────────────────────────────────────────────
FROM chef AS planner

COPY Cargo.toml Cargo.lock ./
COPY crates/proxy-core/Cargo.toml crates/proxy-core/
COPY crates/proxy-api/Cargo.toml crates/proxy-api/
COPY crates/proxy-gateway/Cargo.toml crates/proxy-gateway/
COPY crates/proxy-mcp/Cargo.toml crates/proxy-mcp/
COPY crates/proxy-server/Cargo.toml crates/proxy-server/
COPY crates/proxy-sub/Cargo.toml crates/proxy-sub/
COPY crates/proxy-xray/Cargo.toml crates/proxy-xray/

RUN cargo chef prepare --recipe-path recipe.json

# ──────────────────────────────────────────────
# Stage 3: Build dependencies (cached layer)
# ──────────────────────────────────────────────
FROM chef AS builder

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Now copy the real source code
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY config/ config/

# Build the actual binary (only business code recompiles)
RUN cargo build --release -p proxy-server

# ──────────────────────────────────────────────
# Stage 4: Runtime image
# ──────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# Download GeoLite2 database
RUN mkdir -p /app/geoip \
    && curl -L "https://cdn.jsdelivr.net/gh/P3TERX/GeoLite.mmdb@download/GeoLite2-Country.mmdb" \
         -o /app/geoip/GeoLite2-Country.mmdb

WORKDIR /app

# Copy binary and config
COPY --from=builder /app/target/release/proxy-server /app/proxy-server
COPY config/ /app/config/

EXPOSE 9080 8000 9000

CMD ["/app/proxy-server", "/app/config/settings.yaml"]
