# https://github.com/casey/just

default: check

docker_publish:
    docker buildx build \
        --tag ghcr.io/yaleman/maremma:latest \
        --label org.opencontainers.image.source=https://github.com/yaleman/maremma \
        --label org.opencontainers.image.revision=$(git rev-parse HEAD) \
        --label org.opencontainers.image.created=$(date -u +"%Y-%m-%dT%H:%M:%SZ") \
        .

check: codespell
    cargo clippy
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