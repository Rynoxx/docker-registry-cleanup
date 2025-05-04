FROM rust:1.86.0-bullseye AS build

WORKDIR /app

COPY Cargo.toml Cargo.lock .
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --locked

COPY src src
RUN touch src/main.rs
RUN cargo build --release --locked

FROM debian:stable-20250428-slim AS runtime

WORKDIR /app

COPY --from=build /app/target/release/docker-registry-cleanup .

CMD ["./docker-registry-cleanup"]

