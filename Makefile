FEATUREVISOR_PROJECT ?= ../featurevisor/examples/example-1
FEATUREVISOR_REPO ?= https://github.com/featurevisor/featurevisor.git

.PHONY: build test test-cli test-openfeature fmt lint check check-base check-openfeature package package-openfeature test-example-1 setup-monorepo update-monorepo

build:
	cargo build -p featurevisor --all-features
	cargo build -p featurevisor-openfeature

test:
	cargo test -p featurevisor --all-features
	cargo test -p featurevisor-openfeature

test-cli:
	cargo test -p featurevisor --all-features --test cli

test-openfeature:
	cargo test -p featurevisor-openfeature

fmt:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace --all-features --all-targets -- -D warnings

check-base:
	cargo build -p featurevisor --all-features
	cargo test -p featurevisor --all-features
	cargo clippy -p featurevisor --all-features --all-targets -- -D warnings

check-openfeature:
	cargo build -p featurevisor-openfeature
	cargo test -p featurevisor-openfeature
	cargo clippy -p featurevisor-openfeature --all-targets -- -D warnings
	cargo doc -p featurevisor-openfeature --no-deps

check: fmt check-base check-openfeature

package:
	cargo package -p featurevisor
	cargo package -p featurevisor --list

package-openfeature:
	# Requires the matching featurevisor version to exist on crates.io.
	cargo package -p featurevisor-openfeature --no-verify
	cargo package -p featurevisor-openfeature --list --no-verify

test-example-1:
	$(MAKE) test
	cargo run --features cli --bin featurevisor -- test --projectDirectoryPath=$(FEATUREVISOR_PROJECT) --onlyFailures

setup-monorepo:
	mkdir -p monorepo
	if [ ! -d "monorepo/.git" ]; then \
		git clone $(FEATUREVISOR_REPO) monorepo; \
	else \
		(cd monorepo && git fetch && git checkout main && git pull); \
	fi
	(cd monorepo && make install && make build)

update-monorepo:
	(cd monorepo && git pull)
