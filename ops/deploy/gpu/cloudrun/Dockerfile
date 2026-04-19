FROM nvidia/cuda:12.4.1-devel-ubuntu22.04 AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates build-essential pkg-config git \
    && rm -rf /var/lib/apt/lists/*

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY examples ./examples

RUN cargo build -p converge-llm --bin converge-llm-server --release --no-default-features --features "server,cuda,llama3,import"

FROM nvidia/cuda:12.4.1-runtime-ubuntu22.04

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/converge-llm-server /usr/local/bin/converge-llm-server

ENV BIND_ADDR=0.0.0.0:50051
ENV MAX_SEQ_LEN=4096
ENV MODEL_VARIANT=llama3-8b

EXPOSE 50051

CMD ["converge-llm-server"]
