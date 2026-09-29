# Rust build toolchain — image role: ci_toolchain. NOT a product image.
#
# Disposition (#8980): retired from product publication. This image used to be
# published under the unsuffixed `perl-lsp:<version>` tags on GHCR and Docker
# Hub, so the obvious product reference resolved to a build toolchain that
# ships no `perllsp`. It no longer publishes anywhere.
#
# The end-user runtime is .docker/perl-lsp/Dockerfile, published as
# `effortlessmetrics/perl-lsp:<version>-perl` and `ghcr.io/<repo>-perl`.
#
# The one remaining consumer is the `trivy-docker-scan` job in
# .github/workflows/ci-security.yml, which builds this file locally and scans
# it as `perl-lsp-ci:<sha>`. It is not a reusable or distributed image: it
# bakes in a copy of the repository source and builds as root. Do not give it
# a product identity, and do not depend on it as a prebuilt base.

FROM rust:1.95-slim-bookworm

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    libssl-dev \
    git \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /workspace

# Copy dependency files first for better caching
COPY Cargo.toml Cargo.lock ./
COPY .cargo/ ./.cargo/
COPY crates/*/Cargo.toml ./crates/*/

# Create dummy main files to allow dependency compilation
RUN find . -name "Cargo.toml" -path "./crates/*" -exec dirname {} \; | \
    xargs -I {} sh -c 'mkdir -p {}/src && echo "fn main() {}" > {}/src/main.rs && echo "" > {}/src/lib.rs'

# Build dependencies (this layer will be cached)
RUN cargo build --release || true

# Copy the actual source code
COPY . .

# Set default concurrency environment
ENV RUST_TEST_THREADS=2
ENV OMP_NUM_THREADS=1
ENV OPENBLAS_NUM_THREADS=1
ENV MKL_NUM_THREADS=1
ENV NUMEXPR_NUM_THREADS=1

# Build the project
RUN cargo build --release

RUN useradd --create-home --uid 10001 appuser
USER appuser
