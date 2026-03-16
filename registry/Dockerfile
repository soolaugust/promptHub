# rust:1.80 includes the full toolchain needed by rusqlite bundled (gcc, etc.)
FROM rust:1.85 as builder
WORKDIR /build
COPY . .
RUN cargo build --release -p ph-registry

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/ph-registry /usr/local/bin/ph-registry
RUN mkdir -p /data /etc/prompthub
EXPOSE 8080
ENTRYPOINT ["ph-registry", "/etc/prompthub/registry.yaml"]
