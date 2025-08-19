#!/usr/bin/env bash
#
# Installs or updates the Rust toolchain using rustup.
# This script is designed to be run non-interactively.
#
set -euo pipefail

echo "--- Setting up Rust Toolchain (via rustup) ---"

# Check if rustup is already installed.
if command -v rustup &> /dev/null; then
    echo "Rustup is already installed. Checking for updates..."
    rustup update
else
    echo "Rustup not found. Installing now..."
    # Check if curl is installed, as it's required to download the installer.
    if ! command -v curl &> /dev/null; then
        echo "❌ Error: 'curl' is not installed. Please install it first."
        echo "   On Ubuntu/Debian, run: sudo apt-get update && sudo apt-get install -y curl"
        exit 1
    fi

    # Download and run the rustup installer in non-interactive mode.
    # The '-y' flag automatically accepts the default installation options.
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi

# The rustup script modifies the PATH environment variable in shell profile files.
# To make 'cargo' and 'rustc' available in the current shell, the user needs
# to source the cargo env script. This message provides clear instructions.
echo ""
echo "✅ Rust setup is complete."
echo ""
echo "IMPORTANT: To use 'cargo' in your current terminal, you must first reload your environment by running:"
echo "  source \"\$HOME/.cargo/env\""
echo ""
echo "After running the command above, you can proceed with other 'make' commands."
