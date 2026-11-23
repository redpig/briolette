# Briolette - Multi-stage Docker build
#
# Builds all briolette services into a single image.
# Individual services are run via the entrypoint script or docker-compose.
#
# Usage:
#   docker build -t briolette .
#   docker compose up

FROM rust:1.77-bookworm AS builder

WORKDIR /build

# Install system dependencies for ECDAA/crypto
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    libprotobuf-dev \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

# Build all workspace binaries in release mode
RUN cargo build --release --workspace \
    --exclude briolette-sim \
    --exclude absim \
    --exclude levy_distr \
    --exclude rand_flight

# Runtime image
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /briolette

# Copy all service binaries
COPY --from=builder /build/target/release/briolette-registrar-server /usr/local/bin/
COPY --from=builder /build/target/release/briolette-registrar-client /usr/local/bin/
COPY --from=builder /build/target/release/briolette-clerk-server /usr/local/bin/
COPY --from=builder /build/target/release/briolette-clerk-client /usr/local/bin/
COPY --from=builder /build/target/release/briolette-clerk-generate-epoch /usr/local/bin/
COPY --from=builder /build/target/release/briolette-tokenmap-server /usr/local/bin/
COPY --from=builder /build/target/release/briolette-mint-server /usr/local/bin/
COPY --from=builder /build/target/release/briolette-mint-client /usr/local/bin/
COPY --from=builder /build/target/release/briolette-validate-server /usr/local/bin/
COPY --from=builder /build/target/release/briolette-receiver-server /usr/local/bin/
COPY --from=builder /build/target/release/briolette-receiver-client /usr/local/bin/
COPY --from=builder /build/target/release/briolette-bridge-server /usr/local/bin/
# Wallet CLI (when built)
COPY --from=builder /build/target/release/briolette-wallet-cli /usr/local/bin/ 2>/dev/null || true

# Copy the entrypoint and bootstrap scripts
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# Create data directories
RUN mkdir -p /briolette/data/registrar \
             /briolette/data/clerk \
             /briolette/data/mint \
             /briolette/data/tokenmap \
             /briolette/data/validate \
             /briolette/data/receiver \
             /briolette/data/bridge \
             /briolette/data/wallet

ENTRYPOINT ["entrypoint.sh"]
