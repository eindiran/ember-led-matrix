# Build, flash, and verification entry points for the ember-led-matrix
# firmware (RP2350, thumbv8m.main-none-eabihf).

.DEFAULT_GOAL := help

ELF := target/thumbv8m.main-none-eabihf/release/ember-led-matrix

# One @printf per line keeps the help doc easy to hand-edit.
.PHONY: help
help:
	@printf 'Usage: make <target>\n'
	@printf '\n'
	@printf '  help    Show this help\n'
	@printf '  all     Build everything (release firmware)\n'
	@printf '  build   Build the release firmware ELF\n'
	@printf '  flash   Build, then flash over USB (force-reboots a running board)\n'
	@printf '  test    Run the test suite (no-op: no host-runnable tests)\n'
	@printf '  lint    Static analysis, read-only (fmt --check, clippy)\n'
	@printf '  format  Apply rustfmt formatting in place\n'
	@printf '  audit   Audit dependencies for known vulnerabilities\n'
	@printf '  check   Non-mutating verification: lint, test, pre-commit\n'
	@printf '  clean   Remove build artifacts\n'

.PHONY: all
all: build

.PHONY: build
build:
	cargo build --release

# -f force-reboots a running board into BOOTSEL; for a factory-fresh
# board, hold BOOT while plugging in and run picotool without -f.
.PHONY: flash
flash: build
	picotool load -u -v -x -t elf $(ELF) -f

.PHONY: test
test:
	@:

.PHONY: lint
lint:
	cargo fmt --all -- --check
	cargo clippy --all-targets --release -- -D warnings

.PHONY: format
format:
	cargo fmt --all

.PHONY: audit
audit:
	cargo audit

.PHONY: check
check: lint test
	pre-commit run --all-files

.PHONY: clean
clean:
	cargo clean
