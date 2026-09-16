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

FROM debian:13.6-slim
WORKDIR /app

# `apt-get upgrade -y` pulls in the Debian security-repo fixes
# published after the base-image tag was cut. Without this line,
# the runtime layer inherits whatever CVE-vulnerable stdlib
# packages (perl-base, gzip, libpcre2, libsqlite3, ...) shipped
# in the base and the Trivy scan gate fails the publish workflow
# with HIGH/CRITICAL findings against un-fixed OS packages.
RUN apt-get update && apt-get upgrade -y && apt-get install -y --no-install-recommends \
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
