ARG RUST_VERSION=1.83
FROM library/rust:${RUST_VERSION}-slim-bookworm
RUN apt-get update && apt-get install -y libssl-dev pkg-config
COPY . /app
WORKDIR /app
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y tini libssl-dev pkg-config
COPY --from=0 /app/target/release/tagrs /usr/local/bin/tagrs
ENTRYPOINT ["/usr/bin/tini", "-w", "/usr/local/bin/tagrs", "--"]
