FROM nvidia/cuda:12.4.1-runtime-ubuntu22.04

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY target/release/converge-llm-server /usr/local/bin/converge-llm-server
COPY deploy/gpu/runpod/start-worker.sh /usr/local/bin/start-worker.sh

RUN chmod +x /usr/local/bin/start-worker.sh

EXPOSE 50051

CMD ["/usr/local/bin/start-worker.sh"]
