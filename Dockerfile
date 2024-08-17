FROM debian:latest AS builder

# ARG GITHUB_SHA="${GITHUB_SHA}"

# LABEL com.maremma.git-commit="${GITHUB_SHA}"

# fixing the issue with getting OOMKilled in BuildKit
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
RUN mkdir /maremma
COPY . /maremma/

WORKDIR /maremma
# install the dependencies
RUN --mount=type=cache,target=/var/cache/apt apt-get update && apt-get install -y \
    protobuf-compiler \
    curl \
    git \
    build-essential \
    pkg-config \
    libssl-dev \
    procps

# install rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN mv /root/.cargo/bin/* /usr/local/bin/

# do the build bits
RUN --mount=type=cache,target=/maremma/target/release/deps cargo build --release --bin maremma
RUN chmod +x /maremma/target/release/maremma

RUN make plugins/extract
RUN cd plugins/monitoring-plugins && ./configure \
		--prefix="$(pwd)/../" \
		--with-ipv6 --without-systemd \
		&& make clean && make && make install

# https://github.com/GoogleContainerTools/distroless/blob/main/examples/rust/Dockerfile
FROM gcr.io/distroless/cc-debian12 AS maremma

COPY --from=builder /maremma/target/release/maremma /maremma
COPY --from=builder /maremma/plugins/libexec/* /usr/local/bin/
COPY ./static /static/
USER nonroot
ENTRYPOINT ["/maremma"]
CMD [ "run" ]