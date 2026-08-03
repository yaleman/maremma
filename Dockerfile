FROM debian:12 AS plugin_builder

RUN apt-get update && apt-get upgrade -y && apt-get install -y \
    curl \
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

FROM debian:12 AS cargo_builder

# fixing the issue with getting OOMKilled in BuildKit
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true

# install the dependencies
RUN apt-get update && apt-get upgrade -y && apt-get install -y \
    protobuf-compiler \
    sccache \
    curl \
    git \
    clang \
    build-essential \
    procps \
    mold

# install rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN mv /root/.cargo/bin/* /usr/local/bin/

RUN mkdir /maremma
COPY . /maremma/

WORKDIR /maremma

RUN ./scripts/copy_linker_config.sh
ENV CC="/usr/bin/clang"

# # do the build bits
RUN cargo build --release --bins
RUN chmod +x /maremma/target/release/maremma

# https://github.com/GoogleContainerTools/distroless/blob/main/examples/rust/Dockerfile
FROM debian:12-slim AS maremma

RUN apt-get update && apt-get upgrade -y && apt-get install -y \
    ca-certificates \
    curl \
    dnsutils \
    git \
    iproute2 \
    python3 \
    python3-click \
    python3-venv \
    snmp snmpd libsnmp-base \
    && python3 -m venv /opt/check_goodwe \
    && /opt/check_goodwe/bin/pip install --no-cache-dir \
        "git+https://github.com/yaleman/check_goodwe.git@d33d3357707e86826de64106f0617ec994260983" \
    && rm -rf /var/lib/apt/ /var/cache/apt/

COPY --from=cargo_builder /maremma/target/release/maremma /maremma
COPY --from=cargo_builder /maremma/target/release/check_splunk /usr/local/bin/
COPY --from=plugin_builder /maremma/plugins/libexec/* /usr/local/bin/
COPY ./static /static/
RUN for plugin in check_disk check_dns check_http check_load check_ping check_procs check_snmp check_ssh check_swap check_tcp check_users; do \
        test -x "/usr/local/bin/${plugin}"; \
    done
RUN useradd maremma
RUN chown -R maremma /static
RUN chgrp -R maremma /static
USER maremma
ENTRYPOINT ["/maremma"]
CMD [ "run" ]
