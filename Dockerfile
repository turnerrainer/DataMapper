# Multi-stage build for DataMapper.
# See STANDARDS.md §7 for the security posture rationale.

FROM rust:1.88-slim AS builder
WORKDIR /build

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --bin datamapper

FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl tini \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/datamapper /app/datamapper
# Ship the demo self-contained: config + sample DSL tree. Operators
# bind-mount over either to override (see docker-compose.yml).
COPY datamapper.yaml /app/datamapper.yaml
COPY DSL /app/DSL

EXPOSE 3000
RUN useradd -m -u 1000 datamapper && chown -R datamapper:datamapper /app
USER datamapper

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/app/datamapper"]
