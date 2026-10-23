# Makefile for Hodei Artifacts
#
# Ground rules (H0):
# - Targets must only reference crates that are real members of the Cargo workspace
#   (`cargo metadata --no-deps` is the source of truth, not this file).
# - No hardcoded test counts or coverage percentages: those are data, and stale
#   numbers are a false green. Use `make stats` to measure them now.
# - If the preferred runner is missing, fall back explicitly and say so.

# --- Configuration Variables ---
RUST_LOG_LEVEL ?= info

# Real workspace library members (see Cargo.toml [workspace].members).
# The root package `hodei-artifacts-api` is covered by `--workspace`.
LIB_CRATES := kernel hodei-iam hodei-policies

# --- Runner detection ---------------------------------------------------
# nextest is the fast runner, but it is not always installed. cargo test is the
# fallback: it is slower but always available.
ifeq ($(shell cargo nextest --version >/dev/null 2>&1 && echo yes),yes)
TEST_RUNNER := nextest
else
TEST_RUNNER := cargo
endif

# --- Quick Commands (Recommended) ---

.PHONY: test
test:
ifeq ($(TEST_RUNNER),nextest)
	@echo "🧪 Running library tests with cargo-nextest (optimized)..."
	@cargo nextest run --lib $(foreach c,$(LIB_CRATES),-p $(c))
else
	@echo "🧪 cargo-nextest not installed; running library tests with cargo test..."
	@cargo test --lib $(foreach c,$(LIB_CRATES),-p $(c))
endif

.PHONY: test-verbose
test-verbose:
ifeq ($(TEST_RUNNER),nextest)
	@echo "🧪 Running tests with output..."
	@cargo nextest run --lib $(foreach c,$(LIB_CRATES),-p $(c)) --no-capture
else
	@echo "🧪 cargo-nextest not installed; running tests with output..."
	@cargo test --lib $(foreach c,$(LIB_CRATES),-p $(c)) -- --nocapture
endif

.PHONY: test-all
test-all:
	@echo "🧪 Running the full workspace test suite (all targets, no fail-fast)..."
	@cargo test --workspace --no-fail-fast

.PHONY: test-kernel
test-kernel:
ifeq ($(TEST_RUNNER),nextest)
	@cargo nextest run --lib -p kernel
else
	@cargo test --lib -p kernel
endif

.PHONY: test-iam
test-iam:
ifeq ($(TEST_RUNNER),nextest)
	@cargo nextest run --lib -p hodei-iam
else
	@cargo test --lib -p hodei-iam
endif

.PHONY: test-policies
test-policies:
ifeq ($(TEST_RUNNER),nextest)
	@cargo nextest run --lib -p hodei-policies
else
	@cargo test --lib -p hodei-policies
endif

# --- Traditional cargo test (slower) ---

.PHONY: test-cargo
test-cargo:
	@echo "🐌 Running tests with cargo test (slower)..."
	@cargo test --lib $(foreach c,$(LIB_CRATES),-p $(c))

# --- Coverage ---

.PHONY: coverage
coverage:
	@echo "📊 Measuring code coverage for kernel (requires cargo-tarpaulin)..."
	@cargo tarpaulin -p kernel --lib --timeout 300 --out Html
	@echo "Coverage report generated: target/tarpaulin/tarpaulin-report.html"

.PHONY: coverage-all
coverage-all:
	@echo "📊 Measuring coverage for all library crates (requires cargo-tarpaulin)..."
	@cargo tarpaulin $(foreach c,$(LIB_CRATES),-p $(c)) --lib --timeout 600 --out Html

# --- Build & Check ---

.PHONY: build
build:
	@echo "🔨 Building all crates..."
	@cargo build --workspace

.PHONY: check
check:
	@echo "✅ Checking compilation (all targets)..."
	@cargo check --workspace --all-targets

.PHONY: clippy
clippy:
	@echo "📎 Running clippy (warnings denied)..."
	@cargo clippy --workspace --all-targets -- -D warnings

.PHONY: fmt
fmt:
	@echo "🎨 Formatting code..."
	@cargo fmt --all

.PHONY: fmt-check
fmt-check:
	@echo "🎨 Checking code format..."
	@cargo fmt --all -- --check

# --- Quality gate (mirrors CI) ---

.PHONY: ci
ci:
	@echo "🚦 Running the quality gate: fmt -> clippy -> check -> test..."
	@cargo fmt --all -- --check
	@cargo clippy --workspace --all-targets -- -D warnings
	@cargo check --workspace --all-targets
	@cargo test --workspace --no-fail-fast
	@echo "✅ Quality gate passed"

# --- Utility ---

.PHONY: clean
clean:
	@echo "🧹 Cleaning build artifacts..."
	@cargo clean

.PHONY: stats
stats:
	@echo "📈 Test statistics (measured, not hardcoded):"
	@echo "  runner:            $(TEST_RUNNER)"
	@echo "  library crates:    $(LIB_CRATES)"
	@echo "  workspace members:"
	@cargo metadata --no-deps --format-version 1 2>/dev/null | \
		sed -n 's/.*"name":"\([^"]*\)","version":"[^"]*","manifest_path":"[^"]*\/\([^"/]*\)\/Cargo.toml".*/    - \1/p' | \
		sort -u
	@echo "  test count:        $$(cargo test --workspace --no-run 2>/dev/null | grep -c 'Executable' || echo 0) test binaries"

.PHONY: help
help:
	@echo "╔═══════════════════════════════════════════════════════════════╗"
	@echo "║          Hodei Artifacts - Makefile Commands                 ║"
	@echo "╚═══════════════════════════════════════════════════════════════╝"
	@echo ""
	@echo "🚀 Quick Commands:"
	@echo "  make test           Run library tests (nextest, or cargo test fallback)"
	@echo "  make test-verbose   Run library tests with output"
	@echo "  make test-all       Run the full workspace test suite"
	@echo "  make test-kernel    Test kernel only"
	@echo "  make test-iam       Test hodei-iam only"
	@echo "  make test-policies  Test hodei-policies only"
	@echo ""
	@echo "🔍 Quality:"
	@echo "  make check          Check compilation (all targets)"
	@echo "  make clippy         Run linter (warnings denied)"
	@echo "  make fmt            Format code"
	@echo "  make fmt-check      Check formatting"
	@echo "  make ci             Run the full quality gate"
	@echo ""
	@echo "📊 Coverage:"
	@echo "  make coverage       Measure kernel coverage"
	@echo "  make coverage-all   Measure all library crates coverage"
	@echo ""
	@echo "🛠️  Utility:"
	@echo "  make build          Build all crates"
	@echo "  make clean          Remove build artifacts"
	@echo "  make stats          Measure and print test statistics"
	@echo "  make help           Show this help"
