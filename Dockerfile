# 1. Rust binary, with the HTML assets embedded
FROM rust:1-slim AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets
COPY schema.sql ./
RUN cargo build --release --locked

# 2. Runtime
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd -r -u 10001 -d /var/lib/openpaste -m openpaste
COPY --from=build /src/target/release/openpaste /usr/local/bin/openpaste
USER openpaste
WORKDIR /var/lib/openpaste
ENV BIND=0.0.0.0:8080 \
    BASE_URL=http://localhost:8080 \
    DATABASE_URL="sqlite:///var/lib/openpaste/openpaste.db?mode=rwc" \
    STORAGE_DRIVER=local \
    STORAGE_PATH=/var/lib/openpaste/blobs
EXPOSE 8080
# Un solo healthcheck para ambos modos: en HTTPS el primer curl falla y gana el segundo.
HEALTHCHECK --interval=30s --timeout=3s \
    CMD curl -fsS http://127.0.0.1:8080/healthz || curl -fsSk https://127.0.0.1:8080/healthz || exit 1
CMD ["openpaste", "serve"]
