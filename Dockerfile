# syntax=docker/dockerfile:1
# ── Build stage: download release binaries ──
ARG VERSION=v0.14.1
ARG TARGET=x86_64-unknown-linux-musl
ARG REPO=raybird/Wukong
FROM debian:bookworm-slim AS downloader

ARG VERSION
ARG TARGET
ARG REPO

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl tar && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /bins

RUN set -eux; \
    base_url="https://github.com/${REPO}/releases/download/${VERSION}"; \
    for bin in wukong wukong-telegram wukong-web wukong-schedulerd; do \
      tarball="${bin}-${TARGET}.tar.gz"; \
      curl -fsSL "${base_url}/${tarball}" -o "/tmp/${tarball}"; \
      tar -xzf "/tmp/${tarball}" -C /bins "${bin}"; \
      chmod +x "/bins/${bin}"; \
      rm -f "/tmp/${tarball}"; \
    done

# ── Runtime stage ──
FROM debian:bookworm-slim

# Install runtime deps + gosu + current opencode npm package
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git gosu nodejs npm ripgrep fzf && \
    npm install -g opencode-ai@latest && \
    opencode --version && \
    rm -rf /var/lib/apt/lists/* /root/.npm

# Copy wukong binaries from downloader stage
COPY --from=downloader /bins/wukong /usr/local/bin/wukong
COPY --from=downloader /bins/wukong-telegram /usr/local/bin/wukong-telegram
COPY --from=downloader /bins/wukong-web /usr/local/bin/wukong-web
COPY --from=downloader /bins/wukong-schedulerd /usr/local/bin/wukong-schedulerd

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
