# syntax=docker/dockerfile:1.7
# umap_rust_operator — cc tier (create-rust-operator §5): umaprs links OpenBLAS statically
# (ndarray-linalg, for the spectral start), which needs a Fortran toolchain in the builder and
# glibc at runtime, so the runtime is distroless/cc rather than scratch.

# ---- builder ----
FROM rust:1.94-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
        protobuf-compiler pkg-config git ca-certificates gfortran make \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /build
RUN cargo install cargo-chef --locked

# Dependency layer. cargo-chef's recipe carries the package version, and a release commit always
# bumps it, so the version is normalised for the recipe only and the real manifest restored.
COPY Cargo.toml Cargo.lock ./
RUN cp Cargo.toml Cargo.toml.keep \
 && sed -i 's/^version = ".*"/version = "0.0.0"/' Cargo.toml \
 && cargo chef prepare --recipe-path recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
RUN mv Cargo.toml.keep Cargo.toml

COPY src ./src
COPY operator.json ./
RUN cargo build --release --bin umap_operator \
 && ls -l target/release/umap_operator
RUN mkdir -p /tmp-op && chown 1000:1000 /tmp-op

# ---- runtime ----
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /build/target/release/umap_operator /usr/local/bin/umap_operator
COPY --from=builder --chown=1000:1000 /tmp-op /tmp
USER 1000:1000
WORKDIR /operator
ENV RUST_BACKTRACE=1 RUST_LOG=info TMPDIR=/tmp
ENTRYPOINT ["/usr/local/bin/umap_operator"]
