FEATUREVISOR_PROJECT ?= ../featurevisor/examples/example-1

.PHONY: build test test-cli fmt lint check test-example-1 setup-monorepo update-monorepo

build:
	cargo build --all-features

test:
	cargo test --all-features

test-cli:
	cargo test --all-features --test cli

fmt:
	cargo fmt --all -- --check

lint:
	cargo clippy --all-features --all-targets -- -D warnings

check: fmt lint test

test-example-1:
	$(MAKE) test
	cargo run --features cli --bin featurevisor -- test --projectDirectoryPath=$(FEATUREVISOR_PROJECT) --onlyFailures

setup-monorepo:
	mkdir -p monorepo
	if [ ! -d "monorepo/.git" ]; then \
		git clone ../featurevisor monorepo; \
	else \
		(cd monorepo && git fetch && git checkout main && git pull); \
	fi
	(cd monorepo && make install && make build)

update-monorepo:
	(cd monorepo && git pull)
