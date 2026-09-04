# 1. Frontend bundle (skip with --build-arg by using the headless target below)
FROM node:22-alpine AS web
WORKDIR /web
COPY web/package.json web/package-lock.json* ./
RUN npm ci
COPY web/ ./
RUN npm run build

# 2. Rust binary, with the bundle embedded
FROM rust:1-slim AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY schema.sql ./
COPY --from=web /web/dist ./web/dist
RUN cargo build --release --locked

# 3. Runtime
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
HEALTHCHECK --interval=30s --timeout=3s CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1
CMD ["openpaste", "serve"]
