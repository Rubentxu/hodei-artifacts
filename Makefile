# Makefile for accelerated Rust testing

# --- Configuration Variables ---
# Default crate to test. Use '.' for the current directory/workspace.
CRATE ?= artifact
# Default Rust log level for tests.
RUST_LOG_LEVEL ?= info
# Path to sccache executable. Assumes it's in PATH after installation.
SCCACHE_BIN ?= $(shell which sccache)

# --- Tools Installation ---
.PHONY: install-tools
install-tools:
	@echo "Installing/updating cargo-nextest..."
	@cargo install cargo-nextest || true
	@echo "Installing/updating sccache..."
	@cargo install sccache || true
	@echo "Tools installation complete. Remember to run 'make setup-env' to configure your shell."

# --- Environment Setup ---
.PHONY: setup-env
setup-env:
	@echo "Setting up environment variables for sccache."
	@echo "Add the following line to your shell configuration file (e.g., ~/.bashrc, ~/.zshrc):"
	@echo "export RUSTC_WRAPPER=$(SCCACHE_BIN)"
	@echo "Then, restart your shell or run 'source ~/.bashrc' (or equivalent)."
	@echo ""
	@echo "Optional: For faster linking on Linux, add the following to your .cargo/config.toml:"
	@echo "[target.x86_64-unknown-linux-gnu]"
	@echo "linker = \"clang\""
	@echo "rustflags = [\"-C\", \"link-arg=-fuse-ld=lld\"]"
	@echo "Ensure 'clang' and 'lld' are installed on your system."

# --- Test Execution Targets ---

.PHONY: test-unit
test-unit:
	@echo "Running unit tests for crate: $(CRATE) with cargo-nextest..."
	@RUST_LOG=$(RUST_LOG_LEVEL) cargo nextest run --lib --bins -p $(CRATE) --nocapture

.PHONY: test-integration
test-integration:
	@echo "Running integration tests for crate: $(CRATE) with cargo-nextest..."
	@echo "Note: Integration tests require Docker to be running."
	@RUST_LOG=$(RUST_LOG_LEVEL),testcontainers=debug cargo nextest run --tests -p $(CRATE) --features integration --nocapture

.PHONY: test-all
test-all: test-unit test-integration
	t@echo "All tests (unit and integration) for crate: $(CRATE) completed."

# --- Artifact Crate Specific Targets ---

.PHONY: test-artifact-unit test-artifact-integration test-artifact-all

# Run artifact unit tests
test-artifact-unit:
	@echo "Running artifact unit tests..."
	@RUST_LOG=$(RUST_LOG_LEVEL) cargo nextest run --lib --bins -p artifact --nocapture

# Run artifact integration tests
test-artifact-integration:
	@echo "Running artifact integration tests..."
	@echo "Note: Integration tests require Docker to be running."
	@RUST_LOG=$(RUST_LOG_LEVEL),testcontainers=debug cargo nextest run --tests -p artifact --features integration --nocapture

# Run all artifact tests
test-artifact-all: test-artifact-unit test-artifact-integration
	@echo "All artifact tests completed."

# --- Utility Targets ---

.PHONY: clean
clean:
	@echo "Cleaning build artifacts..."
	@cargo clean

.PHONY: help
help:
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@echo "  install-tools      Installs cargo-nextest and sccache."
	@echo "  setup-env          Provides instructions to set up sccache and linker optimizations."
	@echo "  test-unit          Runs unit tests for the specified CRATE (default: .)."
	@echo "                     Example: make test-unit CRATE=artifact"
	@echo "  test-integration   Runs integration tests for the specified CRATE (default: .)."
	@echo "                     Requires Docker. Example: make test-integration CRATE=artifact"
	@echo "  test-all           Runs both unit and integration tests for the specified CRATE (default: .)."
	@echo "  clean              Cleans Rust build artifacts."
	@echo "  help               Displays this help message."
	@echo ""
	@echo "Configuration Variables (can be overridden on command line):"
	@echo "  CRATE          - Crate name or '.' for current workspace (default: .)"
	@echo "  RUST_LOG_LEVEL - Log level for Rust tracing (default: info)"
	@echo ""
	@echo "Example: make test-integration CRATE=artifact RUST_LOG_LEVEL=debug"
