# Makefile for the Matrix Protocol Helper
# Provides a standard, unified interface for building, testing, and development.

# --- Variables ---
# Use npm as the package manager.
NPM := npm
# Shell to use for commands, with stricter error handling.
SHELL := /bin/bash
.SHELLFLAGS := -euo pipefail -c

# --- Configuration ---
# Set the default goal to 'help' so that running 'make' by itself
# provides instructions on how to use this Makefile.
.DEFAULT_GOAL := help

# Phony targets are commands that do not represent files.
# This prevents 'make' from getting confused if a file with the same name exists.
.PHONY: help check-env install-rust install-dev-linux install-deps install dev build build-all release test clean

# --- Commands ---

help:
	@echo "Matrix Protocol Helper Makefile"
	@echo "---------------------------------"
	@echo "Usage: make [target]"
	@echo ""
	@echo "Primary Targets:"
	@echo "  install            Install all dependencies and build the application."
	@echo "  dev                Start the application in development mode with hot-reloading."
	@echo "  build              Build the application and create distributable artifacts."
	@echo ""
	@echo "Setup & Installation Targets:"
	@echo "  check-env          Verify that all required dependencies (npm, Rust) are installed."
	@echo "  install-rust       Install the Rust toolchain (required for the backend)."
	@echo "  install-dev-linux  (Linux Only) Install essential system dependencies for building."
	@echo "  install-deps       Install all Node.js dependencies."
	@echo ""
	@echo "Other Targets:"
	@echo "  release            Build the application in release mode (same as build)."
	@echo "  build-all          Show instructions for building artifacts for all operating systems."
	@echo "  test               (Placeholder) Run automated tests for the application."
	@echo "  clean              Remove all build artifacts and installed dependencies."
	@echo ""

check-env:
	@echo "🔎 Checking for required dependencies..."
	@if ! command -v $(NPM) &> /dev/null; then \
		echo "❌ Error: 'npm' command not found (part of Node.js)."; \
		echo "   Please install Node.js and npm first. See: https://nodejs.org/"; \
		exit 1; \
	fi
	@if ! command -v cargo &> /dev/null; then \
		echo "❌ Error: 'cargo' command not found (part of the Rust toolchain)."; \
		echo "   Please run 'make install-rust' to install it."; \
		exit 1; \
	fi
	@echo "✅ All dependencies found."

install-rust:
	@echo "⚙️  Installing the Rust toolchain via the official script..."
	@chmod +x scripts/install_rust.sh
	@./scripts/install_rust.sh

install-dev-linux:
	@echo "🐧 Installing essential Linux development packages..."
	@if [[ "$$(uname)" != "Linux" ]]; then \
		echo "⚠️  Warning: This target is intended for Linux systems. Skipping."; \
		exit 0; \
	fi
	@chmod +x scripts/install_essentials.sh
	@echo "   Running script with sudo. You may be prompted for your password."
	@sudo ./scripts/install_essentials.sh

install-deps: check-env
	@echo "📦 Installing Node.js dependencies with npm..."
	@$(NPM) install
	@echo "✅ Dependencies installed."

install: build
	@echo "✅ Installation complete."
	@echo ""
	@echo "To run the application, find the installer in the following directory:"
	@echo "  src-tauri/target/release/bundle/"
	@echo ""
	@echo "For example:"
	@echo "  - On macOS, open the .dmg file and drag the app to your Applications folder."
	@echo "  - On Windows, run the .msi installer."
	@echo "  - On Linux, run the .AppImage or install the .deb file."

dev: install-deps
	@echo "🚀 Starting development server..."
	@$(NPM) run tauri dev

build: install-deps
	@echo "🏗️  Building production artifacts for the current OS..."
	@$(NPM) run tauri build
	@echo "✅ Build complete. Artifacts are in src-tauri/target/release/bundle/"

release: build

build-all:
	@echo "🌍 Building for all platforms (macOS, Windows, Linux)..."
	@echo "---------------------------------------------------------"
	@echo "Tauri cross-platform builds require native toolchains for each OS."
	@echo "The recommended approach is to use a CI/CD pipeline like the one"
	@echo "defined in '.github/workflows/release.yml', which builds on"
	@echo "virtual machines for each target platform."
	@echo ""
	@echo "To build locally, you must run 'make build' on each respective OS."
	@echo "---------------------------------------------------------"

test:
	@echo "🧪 Running tests... (No tests configured yet)"
	@echo "You can add your test command here in the future."

clean:
	@echo "🧹 Cleaning up project..."
	@rm -rf node_modules
	@rm -rf dist
	@rm -rf src-tauri/target
	@echo "✅ Project cleaned."