# syntax=docker/dockerfile:1
# ── Build stage: compile local workspace binaries ──
FROM rust:1-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates pkg-config libssl-dev libsqlite3-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .
RUN cargo build --release --locked -p wukong-cli -p wukong-telegram -p wukong-web

# ── Runtime stage ──
FROM debian:bookworm-slim

# Install runtime deps + gosu + current opencode npm package
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git gosu nodejs npm ripgrep fzf && \
    npm install -g opencode-ai@latest && \
    opencode --version && \
    rm -rf /var/lib/apt/lists/* /root/.npm

# Copy wukong binaries from builder stage
COPY --from=builder /src/target/release/wukong /usr/local/bin/wukong
COPY --from=builder /src/target/release/wukong-telegram /usr/local/bin/wukong-telegram
COPY --from=builder /src/target/release/wukong-web /usr/local/bin/wukong-web

# Create non-root user (UID/GID will be remapped at runtime via entrypoint)
RUN useradd -m -s /bin/bash wukong

# Copy default workspace templates (SOUL.md, AGENTS.md)
RUN mkdir -p /usr/local/share/wukong
COPY workspace/SOUL.md workspace/AGENTS.md /usr/local/share/wukong/

# Prepare directories and entrypoint
COPY scripts/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# Default environment
ENV WUKONG_WORKSPACE=/workspace
ENV HOME=/home/wukong
ENV PATH="/home/wukong/.local/bin:/usr/local/bin:${PATH}"

WORKDIR /workspace
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
