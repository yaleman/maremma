FROM debian:latest AS builder

# fixing the issue with getting OOMKilled in BuildKit
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true

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

RUN mkdir /maremma
COPY . /maremma/

WORKDIR /maremma

# ENV CARGO_HOME="/cargo"
# ENV SCCACHE_DIR="/sccache"
# ENV RUSTC_WRAPPER="/usr/bin/sccache"
# ENV CC="/usr/bin/clang"
# # do the build bits
RUN cargo build --release --bins
RUN chmod +x /maremma/target/release/maremma

RUN cd /maremma && ./scripts/build_plugins.sh && cd plugins/monitoring-plugins && make install

# https://github.com/GoogleContainerTools/distroless/blob/main/examples/rust/Dockerfile
FROM gcr.io/distroless/cc-debian12 AS maremma
# FROM gcr.io/distroless/cc-debian12:debug AS maremma # so you can run --entrypoint=sh

COPY --from=builder /maremma/target/release/maremma /maremma
COPY --from=builder /maremma/target/release/check_splunk /usr/local/bin/
COPY --from=builder /maremma/plugins/libexec/* /usr/local/bin/
COPY ./static /static/
USER nonroot
ENTRYPOINT ["/maremma"]
CMD [ "run" ]