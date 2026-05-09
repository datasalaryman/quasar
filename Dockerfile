FROM rust:slim-trixie

# Accept TARGETARCH for multi-platform builds (Docker BuildKit).
ARG TARGETARCH

# Install minimal dependencies needed to build from source.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
    && rm -rf /var/lib/apt/lists/*

# Copy the workspace source into the image.
COPY . /usr/src/quasar

# Build and install the Quasar CLI from source, then clean up build artifacts
# to keep the final image small.
WORKDIR /usr/src/quasar
RUN cargo install --path cli && \
    rm -rf /usr/src/quasar \
           /usr/local/cargo/registry/cache \
           /usr/local/cargo/git/checkouts

# Create an empty working directory editable by any user.
RUN mkdir -p /workspace && chmod 777 /workspace
WORKDIR /workspace

# Programmatically identify the architecture and make it available inside
# running containers via the ARCH environment variable. If built with Docker
# BuildKit, TARGETARCH is used; otherwise we fall back to dpkg.
ARG TARGETARCH
ENV ARCH=${TARGETARCH}
RUN ARCH="${TARGETARCH:-$(dpkg --print-architecture)}" && \
    echo "export ARCH=$ARCH" > /etc/profile.d/arch.sh && \
    echo "$ARCH" > /etc/arch.txt && \
    chmod 644 /etc/profile.d/arch.sh /etc/arch.txt

# Keep the container running in the background so you can exec into it.
CMD ["tail", "-f", "/dev/null"]
