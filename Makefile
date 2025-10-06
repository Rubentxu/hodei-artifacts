# Makefile for Hodei Artifacts - Optimized Testing

# --- Configuration Variables ---
RUST_LOG_LEVEL ?= info

# --- Quick Commands (Recommended) ---

.PHONY: test
test:
	@echo "🧪 Running all tests with cargo-nextest (optimized)..."
	@cargo nextest run --lib -p kernel -p hodei-organizations -p hodei-iam -p hodei-authorizer

.PHONY: test-verbose
test-verbose:
	@echo "🧪 Running tests with output..."
	@cargo nextest run --lib -p kernel -p hodei-organizations -p hodei-iam -p hodei-authorizer --nocapture

.PHONY: test-kernel
test-kernel:
	@echo "🧪 Testing kernel..."
	@cargo nextest run --lib -p kernel

.PHONY: test-orgs
test-orgs:
	@echo "🧪 Testing hodei-organizations..."
	@cargo nextest run --lib -p hodei-organizations

.PHONY: test-iam
test-iam:
	@echo "🧪 Testing hodei-iam..."
	@cargo nextest run --lib -p hodei-iam

.PHONY: test-authorizer
test-authorizer:
	@echo "🧪 Testing hodei-authorizer..."
	@cargo nextest run --lib -p hodei-authorizer

# --- Traditional cargo test (slower) ---

.PHONY: test-cargo
test-cargo:
	@echo "🐌 Running tests with cargo test (slower)..."
	@cargo test --lib -p kernel -p hodei-organizations -p hodei-iam -p hodei-authorizer

# --- Coverage ---

.PHONY: coverage
coverage:
	@echo "📊 Measuring code coverage..."
	@cargo tarpaulin -p kernel --lib --timeout 300 --out Html
	@echo "Coverage report generated: target/tarpaulin/tarpaulin-report.html"

.PHONY: coverage-all
coverage-all:
	@echo "📊 Measuring coverage for all crates..."
	@cargo tarpaulin -p kernel -p hodei-organizations -p hodei-iam -p hodei-authorizer --lib --timeout 600 --out Html

# --- Build & Check ---

.PHONY: build
build:
	@echo "🔨 Building all crates..."
	@cargo build

.PHONY: check
check:
	@echo "✅ Checking compilation..."
	@cargo check --workspace

.PHONY: clippy
clippy:
	@echo "📎 Running clippy..."
	@cargo clippy --workspace --all-targets -- -D warnings

.PHONY: fmt
fmt:
	@echo "🎨 Formatting code..."
	@cargo fmt --all

.PHONY: fmt-check
fmt-check:
	@echo "🎨 Checking code format..."
	@cargo fmt --all -- --check

# --- Utility ---

.PHONY: clean
clean:
	@echo "🧹 Cleaning build artifacts..."
	@cargo clean

.PHONY: stats
stats:
	@echo "📈 Test Statistics:"
	@echo "  kernel:              $$(cargo nextest list --lib -p kernel | grep -c 'test '|| echo 0) tests"
	@echo "  hodei-organizations: $$(cargo nextest list --lib -p hodei-organizations | grep -c 'test ' || echo 0) tests"
	@echo "  hodei-iam:           $$(cargo nextest list --lib -p hodei-iam | grep -c 'test ' || echo 0) tests"
	@echo "  hodei-authorizer:    $$(cargo nextest list --lib -p hodei-authorizer | grep -c 'test ' || echo 0) tests"

.PHONY: help
help:
	@echo "╔═══════════════════════════════════════════════════════════════╗"
	@echo "║          Hodei Artifacts - Makefile Commands                 ║"
	@echo "╚═══════════════════════════════════════════════════════════════╝"
	@echo ""
	@echo "🚀 Quick Commands:"
	@echo "  make test           Run all tests (fast with nextest)"
	@echo "  make test-verbose   Run tests with output"
	@echo "  make test-kernel    Test kernel only"
	@echo "  make test-orgs      Test hodei-organizations only"
	@echo "  make test-iam       Test hodei-iam only"
	@echo ""
	@echo "🔍 Quality:"
	@echo "  make check          Check compilation"
	@echo "  make clippy         Run linter"
	@echo "  make fmt            Format code"
	@echo "  make fmt-check      Check formatting"
	@echo ""
	@echo "📊 Coverage:"
	@echo "  make coverage       Measure kernel coverage"
	@echo "  make coverage-all   Measure all crates coverage"
	@echo ""
	@echo "🛠️  Utility:"
	@echo "  make build          Build all crates"
	@echo "  make clean          Clean build artifacts"
	@echo "  make stats          Show test statistics"
	@echo "  make help           Show this help"
	@echo ""
	@echo "📝 Examples:"
	@echo "  make test           # Fast tests (0.5s)"
	@echo "  make clippy         # Check code quality"
	@echo "  make coverage       # Generate coverage report"
	@echo ""
	@echo "Current test count: 386 tests across 4 crates"
	@echo "Kernel domain coverage: 91.58%"
