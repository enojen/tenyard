# procs: one static binary on scratch. `docker build --target check procs` runs fmt, clippy and
# the tests; the last stage is the image the box pods run as their `processes` sidecar.
FROM rust:1.99.1-alpine3.22 AS base
RUN rustup component add rustfmt clippy
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY groups.conf ./
COPY src src

FROM base AS check
RUN cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked

FROM base AS build
# Alpine's Rust links musl statically: the binary needs no libc in the image.
RUN cargo build --release --locked

FROM scratch
COPY --from=build /src/target/release/procs /procs
COPY groups.conf /config/groups.conf
USER 65534:65534
ENTRYPOINT ["/procs"]
