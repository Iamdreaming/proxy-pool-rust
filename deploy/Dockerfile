# ──────────────────────────────────────────────
# Stage 1: Prepare recipe (dependency fingerprint)
# ──────────────────────────────────────────────
FROM rust:1.87-slim AS chef

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev curl xz-utils \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-chef from GitHub release (avoids MSRV mismatch with cargo install)
ARG TARGETARCH
RUN case "${TARGETARCH}" in \
      amd64) CHEF_ARCH="x86_64-unknown-linux-gnu" ;; \
      arm64) CHEF_ARCH="aarch64-unknown-linux-gnu" ;; \
      *) echo "unsupported arch: ${TARGETARCH}"; exit 1 ;; \
    esac \
 && curl -sL "https://github.com/LukeMathWalker/cargo-chef/releases/download/v0.1.77/cargo-chef-${CHEF_ARCH}.tar.xz" \
    | tar xJ -C /usr/local/bin/

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

# Create dummy source files so cargo chef prepare can compute the dependency recipe
RUN mkdir -p crates/proxy-core/src && echo "" > crates/proxy-core/src/lib.rs \
 && mkdir -p crates/proxy-api/src && echo "" > crates/proxy-api/src/lib.rs \
 && mkdir -p crates/proxy-gateway/src && echo "" > crates/proxy-gateway/src/lib.rs \
 && mkdir -p crates/proxy-mcp/src && echo "" > crates/proxy-mcp/src/lib.rs \
 && mkdir -p crates/proxy-server/src && echo "fn main(){}" > crates/proxy-server/src/main.rs \
 && mkdir -p crates/proxy-sub/src && echo "" > crates/proxy-sub/src/lib.rs \
 && mkdir -p crates/proxy-xray/src && echo "" > crates/proxy-xray/src/lib.rs

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
ARG GIT_HASH=dev
ENV GIT_HASH=${GIT_HASH}
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
