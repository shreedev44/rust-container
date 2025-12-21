FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

# ---- System deps ----
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    build-essential \
    pkg-config \
    libssl-dev \
 && rm -rf /var/lib/apt/lists/*

# ---- Create non-root user ----
RUN useradd -m -u 10001 executor

USER executor
WORKDIR /home/executor

# ---- Install Rust AS executor ----
RUN curl https://sh.rustup.rs -sSf | sh -s -- \
    -y \
    --profile minimal \
    --default-toolchain stable

ENV PATH="/home/executor/.cargo/bin:${PATH}"

# ---- App ----
WORKDIR /app
COPY target/x86_64-unknown-linux-musl/release/container_agent /app/executor

EXPOSE 8000
ENTRYPOINT ["/app/executor"]
