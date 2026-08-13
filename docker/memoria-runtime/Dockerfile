# syntax=docker/dockerfile:1
# Memoria runtime, published into a volume that Wukong's agent containers mount.
#
# Why a separate image rather than `npm i -g` inside Dockerfile.release: Memoria
# and Wukong release on different cadences. Baking it in would mean cutting a
# Wukong release for every Memoria bump. This image instead copies a
# self-contained runtime into a shared volume, so upgrading is one line in .env
# (MEMORIA_VERSION) plus `docker compose up -d memoria-runtime`.
#
# ⚠ NODE_MAJOR MUST match the Node major in the Wukong image (Debian bookworm
# ships 18.x → ABI 108). better-sqlite3 uses ABI-specific prebuilds, not N-API,
# so a mismatch makes every `memoria` call die with NODE_MODULE_VERSION. That
# failure is loud but only appears at runtime, in the agent's shell, where
# nobody is watching — `scripts/test-memoria-runtime.sh` asserts the pairing
# end-to-end. Do not bump this without running it.
ARG NODE_MAJOR=18
FROM node:${NODE_MAJOR}-bookworm-slim

ARG MEMORIA_VERSION=1.25.0
ARG MEMORIA_PKG=@raybird.chen/memoria

LABEL org.opencontainers.image.title="wukong-memoria-runtime" \
      org.opencontainers.image.description="Memoria CLI + semantic-recall helper, published into a shared volume for Wukong agent containers" \
      org.opencontainers.image.version="$MEMORIA_VERSION"

# rsync: the publish step needs --delete so an upgrade cannot leave a previous
# version's files behind in the volume.
RUN apt-get update && apt-get install -y --no-install-recommends rsync \
    && rm -rf /var/lib/apt/lists/*

ENV MEMORIA_PREFIX=/opt/memoria
RUN npm install -g --prefix "$MEMORIA_PREFIX" "${MEMORIA_PKG}@${MEMORIA_VERSION}"

ENV MEMORIA_PKG_DIR=/opt/memoria/lib/node_modules/@raybird.chen/memoria
ENV MEMORIA_VECTOR_DIR=/opt/memoria/lib/node_modules/@raybird.chen/memoria/skills/memoria-vector

# The semantic-recall helper is deliberately outside Memoria's core dependencies
# (it is spawned, never imported), so npm ships its source but not its ~850 MB
# of deps. Install them here — including devDependencies, because
# @huggingface/transformers is listed as a devDependency yet is required at
# runtime whenever MEMORIA_EMBED_PROVIDER=local.
RUN cd "$MEMORIA_VECTOR_DIR" && npm install --include=dev --no-audit --no-fund

# Bake the embedding model into the image. transformers.js caches downloads
# under node_modules/@huggingface/transformers/.cache, and the consumer mounts
# this runtime read-only — so a model that is not here at build time cannot be
# fetched later, and the first vector query would fail. Pre-fetching also keeps
# the image reproducible and the first recall fast.
RUN cd "$MEMORIA_VECTOR_DIR" \
    && MEMORIA_EMBED_PROVIDER=local node -e "\
        import('./embed.mjs').then(async (m) => { \
          const v = await m.embedTexts(['warm the model cache'], 'passage'); \
          if (!Array.isArray(v[0]) || v[0].length !== m.DIM) { \
            throw new Error('unexpected embedding shape'); \
          } \
          console.log('model cached, dim=' + v[0].length); \
        }).catch((e) => { console.error(e); process.exit(1); })"

# onnxruntime ships prebuilt binaries for every platform it supports; only the
# linux one can ever run here. Dropping the rest is ~200 MB off the image and
# cannot affect behaviour.
RUN find /opt/memoria -type d \( -path '*onnxruntime*/bin/napi-v3/darwin*' \
      -o -path '*onnxruntime*/bin/napi-v3/win32*' \) -prune -exec rm -rf {} + 2>/dev/null || true

COPY publish.sh /usr/local/bin/publish.sh
# Staged through /tmp on purpose: `npm install -g` leaves /opt/memoria/bin/memoria
# as a symlink to dist/cli.mjs, and COPY follows an existing symlink and writes
# through it. Copying the wrapper straight onto that path replaces the CLI
# itself, and the wrapper then execs a symlink pointing back at the wrapper —
# an infinite respawn that looks like a hang, not an error.
COPY memoria-wrapper.sh memoria-vector-sync.sh /tmp/memoria-bin/
RUN rm -f /opt/memoria/bin/memoria && \
    ln -s ../lib/node_modules/@raybird.chen/memoria/dist/cli.mjs /opt/memoria/bin/memoria-real && \
    install -m 0755 /tmp/memoria-bin/memoria-wrapper.sh /opt/memoria/bin/memoria && \
    install -m 0755 /tmp/memoria-bin/memoria-vector-sync.sh /opt/memoria/bin/memoria-vector-sync && \
    rm -rf /tmp/memoria-bin && \
    chmod +x /usr/local/bin/publish.sh && \
    # The wrapper must not be reachable from its own exec target.
    [ "$(readlink /opt/memoria/bin/memoria-real)" = "../lib/node_modules/@raybird.chen/memoria/dist/cli.mjs" ] && \
    head -1 /opt/memoria/lib/node_modules/@raybird.chen/memoria/dist/cli.mjs | grep -q node

# Consumers mount this runtime as a non-root user, so everything must be
# world-readable and the binaries world-executable.
RUN chmod -R a+rX /opt/memoria

RUN printf '%s\n' "$MEMORIA_VERSION" > /opt/memoria/VERSION

ENTRYPOINT ["/usr/local/bin/publish.sh"]
