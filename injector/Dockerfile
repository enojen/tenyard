FROM rust:1.99.1-alpine3.22 AS base
RUN rustup component add rustfmt clippy
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src

FROM base AS check
RUN cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked

FROM base AS build
RUN cargo build --release --locked

FROM scratch
COPY --from=build /src/target/release/injector /injector
USER 65534:65534
ENTRYPOINT ["/injector"]
