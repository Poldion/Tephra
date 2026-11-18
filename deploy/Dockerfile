# syntax=docker/dockerfile:1
#
# Sync server of Tephra.
# Build from the repository root:
#   docker build -f deploy/Dockerfile -t tephra-server .

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p tephra-server \
    && cp target/release/tephra-server /usr/local/bin/tephra-server

FROM debian:bookworm-slim
ARG VERSION=dev
LABEL org.opencontainers.image.title="Tephra sync server" \
      org.opencontainers.image.description="Stores the encrypted vault records of Tephra and keeps devices in sync" \
      org.opencontainers.image.source="https://github.com/Poldion/Tephra" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="${VERSION}"

# Unprivileged user; the database lives in /data.
RUN useradd --system --uid 10001 --user-group --home-dir /data --shell /usr/sbin/nologin tephra \
    && install -d -o tephra -g tephra -m 0700 /data

COPY --from=build /usr/local/bin/tephra-server /usr/local/bin/tephra-server
COPY LICENSE THIRD_PARTY_NOTICES.md /usr/share/doc/tephra-server/

USER tephra
WORKDIR /data
ENV TEPHRA_SERVER_DB=/data/tephra-server.sqlite \
    TEPHRA_SERVER_BIND=0.0.0.0:8787
VOLUME /data
EXPOSE 8787

ENTRYPOINT ["tephra-server"]
CMD ["serve"]
