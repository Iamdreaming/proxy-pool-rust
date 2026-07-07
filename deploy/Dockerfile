# ──────────────────────────────────────────────
# Stage 1: Prepare cargo-chef toolchain
# ──────────────────────────────────────────────
FROM rust:1.96-slim AS chef

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev curl xz-utils protobuf-compiler musl-tools \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-chef from GitHub release (avoids MSRV mismatch with cargo install)
ARG TARGETARCH
RUN case "${TARGETARCH}" in \
      amd64) CHEF_ARCH="x86_64-unknown-linux-gnu" ;; \
      arm64) CHEF_ARCH="aarch64-unknown-linux-gnu" ;; \
      *) echo "unsupported arch: ${TARGETARCH}"; exit 1 ;; \
    esac \
 && curl -sL "https://github.com/LukeMathWalker/cargo-chef/releases/download/v0.1.77/cargo-chef-${CHEF_ARCH}.tar.xz" \
    | tar xJ --strip-components=1 -C /usr/local/bin/ "cargo-chef-${CHEF_ARCH}/cargo-chef"

# Install musl target for static linking (no glibc dependency in runtime image)
RUN rustup target add x86_64-unknown-linux-musl

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
 && echo "fn main(){}" > crates/proxy-server/build.rs \
 && mkdir -p crates/proxy-sub/src && echo "" > crates/proxy-sub/src/lib.rs \
 && mkdir -p crates/proxy-xray/src && echo "" > crates/proxy-xray/src/lib.rs

RUN cargo chef prepare --recipe-path recipe.json

# ──────────────────────────────────────────────
# Stage 3: Build web UI
# ──────────────────────────────────────────────
FROM node:22-slim AS web-builder

WORKDIR /web
COPY web/package.json web/package-lock.json* ./
RUN npm ci
COPY web/ ./
RUN npm run build

# ──────────────────────────────────────────────
# Stage 4: Build dependencies + binary
# ──────────────────────────────────────────────
FROM chef AS builder

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --target x86_64-unknown-linux-musl --recipe-path recipe.json

# Now copy the real source code
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Build the actual binary (only business code recompiles)
# Use musl target for static linking — no glibc dependency in runtime image
ARG GIT_HASH=unknown
ENV GIT_HASH=${GIT_HASH}
RUN cargo build --release -p proxy-server --target x86_64-unknown-linux-musl \
 && cp /app/target/x86_64-unknown-linux-musl/release/proxy-server /app/target/release/proxy-server

# ──────────────────────────────────────────────
# Stage 5: Runtime image
# ──────────────────────────────────────────────
FROM debian:trixie-slim

ARG TARGETARCH
ARG XRAY_VERSION=v26.3.27
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl unzip \
    && case "${TARGETARCH}" in \
         amd64) XRAY_ASSET="Xray-linux-64.zip" ;; \
         arm64) XRAY_ASSET="Xray-linux-arm64-v8a.zip" ;; \
         *) echo "unsupported xray arch: ${TARGETARCH}"; exit 1 ;; \
       esac \
    && if [ "${XRAY_VERSION}" = "latest" ]; then \
         XRAY_URL="https://github.com/XTLS/Xray-core/releases/latest/download/${XRAY_ASSET}"; \
       else \
         XRAY_URL="https://github.com/XTLS/Xray-core/releases/download/${XRAY_VERSION}/${XRAY_ASSET}"; \
       fi \
    && curl -fsSL "${XRAY_URL}" -o /tmp/xray.zip \
    && mkdir -p /tmp/xray \
    && unzip -q /tmp/xray.zip -d /tmp/xray \
    && install -m 0755 /tmp/xray/xray /usr/local/bin/xray \
    && xray version \
    && apt-get purge -y --auto-remove unzip \
    && rm -rf /var/lib/apt/lists/* /tmp/xray /tmp/xray.zip

# Download GeoLite2 database
RUN mkdir -p /app/geoip \
    && curl -L "https://cdn.jsdelivr.net/gh/P3TERX/GeoLite.mmdb@download/GeoLite2-Country.mmdb" \
         -o /app/geoip/GeoLite2-Country.mmdb

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/proxy-server /app/proxy-server

# Copy web UI from web-builder
COPY --from=web-builder /web/dist /app/web

# Copy example config (user should mount their own settings.yaml via volume)
COPY config/*.example.yaml /app/config/

EXPOSE 9080 8000 9000

CMD ["/app/proxy-server", "/app/config/settings.yaml"]
