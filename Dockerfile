FROM ekidd/rust-musl-builder:latest AS builder
COPY --chown=rust:rust . .
RUN cargo build --release

FROM alpine:latest
RUN apk --no-cache --update upgrade
COPY --from=builder /home/rust/src/target/x86_64-unknown-linux-musl/release/colibri-proxy /usr/local/bin/
ENTRYPOINT ["/usr/local/bin/colibri-proxy"]
