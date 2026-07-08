# syntax=docker/dockerfile:1
# ── Build stage: download release binaries ──
# VERSION is the wukong release tag to pull binaries from. The release workflow
# rewrites this default for the packaged docker bundle; for a local `docker
# build` pass --build-arg VERSION=vX.Y.Z to select a specific release.
ARG VERSION=v0.17.1
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

# OPENCODE_VERSION pins the opencode CLI for reproducible builds. Defaults to
# `latest`; pass --build-arg OPENCODE_VERSION=X.Y.Z to lock a known-good release.
ARG OPENCODE_VERSION=latest

# Install runtime deps + gosu + opencode npm package (pinned via OPENCODE_VERSION)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git gh gosu nodejs npm python3 python3-pip pipx ripgrep fzf && \
    npm install -g "opencode-ai@${OPENCODE_VERSION}" && \
    opencode --version && \
    rm -rf /var/lib/apt/lists/* /root/.npm

# Copy wukong binaries from downloader stage
COPY --from=downloader /bins/wukong /usr/local/bin/wukong
COPY --from=downloader /bins/wukong-telegram /usr/local/bin/wukong-telegram
COPY --from=downloader /bins/wukong-web /usr/local/bin/wukong-web
COPY --from=downloader /bins/wukong-schedulerd /usr/local/bin/wukong-schedulerd

# Create non-root user (UID/GID will be remapped at runtime via entrypoint)
RUN useradd -m -s /bin/bash wukong

ENV HOME=/home/wukong
ENV PIPX_HOME=/home/wukong/.local/pipx
ENV PIPX_BIN_DIR=/home/wukong/.local/bin
ENV PATH="/home/wukong/.local/bin:/usr/local/bin:${PATH}"

# Preinstall Agent Reach CLI only; user-specific channel setup runs interactively.
RUN mkdir -p "$PIPX_HOME" "$PIPX_BIN_DIR" && \
    chown -R wukong:wukong /home/wukong/.local && \
    gosu wukong pipx install https://github.com/Panniantong/agent-reach/archive/main.zip && \
    gosu wukong agent-reach --help >/dev/null

# Copy default workspace templates (SOUL.md, AGENTS.md)
RUN mkdir -p /usr/local/share/wukong
COPY workspace/SOUL.md workspace/AGENTS.md /usr/local/share/wukong/

# Copy runtime-readable Superpowers skill assets from the canonical source tree.
COPY crates/wukong-skills/assets/superpowers /usr/local/share/wukong/skills/superpowers

# Prepare directories and entrypoint
COPY scripts/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# Default environment
ENV WUKONG_WORKSPACE=/workspace

WORKDIR /workspace
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
