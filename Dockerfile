# syntax=docker/dockerfile:1
# ── Build stage: download release binaries ──
ARG VERSION=v0.13.1
ARG TARGET=x86_64-unknown-linux-musl
ARG REPO=raybird/Wukong
FROM debian:bookworm-slim AS downloader

RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /bins

RUN for bin in wukong wukong-telegram wukong-web; do \
      tarball="${bin}-${TARGET}.tar.gz"; \
      curl -fsSL "https://github.com/${REPO}/releases/download/${VERSION}/${tarball}" -o "/tmp/${tarball}"; \
      tar -xzf "/tmp/${tarball}" -C /bins "${bin}"; \
      chmod +x "/bins/${bin}"; \
      rm -f "/tmp/${tarball}"; \
    done

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

# Copy wukong binaries from downloader stage
COPY --from=downloader /bins/* /usr/local/bin/

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
