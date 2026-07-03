FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev git
WORKDIR /app
COPY . .
RUN cargo build --release --locked -p spectra-cli

FROM alpine:3

RUN apk add --no-cache git ca-certificates
COPY --from=builder /app/target/release/spectra /usr/local/bin/spectra
ENTRYPOINT ["spectra"]
