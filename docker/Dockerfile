FROM rust:1.94-bookworm AS builder

WORKDIR /app
ARG CONVERGE_RUNTIME_FEATURES=gcp,auth,firebase

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY examples ./examples
COPY schema ./schema

RUN cargo build -p converge-runtime --release --features "${CONVERGE_RUNTIME_FEATURES}"

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/converge-runtime /usr/local/bin/converge-runtime

ENV RUST_LOG=info
ENV PORT=8080
ENV LOCAL_DEV=true
ENV GCP_PROJECT_ID=local-project
ENV GOOGLE_CLOUD_PROJECT=local-project
ENV FIREBASE_PROJECT_ID=local-project

EXPOSE 8080

HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=12 \
  CMD curl -fsS http://127.0.0.1:8080/health || exit 1

CMD ["converge-runtime"]
