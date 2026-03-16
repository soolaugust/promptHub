# Build with entrypoint-based seeding
FROM rust:1.85 as builder
WORKDIR /build
COPY . .
RUN cargo build --release -p ph-registry

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/ph-registry /usr/local/bin/ph-registry
COPY registry/registry.demo.yaml /etc/prompthub/registry.yaml
COPY registry/entrypoint.sh /entrypoint.sh
COPY prompthub/layers /layers
RUN chmod +x /entrypoint.sh && mkdir -p /data
EXPOSE 8080
ENTRYPOINT ["/entrypoint.sh"]
