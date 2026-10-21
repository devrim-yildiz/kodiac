# Build on the target Pi or with buildx --platform linux/arm64.
FROM node:22-bookworm-slim AS frontend
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web ./
RUN npm run build

FROM rust:1.99.1-bookworm AS build
WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY --from=frontend /web/dist ./web/dist
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home --shell /usr/sbin/nologin media \
    && install -d -o media -g media /data

COPY --from=build /build/target/release/kodiac /usr/local/bin/kodiac
USER 10001:10001
WORKDIR /data
ENV METADATA_PATH=/data/metadata
EXPOSE 8096
ENTRYPOINT ["kodiac"]
CMD ["--media-path", "/media", "--db-path", "/data/media.db", "--bind", "0.0.0.0"]
