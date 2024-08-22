# https://github.com/casey/just

default: check

check: codespell
    cargo clippy
    cargo test

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
    --skip='./tarpaulin-report.html' \
    --skip='./static/*' \
    --skip='./docs/*,./.git' \
    --skip='./plugins/*'