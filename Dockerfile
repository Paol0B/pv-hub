# ---- builder: static musl binary (Alpine's default target) ----
FROM rust:1-alpine AS builder
RUN apk add --no-cache build-base
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets
RUN cargo build --release

# ---- runtime: tiny, static, non-root ----
FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=builder /app/target/release/pv-hub /pv-hub
EXPOSE 8080 502
USER nonroot
ENTRYPOINT ["/pv-hub"]
