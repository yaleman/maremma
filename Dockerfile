FROM debian:latest as plugin_builder

RUN apt-get update && apt-get install -y \
    curl \
    jq \
    build-essential \
    pkg-config \
    procps \
    snmp

RUN mkdir /maremma/
COPY . /maremma/
WORKDIR /maremma
RUN ./scripts/build_plugins.sh
RUN cd plugins/monitoring-plugins && make install

# MIBS path usr/share/snmp/mibs/

FROM debian:latest AS cargo_builder

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

RUN ./scripts/copy_linker_config.sh

# ENV SCCACHE_DIR="/sccache"
# ENV RUSTC_WRAPPER="/usr/bin/sccache"
ENV CC="/usr/bin/clang"
# # do the build bits
RUN cargo build --release --bins
RUN chmod +x /maremma/target/release/maremma

# https://github.com/GoogleContainerTools/distroless/blob/main/examples/rust/Dockerfile
FROM debian:latest AS maremma
# FROM gcr.io/distroless/cc-debian12:debug AS maremma # so you can run --entrypoint=sh

RUN apt-get update && apt-get install -y \
    snmp snmpd libsnmp-base \
    && rm -rf /var/lib/apt/ /var/cache/apt/

COPY --from=cargo_builder /maremma/target/release/maremma /maremma
COPY --from=cargo_builder /maremma/target/release/check_splunk /usr/local/bin/
COPY --from=plugin_builder /maremma/plugins/libexec/* /usr/local/bin/
COPY ./static /static/
# USER nonroot
ENTRYPOINT ["/maremma"]
CMD [ "run" ]