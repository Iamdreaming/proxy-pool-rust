# Multi-stage build for proxy-pool-rust
FROM rust:1.85-slim AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY crates/proxy-core/Cargo.toml crates/proxy-core/
COPY crates/proxy-api/Cargo.toml crates/proxy-api/
COPY crates/proxy-gateway/Cargo.toml crates/proxy-gateway/
COPY crates/proxy-mcp/Cargo.toml crates/proxy-mcp/
COPY crates/proxy-server/Cargo.toml crates/proxy-server/

# Create dummy source files for initial build
RUN mkdir -p crates/proxy-core/src && echo "" > crates/proxy-core/src/lib.rs \
    && mkdir -p crates/proxy-api/src && echo "" > crates/proxy-api/src/lib.rs \
    && mkdir -p crates/proxy-gateway/src && echo "" > crates/proxy-gateway/src/lib.rs \
    && mkdir -p crates/proxy-mcp/src && echo "" > crates/proxy-mcp/src/lib.rs \
    && mkdir -p crates/proxy-server/src && echo "fn main(){}" > crates/proxy-server/src/main.rs

# Build dependencies only (cached layer)
RUN cargo build --release 2>/dev/null || true

# Copy actual source code
COPY crates/ crates/
COPY config/ config/

# Touch source files to rebuild with real code
RUN find crates -name "*.rs" -exec touch {} +

# Build the actual binary
RUN cargo build --release -p proxy-server

# Runtime image
FROM debian:bookworm-slim

# Install runtime dependencies
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
