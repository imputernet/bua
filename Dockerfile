# Dockerfile — Bua runtime container
#
# Multi-stage build:
#   Stage 1 (builder): Rust + Cargo build environment
#   Stage 2 (runtime): Minimal Debian image with only the binary
#
# Build:
#   docker build -t bua:latest .
#
# Run:
#   docker run --rm -v $(pwd):/workspace bua:latest \
#     bua run /workspace/agent.ts --allow-fs=/workspace

# ---------------------------------------------------------------------------
# Stage 1: Builder
# ---------------------------------------------------------------------------
FROM rust:stable-bookworm AS builder

ARG BUA_VERSION=dev
ENV BUA_VERSION=${BUA_VERSION}

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    clang \
    libjavascriptcoregtk-4.1-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy workspace files (layered for Docker cache efficiency)
COPY Cargo.toml Cargo.lock ./
COPY core/Cargo.toml core/
COPY runtime/Cargo.toml runtime/build.rs runtime/
COPY cli/Cargo.toml cli/
COPY jsc/bindings/Cargo.toml jsc/bindings/

# Pre-build dependencies (cache layer)
RUN mkdir -p core/src runtime/src cli/src jsc/bindings \
    && echo 'pub fn placeholder() {}' > core/src/lib.rs \
    && echo 'pub fn placeholder() {}' > runtime/src/lib.rs \
    && echo 'fn main() {}' > cli/src/main.rs \
    && echo '// placeholder' > jsc/bindings/bua_jsc_sys.rs \
    && cargo build --release 2>/dev/null || true \
    && rm -f target/release/deps/bua* target/release/deps/bua_*

# Copy full source
COPY . .

# Build the real binary
RUN cargo build --release --package bua

# Strip the binary
RUN strip target/release/bua

# ---------------------------------------------------------------------------
# Stage 2: Runtime image
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

ARG BUA_VERSION=dev
LABEL org.opencontainers.image.title="Bua Runtime" \
      org.opencontainers.image.description="AI-native deterministic JavaScript runtime for autonomous agents" \
      org.opencontainers.image.version="${BUA_VERSION}" \
      org.opencontainers.image.source="https://github.com/bua-runtime/bua" \
      org.opencontainers.image.licenses="MIT"

# Runtime JSC library (needed at runtime on Linux)
RUN apt-get update && apt-get install -y --no-install-recommends \
    libjavascriptcoregtk-4.1-0 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -m -s /bin/sh -u 1000 bua

COPY --from=builder /build/target/release/bua /usr/local/bin/bua

# Default workspace directory
RUN mkdir -p /workspace && chown bua:bua /workspace
WORKDIR /workspace

USER bua

ENTRYPOINT ["bua"]
CMD ["--help"]
