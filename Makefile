CONTAINER_TOOL_ARGS ?=
IMAGE_BASE ?= maremma
IMAGE_NAME ?= maremma
IMAGE_VERSION ?= latest
IMAGE_EXT_VERSION ?= $(shell cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "maremma")  | .version')
IMAGE_ARCH ?= "linux/amd64,linux/arm64"
CONTAINER_BUILD_ARGS ?=
CONTAINER_TOOL ?= docker
BUILDKIT_PROGRESS ?= plain
TESTS ?=
GIT_COMMIT := $(shell git rev-parse HEAD)
MARKDOWN_FORMAT_ARGS ?= --options-line-width=100

.DEFAULT: help
.PHONY: help
help:
	@grep -E -h '\s##\s' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'

.PHONY: run
run: ## Run the test server
run:
	cargo run

.PHONY: docker/build
docker/build: ## Build multiarch server images
docker/build:
	@$(CONTAINER_TOOL) buildx build $(CONTAINER_TOOL_ARGS) \
		 --platform $(IMAGE_ARCH) \
		-t $(IMAGE_BASE)/$(IMAGE_NAME):$(IMAGE_VERSION) \
		-t $(IMAGE_BASE)/$(IMAGE_NAME):$(IMAGE_EXT_VERSION) \
		--progress $(BUILDKIT_PROGRESS) \
		--compress \
		--label "com.$(IMAGE_BASE).git-commit=$(GIT_COMMIT)" \
		--label "com.$(IMAGE_BASE).version=$(IMAGE_EXT_VERSION)" \
		$(CONTAINER_BUILD_ARGS) .

.PHONY: test
test:
	cargo test

.PHONY: doc
doc: ## Build the rust documentation locally
doc:
	cargo doc --document-private-items

.PHONY: doc/format/check
doc/format/check: ## Check docs format
	find . -type f  \
		-not -path './target/*' \
		-not -path './docs/*' \
		-not -path '*/.venv/*' -not -path './vendor/*'\
		-not -path '*/.*/*' \
		-name \*.md \
		-exec deno fmt --check $(MARKDOWN_FORMAT_ARGS) "{}" +

.PHONY: doc/format/fix
doc/format/fix: ## Fix docs formatting
	find . -type f  -not -path './target/*' -not -path '*/.venv/*' -not -path './vendor/*'\
		-name \*.md \
		-exec deno fmt  $(MARKDOWN_FORMAT_ARGS) "{}" +

.PHONY: release/prep
prep:
	cargo outdated -R
	cargo audit

.PHONY: coverage
coverage: ## Run coverage
coverage:
	cargo tarpaulin --out Html
	@echo "Coverage file at file://$(PWD)/tarpaulin-report.html"