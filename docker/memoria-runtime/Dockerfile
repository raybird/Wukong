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
ENV MEMORIA_PKG_DIR=/opt/memoria/lib/node_modules/@raybird.chen/memoria
ENV MEMORIA_VECTOR_DIR=/opt/memoria/lib/node_modules/@raybird.chen/memoria/skills/memoria-vector

# Everything that writes under /opt/memoria happens in ONE layer, on purpose.
# A layer records the files as they stand when it ends, so a later `rm` cannot
# shrink the image — the deleted bytes stay in the earlier layer — and a later
# `chmod -R` is worse still: changing metadata on every file copies the whole
# tree into a new layer. Measured on the split version of this file with
# `docker history`: the delete layer was 6.78 kB (it reclaimed nothing, while
# its comment claimed ~200 MB) and the chmod layer was 899 MB, a duplicate of
# the 889 MB install above it. Folding both in here is worth roughly a gigabyte.
#
# Cache granularity is the cost, and it is the right thing to trade: this image
# is rebuilt only when MEMORIA_VERSION moves.
#
# The steps, in order:
#   1. Memoria itself.
#   2. The semantic-recall helper's dependencies. They are deliberately outside
#      Memoria's core deps (the helper is spawned, never imported), so npm ships
#      its source but not its ~850 MB of runtime. `--include=dev` is required:
#      @huggingface/transformers is declared a devDependency yet is needed at
#      runtime whenever MEMORIA_EMBED_PROVIDER=local.
#   3. Bake the embedding model. transformers.js caches under
#      node_modules/@huggingface/transformers/.cache and the consumer mounts
#      this runtime read-only, so a model absent at build time can never be
#      fetched later — the first vector query would simply fail.
#   4. Drop onnxruntime prebuilds for platforms that cannot run here.
#   5. Make it all world-readable: consumers mount it as an unprivileged user.
RUN npm install -g --prefix "$MEMORIA_PREFIX" "${MEMORIA_PKG}@${MEMORIA_VERSION}" \
 && cd "$MEMORIA_VECTOR_DIR" \
 && npm install --include=dev --no-audit --no-fund \
 && MEMORIA_EMBED_PROVIDER=local node -e "\
        import('./embed.mjs').then(async (m) => { \
          const v = await m.embedTexts(['warm the model cache'], 'passage'); \
          if (!Array.isArray(v[0]) || v[0].length !== m.DIM) { \
            throw new Error('unexpected embedding shape'); \
          } \
          console.log('model cached, dim=' + v[0].length); \
        }).catch((e) => { console.error(e); process.exit(1); })" \
 && find /opt/memoria -type d \( -path '*onnxruntime*/bin/napi-v3/darwin*' \
        -o -path '*onnxruntime*/bin/napi-v3/win32*' \) -prune -exec rm -rf {} + \
 && chmod -R a+rX /opt/memoria

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
    head -1 /opt/memoria/lib/node_modules/@raybird.chen/memoria/dist/cli.mjs | grep -q node && \
    printf '%s\n' "$MEMORIA_VERSION" > /opt/memoria/VERSION
# No recursive chmod here: this layer only touched a handful of small files, and
# `install -m 0755` already set their modes. A `chmod -R` over /opt/memoria would
# re-copy the entire ~900 MB tree into this layer for nothing.

ENTRYPOINT ["/usr/local/bin/publish.sh"]
