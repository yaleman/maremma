# https://github.com/casey/just

# list things
default: list

# List the options
list:
    just --list

# Build the docker image locally using buildx
docker_buildx:
    docker buildx build \
        --tag ghcr.io/yaleman/maremma:latest \
        --tag ghcr.io/yaleman/maremma:$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "maremma")  | .version') \
        --tag ghcr.io/yaleman/maremma:$(git rev-parse HEAD) \
        --label org.opencontainers.image.source=https://github.com/yaleman/maremma \
        --label org.opencontainers.image.revision=$(git rev-parse HEAD) \
        --label org.opencontainers.image.created=$(date -u +"%Y-%m-%dT%H:%M:%SZ") \
        .

# Build the docker image locally
docker_build:
    docker build \
        --tag ghcr.io/yaleman/maremma:latest \
        --tag ghcr.io/yaleman/maremma:$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "maremma")  | .version') \
        --tag ghcr.io/yaleman/maremma:$(git rev-parse HEAD) \
        --label org.opencontainers.image.source=https://github.com/yaleman/maremma \
        --label org.opencontainers.image.revision=$(git rev-parse HEAD) \
        --label org.opencontainers.image.created=$(date -u +"%Y-%m-%dT%H:%M:%SZ") \
        .

# Publish multi-arch docker image to ghcr.io
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

# Publish a dev build
docker_publish_dev:
    docker buildx build \
        --platform linux/amd64,linux/arm64 \
        --tag ghcr.io/yaleman/maremma:dev \
        --tag ghcr.io/yaleman/maremma:$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "maremma")  | .version')-dev \
        --tag ghcr.io/yaleman/maremma:$(git rev-parse HEAD) \
        --label org.opencontainers.image.source=https://github.com/yaleman/maremma \
        --label org.opencontainers.image.revision=$(git rev-parse HEAD) \
        --label org.opencontainers.image.created=$(date -u +"%Y-%m-%dT%H:%M:%SZ") \
        --push \
        .


# Serve the book
book:
    cd docs && mdbook serve

# Run a local debug instance
run:
    cargo run -- run

# Run and enable the tokio console
run_tokio:
     RUSTFLAGS="--cfg tokio_unstable" cargo run -- run --tokio-console

# Run in docker
run_docker:
    docker run -it --rm \
        -p 8888:8888 \
        --init \
        --mount "type=bind,source=$(pwd)/maremma.json,target=/maremma.json" \
        --mount "type=bind,source=$(pwd)/,target=/data/" \
        --mount "type=bind,source=$CERTDIR,target=/certs/" \
        ghcr.io/yaleman/maremma:latest

# Run all the checks
check: codespell clippy test doc_check


# Spell check the things
codespell:
    codespell -c \
    --ignore-words .codespell_ignore \
    --skip='./target' \
    --skip='./Cargo.lock' \
    --skip='./tarpaulin-report.html' \
    --skip='./static/*' \
    --skip='./docs/*,./.git' \
    --skip='./plugins/*'

# Ask the clip for the judgement
clippy:
    cargo clippy --all-features

test:
    cargo test

# Things to do before a release
release_prep: check schema doc semgrep
    cargo deny check
    cargo build --release

# Semgrep things
semgrep:
    semgrep ci --config auto \
    --exclude-rule "yaml.github-actions.security.third-party-action-not-pinned-to-commit-sha.third-party-action-not-pinned-to-commit-sha" \
    --exclude-rule "generic.html-templates.security.var-in-script-tag.var-in-script-tag" \
    --exclude-rule "javascript.express.security.audit.xss.mustache.var-in-href.var-in-href" \
    --exclude-rule "python.django.security.django-no-csrf-token.django-no-csrf-token" \
    --exclude-rule "python.django.security.audit.xss.template-href-var.template-href-var" \
    --exclude-rule "python.django.security.audit.xss.var-in-script-tag.var-in-script-tag" \
    --exclude-rule "python.flask.security.xss.audit.template-href-var.template-href-var" \
    --exclude-rule "python.flask.security.xss.audit.template-href-var.template-href-var"

# Export the schema
schema:
    ./scripts/update_schema.sh

# Build the rustdocs
doc:
	cargo doc --document-private-items

# Run cargo tarpaulin
coverage:
    cargo tarpaulin --out Html
    @echo "Coverage file at file://$(PWD)/tarpaulin-report.html"

# Run cargo tarpaulin and upload to coveralls
coveralls:
    cargo tarpaulin --coveralls $COVERALLS_REPO_TOKEN

# Check docs format
doc_check:
	find . -type f  \
		-not -path './target/*' \
		-not -path './docs/*' \
		-not -path '*/.venv/*' -not -path './vendor/*'\
		-not -path '*/.*/*' \
		-name \*.md \
		-exec deno fmt --check --options-line-width=100 "{}" +

# Fix docs formatting
doc_fix:
	find . -type f  -not -path './target/*' -not -path '*/.venv/*' -not -path './vendor/*'\
		-name \*.md \
		-exec deno fmt --options-line-width=100 "{}" +

# Run trivy on the image
trivy_image:
    trivy image ghcr.io/yaleman/maremma:latest --scanners misconfig,vuln,secret

# Run trivy on the repo
trivy_repo:
    trivy repo $(pwd) --skip-dirs 'target/**' --skip-files .envrc -d