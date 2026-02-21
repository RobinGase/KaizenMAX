FROM rust:1.86-slim AS builder

WORKDIR /app

COPY core/Cargo.toml core/Cargo.lock ./
COPY core/src ./src

RUN cargo build --release --bin kaizen-vaultd

FROM debian:bookworm-slim

RUN useradd --create-home --uid 10001 vaultd

WORKDIR /app

COPY --from=builder /app/target/release/kaizen-vaultd /usr/local/bin/kaizen-vaultd

RUN mkdir -p /data && chown -R vaultd:vaultd /data

USER vaultd

ENV KAIZEN_VAULTD_HOST=0.0.0.0 \
    KAIZEN_VAULTD_PORT=9210 \
    KAIZEN_VAULT_PATH=/data/vault.json \
    KAIZEN_VAULT_KEY_PATH=/data/vault.key

EXPOSE 9210

ENTRYPOINT ["/usr/local/bin/kaizen-vaultd"]
