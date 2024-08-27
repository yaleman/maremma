FROM debian:latest AS tools

# ARG GITHUB_SHA="${GITHUB_SHA}"

# LABEL com.maremma.git-commit="${GITHUB_SHA}"

# fixing the issue with getting OOMKilled in BuildKit
# ENV CARGO_NET_GIT_FETCH_WITH_CLI=true

# install the dependencies
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    sccache \
    curl \
    git \
    jq \
    clang \
    build-essential \
    pkg-config \
    libssl-dev \
    procps \
    mold

# install rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN mv /root/.cargo/bin/* /usr/local/bin/

FROM tools AS builder

RUN mkdir /maremma
COPY . /maremma/

WORKDIR /maremma

# # do the build bits
RUN --mount=type=cache,id=cargo,target=/cargo \
    --mount=type=cache,id=sccache,target=/sccache \
    export CARGO_HOME=/cargo && \
    export SCCACHE_DIR=/sccache && \
    export RUSTC_WRAPPER=/usr/bin/sccache && \
    export CC="/usr/bin/clang" && \
    cargo build --release --bin maremma
RUN chmod +x /maremma/target/release/maremma

RUN cd /maremma && ./scripts/build_plugins.sh && cd plugins/monitoring-plugins && make install

# https://github.com/GoogleContainerTools/distroless/blob/main/examples/rust/Dockerfile
FROM gcr.io/distroless/cc-debian12 AS maremma

COPY --from=builder /maremma/target/release/maremma /maremma
COPY --from=builder /maremma/plugins/libexec/* /usr/local/bin/
COPY ./static /static/
USER nonroot
ENTRYPOINT ["/maremma"]
CMD [ "run" ]