# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    git ca-certificates pkg-config \
    && rm -rf /var/lib/apt/lists/*

ARG NOVOID_CORE_REPO=https://github.com/hanovoid/novoid-core.git
ARG NOVOID_CORE_REF=main
ARG NOVOID_CORE_COMMIT=3007eb43ba6a1973176c68cc3d13ee008c89c58c

# 1. Build Novoid Core (upstream proxy & embedded WebUI)
WORKDIR /build/novoid-core
RUN echo "Building novoid-core ${NOVOID_CORE_COMMIT}" \
    && git clone --depth 1 --branch "${NOVOID_CORE_REF}" "${NOVOID_CORE_REPO}" . \
    && cargo build --release --bin novoid-core --bin acc

# 2. Build Novoid Proxy (2-tier model & tier router)
WORKDIR /build/novoid-proxy
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --bin novoid-proxy

# 3. Minimal runtime image
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl bash \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/novoid-core/target/release/novoid-core /usr/local/bin/novoid-core
COPY --from=builder /build/novoid-core/target/release/acc /usr/local/bin/acc
COPY --from=builder /build/novoid-proxy/target/release/novoid-proxy /usr/local/bin/novoid-proxy

WORKDIR /app
COPY start.sh ./start.sh
RUN chmod +x /app/start.sh

EXPOSE 8080
CMD ["/app/start.sh"]
