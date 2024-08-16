FROM debian:latest AS builder

# ARG GITHUB_SHA="${GITHUB_SHA}"

# LABEL com.maremma.git-commit="${GITHUB_SHA}"

# fixing the issue with getting OOMKilled in BuildKit
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
RUN mkdir /maremma
COPY . /maremma/

WORKDIR /maremma
# install the dependencies
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    curl \
    git \
    build-essential \
    pkg-config \
    libssl-dev
# install rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN mv /root/.cargo/bin/* /usr/local/bin/
# do the build bits
RUN cargo build --release --bin maremma
RUN chmod +x /maremma/target/release/maremma

FROM gcr.io/distroless/cc-debian12 AS maremma
# # ======================
# https://github.com/GoogleContainerTools/distroless/blob/main/examples/rust/Dockerfile
WORKDIR /app
COPY --from=builder /maremma/target/release/maremma /app/

COPY static/ /app/static/
USER nonroot
ENTRYPOINT ["/app/maremma"]
CMD [ "run" ]