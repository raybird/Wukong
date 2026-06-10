# syntax=docker/dockerfile:1
# ── Build stage ──
FROM rust:1.96-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .

# Build all workspace binaries in release mode
RUN cargo build --release --workspace && \
    mkdir -p /bins && \
    cp target/release/wukong /bins/ && \
    cp target/release/wukong-telegram /bins/ && \
    cp target/release/wukong-web /bins/

# ── Runtime stage ──
FROM debian:bookworm-slim

# Install runtime deps + gosu + opencode
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git gosu && \
    # Install opencode via official install script (installs to /root/.local/bin by default)
    curl -fsSL https://raw.githubusercontent.com/opencode-ai/opencode/refs/heads/main/install | bash && \
    # Move opencode binary to globally accessible location for all users
    if [ -f "/root/.local/bin/opencode" ]; then \
        cp /root/.local/bin/opencode /usr/local/bin/opencode; \
        chmod +x /usr/local/bin/opencode; \
    elif command -v opencode >/dev/null 2>&1; then \
        cp "$(command -v opencode)" /usr/local/bin/opencode; \
        chmod +x /usr/local/bin/opencode; \
    fi && \
    rm -rf /var/lib/apt/lists/*

# Copy wukong binaries
COPY --from=builder /bins/* /usr/local/bin/

# Create non-root user (UID/GID will be remapped at runtime via entrypoint)
RUN useradd -m -s /bin/bash wukong

# Prepare directories and entrypoint
COPY scripts/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# Default environment
ENV WUKONG_WORKSPACE=/workspace
ENV HOME=/home/wukong
ENV PATH="/home/wukong/.local/bin:/usr/local/bin:${PATH}"

WORKDIR /workspace
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
