# https://github.com/casey/just

default: check

check:
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
