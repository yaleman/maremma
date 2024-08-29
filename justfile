# https://github.com/casey/just

default: check

docker_buildx:
    docker buildx build \
        --tag ghcr.io/yaleman/maremma:latest \
        --tag ghcr.io/yaleman/maremma:$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "maremma")  | .version') \
        --tag ghcr.io/yaleman/maremma:$(git rev-parse HEAD) \
        --label org.opencontainers.image.source=https://github.com/yaleman/maremma \
        --label org.opencontainers.image.revision=$(git rev-parse HEAD) \
        --label org.opencontainers.image.created=$(date -u +"%Y-%m-%dT%H:%M:%SZ") \
        .

docker_publish:
    docker buildx build \
        --platform linux/amd64,linux/arm64 \
        --tag ghcr.io/yaleman/maremma:latest \
        --tag ghcr.io/yaleman/maremma:$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "maremma")  | .version') \
        --tag ghcr.io/yaleman/maremma:$(git rev-parse HEAD) \
        --label org.opencontainers.image.source=https://github.com/yaleman/maremma \
        --label org.opencontainers.image.revision=$(git rev-parse HEAD) \
        --label org.opencontainers.image.created=$(date -u +"%Y-%m-%dT%H:%M:%SZ") \
        --push \
        .

clippy:
    cargo clippy --all-features

check: codespell clippy test

test:
    cargo test
book:
    cd docs && mdbook serve

run:
    cargo run run

release_prep:
    cargo deny
    cargo build --release


coverage:
    cargo tarpaulin --out Html
    @echo "Coverage file at file://$(PWD)/tarpaulin-report.html"

coveralls:
    cargo tarpaulin --coveralls $COVERALLS_REPO_TOKEN

schema:
    cargo run export-config-schema > maremma.schema.json

codespell:
    codespell -c \
    --ignore-words .codespell_ignore \
    --skip='./target' \
    --skip='./Cargo.lock' \
    --skip='./tarpaulin-report.html' \
    --skip='./static/*' \
    --skip='./docs/*,./.git' \
    --skip='./plugins/*'